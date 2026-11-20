//! ブラウザ・メモ帳・エクスプローラーの起動。`std::process::Command` のみ（`windows` クレートは使わない）。

use std::path::Path;
use std::process::{Command, Stdio};

/// 外部アプリの起動（テストではフェイクを差し込む）
pub trait Opener {
    /// URL を既定のブラウザで開く
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// 設定ファイルをテキストエディタで開く（Windows はメモ帳）
    fn open_file_in_editor(&self, path: &Path) -> Result<(), String>;
    /// フォルダをファイルマネージャーで開く
    fn open_folder(&self, path: &Path) -> Result<(), String>;
}

/// OS の仕組みで開く実装
pub struct SystemOpener;

/// 起動して待たずに返す（終了待ちは別スレッドで行い、ゾンビを残さない）
fn launch(mut command: Command) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("アプリを起動できません（{}）", e.kind()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

impl Opener for SystemOpener {
    fn open_url(&self, url: &str) -> Result<(), String> {
        if cfg!(windows) {
            // `cmd /c start` は `&` を解釈するので使わない。rundll32 は引数をそのまま URL として扱う
            let mut c = Command::new("rundll32.exe");
            c.arg("url.dll,FileProtocolHandler").arg(url);
            launch(c)
        } else if cfg!(target_os = "macos") {
            let mut c = Command::new("open");
            c.arg(url);
            launch(c)
        } else {
            let mut c = Command::new("xdg-open");
            c.arg(url);
            launch(c)
        }
    }

    fn open_file_in_editor(&self, path: &Path) -> Result<(), String> {
        if cfg!(windows) {
            // `.toml` に関連付けが無くても開けるようメモ帳を明示する（無いファイルは作成を尋ねる）
            let mut c = Command::new("notepad.exe");
            c.arg(path);
            launch(c)
        } else {
            let mut c = Command::new(if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            });
            c.arg(path);
            launch(c)
        }
    }

    fn open_folder(&self, path: &Path) -> Result<(), String> {
        let program = if cfg!(windows) {
            "explorer.exe"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let mut c = Command::new(program);
        c.arg(path);
        launch(c)
    }
}
