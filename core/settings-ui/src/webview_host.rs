//! WebView2 ホストの OS 非依存な判断（起動方式の優先順位・ポート選択・ホスト⇄ページのメッセージ・
//! ナビゲーションの許可・右クリックメニュー・背景色・ログ）。`docs/decisions.md` H-130 候補。
//!
//! Windows のウィンドウ層（`webview`、unsafe を含む）はここで決めた結果を実行するだけにして、
//! 判断は Linux でもテストできるようにする。
//!
//! # ホスト⇄ページのメッセージ
//!
//! ページ → ホスト: `window.chrome.webview.postMessage({ type: "...", ... })`（オブジェクトを送る。
//! JSON 文字列でもよい）。`type` は次のどれか。それ以外・形が違うものは無視する（ホストは何も実行しない）。
//!
//! | `type` | 追加の項目 | ホストの動作 |
//! | --- | --- | --- |
//! | `close-request` | なし | ウィンドウを閉じる（imserver の終了まで従来どおり）。Esc は**ページが**モーダルを見て送る |
//! | `open-external` | `url`: `http:` / `https:` のみ（2048 文字まで） | 既定のブラウザで開く |
//! | `set-title` | `title`: 文字列（空・省略で既定に戻す） | ウィンドウのタイトルを `<title> - Nagi 設定` にする |
//! | `save-window-state` | なし | ウィンドウの位置・サイズ・最大化を `settings-ui-state.json` に今すぐ保存 |
//!
//! ホスト → ページ: 送らない。テーマ（ライト / ダーク）とハイコントラストは WebView2 がシステムに追従し、
//! ページは CSS の `prefers-color-scheme` / `forced-colors` を使える。

use crate::cli::Args;
use crate::theme::{Rgb, ThemeKind};
use serde_json::Value;
use std::path::{Path, PathBuf};

// ---- 起動方式 ----

/// 設定画面の表示方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// ローカル HTTP サーバー + 既定のブラウザ（`--web`・Windows 以外）
    Browser,
    /// WebView2 のウィンドウ。`fallback_browser` が真なら、使えないときに既定のブラウザで開く
    WebView {
        /// WebView2 を使えないとき、既定のブラウザ（`--web` と同じ画面）にフォールバックするか
        fallback_browser: bool,
    },
}

/// 引数と OS から表示方式を決める。優先順位:
/// Windows 以外は常にブラウザ → `--web` → `--webview`（フォールバックなし）→
/// 既定（WebView2。使えなければ既定のブラウザ）。引数の排他は `parse_args` が保証する
pub fn choose_mode(args: &Args, is_windows: bool) -> Mode {
    if !is_windows || args.web {
        Mode::Browser
    } else {
        Mode::WebView {
            fallback_browser: !args.webview,
        }
    }
}

// ---- ポート ----

/// WebView2 で使う待ち受けポートの先頭。オリジンが同じなら localStorage（詳細設定のオン / オフ等）が
/// 次回の起動にも残るので、固定の番号を優先する（ephemeral の範囲 49152〜より下）
pub const PREFERRED_PORT_BASE: u16 = 47_831;
/// 先頭から数えて試す個数。すべて塞がっていれば OS 任せのポート（0）にする
pub const PREFERRED_PORT_COUNT: u16 = 16;

/// 試すポートの並び（最後に 0 = OS 任せ）
pub fn port_candidates() -> Vec<u16> {
    (0..PREFERRED_PORT_COUNT)
        .map(|i| PREFERRED_PORT_BASE + i)
        .chain(std::iter::once(0))
        .collect()
}

// ---- ページ → ホストのメッセージ ----

/// ページから届くメッセージ
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageMessage {
    /// ウィンドウを閉じる
    CloseRequest,
    /// 既定のブラウザで URL を開く（`is_external_url` を通ったもの）
    OpenExternal(String),
    /// ウィンドウのタイトル（`None` は既定に戻す）
    SetTitle(Option<String>),
    /// ウィンドウの位置・サイズを今すぐ保存する
    SaveWindowState,
}

/// `postMessage` の中身（JSON）を解釈する。知らない `type`・壊れた形は `None`
pub fn parse_page_message(json: &str) -> Option<PageMessage> {
    if json.len() > 8192 {
        return None;
    }
    let mut value: Value = serde_json::from_str(json).ok()?;
    // 文字列で送られたものは、その中身が JSON のオブジェクトのとき（二重にエンコードされている）だけ読む
    if let Value::String(inner) = &value {
        value = serde_json::from_str(inner).ok()?;
    }
    let map = value.as_object()?;
    match map.get("type")?.as_str()? {
        "close-request" => Some(PageMessage::CloseRequest),
        "open-external" => {
            let url = map.get("url")?.as_str()?;
            is_external_url(url).then(|| PageMessage::OpenExternal(url.to_owned()))
        }
        "set-title" => match map.get("title") {
            None | Some(Value::Null) => Some(PageMessage::SetTitle(None)),
            Some(Value::String(s)) => Some(PageMessage::SetTitle(sanitize_title(s))),
            Some(_) => None,
        },
        "save-window-state" => Some(PageMessage::SaveWindowState),
        _ => None,
    }
}

/// 既定のブラウザに渡してよい URL か。`http:` / `https:` でホストがあり、制御文字・空白を含まない
pub fn is_external_url(url: &str) -> bool {
    if url.is_empty() || url.len() > 2048 || url.chars().any(|c| c.is_control() || c == ' ') {
        return false;
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    let Some(rest) = rest else {
        return false;
    };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..host_end];
    // `user@host` 形式は紛らわしいので受けない
    !authority.is_empty() && !authority.contains('@') && !authority.starts_with(':')
}

/// ページが決めたタイトルを整える（制御文字を除き、前後の空白を削り、40 文字まで）。空なら `None`
fn sanitize_title(raw: &str) -> Option<String> {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(40).collect())
    }
}

/// ウィンドウの既定のタイトル
pub const BASE_TITLE: &str = "Nagi 設定";

/// ウィンドウのタイトル。ページのタイトルがあれば `<ページ> - Nagi 設定`
pub fn window_title(page_title: Option<&str>) -> String {
    match page_title {
        Some(t) if !t.is_empty() && t != BASE_TITLE => format!("{t} - {BASE_TITLE}"),
        _ => BASE_TITLE.to_owned(),
    }
}

// ---- ナビゲーション ----

/// 自サーバーのオリジン（`http://127.0.0.1:<port>`）か。ナビゲーションはここだけを許し、
/// 初期の `about:blank` も許す
pub fn is_allowed_navigation(uri: &str, port: u16) -> bool {
    if uri == "about:blank" {
        return true;
    }
    let origin = format!("http://127.0.0.1:{port}");
    match uri.strip_prefix(&origin) {
        Some(rest) => rest.is_empty() || rest.starts_with(['/', '?', '#']),
        None => false,
    }
}

// ---- 右クリックメニュー ----

/// WebView2 の既定の右クリックメニューのうち残す項目（`ICoreWebView2ContextMenuItem::Name`）。
/// テキスト入力欄・選択範囲の編集系だけにする（戻る・再読み込み・名前を付けて保存・印刷・
/// 検証などは出さない）
pub fn keep_context_menu_item(name: &str) -> bool {
    matches!(
        name,
        "cut" | "copy" | "paste" | "pasteAsPlainText" | "selectAll" | "undo" | "redo"
    )
}

// ---- 環境 ----

/// DevTools を有効にするか（環境変数 `NAGI_SETTINGS_DEVTOOLS=1` のときだけ）
pub fn devtools_enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

/// WebView2 のユーザーデータフォルダ。Windows は `%LOCALAPPDATA%\Nagi\webview2`（標準ユーザーで書ける場所）
pub fn user_data_dir(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if os != "windows" {
        return None;
    }
    let local = env("LOCALAPPDATA")?;
    Some(PathBuf::from(format!(
        "{}\\Nagi\\webview2",
        local.trim_end_matches(['\\', '/'])
    )))
}

/// 診断ログの場所（`%LOCALAPPDATA%\Nagi\logs\settings-ui.log`。TSF 層のログと同じフォルダ）
pub fn log_path(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if os != "windows" {
        return None;
    }
    let local = env("LOCALAPPDATA")?;
    Some(PathBuf::from(format!(
        "{}\\Nagi\\logs\\settings-ui.log",
        local.trim_end_matches(['\\', '/'])
    )))
}

/// ログの上限（これを超えていたら書かない。設定画面の起動理由の 1 行だけなので小さい）
const LOG_LIMIT: u64 = 64 * 1024;

/// ログに 1 行追記する（失敗しても何もしない。入力内容は書かない）
pub fn append_log(path: &Path, line: &str) {
    use std::io::Write;
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_LIMIT) {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 起動時の背景色（白フラッシュを避けるため、ページの背景 `--bg` と同じ色にする）。
/// `assets/app.css` の `--bg`（ライト `#f6f7f9`・ダーク `#14161a`）と揃える。ハイコントラストは
/// `None`（システムの `COLOR_WINDOW` を使う）
pub fn background_rgb(kind: ThemeKind) -> Option<Rgb> {
    match kind {
        ThemeKind::Light => Some(Rgb::hex(0xf3_f3_f3)),
        ThemeKind::Dark => Some(Rgb::hex(0x20_20_20)),
        ThemeKind::HighContrast => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::parse_args;

    fn args(list: &[&str]) -> Args {
        parse_args(list.iter().map(|s| (*s).to_owned())).unwrap()
    }

    #[test]
    fn mode_priority() {
        let wv = |fallback_browser| Mode::WebView { fallback_browser };
        // 既定: WebView2（使えなければブラウザへフォールバック）
        assert_eq!(choose_mode(&args(&[]), true), wv(true));
        assert_eq!(choose_mode(&args(&["--section", "update"]), true), wv(true));
        assert_eq!(choose_mode(&args(&["--webview"]), true), wv(false));
        assert_eq!(choose_mode(&args(&["--web"]), true), Mode::Browser);
        // Windows 以外は常にブラウザ
        for list in [&[][..], &["--webview"], &["--web"]] {
            assert_eq!(choose_mode(&args(list), false), Mode::Browser);
        }
    }

    #[test]
    fn ports_prefer_a_fixed_range_then_os_assigned() {
        let c = port_candidates();
        assert_eq!(c.first(), Some(&PREFERRED_PORT_BASE));
        assert_eq!(c.last(), Some(&0));
        assert_eq!(c.len(), usize::from(PREFERRED_PORT_COUNT) + 1);
        assert!(c[..c.len() - 1].windows(2).all(|w| w[1] == w[0] + 1));
        // ephemeral（49152〜）にかからない
        assert!(c[..c.len() - 1].iter().all(|p| *p < 49_152));
    }

    #[test]
    fn page_messages_parse_strictly() {
        let p = parse_page_message;
        assert_eq!(
            p(r#"{"type":"close-request"}"#),
            Some(PageMessage::CloseRequest)
        );
        assert_eq!(
            p(r#"{"type":"save-window-state"}"#),
            Some(PageMessage::SaveWindowState)
        );
        assert_eq!(
            p(r#"{"type":"open-external","url":"https://example.com/a?b=1#c"}"#),
            Some(PageMessage::OpenExternal(
                "https://example.com/a?b=1#c".to_owned()
            ))
        );
        // JSON 文字列として送られた場合も同じ
        assert_eq!(
            p(r#""{\"type\":\"close-request\"}""#),
            Some(PageMessage::CloseRequest)
        );
        // 許可しない URL・知らない type・壊れた形
        for bad in [
            r#"{"type":"open-external","url":"file:///c:/windows/system32/calc.exe"}"#,
            r#"{"type":"open-external","url":"javascript:alert(1)"}"#,
            r#"{"type":"open-external","url":"ms-settings:network"}"#,
            r#"{"type":"open-external","url":"https://a b"}"#,
            r#"{"type":"open-external"}"#,
            r#"{"type":"set-title","title":5}"#,
            r#"{"type":"launch","path":"calc.exe"}"#,
            r#"{"kind":"close-request"}"#,
            r#"["close-request"]"#,
            r#""close-request""#,
            "close-request",
            "",
        ] {
            assert_eq!(p(bad), None, "{bad}");
        }
        // 大きすぎるものは読まない
        let big = format!(r#"{{"type":"set-title","title":"{}"}}"#, "a".repeat(9000));
        assert_eq!(p(&big), None);
    }

    #[test]
    fn titles_are_sanitized() {
        assert_eq!(
            parse_page_message(r#"{"type":"set-title","title":" 変換\n\u0007 "}"#),
            Some(PageMessage::SetTitle(Some("変換".to_owned())))
        );
        assert_eq!(
            parse_page_message(r#"{"type":"set-title","title":"  "}"#),
            Some(PageMessage::SetTitle(None))
        );
        assert_eq!(
            parse_page_message(r#"{"type":"set-title"}"#),
            Some(PageMessage::SetTitle(None))
        );
        let long = format!(r#"{{"type":"set-title","title":"{}"}}"#, "あ".repeat(100));
        let Some(PageMessage::SetTitle(Some(t))) = parse_page_message(&long) else {
            panic!("title");
        };
        assert_eq!(t.chars().count(), 40);
        assert_eq!(window_title(None), "Nagi 設定");
        assert_eq!(window_title(Some("")), "Nagi 設定");
        assert_eq!(window_title(Some("Nagi 設定")), "Nagi 設定");
        assert_eq!(window_title(Some("更新")), "更新 - Nagi 設定");
    }

    #[test]
    fn external_urls() {
        assert!(is_external_url("https://shiki-01.com/nagi"));
        assert!(is_external_url("http://example.com"));
        assert!(is_external_url("https://example.com:8443/x"));
        for bad in [
            "",
            "ftp://example.com",
            "//example.com",
            "https://",
            "https:///x",
            "https://user@example.com/",
            "https://:80/",
            "https://example.com/\n",
            "https://exa mple.com",
            "HTTPS://example.com",
        ] {
            assert!(!is_external_url(bad), "{bad:?}");
        }
        assert!(!is_external_url(&format!(
            "https://e.com/{}",
            "a".repeat(3000)
        )));
    }

    #[test]
    fn navigation_is_limited_to_own_origin() {
        let ok = |u: &str| is_allowed_navigation(u, 47_831);
        assert!(ok("http://127.0.0.1:47831/?t=abc"));
        assert!(ok("http://127.0.0.1:47831/"));
        assert!(ok("http://127.0.0.1:47831"));
        assert!(ok("http://127.0.0.1:47831/#update"));
        assert!(ok("http://127.0.0.1:47831?t=abc"));
        assert!(ok("about:blank"));
        for bad in [
            "http://127.0.0.1:47832/",
            "http://127.0.0.1:478310/",
            "http://127.0.0.1:47831.evil.com/",
            "http://127.0.0.1:47831@evil.com/",
            "https://127.0.0.1:47831/",
            "http://localhost:47831/",
            "https://example.com/",
            "file:///c:/x.html",
            "javascript:alert(1)",
            "data:text/html,x",
            "",
        ] {
            assert!(!ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn context_menu_keeps_only_editing_items() {
        for keep in ["cut", "copy", "paste", "selectAll", "undo", "redo"] {
            assert!(keep_context_menu_item(keep), "{keep}");
        }
        for drop in [
            "back",
            "reload",
            "saveAs",
            "print",
            "inspectElement",
            "openLinkInNewWindow",
            "webCapture",
            "share",
            "emoji",
            "",
            "separator",
        ] {
            assert!(!keep_context_menu_item(drop), "{drop}");
        }
    }

    #[test]
    fn devtools_only_with_explicit_one() {
        assert!(devtools_enabled(Some("1")));
        assert!(!devtools_enabled(Some("0")));
        assert!(!devtools_enabled(Some("true")));
        assert!(!devtools_enabled(None));
    }

    #[test]
    fn paths_and_background() {
        let env =
            |k: &str| (k == "LOCALAPPDATA").then(|| "C:\\Users\\u\\AppData\\Local\\".to_owned());
        assert_eq!(
            user_data_dir("windows", &env),
            Some(PathBuf::from(
                "C:\\Users\\u\\AppData\\Local\\Nagi\\webview2"
            ))
        );
        assert_eq!(
            log_path("windows", &env),
            Some(PathBuf::from(
                "C:\\Users\\u\\AppData\\Local\\Nagi\\logs\\settings-ui.log"
            ))
        );
        assert_eq!(user_data_dir("linux", &env), None);
        assert_eq!(user_data_dir("windows", &|_| None), None);
        assert_eq!(
            background_rgb(ThemeKind::Light),
            Some(Rgb(0xf3, 0xf3, 0xf3))
        );
        assert_eq!(background_rgb(ThemeKind::Dark), Some(Rgb(0x20, 0x20, 0x20)));
        assert_eq!(background_rgb(ThemeKind::HighContrast), None);
    }

    #[test]
    fn log_appends_and_respects_the_limit() {
        let dir = std::env::temp_dir().join(format!("nagi-wv-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("logs").join("settings-ui.log");
        append_log(&path, "one");
        append_log(&path, "two");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        std::fs::write(&path, vec![b'x'; (LOG_LIMIT + 1) as usize]).unwrap();
        append_log(&path, "three");
        assert!(!std::fs::read_to_string(&path).unwrap().contains("three"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
