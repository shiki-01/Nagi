//! `nagi-settings`: 設定画面を起動する。
//!
//! 使い方: `nagi-settings [--imserver <path>] [--data-dir <dir>] [--user-dir <dir>] [--web | --native | --webview]`。
//! Windows の既定は WebView2 のウィンドウ（ローカルの HTTP サーバーの Web 画面を表示。閉じると終了。2 つ目の起動は
//! 既存のウィンドウを前面に出す）。WebView2 が使えない・初期化に失敗したときはネイティブの Win32 ウィンドウで開く。
//! `--native`: ネイティブの Win32 ウィンドウに固定。`--webview`: WebView2 に固定（使えなければエラー終了）。
//! `--web`（Windows 以外では常に）: ローカルの HTTP サーバー + 既定ブラウザの Web 画面（ブラウザのタブを閉じるか、
//! 15 分間操作が無ければ終了する）。`NAGI_SETTINGS_DEVTOOLS=1`: WebView2 の DevTools を有効にする（検証用）。
//! 設定画面が使う `imserver` は `--no-llm` の別プロセス（親の PID を渡し、親が落ちたら止まる）。
//! `--section update --auto-check`: 更新ページを開き、更新の確認を自動で実行する（右クリックメニュー・スタートメニューの「更新を確認」）。
//! `--diag`: 画面を開かず実機の診断を実行する（`--diag-item <id>`・`--diag-out <dir>`・`--diag-json`・`--no-network`）。
//! 検証用（`--web` のときだけ）: `--no-browser`（ブラウザを開かない）、`--url-file <path>`（URL をファイルに書く）。

// Windows ではコンソールを出さない（言語バーのメニューから起動される GUI 起点のアプリ）
#![cfg_attr(windows, windows_subsystem = "windows")]

use nagi_settings_ui::backend::{FailedBackend, ImserverBackend, ImserverSpec, SettingsBackend};
use nagi_settings_ui::cli::{
    Args, default_apply_script, default_data_dir, default_imserver, default_update_dir, parse_args,
};
use nagi_settings_ui::http::App;
use nagi_settings_ui::opener::{Opener, SystemOpener};
use nagi_settings_ui::webview_host::{Mode, choose_mode};
use nagi_settings_ui::{run, token};
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // URL（トークン）はメッセージに含めない
            eprintln!("nagi-settings: {message}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1))?;
    if args.diag {
        // 診断（画面は開かない）。NG があれば終了コード 1
        let code = nagi_settings_ui::diag::entry::run_cli(&args)?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }
    match choose_mode(&args, cfg!(windows)) {
        Mode::Browser => run_web(&args),
        #[cfg(windows)]
        Mode::Native => run_native(&args),
        #[cfg(windows)]
        Mode::WebView { fallback_native } => run_webview(&args, fallback_native),
        // `choose_mode` は Windows 以外ではブラウザしか返さない
        #[cfg(not(windows))]
        Mode::Native | Mode::WebView { .. } => run_web(&args),
    }
}

/// WebView2 のウィンドウで開く（Windows の既定）。使えない・初期化に失敗したとき、`fallback_native` ならネイティブ画面で
/// 開き直す（理由は標準エラーと `%LOCALAPPDATA%\Nagi\logs\settings-ui.log` に 1 行）
#[cfg(windows)]
fn run_webview(args: &Args, fallback_native: bool) -> Result<(), String> {
    use nagi_settings_ui::webview::{WebViewOptions, run};
    use nagi_settings_ui::webview_host::{append_log, devtools_enabled, log_path, user_data_dir};
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let options = WebViewOptions {
        spec: imserver_spec(args)?,
        start_fragment: args.start_fragment(),
        ui_state_path: nagi_settings_ui::native_ui_state::default_path(std::env::consts::OS, &env),
        user_data_dir: user_data_dir(std::env::consts::OS, &env),
        devtools: devtools_enabled(std::env::var("NAGI_SETTINGS_DEVTOOLS").ok().as_deref()),
        diag: std::sync::Arc::new(nagi_settings_ui::diag::service::ArgsDiagService::new(
            args.clone(),
        )),
    };
    let Err(reason) = run(options) else {
        return Ok(());
    };
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = if fallback_native {
        format!("{secs} settings-ui: WebView2 を使えないためネイティブ画面で開きます: {reason}")
    } else {
        format!("{secs} settings-ui: WebView2 を使えません: {reason}")
    };
    eprintln!("nagi-settings: {line}");
    if let Some(path) = log_path(std::env::consts::OS, &env) {
        append_log(&path, &line);
    }
    if fallback_native {
        run_native(args)
    } else {
        Err(reason)
    }
}

/// `imserver` の起動条件（Web 画面・ネイティブ画面で共通）
fn imserver_spec(args: &Args) -> Result<ImserverSpec, String> {
    let exe = args
        .imserver
        .clone()
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| default_imserver(&p))
        })
        .ok_or_else(|| "imserver の場所を決められません".to_owned())?;
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    Ok(ImserverSpec {
        exe,
        data_dir: args
            .data_dir
            .clone()
            .or_else(|| default_data_dir(std::env::consts::OS, &env)),
        user_dir: args.user_dir.clone(),
        parent_pid: std::process::id(),
        update_dir: args
            .update_dir
            .clone()
            .or_else(|| default_update_dir(std::env::consts::OS, &env)),
        apply_script: std::env::current_exe()
            .ok()
            .and_then(|p| default_apply_script(&p)),
    })
}

/// ネイティブの Win32 ウィンドウで開く（Windows の既定）
#[cfg(windows)]
fn run_native(args: &Args) -> Result<(), String> {
    use nagi_settings_ui::native::{NativeOptions, run};
    let spec = imserver_spec(args)?;
    // imserver の起動（最大 60 秒）はワーカースレッドで行い、その間もウィンドウは動く
    let factory: nagi_settings_ui::native_worker::Factory = Box::new(move || {
        ImserverBackend::spawn(&spec).map(|b| Box::new(b) as Box<dyn SettingsBackend>)
    });
    run(NativeOptions {
        factory,
        diag: std::sync::Arc::new(nagi_settings_ui::diag::service::ArgsDiagService::new(
            args.clone(),
        )),
        open_update: args.opens_update_page(),
        auto_check: args.auto_check,
        ui_state_path: {
            let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
            nagi_settings_ui::native_ui_state::default_path(std::env::consts::OS, &env)
        },
    })
}

/// ローカルの HTTP サーバー + 既定ブラウザで開く（`--web`・Windows 以外）
fn run_web(args: &Args) -> Result<(), String> {
    let token = token::generate_token()?;
    let (server, port) = run::bind()?;
    let spec = imserver_spec(args)?;
    // 起動に失敗しても画面は開き、理由を表示する（GUI なので端末には出ない）
    let backend: Box<dyn SettingsBackend> = match ImserverBackend::spawn(&spec) {
        Ok(b) => Box::new(b),
        Err(error) => Box::new(FailedBackend { error }),
    };

    let mut app = App::new(backend, Box::new(SystemOpener), token, port, Instant::now());
    app.load_config_path();
    app.set_diag_service(std::sync::Arc::new(
        nagi_settings_ui::diag::service::ArgsDiagService::new(args.clone()),
    ));
    app.set_start_fragment(args.start_fragment());
    let url = app.page_url();
    if let Some(path) = &args.url_file {
        std::fs::write(path, &url).map_err(|e| format!("URL を書き込めません（{}）", e.kind()))?;
    }
    if !args.no_browser {
        SystemOpener
            .open_url(&url)
            .map_err(|e| format!("ブラウザを開けません: {e}"))?;
    }
    run::serve(&server, &mut app);
    Ok(())
}
