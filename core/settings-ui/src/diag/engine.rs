//! 変換エンジン（imserver）の起動・応答・終了・メモリの診断。
//!
//! 診断用のエンジンは `--user-dir` に作業ディレクトリを渡して起動する（利用者の `learning.tsv` などを
//! 読み書きしない）。stderr は末尾だけ保持し、起動失敗の理由に使う（imserver は入力内容を出力しない）。

use super::ctx::Ctx;
use super::sys::ProcMemory;
use super::types::{ItemId, ItemResult};
use crate::backend::{BackendError, RpcClient};
use nagi_protocol::methods::{InitializeParams, InitializeResult, StatusResult, method};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// JSON-RPC の呼び出し口（テストではフェイクを差し込む）
pub trait Rpc {
    /// 要求を送り、結果（`result`）を返す
    fn call_value(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, BackendError>;
}

impl Rpc for RpcClient {
    fn call_value(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, BackendError> {
        self.request(method, params, timeout)
    }
}

/// 結果を型に読む
pub fn call<T: DeserializeOwned>(
    rpc: &mut dyn Rpc,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<T, BackendError> {
    let value = rpc.call_value(method, params, timeout)?;
    serde_json::from_value(value)
        .map_err(|_| BackendError::transport("エンジンの応答を解釈できません"))
}

/// 閉じられる標準入力（EOF による終了を検証するため）
struct SharedStdin(Arc<Mutex<Option<ChildStdin>>>);

impl Write for SharedStdin {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self
            .0
            .lock()
            .map_err(|_| std::io::ErrorKind::Other)?
            .as_mut()
        {
            Some(stdin) => stdin.write(buf),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self
            .0
            .lock()
            .map_err(|_| std::io::ErrorKind::Other)?
            .as_mut()
        {
            Some(stdin) => stdin.flush(),
            None => Ok(()),
        }
    }
}

/// 起動の指定
pub struct LaunchOptions {
    /// 作業ディレクトリの名前（`Ctx::scratch` の下。`--user-dir` になる）
    pub scratch_name: &'static str,
    /// `config.toml` の内容（`None` なら置かない）
    pub config_toml: Option<&'static str>,
    /// `--no-llm`
    pub no_llm: bool,
    /// `--parent-pid`（`None` なら診断プログラム自身）
    pub parent_pid: Option<u32>,
}

/// 起動した診断用のエンジン
pub struct EngineSession {
    child: Child,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// JSON-RPC クライアント
    pub rpc: RpcClient,
    /// PID
    pub pid: u32,
    /// `initialize` の結果
    pub init: InitializeResult,
    /// 起動から `initialize` 完了までの時間（ミリ秒）
    pub init_ms: u64,
    stderr: Arc<Mutex<VecDeque<String>>>,
}

/// 起動の失敗
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchFailure {
    /// 利用者向けの説明
    pub message: String,
    /// 推奨される対処
    pub advice: String,
    /// stderr の末尾（imserver のログ。入力内容は含まれない）
    pub stderr_tail: Vec<String>,
}

const STDERR_KEEP: usize = 12;
const STDERR_LINE_CHARS: usize = 200;

/// 終了コード（Windows の NTSTATUS を含む）の説明
pub fn describe_exit_code(code: Option<i32>) -> String {
    let Some(code) = code else {
        return "終了コードなし（シグナルで終了）".to_owned();
    };
    let raw = code as u32;
    let known = match raw {
        0xC000_0135 => Some("必要な DLL が見つからない（VC++ ランタイムなどの不足）"),
        0xC000_007B => Some("DLL の CPU の種類が合わない（x64 と ARM64 の混在）"),
        0xC000_0005 => Some("メモリ参照違反"),
        0xC000_0409 => Some("異常終了（スタック破壊・fail fast）"),
        0xC000_001D => Some("この CPU が対応しない命令を実行した"),
        0xC000_0142 => Some("DLL の初期化に失敗した"),
        _ => None,
    };
    match known {
        Some(text) => format!("終了コード 0x{raw:08X}: {text}"),
        None => format!("終了コード {code}（0x{raw:08X}）"),
    }
}

/// 起動できなかった（`spawn` の失敗）理由の説明
fn describe_spawn_error(e: &std::io::Error, exe: &Path) -> LaunchFailure {
    let (message, advice) = match (e.kind(), e.raw_os_error()) {
        (std::io::ErrorKind::NotFound, _) => (
            "imserver が見つかりません".to_owned(),
            "Nagi を入れ直してください（インストール先に imserver.exe があるか確認）。".to_owned(),
        ),
        (_, Some(193)) => (
            "imserver がこの PC の CPU では実行できません（ERROR_BAD_EXE_FORMAT）".to_owned(),
            "CPU に合う配布物（x64 / ARM64）を入れ直してください。".to_owned(),
        ),
        (std::io::ErrorKind::PermissionDenied, _) => (
            "imserver の実行を拒否されました".to_owned(),
            "ウイルス対策ソフトの隔離・実行ブロックを確認してください。".to_owned(),
        ),
        _ => (
            format!("imserver を起動できません（{}）", e.kind()),
            "ウイルス対策ソフトの隔離・ファイルの破損を確認し、Nagi を入れ直してください。"
                .to_owned(),
        ),
    };
    let _ = exe;
    LaunchFailure {
        message,
        advice,
        stderr_tail: Vec::new(),
    }
}

fn no_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// 診断用のエンジンを起動して `initialize` まで済ませる
pub fn launch(ctx: &Ctx, options: &LaunchOptions) -> Result<EngineSession, LaunchFailure> {
    if !ctx.imserver_exe.is_file() {
        return Err(LaunchFailure {
            message: format!(
                "imserver が見つかりません（{}）",
                ctx.show(&ctx.imserver_exe)
            ),
            advice: "Nagi を入れ直してください（インストール先に imserver があるか確認）。"
                .to_owned(),
            stderr_tail: Vec::new(),
        });
    }
    let scratch: PathBuf = ctx
        .scratch(options.scratch_name)
        .map_err(|e| LaunchFailure {
            message: format!("診断用の作業フォルダを作れません（{}）", e.kind()),
            advice: "一時フォルダ（TEMP）の空き容量と権限を確認してください。".to_owned(),
            stderr_tail: Vec::new(),
        })?;
    if let Some(toml) = options.config_toml {
        std::fs::write(scratch.join("config.toml"), toml).map_err(|e| LaunchFailure {
            message: format!("診断用の設定を書けません（{}）", e.kind()),
            advice: "一時フォルダ（TEMP）の権限を確認してください。".to_owned(),
            stderr_tail: Vec::new(),
        })?;
    }
    let mut command = Command::new(&ctx.imserver_exe);
    if let Some(dir) = &ctx.data_dir {
        command.arg("--data-dir").arg(dir);
    }
    command.arg("--user-dir").arg(&scratch);
    if options.no_llm {
        command.arg("--no-llm");
    }
    command
        .arg("--parent-pid")
        .arg(
            options
                .parent_pid
                .unwrap_or_else(std::process::id)
                .to_string(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut command);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| describe_spawn_error(&e, &ctx.imserver_exe))?;
    let pid = child.id();
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = child.kill();
        return Err(LaunchFailure {
            message: "imserver の入出力を開けません".to_owned(),
            advice: "もう一度実行してください。".to_owned(),
            stderr_tail: Vec::new(),
        });
    };
    let stdin = Arc::new(Mutex::new(Some(stdin)));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let tail_writer = Arc::clone(&tail);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let clipped: String = line.chars().take(STDERR_LINE_CHARS).collect();
            if let Ok(mut t) = tail_writer.lock() {
                if t.len() >= STDERR_KEEP {
                    t.pop_front();
                }
                t.push_back(clipped);
            }
        }
    });
    let mut rpc = RpcClient::new(Box::new(SharedStdin(Arc::clone(&stdin))), rx);
    let params = InitializeParams {
        protocol_version: nagi_protocol::PROTOCOL_VERSION,
        client_name: "nagi-diag".to_owned(),
        client_version: env!("CARGO_PKG_VERSION").to_owned(),
        os: ctx.os.clone(),
        capabilities: None,
    };
    let init: Result<InitializeResult, BackendError> = call(
        &mut rpc,
        method::INITIALIZE,
        serde_json::to_value(&params).unwrap_or(Value::Null),
        ctx.timeouts.initialize,
    );
    let stderr_tail = |t: &Arc<Mutex<VecDeque<String>>>| -> Vec<String> {
        // stderr の読み取りスレッドが追いつくのを少し待つ
        std::thread::sleep(Duration::from_millis(150));
        t.lock()
            .map(|t| t.iter().cloned().collect())
            .unwrap_or_default()
    };
    match init {
        Ok(init) => Ok(EngineSession {
            child,
            stdin,
            rpc,
            pid,
            init,
            init_ms: started.elapsed().as_millis() as u64,
            stderr: tail,
        }),
        Err(e) => {
            // 早期終了なら終了コードを読む（少し待ってから）
            std::thread::sleep(Duration::from_millis(200));
            let exited = child.try_wait().ok().flatten();
            let advice_for_code = |code: Option<i32>| match code.map(|c| c as u32) {
                Some(0xC000_0135) => {
                    "VC++ 再頒布可能パッケージを入れてください（VC++ ランタイムの項目を参照）。"
                }
                Some(0xC000_007B) => {
                    "CPU に合う配布物（x64 / ARM64）を入れ直してください（CPU の項目を参照）。"
                }
                _ => {
                    "stderr の抜粋と %LOCALAPPDATA%\\Nagi\\logs の imserver-*.log を報告してください。辞書・モデルの項目も確認してください。"
                }
            };
            let failure = match exited {
                Some(status) => LaunchFailure {
                    message: format!(
                        "imserver が起動直後に終了しました（{}）",
                        describe_exit_code(status.code())
                    ),
                    advice: advice_for_code(status.code()).to_owned(),
                    stderr_tail: stderr_tail(&tail),
                },
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    LaunchFailure {
                        message: format!("imserver が初期化に応答しません（{}）", e.message),
                        advice: "辞書・モデルの読み込みが遅い（初回・ウイルス対策ソフトのスキャン）か、固まっています。もう一度実行して、同じなら報告してください。".to_owned(),
                        stderr_tail: stderr_tail(&tail),
                    }
                }
            };
            Err(failure)
        }
    }
}

impl EngineSession {
    /// stderr の末尾
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr
            .lock()
            .map(|t| t.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// プロセスがまだ動いているか
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// 標準入力を閉じる（EOF）
    pub fn close_stdin(&mut self) {
        if let Ok(mut s) = self.stdin.lock() {
            *s = None;
        }
    }

    /// 終了を待つ。終了したら終了コード（シグナル終了なら `None` の `Some(None)`）、時間切れなら `None`
    pub fn wait_exit(&mut self, timeout: Duration) -> Option<Option<i32>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Some(status.code());
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// 強制終了する
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for EngineSession {
    fn drop(&mut self) {
        // 診断の後始末（残さない）
        self.close_stdin();
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// 診断中に持ち回るエンジン（最初に必要になったときに起動する）
#[derive(Default)]
pub struct Engines {
    main: Option<Result<EngineSession, LaunchFailure>>,
    probe: Option<Result<EngineSession, LaunchFailure>>,
}

/// 曖昧入力を有効にした診断用の設定（`docs/real-hardware-checklist.md` §9.0）
pub const PROBE_CONFIG: &str = "[input.fuzzy]\nenabled = true\nstrength = \"normal\"\n";

impl Engines {
    /// 通常設定（LLM あり）のエンジン
    pub fn main(&mut self, ctx: &Ctx) -> &mut Result<EngineSession, LaunchFailure> {
        self.main.get_or_insert_with(|| {
            launch(
                ctx,
                &LaunchOptions {
                    scratch_name: "main",
                    config_toml: None,
                    no_llm: false,
                    parent_pid: None,
                },
            )
        })
    }

    /// 曖昧入力を有効にした LLM なしのエンジン（曖昧入力・再変換の検査用）
    pub fn probe(&mut self, ctx: &Ctx) -> &mut Result<EngineSession, LaunchFailure> {
        self.probe.get_or_insert_with(|| {
            launch(
                ctx,
                &LaunchOptions {
                    scratch_name: "probe",
                    config_toml: Some(PROBE_CONFIG),
                    no_llm: true,
                    parent_pid: None,
                },
            )
        })
    }

    /// 起動済みのエンジンを順に取り出す（終了検査用）
    pub fn take_main(&mut self) -> Option<Result<EngineSession, LaunchFailure>> {
        self.main.take()
    }

    /// 起動済みの診断用エンジンを取り出す
    pub fn take_probe(&mut self) -> Option<Result<EngineSession, LaunchFailure>> {
        self.probe.take()
    }
}

fn failure_to_result(id: ItemId, f: &LaunchFailure) -> ItemResult {
    ItemResult::ng(id, f.message.clone(), f.advice.clone()).with_evidence(f.stderr_tail.clone())
}

const NOT_RUNNING: &str = "変換エンジンを起動できないため判定できません（engine.start を参照）";

/// 起動・応答
pub fn check_start(ctx: &Ctx, engines: &mut Engines) -> ItemResult {
    let id = ItemId::EngineStart;
    let session = match engines.main(ctx) {
        Ok(s) => s,
        Err(f) => return failure_to_result(id, f),
    };
    let ping = {
        let t = Instant::now();
        let r = session
            .rpc
            .call_value(method::PING, json!({}), ctx.timeouts.request);
        r.map(|_| t.elapsed().as_millis() as u64)
    };
    let status: Result<StatusResult, BackendError> = call(
        &mut session.rpc,
        method::GET_STATUS,
        json!({}),
        ctx.timeouts.request,
    );
    let init = &session.init;
    let mut lines = vec![
        format!(
            "サーバー: {} {}（プロトコル {}）",
            init.server_name, init.server_version, init.protocol_version
        ),
        format!("初期化にかかった時間: {} ms", session.init_ms),
        format!("プロファイル: {}", init.profile),
    ];
    if let Ok(s) = &status {
        lines.push(format!(
            "プロファイルの理由: {} / 変換方式: {} / LLM: {}",
            s.profile_reason,
            s.backend,
            if s.llm { "使用" } else { "不使用" }
        ));
    }
    let ping_ms = match ping {
        Ok(ms) => ms,
        Err(e) => {
            return ItemResult::ng(
                id,
                format!("起動はしましたが ping に応答しません（{}）", e.message),
                "もう一度実行して、同じなら stderr の抜粋とともに報告してください。",
            )
            .with_evidence(lines.into_iter().chain(session.stderr_tail()));
        }
    };
    lines.push(format!("ping の往復: {ping_ms} ms"));
    let version_mismatch = init.server_version != env!("CARGO_PKG_VERSION");
    if version_mismatch {
        return ItemResult::warn(
            id,
            format!(
                "起動・応答は正常ですが、imserver の版（{}）と診断プログラムの版（{}）が違います",
                init.server_version,
                env!("CARGO_PKG_VERSION")
            ),
            "更新の途中の可能性があります。サインアウトまたは再起動してから再実行してください。",
        )
        .with_evidence(lines);
    }
    if init.profile == "minimal" {
        let reason = status.as_ref().ok().map(|s| s.profile_reason.as_str());
        if reason.is_some_and(is_model_loading_reason) {
            return ItemResult::ok(
                id,
                format!(
                    "起動して応答しました（初期化 {} ms）。診断用エンジンは起動直後のため辞書のみ。実際の LLM 動作は engine.memory で確認",
                    session.init_ms
                ),
            )
            .with_evidence(lines);
        }
        return ItemResult::warn(
            id,
            "起動・応答は正常ですが、辞書のみの動作（minimal）です",
            "辞書・モデルの項目を確認してください。メモリ不足・モデルの読み込み失敗でも minimal になります。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(
        id,
        format!(
            "起動して応答しました（初期化 {} ms、プロファイル {}）",
            session.init_ms, init.profile
        ),
    )
    .with_evidence(lines)
}

/// minimal の理由が「モデルを非同期に読み込み中」か（診断の imserver は起動直後なので正常）
pub fn is_model_loading_reason(reason: &str) -> bool {
    reason.contains("変換モデルを読み込み中です")
}

/// メモリの判定。LLM あり: 専用メモリが 1.5GB 超で要確認。LLM なし: 500MB 超で要確認
/// （`CLAUDE.md` のアイドルメモリ目安 < 500MB は light の値）
pub fn judge_memory(mem: ProcMemory, llm: bool, profile: &str) -> ItemResult {
    let id = ItemId::EngineMemory;
    let ws_mb = mem.working_set_kb / 1024;
    let private_mb = mem.private_kb.map(|k| k / 1024);
    let basis_mb = private_mb.unwrap_or(ws_mb);
    let limit_mb: u64 = if llm { 1536 } else { 500 };
    let lines = vec![
        format!("ワーキングセット: {ws_mb} MB"),
        format!(
            "専用メモリ: {}",
            private_mb.map_or("取得できません".to_owned(), |m| format!("{m} MB"))
        ),
        format!(
            "プロファイル: {profile} / LLM: {}",
            if llm { "使用" } else { "不使用" }
        ),
    ];
    let detail = format!(
        "常駐メモリ {basis_mb} MB（{}、判定の目安 {limit_mb} MB）",
        if private_mb.is_some() {
            "専用メモリ"
        } else {
            "ワーキングセット"
        }
    );
    if basis_mb > limit_mb {
        return ItemResult::warn(
            id,
            format!("{detail}。目安を超えています"),
            "モデルを小さいもの（xsmall）にするか、設定でプロファイルを light にしてください。数値と環境（メモリ容量）を報告してください。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(id, detail).with_evidence(lines)
}

/// メモリ項目
pub fn check_memory(ctx: &Ctx, engines: &mut Engines) -> ItemResult {
    let id = ItemId::EngineMemory;
    let session = match engines.main(ctx) {
        Ok(s) => s,
        Err(_) => return ItemResult::skipped(id, NOT_RUNNING),
    };
    // 初期化直後の一時的な増加が落ち着くのを待つ
    std::thread::sleep(Duration::from_millis(500));
    let status: Result<StatusResult, BackendError> = call(
        &mut session.rpc,
        method::GET_STATUS,
        json!({}),
        ctx.timeouts.request,
    );
    let (llm, profile) = match &status {
        Ok(s) => (s.llm, s.profile.clone()),
        Err(_) => (false, session.init.profile.clone()),
    };
    match ctx.system.process_memory(session.pid) {
        Some(mem) => judge_memory(mem, llm, &profile),
        None => ItemResult::warn(
            id,
            "メモリ使用量を取得できませんでした",
            "タスクマネージャーで imserver.exe のメモリを目視してください。",
        ),
    }
}

/// 終了（`shutdown` と標準入力の EOF）
pub fn check_shutdown(ctx: &Ctx, engines: &mut Engines) -> ItemResult {
    let id = ItemId::EngineShutdown;
    let main = engines.take_main();
    let probe = engines.take_probe();
    if main.is_none() && probe.is_none() {
        return ItemResult::skipped(id, NOT_RUNNING);
    }
    let mut lines = Vec::new();
    let mut problems = Vec::new();
    if let Some(Ok(mut s)) = main {
        let t = Instant::now();
        let r = s
            .rpc
            .call_value(method::SHUTDOWN, json!({}), ctx.timeouts.request);
        match (r, s.wait_exit(ctx.timeouts.exit)) {
            (_, Some(Some(0))) => lines.push(format!(
                "shutdown 要求で終了しました（{} ms、終了コード 0）",
                t.elapsed().as_millis()
            )),
            (_, Some(code)) => {
                lines.push(format!(
                    "shutdown 要求で終了しましたが、{}",
                    describe_exit_code(code)
                ));
                problems.push("終了コードが 0 ではありません".to_owned());
            }
            (_, None) => {
                lines.push("shutdown 要求を送っても終了しませんでした".to_owned());
                problems.push("shutdown で終了しません".to_owned());
                s.kill();
            }
        }
    }
    if let Some(Ok(mut s)) = probe {
        let t = Instant::now();
        s.close_stdin();
        match s.wait_exit(ctx.timeouts.exit) {
            Some(Some(0)) => lines.push(format!(
                "標準入力を閉じる（EOF）と終了しました（{} ms、終了コード 0）",
                t.elapsed().as_millis()
            )),
            Some(code) => {
                lines.push(format!(
                    "EOF で終了しましたが、{}",
                    describe_exit_code(code)
                ));
                problems.push("EOF での終了コードが 0 ではありません".to_owned());
            }
            None => {
                lines.push("標準入力を閉じても終了しませんでした".to_owned());
                problems.push("EOF で終了しません".to_owned());
                s.kill();
            }
        }
    }
    if lines.is_empty() {
        return ItemResult::skipped(id, NOT_RUNNING);
    }
    if problems.is_empty() {
        return ItemResult::ok(
            id,
            "shutdown 要求と標準入力の EOF のどちらでも正常に終了しました",
        )
        .with_evidence(lines);
    }
    ItemResult::ng(
        id,
        problems.join(" / "),
        "ホストアプリを閉じても imserver が残る原因になります。タスクマネージャーに imserver.exe が残っていれば終了してから、報告してください。",
    )
    .with_evidence(lines)
}

/// 親プロセスの消滅での終了（Windows のみ。`--parent-pid`、H-96）
pub fn check_parent_exit(ctx: &Ctx) -> ItemResult {
    let id = ItemId::EngineParentExit;
    if !ctx.is_windows() {
        return ItemResult::skipped(id, "親プロセスの監視（--parent-pid）は Windows のみです");
    }
    let Some(mut sleeper) = ctx.system.spawn_sleeper() else {
        return ItemResult::warn(
            id,
            "検証用の親プロセスを起動できませんでした",
            "もう一度実行してください。",
        );
    };
    let mut session = match launch(
        ctx,
        &LaunchOptions {
            scratch_name: "parent",
            config_toml: None,
            no_llm: true,
            parent_pid: Some(sleeper.pid()),
        },
    ) {
        Ok(s) => s,
        Err(f) => return failure_to_result(id, &f),
    };
    let t = Instant::now();
    sleeper.kill();
    // 標準入力は開いたまま。終了すれば親の監視（--parent-pid）が効いている
    match session.wait_exit(ctx.timeouts.exit) {
        Some(_) => ItemResult::ok(
            id,
            format!(
                "親プロセスが消えてから {} ms でエンジンが終了しました",
                t.elapsed().as_millis()
            ),
        ),
        None => {
            session.kill();
            ItemResult::ng(
                id,
                format!(
                    "親プロセスが消えても {} 秒以内にエンジンが終了しませんでした",
                    ctx.timeouts.exit.as_secs()
                ),
                "ホストアプリを強制終了すると imserver.exe が残ります（メモリを消費し続けます）。タスクマネージャーで終了し、報告してください。",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::types::Status;

    #[test]
    fn exit_codes_are_described() {
        assert!(describe_exit_code(Some(0xC000_0135_u32 as i32)).contains("DLL が見つからない"));
        assert!(describe_exit_code(Some(0xC000_007B_u32 as i32)).contains("CPU"));
        assert!(describe_exit_code(Some(3)).contains("終了コード 3"));
        assert!(describe_exit_code(None).contains("シグナル"));
    }

    #[test]
    fn spawn_errors_are_described() {
        let nf = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(
            describe_spawn_error(&nf, Path::new("x"))
                .message
                .contains("見つかりません")
        );
        let bad = std::io::Error::from_raw_os_error(193);
        assert!(
            describe_spawn_error(&bad, Path::new("x"))
                .message
                .contains("CPU")
        );
    }

    #[test]
    fn memory_judgement() {
        let mem = |ws_mb: u64, private_mb: Option<u64>| ProcMemory {
            working_set_kb: ws_mb * 1024,
            private_kb: private_mb.map(|m| m * 1024),
        };
        assert_eq!(
            judge_memory(mem(300, Some(250)), false, "light").status,
            Status::Ok
        );
        assert_eq!(
            judge_memory(mem(900, Some(700)), false, "light").status,
            Status::Warn
        );
        // LLM ありは専用メモリで判定する（mmap したモデルはワーキングセットに載る）
        assert_eq!(
            judge_memory(mem(3000, Some(800)), true, "balanced").status,
            Status::Ok
        );
        assert_eq!(
            judge_memory(mem(3000, Some(2000)), true, "balanced").status,
            Status::Warn
        );
        assert_eq!(
            judge_memory(mem(1000, None), true, "balanced").status,
            Status::Ok
        );
        assert!(
            judge_memory(mem(10, Some(5)), true, "x")
                .detail
                .contains("5 MB")
        );
    }

    #[test]
    fn launch_reports_missing_imserver() {
        let ctx = crate::diag::testutil::ctx_with_imserver("/definitely/not/here/imserver");
        let mut engines = Engines::default();
        let r = check_start(&ctx, &mut engines);
        assert_eq!(r.status, Status::Ng);
        assert!(r.detail.contains("見つかりません"));
        // 後続の項目は起動できないことを理由にスキップする
        assert_eq!(check_memory(&ctx, &mut engines).status, Status::Skipped);
        assert_eq!(check_shutdown(&ctx, &mut engines).status, Status::Skipped);
    }

    #[test]
    fn parent_exit_is_skipped_off_windows() {
        let ctx = crate::diag::testutil::ctx_with_imserver("/x/imserver");
        if !ctx.is_windows() {
            assert_eq!(check_parent_exit(&ctx).status, Status::Skipped);
        }
    }
}
