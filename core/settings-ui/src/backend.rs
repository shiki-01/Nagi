//! コア（`imserver`）との通信。stdio の JSON-RPC 2.0（改行区切り）。
//!
//! 画面側は [`SettingsBackend`] だけを見る。テストではフェイクを差し込む。

use nagi_protocol::jsonrpc::error_code;
use nagi_protocol::methods::{InitializeParams, StatusResult, method};
use nagi_protocol::settings::{
    SettingChange, SettingsSnapshot, UpdateSettingsParams, UpdateSettingsResult,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// バックエンドの失敗
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    /// JSON-RPC のエラーコード（通信の失敗など、コアが返したエラーでなければ `None`）
    pub code: Option<i32>,
    /// 利用者に見せてよいメッセージ（日本語。設定値は含まない）
    pub message: String,
}

impl BackendError {
    /// コアが返したエラー
    pub fn rpc(code: i32, message: impl Into<String>) -> Self {
        Self {
            code: Some(code),
            message: message.into(),
        }
    }

    /// 通信などの失敗
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }

    /// 値の検証エラー（`INVALID_PARAMS`）か
    pub fn is_invalid_params(&self) -> bool {
        self.code == Some(error_code::INVALID_PARAMS)
    }
}

/// 更新確認の結果の種別（`imserver --check-update` の `status`。`docs/decisions.md` H-100）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    /// 最新版を使っている
    UpToDate,
    /// 新しい版がある（取得していない）
    Available,
    /// 新しい版を取得・検証して保存済み（適用できる）
    Staged,
    /// 確認または取得に失敗した
    Error,
}

/// 更新確認の結果（`imserver --check-update` が標準出力に出す 1 行の JSON と同じ形）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheck {
    /// 現在の版
    pub current: String,
    /// マニフェストの最新版（取得できなければ `None`）
    pub latest: Option<String>,
    /// 結果の種別
    pub status: UpdateStatus,
    /// リリースノートの URL
    pub notes_url: Option<String>,
    /// 失敗の理由（日本語）
    pub error: Option<String>,
}

/// 設定画面が使うコアの操作
pub trait SettingsBackend {
    /// 設定項目と現在値
    fn get_settings(&mut self) -> Result<SettingsSnapshot, BackendError>;
    /// 設定の書き込み（検証はコア）
    fn update_settings(
        &mut self,
        changes: Vec<SettingChange>,
    ) -> Result<UpdateSettingsResult, BackendError>;
    /// 学習データの全削除
    fn clear_learning(&mut self) -> Result<(), BackendError>;
    /// 設定ファイル（`config.toml`）の場所。決まっていなければ `None`
    fn config_path(&mut self) -> Result<Option<String>, BackendError>;
    /// 更新を確認する（間引きなしの一回限りの動作。新版があれば取得・署名検証まで行う）
    fn check_update(&mut self) -> Result<UpdateCheck, BackendError>;
    /// 取得済みの更新を適用する（`apply-update.ps1` を新しいコンソールで起動する固定の動作。
    /// 引数・パスは呼び出し側から受け取らない。昇格（UAC）はスクリプトが行う）
    fn apply_update(&mut self) -> Result<(), BackendError>;
}

impl SettingsBackend for Box<dyn SettingsBackend> {
    fn get_settings(&mut self) -> Result<SettingsSnapshot, BackendError> {
        (**self).get_settings()
    }

    fn update_settings(
        &mut self,
        changes: Vec<SettingChange>,
    ) -> Result<UpdateSettingsResult, BackendError> {
        (**self).update_settings(changes)
    }

    fn clear_learning(&mut self) -> Result<(), BackendError> {
        (**self).clear_learning()
    }

    fn config_path(&mut self) -> Result<Option<String>, BackendError> {
        (**self).config_path()
    }

    fn check_update(&mut self) -> Result<UpdateCheck, BackendError> {
        (**self).check_update()
    }

    fn apply_update(&mut self) -> Result<(), BackendError> {
        (**self).apply_update()
    }
}

/// 改行区切りの JSON-RPC クライアント（トランスポートは注入）
pub struct RpcClient {
    writer: Box<dyn Write + Send>,
    lines: Receiver<String>,
    next_id: u64,
}

impl RpcClient {
    /// 書き込み先と、応答の行を受け取るチャネルから作る
    pub fn new(writer: Box<dyn Write + Send>, lines: Receiver<String>) -> Self {
        Self {
            writer,
            lines,
            next_id: 1,
        }
    }

    /// 要求を送り、同じ `id` の応答を待つ。途中の通知（`id` なし）や他の `id` の行は読み捨てる
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, BackendError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.writer, "{request}")
            .and_then(|()| self.writer.flush())
            .map_err(|_| BackendError::transport("設定エンジンに要求を送れません"))?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(remaining) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(BackendError::transport("設定エンジンの応答がありません"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(BackendError::transport("設定エンジンが終了しました"));
                }
            };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let code = error
                    .get("code")
                    .and_then(Value::as_i64)
                    .and_then(|c| i32::try_from(c).ok())
                    .unwrap_or(error_code::INTERNAL_ERROR);
                let text = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("設定エンジンがエラーを返しました");
                return Err(BackendError::rpc(code, text));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// `request` の結果を型に変換する
    pub fn call<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<T, BackendError> {
        let value = self.request(method, params, timeout)?;
        serde_json::from_value(value)
            .map_err(|_| BackendError::transport("設定エンジンの応答を解釈できません"))
    }

    /// 書き込み先を閉じる（子プロセスの標準入力を EOF にする）
    fn close_writer(&mut self) {
        self.writer = Box::new(std::io::sink());
    }
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// 起動直後の `initialize` は辞書の読み込みを待つことがある
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

fn settings_of(client: &mut RpcClient) -> Result<SettingsSnapshot, BackendError> {
    client.call(method::GET_SETTINGS, json!({}), REQUEST_TIMEOUT)
}

/// `imserver` の起動条件
#[derive(Debug, Clone)]
pub struct ImserverSpec {
    /// `imserver` の実行ファイル
    pub exe: PathBuf,
    /// `--data-dir`（`None` なら渡さない。`NAGI_DATA_DIR` があれば imserver が使う）
    pub data_dir: Option<PathBuf>,
    /// `--user-dir`（`None` なら渡さない。imserver が `NAGI_CONFIG_DIR` / OS の既定から決める）
    pub user_dir: Option<PathBuf>,
    /// 親（この設定画面）の PID。落ちたら imserver も終了する（Windows のみ有効）
    pub parent_pid: u32,
    /// 更新の保存先（`--update-dir`。Windows は `%LOCALAPPDATA%\Nagi\update`。`None` なら imserver の既定）
    pub update_dir: Option<PathBuf>,
    /// `apply-update.ps1` の場所（この設定画面の exe と同じディレクトリ）。`None` なら適用できない
    pub apply_script: Option<PathBuf>,
}

impl ImserverSpec {
    /// 起動引数（設定画面は変換しないので、常に `--no-llm`）
    pub fn arguments(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(dir) = &self.data_dir {
            args.push("--data-dir".to_owned());
            args.push(dir.to_string_lossy().into_owned());
        }
        if let Some(dir) = &self.user_dir {
            args.push("--user-dir".to_owned());
            args.push(dir.to_string_lossy().into_owned());
        }
        args.push("--no-llm".to_owned());
        args.push("--parent-pid".to_owned());
        args.push(self.parent_pid.to_string());
        args
    }

    /// 更新確認（`imserver --check-update`）の起動引数。新版があれば取得・検証・保存まで行う
    /// （`--stage-updates`。`--no-llm` は不要: JSON-RPC を起動しない一回限りの動作）
    pub fn check_update_arguments(&self) -> Vec<String> {
        let mut args = vec!["--check-update".to_owned(), "--stage-updates".to_owned()];
        if let Some(dir) = &self.update_dir {
            args.push("--update-dir".to_owned());
            args.push(dir.to_string_lossy().into_owned());
        }
        if let Some(dir) = &self.user_dir {
            args.push("--user-dir".to_owned());
            args.push(dir.to_string_lossy().into_owned());
        }
        if let Some(dir) = &self.data_dir {
            args.push("--data-dir".to_owned());
            args.push(dir.to_string_lossy().into_owned());
        }
        args.push("--parent-pid".to_owned());
        args.push(self.parent_pid.to_string());
        args
    }
}

/// 更新確認の最長時間（マニフェストの取得 + 配布物の取得・検証）
const CHECK_UPDATE_TIMEOUT: Duration = Duration::from_secs(60);

/// 標準出力の最後の空でない行を [`UpdateCheck`] として読む
pub fn parse_update_output(stdout: &str) -> Result<UpdateCheck, BackendError> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| BackendError::transport("更新の確認結果を取得できませんでした"))?;
    serde_json::from_str(line)
        .map_err(|_| BackendError::transport("更新の確認結果を解釈できませんでした"))
}

/// `imserver --check-update` を子プロセスとして実行し、結果を読む（タイムアウトなら強制終了）
fn run_check_update(spec: &ImserverSpec, timeout: Duration) -> Result<UpdateCheck, BackendError> {
    let mut command = Command::new(&spec.exe);
    command
        .args(spec.check_update_arguments())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    no_window(&mut command);
    let mut child = command.spawn().map_err(|e| {
        BackendError::transport(format!(
            "更新の確認を起動できません（{}）: {}",
            e.kind(),
            spec.exe.display()
        ))
    })?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Err(BackendError::transport("更新の確認の出力を開けません"));
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        let _ = tx.send(text);
    });
    match rx.recv_timeout(timeout) {
        Ok(text) => {
            let _ = child.wait();
            parse_update_output(&text)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(BackendError::transport(
                "更新の確認が時間内に終わりませんでした。ネットワークの状態を確認して、もう一度お試しください",
            ))
        }
    }
}

/// コンソールアプリの子プロセスの黒い窓を出さない（Windows のみ）
fn no_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// `apply-update.ps1` を起動する PowerShell の引数（実行後もコンソールを閉じない。
/// `frontend-windows` の `apply_update_arguments` と同じ。昇格はスクリプトが行う）
pub fn apply_update_arguments(script: &std::path::Path) -> Vec<String> {
    vec![
        "-NoProfile".to_owned(),
        "-ExecutionPolicy".to_owned(),
        "Bypass".to_owned(),
        "-NoExit".to_owned(),
        "-File".to_owned(),
        script.to_string_lossy().into_owned(),
    ]
}

/// `apply-update.ps1` を新しいコンソールで起動する（待たない）。Windows 以外では使えない
fn launch_apply_script(script: &std::path::Path) -> Result<(), BackendError> {
    if !cfg!(windows) {
        return Err(BackendError::transport(
            "更新の適用は Windows でのみ行えます",
        ));
    }
    if !script.is_file() {
        return Err(BackendError::transport(
            "apply-update.ps1 が見つかりません（Nagi を入れ直してください）",
        ));
    }
    let mut command = Command::new("powershell.exe");
    command
        .args(apply_update_arguments(script))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 新しいコンソールで起動する（スクリプトの進捗・UAC の結果をそこで見せる）
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    let mut child = command.spawn().map_err(|e| {
        BackendError::transport(format!("更新の適用を起動できません（{}）", e.kind()))
    })?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// 子プロセスの `imserver` を相手にするバックエンド
pub struct ImserverBackend {
    client: RpcClient,
    child: Child,
    spec: ImserverSpec,
}

impl ImserverBackend {
    /// `imserver` を起動して `initialize` まで済ませる
    pub fn spawn(spec: &ImserverSpec) -> Result<Self, BackendError> {
        let mut command = Command::new(&spec.exe);
        command
            .args(spec.arguments())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // GUI アプリから起動するので、imserver（コンソールアプリ）の黒い窓を出さない
        no_window(&mut command);
        let mut child = command.spawn().map_err(|e| {
            BackendError::transport(format!(
                "設定エンジン（imserver）を起動できません（{}）: {}",
                e.kind(),
                spec.exe.display()
            ))
        })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(BackendError::transport("設定エンジンの入出力を開けません"));
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut backend = Self {
            client: RpcClient::new(Box::new(stdin), rx),
            child,
            spec: spec.clone(),
        };
        let params = InitializeParams {
            protocol_version: nagi_protocol::PROTOCOL_VERSION,
            client_name: "nagi-settings".to_owned(),
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            os: std::env::consts::OS.to_owned(),
            capabilities: None,
        };
        let params = serde_json::to_value(params)
            .map_err(|_| BackendError::transport("initialize を組み立てられません"))?;
        backend
            .client
            .request(method::INITIALIZE, params, INITIALIZE_TIMEOUT)?;
        Ok(backend)
    }

    /// 終了を頼み、少し待ってから、残っていれば強制終了する
    fn shutdown(&mut self) {
        let _ = self
            .client
            .request(method::SHUTDOWN, json!({}), Duration::from_secs(2));
        self.client.close_writer();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ImserverBackend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl SettingsBackend for ImserverBackend {
    fn get_settings(&mut self) -> Result<SettingsSnapshot, BackendError> {
        settings_of(&mut self.client)
    }

    fn update_settings(
        &mut self,
        changes: Vec<SettingChange>,
    ) -> Result<UpdateSettingsResult, BackendError> {
        let params = serde_json::to_value(UpdateSettingsParams { changes })
            .map_err(|_| BackendError::transport("変更を組み立てられません"))?;
        self.client
            .call(method::UPDATE_SETTINGS, params, REQUEST_TIMEOUT)
    }

    fn clear_learning(&mut self) -> Result<(), BackendError> {
        self.client
            .request(method::CLEAR_LEARNING, json!({}), REQUEST_TIMEOUT)
            .map(|_| ())
    }

    fn config_path(&mut self) -> Result<Option<String>, BackendError> {
        let status: StatusResult =
            self.client
                .call(method::GET_STATUS, json!({}), REQUEST_TIMEOUT)?;
        Ok(status.config_path.filter(|p| !p.is_empty()))
    }

    fn check_update(&mut self) -> Result<UpdateCheck, BackendError> {
        run_check_update(&self.spec, CHECK_UPDATE_TIMEOUT)
    }

    fn apply_update(&mut self) -> Result<(), BackendError> {
        let script = self
            .spec
            .apply_script
            .as_deref()
            .ok_or_else(|| BackendError::transport("apply-update.ps1 の場所が分かりません"))?;
        launch_apply_script(script)
    }
}

/// 起動に失敗したときのバックエンド。画面は開き、失敗の理由を表示する
pub struct FailedBackend {
    /// 表示する理由
    pub error: BackendError,
}

impl SettingsBackend for FailedBackend {
    fn get_settings(&mut self) -> Result<SettingsSnapshot, BackendError> {
        Err(self.error.clone())
    }

    fn update_settings(
        &mut self,
        _changes: Vec<SettingChange>,
    ) -> Result<UpdateSettingsResult, BackendError> {
        Err(self.error.clone())
    }

    fn clear_learning(&mut self) -> Result<(), BackendError> {
        Err(self.error.clone())
    }

    fn config_path(&mut self) -> Result<Option<String>, BackendError> {
        Err(self.error.clone())
    }

    fn check_update(&mut self) -> Result<UpdateCheck, BackendError> {
        Err(self.error.clone())
    }

    fn apply_update(&mut self) -> Result<(), BackendError> {
        Err(self.error.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn client_with(lines: &[&str]) -> (RpcClient, Sink) {
        let (tx, rx) = mpsc::channel();
        for l in lines {
            tx.send((*l).to_owned()).unwrap();
        }
        // tx を保持しない: 読み切ったら Disconnected になる
        let sink = Sink::default();
        (RpcClient::new(Box::new(sink.clone()), rx), sink)
    }

    #[test]
    fn request_skips_notifications_and_matches_the_id() {
        let (mut c, sink) = client_with(&[
            r#"{"jsonrpc":"2.0","method":"notify","params":{}}"#,
            "not json",
            r#"{"jsonrpc":"2.0","id":99,"result":"other"}"#,
            r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#,
        ]);
        let r = c
            .request("ping", json!({}), Duration::from_secs(1))
            .unwrap();
        assert_eq!(r, json!({"ok": true}));
        let sent = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(sent.ends_with('\n'));
        let v: Value = serde_json::from_str(sent.trim()).unwrap();
        assert_eq!(v["method"], "ping");
        assert_eq!(v["id"], 1);
    }

    #[test]
    fn rpc_error_keeps_code_and_message() {
        let (mut c, _) = client_with(&[
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"範囲外です"}}"#,
        ]);
        let e = c
            .request("update_settings", json!({}), Duration::from_secs(1))
            .unwrap_err();
        assert!(e.is_invalid_params());
        assert_eq!(e.message, "範囲外です");
    }

    #[test]
    fn closed_channel_is_a_transport_error() {
        let (mut c, _) = client_with(&[]);
        let e = c
            .request("ping", json!({}), Duration::from_secs(1))
            .unwrap_err();
        assert_eq!(e.code, None);
    }

    #[test]
    fn spawn_arguments_pass_dirs_no_llm_and_parent() {
        let spec = ImserverSpec {
            exe: PathBuf::from("imserver"),
            data_dir: Some(PathBuf::from("/d")),
            user_dir: None,
            parent_pid: 42,
            update_dir: Some(PathBuf::from("/up")),
            apply_script: None,
        };
        assert_eq!(
            spec.arguments(),
            ["--data-dir", "/d", "--no-llm", "--parent-pid", "42"]
        );
        assert_eq!(
            spec.check_update_arguments(),
            [
                "--check-update",
                "--stage-updates",
                "--update-dir",
                "/up",
                "--data-dir",
                "/d",
                "--parent-pid",
                "42"
            ]
        );
    }

    #[test]
    fn update_output_uses_the_last_json_line() {
        let ok = r#"{"current":"0.1.14","latest":"0.1.15","status":"staged","notes_url":null,"error":null}"#;
        let r = parse_update_output(&format!("noise\n{ok}\n\n")).unwrap();
        assert_eq!(r.status, UpdateStatus::Staged);
        assert_eq!(r.latest.as_deref(), Some("0.1.15"));
        assert!(parse_update_output("").is_err());
        assert!(parse_update_output("not json").is_err());
        // 未知の status は受け付けない
        assert!(
            parse_update_output(
                r#"{"current":"1","latest":null,"status":"weird","notes_url":null,"error":null}"#
            )
            .is_err()
        );
    }

    #[test]
    fn apply_arguments_are_fixed_and_keep_the_console() {
        assert_eq!(
            apply_update_arguments(std::path::Path::new("C:/p/apply-update.ps1")),
            [
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-NoExit",
                "-File",
                "C:/p/apply-update.ps1"
            ]
        );
    }
}
