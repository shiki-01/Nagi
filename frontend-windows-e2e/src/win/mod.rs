//! Windows 実装: テストホストのウィンドウ・SendInput・IME の登録と有効化・スクリーンショット。
//! 手順の解釈は `runner`、ここは OS を叩くだけ。`unsafe` はこのモジュール配下に隔離する。
//!
//! 実際の Windows（GitHub ホストランナー）での動作は未検証。`.github/workflows/windows-e2e.yml` 参照。

#![allow(unsafe_code)]

mod capture;
mod host;
mod ime;
mod input;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    TranslateMessage,
};

use crate::cli::Args;
use crate::keys;
use crate::probe::{ProbeReader, committed_text};
use crate::runner::{self, Artifacts, Host, Observation, ResultEntry};
use crate::scenario;

/// メッセージを処理しながら `ms` ミリ秒待つ（0 なら溜まっている分だけ処理して戻る）。
/// SendInput のキーは自分のスレッドのメッセージ処理の中で TSF（IME）に渡るため、待つときは必ずこれを使う
pub fn pump(ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            let _ = unsafe { TranslateMessage(&msg) };
            unsafe { DispatchMessageW(&msg) };
        }
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let slice = u32::try_from((deadline - now).as_millis().min(10)).unwrap_or(10);
        unsafe { MsgWaitForMultipleObjects(None, false, slice, QS_ALLINPUT) };
    }
}

/// テストホスト（EDIT を持つウィンドウ）を操作する [`Host`] 実装
struct WinHost {
    window: host::HostWindow,
    edit: HWND,
    probe: ProbeReader,
    ime: ime::ImeSession,
}

/// キーを 1 イベントずつ送って IME に処理させる間隔（ミリ秒）
const EVENT_GAP_MS: u64 = 4;
/// 1 キー（押下〜離上）のあとに待つ時間（ミリ秒）。エンジンへの同期要求の往復を待つ
const KEY_GAP_MS: u64 = 20;

impl WinHost {
    fn send_key(&mut self, spec: &keys::KeySpec) -> Result<(), String> {
        for ev in input::key_events(spec) {
            input::send(ev)?;
            pump(EVENT_GAP_MS);
        }
        pump(KEY_GAP_MS);
        Ok(())
    }

    fn read_observation(&mut self) -> Observation {
        pump(0);
        self.probe.poll();
        let marked = self.probe.marked().to_owned();
        let full = host::edit_text(self.edit);
        Observation {
            committed: committed_text(&full, &marked),
            marked,
        }
    }

    /// 前のシナリオの未確定を取り消す（Escape を数回）
    fn cancel_composition(&mut self) -> Result<(), String> {
        for _ in 0..4 {
            if self.read_observation().marked.is_empty() {
                return Ok(());
            }
            self.press("Escape")?;
            pump(150);
        }
        if self.read_observation().marked.is_empty() {
            Ok(())
        } else {
            Err("前のシナリオの未確定文字列を取り消せません".to_owned())
        }
    }

    /// 診断のまとめ（フォーカス・モード・送出方式・プローブの末尾・tsf.log の末尾）
    fn snapshot(&self) -> String {
        format!(
            "{}\nime: {}\nmodes: {}\ninput_mode={}\nedit_text={:?}\nprobe_lines={} committed_chars={}\n--- probe 末尾 ---\n{}\n{}",
            host::describe_focus(&self.window),
            self.ime.describe(),
            self.ime.describe_modes(),
            input::mode_name(),
            host::edit_text(self.edit),
            self.probe.line_count(),
            self.probe.committed_total().chars().count(),
            self.probe.tail(),
            tsf_log_tail()
        )
    }

    /// IME が実際にキーを処理できる（imserver に接続済み）まで待つ。
    /// エンジンが未接続の間は素通しになるため、「a → あ」が出るまで試す。
    /// 反応が無いときは、原因の切り分けを兼ねて対処を順に変える:
    /// 1 回目 純スキャンコード → 2 回目以降 仮想キー併用に切り替え、毎回モードの設定・前面化・フォーカスの付け直しをする
    fn wait_ready(&mut self, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        let mut round = 0u32;
        loop {
            input::set_vk_mode(round % 2 == 1);
            // IME を有効にした後にモードを書く（Activate 時点では TIP が未設定のことがある）
            let changed = self.ime.ensure_hiragana().unwrap_or(false);
            if round > 0 {
                let _ = host::bring_to_front(&self.window);
            }
            host::refocus(&self.window);
            println!(
                "[ready round={round} {}] modes_changed={changed} {}",
                input::mode_name(),
                host::describe_focus(&self.window)
            );
            self.type_text("a")?;
            pump(400);
            let obs = self.read_observation();
            println!(
                "[ready round={round}] marked={:?} edit_text={:?} probe_lines={}",
                obs.marked,
                host::edit_text(self.edit),
                self.probe.line_count()
            );
            if obs.marked == "あ" {
                self.press("Escape")?;
                pump(200);
                input::set_vk_mode(false);
                return self.begin_scenario();
            }
            // 素通しで EDIT に入った分や、途中の状態を片付けて再試行
            let _ = self.press("Escape");
            host::set_edit_text(self.edit, "");
            if start.elapsed() >= timeout {
                return Err(format!(
                    "IME が応答しません（{}ms 待った、{round} 回試行）\n{}",
                    timeout.as_millis(),
                    self.snapshot()
                ));
            }
            round += 1;
            pump(500);
        }
    }
}

impl Host for WinHost {
    fn begin_scenario(&mut self) -> Result<(), String> {
        self.cancel_composition()?;
        host::set_edit_text(self.edit, "");
        host::focus(self.edit);
        if self.ime.ensure_hiragana()? {
            pump(200);
        }
        pump(50);
        self.read_observation();
        self.probe.reset();
        Ok(())
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        for c in text.chars() {
            let (stroke, shift) = keys::char_stroke(c)
                .ok_or_else(|| format!("US 配列で送れない文字: {c:?}（type は ASCII のみ）"))?;
            let spec = keys::KeySpec {
                modifiers: if shift { vec![keys::SHIFT] } else { vec![] },
                key: stroke,
            };
            self.send_key(&spec)?;
        }
        Ok(())
    }

    fn press(&mut self, spec: &str) -> Result<(), String> {
        let spec = keys::parse_key(spec)?;
        self.send_key(&spec)
    }

    fn observe(&mut self) -> Result<Observation, String> {
        Ok(self.read_observation())
    }

    fn idle(&mut self, ms: u64) {
        pump(ms);
    }

    fn screenshot(&mut self, path: &Path) -> Result<(), String> {
        pump(50);
        capture::save_screen_png(path)
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// `%LOCALAPPDATA%\Nagi\logs\tsf.log` の末尾（診断用。入力内容は書かれない）
fn tsf_log_tail() -> String {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return "(LOCALAPPDATA 未設定)".to_owned();
    };
    let path = PathBuf::from(base)
        .join("Nagi")
        .join("logs")
        .join("tsf.log");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let lines: Vec<_> = text.lines().collect();
            format!(
                "--- {} (末尾) ---\n{}",
                path.display(),
                lines[lines.len().saturating_sub(40)..].join("\n")
            )
        }
        Err(_) => format!("(tsf.log がありません: {})", path.display()),
    }
}

fn print_tsf_log_tail() {
    eprintln!("{}", tsf_log_tail());
}

fn write_results(out: &Path, results: &[ResultEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(results).map_err(|e| e.to_string())?;
    std::fs::write(out.join("results.json"), json).map_err(|e| e.to_string())
}

/// 実行する。戻り値は終了コード（0: 全て成功 / 1: 失敗あり / 2: 環境を用意できなかった）
pub fn run(args: &Args) -> i32 {
    match run_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("nagi-e2e-win: {e}");
            print_tsf_log_tail();
            2
        }
    }
}

fn run_inner(args: &Args) -> Result<i32, String> {
    let suites = scenario::load_dir(&args.scenarios)?;
    let shots = args.out.join("screenshots");
    std::fs::create_dir_all(&shots).map_err(|e| format!("出力先を作れません: {e}"))?;

    // 作業ディレクトリ: user-dir のコピー（NAGI_CONFIG_DIR）・プローブ・空の APPDATA
    let work = std::env::temp_dir().join(format!("nagi-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let user_dir = work.join("user-dir");
    copy_dir(&args.fixtures, &user_dir).map_err(|e| {
        format!(
            "フィクスチャをコピーできません: {}: {e}",
            args.fixtures.display()
        )
    })?;
    let appdata = work.join("appdata");
    std::fs::create_dir_all(&appdata).map_err(|e| e.to_string())?;
    // TSF は data-dir を %APPDATA%\Nagi\data として imserver へ渡す（`engine_location`）。
    // ユーザー辞書の重ね合わせにはシステム辞書が要るため、最小辞書を置く
    if let Some(data) = &args.data_dir {
        copy_dir(data, &appdata.join("Nagi").join("data"))
            .map_err(|e| format!("data-dir をコピーできません: {}: {e}", data.display()))?;
    }
    let probe_path = work.join("probe.jsonl");
    std::fs::write(&probe_path, "").map_err(|e| e.to_string())?;

    // IME（DLL）を読み込むのは自プロセス。環境変数は DLL・imserver（子プロセスは継承）が読む前に、
    // 他のスレッドを立てる前のここで設定する。APPDATA を空にして、システム辞書・モデルを使わない
    // （%APPDATA%\Nagi\data が無ければ辞書なし・LLM なしで決定的に動く）
    unsafe {
        std::env::set_var("NAGI_CONFIG_DIR", &user_dir);
        std::env::set_var("NAGI_E2E_PROBE", &probe_path);
        std::env::set_var("APPDATA", &appdata);
    }

    let dll = std::fs::canonicalize(&args.dll)
        .map_err(|e| format!("DLL が見つかりません: {}: {e}", args.dll.display()))?;
    // canonicalize の `\\?\` 付きパスは regsvr32 に渡せない場合がある
    let dll = PathBuf::from(dll.to_string_lossy().trim_start_matches(r"\\?\"));
    if !args.skip_register {
        ime::register(&dll)?;
    }
    let result = run_registered(args, &suites, &probe_path, &shots);
    if !args.skip_register
        && !args.keep_registered
        && let Err(e) = ime::unregister(&dll)
    {
        eprintln!("nagi-e2e-win: 登録の解除に失敗: {e}");
    }
    let _ = std::fs::remove_dir_all(&work);
    let results = match result {
        Ok(r) => r,
        Err(e) => {
            // セットアップの失敗も results.json に残す（診断は標準エラーにも出る）
            let entry = ResultEntry {
                suite: "setup".to_owned(),
                name: "ime-activation".to_owned(),
                ok: false,
                message: e.clone(),
            };
            let _ = write_results(&args.out, &[entry]);
            return Err(e);
        }
    };
    write_results(&args.out, &results)?;
    let failed = results.iter().filter(|r| !r.ok).count();
    println!(
        "{} 件中 {} 件成功、{} 件失敗",
        results.len(),
        results.len() - failed,
        failed
    );
    // 実行したシナリオが 0 件のときは失敗扱い（シナリオの置き場所の取り違えを見逃さない）
    Ok(i32::from(failed > 0 || results.is_empty()))
}

fn run_registered(
    args: &Args,
    suites: &[scenario::Suite],
    probe_path: &Path,
    shots: &Path,
) -> Result<Vec<ResultEntry>, String> {
    let window = host::create()?;
    host::bring_to_front(&window)?;
    let ime = ime::ImeSession::activate()?;
    let mut host = WinHost {
        window,
        edit: window.edit,
        probe: ProbeReader::new(probe_path),
        ime,
    };
    host.wait_ready(Duration::from_millis(args.ready_timeout_ms))
        .inspect_err(|_| {
            let _ = capture::save_screen_png(&shots.join("FAIL__ime-not-ready.png"));
        })?;
    let artifacts = Artifacts {
        dir: shots.to_path_buf(),
    };
    Ok(runner::run_all(
        &mut host,
        suites,
        "windows",
        args.filter.as_deref(),
        &artifacts,
    ))
}
