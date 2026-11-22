//! キー割当ページ（`assets/keymap.js`）の静的な検査と、キー仕様への正規化の検査（node があれば実行）。

use nagi_settings_ui::assets::{find_static, render_page};
use std::path::PathBuf;
use std::process::Command;

fn js() -> &'static str {
    find_static("/keymap.js")
        .expect("keymap.js は配られる")
        .body
}

fn js_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/keymap.js")
}

#[test]
fn page_loads_keymap_script_before_the_app_with_the_token() {
    let page = render_page("tok");
    let keymap = page
        .find("<script src=\"/keymap.js?t=tok\" defer></script>")
        .expect("keymap.js を読む");
    let app = page
        .find("<script src=\"/app.js?t=tok\" defer></script>")
        .expect("app.js を読む");
    // defer は書かれた順に実行される。登録（NagiPages への追加）が app.js より先に終わる
    assert!(keymap < app);
}

#[test]
fn script_is_csp_safe_and_never_builds_html_from_values() {
    let js = js();
    for needle in [
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "eval(",
        "new Function",
        "setAttribute(\"style\"",
        "cssText",
        "http://",
        "https://",
        "localStorage",
    ] {
        assert!(!js.contains(needle), "{needle}");
    }
}

#[test]
fn script_only_calls_the_keymap_apis_and_the_settings_update() {
    // 呼ぶ API は /api/keymap/* と、プリセットを保存するための /api/update だけ
    let js = js();
    for path in ["get", "set", "validate", "reset", "export", "import"] {
        assert!(js.contains(&format!("post(\"{path}\"")), "{path}");
    }
    assert!(js.contains("\"/api/update\""));
    assert!(js.contains("\"keymap.preset\""));
    assert!(!js.contains("/api/clear_learning"));
    assert!(!js.contains("fetch("), "通信は app.js の api() を通す");
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

#[test]
fn script_passes_a_node_syntax_check_when_node_is_available() {
    if !node_available() {
        eprintln!("skip: node が無い");
        return;
    }
    let out = Command::new("node")
        .arg("--check")
        .arg(js_path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "node --check: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 押されたキー（KeyboardEvent の key / code / 修飾）→ キー仕様。コアの `parse_key` が読める書き方になっていること
#[test]
fn key_events_are_normalized_to_core_key_specs() {
    if !node_available() {
        eprintln!("skip: node が無い");
        return;
    }
    let script = r#"
const fs = require('fs');
global.window = {};
new Function('window', fs.readFileSync(process.argv[1], 'utf8'))(global.window);
const K = global.window.NagiKeymapKeys;
const ev = (o) => Object.assign({ key: '', code: '', ctrlKey: false, shiftKey: false, altKey: false, metaKey: false,
  getModifierState: () => false }, o);
const cases = [
  [{ key: 'j', code: 'KeyJ', ctrlKey: true, shiftKey: true }],
  [{ key: 'J', code: 'KeyJ', ctrlKey: true, shiftKey: true }],
  [{ key: 'j', code: 'KeyJ', ctrlKey: true }],
  [{ key: 'A', code: 'KeyA', shiftKey: true }],
  [{ key: 'F8', code: 'F8' }],
  [{ key: 'F8', code: 'F8', shiftKey: true }],
  [{ key: 'F13', code: 'F13' }],
  [{ key: 'ArrowRight', code: 'ArrowRight', ctrlKey: true, shiftKey: true }],
  [{ key: ' ', code: 'Space', ctrlKey: true }],
  [{ key: ' ', code: 'Space', shiftKey: true }],
  [{ key: 'Backspace', code: 'Backspace', ctrlKey: true }],
  [{ key: 'PageDown', code: 'PageDown' }],
  [{ key: 'Enter', code: 'Enter' }],
  [{ key: 'Enter', code: 'NumpadEnter' }],
  [{ key: 'Convert', code: 'Convert' }],
  [{ key: 'NonConvert', code: 'NonConvert' }],
  [{ key: 'Process', code: 'Convert' }],
  [{ key: 'Process', code: 'KeyK', ctrlKey: true }],
  [{ key: 'KanaMode', code: 'KanaMode' }],
  [{ key: 'Alphanumeric', code: 'CapsLock' }],
  [{ key: 'ZenkakuHankaku', code: 'Backquote' }],
  [{ key: 'Katakana', code: 'KanaMode' }],
  [{ key: ';', code: 'Semicolon', ctrlKey: true, shiftKey: true }],
  [{ key: '+', code: 'Equal', shiftKey: true }],
  [{ key: 'あ', code: 'KeyA' }],
  [{ key: 'Control', code: 'ControlLeft', ctrlKey: true }],
  [{ key: 'Shift', code: 'ShiftLeft', ctrlKey: true, shiftKey: true }],
  [{ key: 'Escape', code: 'Escape' }],
  [{ key: 'Escape', code: 'Escape', ctrlKey: true }],
  [{ key: 'Tab', code: 'Tab' }],
  [{ key: 'Tab', code: 'Tab', shiftKey: true }],
  [{ key: 'Tab', code: 'Tab', ctrlKey: true }],
  [{ key: 'q', code: 'KeyQ', ctrlKey: true, altKey: true, getModifierState: (m) => m === 'AltGraph' }],
  [{ key: 'j', code: 'KeyJ', metaKey: true }],
  [{ key: 'Delete', code: 'Delete', ctrlKey: true, altKey: true }],
];
const out = cases.map(([o]) => { const r = K.specFromEvent(ev(o)); return r.kind === 'ok' ? r.spec : r.kind; });
out.push(K.displayParts('Ctrl+Shift+j').join('|'));
out.push(K.displayParts('Henkan').join('|'));
out.push(K.displayParts('Escape').join('|'));
console.log(JSON.stringify(out));
"#;
    let out = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(js_path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got: Vec<String> =
        serde_json::from_slice(String::from_utf8_lossy(&out.stdout).trim().as_bytes()).unwrap();
    let want = [
        "Ctrl+Shift+j",
        "Ctrl+Shift+j",
        "Ctrl+j",
        "Shift+a",
        "F8",
        "Shift+F8",
        "unsupported",
        "Ctrl+Shift+Right",
        "Ctrl+Space",
        "Shift+Space",
        "Ctrl+BackSpace",
        "PageDown",
        "Enter",
        "Enter",
        "Henkan",
        "Muhenkan",
        "Henkan",
        "Ctrl+k",
        "Kana",
        "Eisu",
        "Zenkaku",
        "Katakana",
        "Ctrl+Shift+;",
        "unsupported",
        "unsupported",
        "pending",
        "pending",
        "cancel",
        "Ctrl+Escape",
        "pass",
        "pass",
        "Ctrl+Tab",
        "q",
        "Meta+j",
        "Ctrl+Alt+Delete",
        "Ctrl|Shift|J",
        "変換",
        "Esc",
    ];
    assert_eq!(got, want);
}
