//! `nagi-settings`: 設定画面を起動する（ローカルの HTTP サーバー + 既定ブラウザ）。
//!
//! 使い方: `nagi-settings [--imserver <path>] [--data-dir <dir>] [--user-dir <dir>]`。
//! 設定画面が使う `imserver` は `--no-llm` の別プロセス。ブラウザのタブを閉じるか、15 分間操作が無ければ終了する。
//! 検証用: `--no-browser`（ブラウザを開かない）、`--url-file <path>`（URL をファイルに書く）。

// Windows ではコンソールを出さない（言語バーのメニューから起動される GUI 起点のアプリ）
#![cfg_attr(windows, windows_subsystem = "windows")]

use nagi_settings_ui::backend::{FailedBackend, ImserverBackend, ImserverSpec, SettingsBackend};
use nagi_settings_ui::cli::{default_data_dir, default_imserver, parse_args};
use nagi_settings_ui::http::App;
use nagi_settings_ui::opener::{Opener, SystemOpener};
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
    let token = token::generate_token()?;
    let (server, port) = run::bind()?;

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
    let spec = ImserverSpec {
        exe,
        data_dir: args
            .data_dir
            .clone()
            .or_else(|| default_data_dir(std::env::consts::OS, &env)),
        user_dir: args.user_dir.clone(),
        parent_pid: std::process::id(),
    };
    // 起動に失敗しても画面は開き、理由を表示する（GUI なので端末には出ない）
    let backend: Box<dyn SettingsBackend> = match ImserverBackend::spawn(&spec) {
        Ok(b) => Box::new(b),
        Err(error) => Box::new(FailedBackend { error }),
    };

    let mut app = App::new(backend, Box::new(SystemOpener), token, port, Instant::now());
    app.load_config_path();
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
