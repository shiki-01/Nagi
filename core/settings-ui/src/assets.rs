//! 画面（HTML / CSS / JS）。バイナリに埋め込み、外部の CDN・リソースは使わない。
//!
//! script / style はインラインにし、CSP の nonce で許可する（レスポンスごとに nonce を作る）。
//! トークンは `<meta>` で渡し、JS が API 呼び出しのヘッダーに付ける。

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_CSS: &str = include_str!("../assets/app.css");
const APP_JS: &str = include_str!("../assets/app.js");

/// HTML の属性値・テキストに入れてよい形にする（nonce・トークンは 16 進だが念のため）
fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&#39;".to_owned(),
            c => c.to_string(),
        })
        .collect()
}

/// ページを組み立てる（`nonce` は CSP と同じ値）
pub fn render_page(nonce: &str, token: &str) -> String {
    INDEX_HTML
        .replace("{{CSS}}", APP_CSS)
        .replace("{{JS}}", APP_JS)
        .replace("{{NONCE}}", &escape(nonce))
        .replace("{{TOKEN}}", &escape(token))
}

/// テスト用: 埋め込みの素材（プレースホルダーつき）
#[doc(hidden)]
pub fn sources() -> [&'static str; 3] {
    [INDEX_HTML, APP_CSS, APP_JS]
}
