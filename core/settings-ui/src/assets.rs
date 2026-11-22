//! 画面（HTML / CSS / JS / アイコン）。バイナリに埋め込み、外部の CDN・リソースは使わない。
//!
//! CSP を厳格にするため（`script-src 'self'; style-src 'self'`）、CSS・JS・アイコンは別ファイルとして
//! 同一オリジンから配る（インライン script / style は使わない）。静的ファイルの取得にもトークンが要る
//! （`<script src>` などはヘッダーを付けられないので、URL の `?t=<token>` で検証する）。
//! トークンは `<meta>` でも渡し、JS が API 呼び出しのヘッダーに付ける。

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_CSS: &str = include_str!("../assets/app.css");
const APP_JS: &str = include_str!("../assets/app.js");
const ICON_SVG: &str = include_str!("../assets/icon.svg");

/// 配る静的ファイル 1 つ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticAsset {
    /// URL のパス
    pub path: &'static str,
    /// `Content-Type`
    pub content_type: &'static str,
    /// 中身
    pub body: &'static str,
}

/// 配る静的ファイルの一覧（HTML が参照するものだけ）
pub const STATIC_ASSETS: [StaticAsset; 3] = [
    StaticAsset {
        path: "/app.css",
        content_type: "text/css; charset=utf-8",
        body: APP_CSS,
    },
    StaticAsset {
        path: "/app.js",
        content_type: "text/javascript; charset=utf-8",
        body: APP_JS,
    },
    StaticAsset {
        path: "/icon.svg",
        content_type: "image/svg+xml; charset=utf-8",
        body: ICON_SVG,
    },
];

/// パスから静的ファイルを引く
pub fn find_static(path: &str) -> Option<&'static StaticAsset> {
    STATIC_ASSETS.iter().find(|a| a.path == path)
}

/// HTML の属性値・テキストに入れてよい形にする（トークンは 16 進だが念のため）
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

/// ページを組み立てる（トークンを meta と静的ファイルの URL に入れる）
pub fn render_page(token: &str) -> String {
    INDEX_HTML.replace("{{TOKEN}}", &escape(token))
}

/// テスト用: 埋め込みの素材（プレースホルダーつき）。順に HTML・CSS・JS・アイコン
#[doc(hidden)]
pub fn sources() -> [&'static str; 4] {
    [INDEX_HTML, APP_CSS, APP_JS, ICON_SVG]
}
