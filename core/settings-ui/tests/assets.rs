//! 埋め込みの画面（HTML / CSS / JS）の静的な検査。

use nagi_settings_ui::assets::{STATIC_ASSETS, find_static, render_page, sources};

#[test]
fn page_has_no_external_urls_or_cdn_references() {
    let page = render_page("token");
    for text in sources()
        .iter()
        .copied()
        .chain(std::iter::once(page.as_str()))
    {
        for needle in [
            "http://", "https://", "//cdn", "@import", "url(http", "ws://", "wss://",
        ] {
            // SVG の名前空間（xmlns）は通信しない識別子なので除く
            let text = text.replace("http://www.w3.org/2000/svg", "");
            assert!(!text.contains(needle), "外部参照の疑い: {needle}");
        }
        assert!(!text.contains("<iframe"));
        assert!(!text.contains("<img src=\"http"));
    }
}

#[test]
fn page_loads_only_same_origin_files_with_the_token_and_has_no_inline_code() {
    let page = render_page("abc123");
    // CSS・JS・アイコンは同一オリジンの別ファイル（トークンつきの URL）。インラインの script / style は無い
    assert!(page.contains("<script src=\"/app.js?t=abc123\" defer></script>"));
    assert!(page.contains("<link rel=\"stylesheet\" href=\"/app.css?t=abc123\">"));
    assert!(page.contains("href=\"/icon.svg?t=abc123\""));
    assert!(page.contains("name=\"nagi-token\" content=\"abc123\""));
    assert!(!page.contains("<style"), "インラインの style は使わない");
    assert!(
        !page.contains("<script>") && !page.contains("<script nonce"),
        "インラインの script は使わない"
    );
    assert!(!page.contains("{{"));
    // CSP でブロックされるインラインのイベントハンドラ・style 属性を使わない
    for handler in [
        " onclick=",
        " onchange=",
        " onload=",
        " onerror=",
        " style=",
    ] {
        assert!(!page.contains(handler), "{handler}");
    }
    assert!(page.contains("lang=\"ja\""));
    // 参照する静的ファイルはすべて配られる
    for asset in STATIC_ASSETS {
        assert!(find_static(asset.path).is_some());
        assert!(!asset.body.is_empty(), "{}", asset.path);
    }
    assert!(find_static("/index.html").is_none());
}

#[test]
fn css_covers_the_required_media_features() {
    let css = sources()[1];
    for needle in [
        "prefers-color-scheme: dark",
        "forced-colors: active",
        "prefers-reduced-motion: reduce",
        "prefers-contrast: more",
        "Segoe UI Variable Text",
        "Yu Gothic UI",
        "Meiryo UI",
        "min-width: 840px",
    ] {
        assert!(css.contains(needle), "{needle}");
    }
    // スクロールの滑らかさは OS に任せる（scroll-behavior は触らない）
    assert!(!css.contains("scroll-behavior"));
    // CSP で許可されない参照を持ち込まない
    assert!(!css.contains("url("));
}

#[test]
fn script_does_not_use_innerhtml_for_dynamic_values() {
    let js = sources()[2];
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("outerHTML"));
    assert!(!js.contains("insertAdjacentHTML"));
    assert!(!js.contains("document.write"));
    assert!(!js.contains("eval("));
    assert!(!js.contains("new Function"));
    // CSSOM 以外で style 属性を作らない（CSP の style-src 'self' でブロックされる）
    assert!(!js.contains("setAttribute(\"style\""));
    assert!(!js.contains("cssText"));
}

#[test]
fn script_talks_to_the_host_only_through_the_agreed_messages() {
    let js = sources()[2];
    assert!(js.contains("window.chrome.webview"));
    assert!(js.contains("\"close-request\""));
    assert!(js.contains("\"open-external\""));
    // 外部リンクは rel="noopener noreferrer"
    assert!(js.contains("noopener noreferrer"));
}

#[test]
fn page_escapes_the_token() {
    let page = render_page("t<>&'\"");
    assert!(page.contains("t&lt;&gt;&amp;&#39;&quot;"));
    assert!(!page.contains("t<>&"));
}

#[test]
fn bundled_icon_matches_the_web_site_icon_when_available() {
    // ブランドのアイコンは web/static/icon.svg と同じ（リポジトリ内で実行しているときだけ照合する）
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/static/icon.svg");
    if let Ok(site) = std::fs::read_to_string(path) {
        assert_eq!(site, sources()[3]);
    }
}

#[test]
fn script_passes_a_node_syntax_check_when_node_is_available() {
    // 依存を増やさない軽量なスモークテスト（`node --check`）。node が無い環境では skip する
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/app.js");
    match std::process::Command::new("node")
        .arg("--check")
        .arg(&path)
        .output()
    {
        Ok(out) => assert!(
            out.status.success(),
            "node --check: {}",
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(_) => eprintln!("skip: node が無い"),
    }
}
