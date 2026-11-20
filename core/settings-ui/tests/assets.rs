//! 埋め込みの画面（HTML / CSS / JS）の静的な検査。

use nagi_settings_ui::assets::{render_page, sources};

#[test]
fn page_has_no_external_urls_or_resources() {
    let page = render_page("nonce", "token");
    for text in sources()
        .iter()
        .copied()
        .chain(std::iter::once(page.as_str()))
    {
        for needle in [
            "http://", "https://", "//cdn", "@import", "url(http", "ws://", "wss://",
        ] {
            assert!(!text.contains(needle), "外部参照の疑い: {needle}");
        }
        // 外部ファイルを読み込む要素・属性を使わない（スクリプト・CSS はインライン）
        assert!(!text.contains("<link"), "<link> は使わない");
        assert!(!text.contains("<img"), "<img> は使わない");
        assert!(!text.contains("<iframe"));
        assert!(!text.contains(" src="), "src 属性は使わない");
        assert!(!text.contains("href="), "href 属性は使わない");
    }
}

#[test]
fn page_uses_nonce_and_has_no_inline_handlers_or_style_attributes() {
    let page = render_page("abc123", "tok");
    assert!(page.contains("<script nonce=\"abc123\">"));
    assert!(page.contains("<style nonce=\"abc123\">"));
    assert!(page.contains("name=\"nagi-token\" content=\"tok\""));
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
    assert!(page.contains("prefers-color-scheme: dark"));
}

#[test]
fn page_does_not_use_innerhtml_for_dynamic_values() {
    let js = sources()[2];
    assert!(!js.contains("innerHTML"));
    assert!(!js.contains("insertAdjacentHTML"));
    assert!(!js.contains("eval("));
}

#[test]
fn page_escapes_the_token_and_nonce() {
    let page = render_page("a\"b", "t<>&'");
    assert!(page.contains("a&quot;b"));
    assert!(page.contains("t&lt;&gt;&amp;&#39;"));
}
