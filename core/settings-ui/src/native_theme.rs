//! ネイティブ設定ウィンドウの色・形（OS 非依存の純粋関数と定数）。`docs/decisions.md` H-118。
//!
//! 色はライト・ダークの固定パレット（RGB）と、ハイコントラスト（HC）のシステム色の参照（[`SysColor`]）の
//! どちらかで持つ。HC のときは固定の RGB を一切使わず、角丸・アクセント色などの装飾も無効にする
//! （[`Palette::decorative`] が偽）。実際の色への解決（`GetSysColor`）と描画は Win32 層の仕事。
//!
//! 文字と背景の組み合わせはコントラスト比 4.5:1 以上（非文字の部品は 3:1 以上）を
//! [`Palette::contrast_pairs`] に列挙し、テストで担保する。

use crate::native_diag::BadgeKind;
use crate::native_layout::{Rect, scale};

/// RGB（各 0..=255）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `0xRRGGBB` から作る
    pub const fn hex(v: u32) -> Self {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Win32 の `COLORREF`（`0x00BBGGRR`）
    pub const fn colorref(self) -> u32 {
        (self.0 as u32) | ((self.1 as u32) << 8) | ((self.2 as u32) << 16)
    }

    /// GDI+ の ARGB（不透明）
    pub const fn argb(self) -> u32 {
        self.argb_alpha(0xFF)
    }

    /// GDI+ の ARGB（不透明度 `alpha`: 0 = 透明、255 = 不透明）。影など、下の色に重ねる描画に使う
    pub const fn argb_alpha(self, alpha: u8) -> u32 {
        ((alpha as u32) << 24) | ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | self.2 as u32
    }

    /// 相対輝度（WCAG 2.x）
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }
}

/// コントラスト比（WCAG 2.x。1.0..=21.0）
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// ハイコントラストで使うシステム色（`GetSysColor` の `COLOR_*` に対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysColor {
    /// `COLOR_WINDOW`
    Window,
    /// `COLOR_WINDOWTEXT`
    WindowText,
    /// `COLOR_GRAYTEXT`
    GrayText,
    /// `COLOR_HIGHLIGHT`
    Highlight,
    /// `COLOR_HIGHLIGHTTEXT`
    HighlightText,
    /// `COLOR_BTNFACE`
    ButtonFace,
    /// `COLOR_BTNTEXT`
    ButtonText,
    /// `COLOR_HOTLIGHT`（リンク・ホバーの色）
    HotLight,
}

/// 固定の RGB か、システム色の参照
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// 固定の色（ライト・ダーク）
    Rgb(Rgb),
    /// システム色（ハイコントラスト）
    Sys(SysColor),
}

impl Color {
    const fn hex(v: u32) -> Self {
        Color::Rgb(Rgb::hex(v))
    }

    /// 固定の RGB なら返す（システム色は `None`）
    pub fn rgb(self) -> Option<Rgb> {
        match self {
            Color::Rgb(c) => Some(c),
            Color::Sys(_) => None,
        }
    }
}

/// テーマの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    /// ライト
    Light,
    /// ダーク
    Dark,
    /// ハイコントラスト（システム色のみ）
    HighContrast,
}

impl ThemeKind {
    /// ハイコントラストが最優先。次にアプリのテーマ（ダークか）
    pub fn decide(high_contrast: bool, apps_dark: bool) -> Self {
        if high_contrast {
            ThemeKind::HighContrast
        } else if apps_dark {
            ThemeKind::Dark
        } else {
            ThemeKind::Light
        }
    }
}

/// ボタンの見た目
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// 通常（枠つき）
    Normal,
    /// 主要な操作（アクセント色で塗る）
    Primary,
    /// 取り消せない操作（文字・枠を警告色にする）
    Danger,
    /// 控えめ（枠なし。ホバーで背景が付く。「既定に戻す」）
    Subtle,
}

/// 固定 ID のボタンの見た目（`native_model::ids`）。ここに無い ID は [`ButtonStyle::Normal`]
pub fn button_style_for_id(id: i32) -> ButtonStyle {
    use crate::native_model::ids;
    match id {
        ids::UPDATE_CHECK | ids::UPDATE_APPLY | ids::DIAG_RUN_ALL | ids::RETRY => {
            ButtonStyle::Primary
        }
        ids::CLEAR_LEARNING => ButtonStyle::Danger,
        _ => ButtonStyle::Normal,
    }
}

/// フッターのステータスの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    /// 進行中・お知らせ
    Info,
    /// 成功
    Success,
    /// エラー
    Error,
}

impl StatusKind {
    /// 表示の色分け（診断の結果と同じバッジの色）
    pub fn badge(self) -> BadgeKind {
        match self {
            StatusKind::Info => BadgeKind::Info,
            StatusKind::Success => BadgeKind::Ok,
            StatusKind::Error => BadgeKind::Ng,
        }
    }

    /// 先頭に置くアイコンの文字（Segoe のアイコンフォントがあるときだけ描く。種類は文言でも分かる）
    pub fn glyph(self) -> char {
        match self {
            StatusKind::Info => '\u{E946}',
            StatusKind::Success => '\u{E73E}',
            StatusKind::Error => '\u{E783}',
        }
    }
}

/// ステータスの文言から種類を決める。エラーは呼び出し側が指定する。「…」で終わる文言は進行中、
/// 「しました」を含むものは成功、それ以外はお知らせ
pub fn status_kind(text: &str, is_error: bool) -> StatusKind {
    let text = text.trim();
    if is_error {
        StatusKind::Error
    } else if text.ends_with('…') {
        StatusKind::Info
    } else if text.contains("しました") {
        StatusKind::Success
    } else {
        StatusKind::Info
    }
}

/// 1 つのバッジ（状態の小さな札）の前景・背景
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadgeColors {
    /// 文字
    pub fg: Color,
    /// 背景
    pub bg: Color,
}

/// 色の一式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// テーマの種類
    pub kind: ThemeKind,
    /// 角丸・アクセント色などの装飾を使うか（HC では偽）
    pub decorative: bool,
    /// ページ（右の本体）の背景
    pub page_bg: Color,
    /// サイドバー・フッターの背景
    pub sidebar_bg: Color,
    /// カードの背景
    pub card_bg: Color,
    /// カードの枠
    pub card_border: Color,
    /// 入力欄・選択欄の背景
    pub field_bg: Color,
    /// カードの中の項目の間の区切り・サイドバーとの境の線
    pub divider: Color,
    /// 本文
    pub text: Color,
    /// 説明などの補助の文字
    pub text_secondary: Color,
    /// 無効なボタンの文字（読めなくてよい）
    pub disabled_text: Color,
    /// エラーの文字
    pub error_text: Color,
    /// アクセント（塗り・線）
    pub accent: Color,
    /// アクセントの上の文字・つまみ
    pub on_accent: Color,
    /// アクセントのホバー
    pub accent_hover: Color,
    /// アクセントの押下
    pub accent_pressed: Color,
    /// アクセント色の文字（カード上の控えめなボタン）
    pub accent_text: Color,
    /// サイドバーのホバーの帯
    pub nav_hover_bg: Color,
    /// サイドバーの選択の帯
    pub nav_selected_bg: Color,
    /// サイドバーの選択の帯の上の文字
    pub nav_selected_text: Color,
    /// ボタンの背景（通常・ホバー・押下）
    pub button_bg: [Color; 3],
    /// ボタンの枠
    pub button_border: Color,
    /// ボタンの文字（ハイコントラストでは `COLOR_BTNTEXT`）
    pub button_text: Color,
    /// 警告色のボタンの文字・枠
    pub danger: Color,
    /// トグルスイッチ（オフ）の枠とつまみ
    pub toggle_off: Color,
    /// フォーカスの輪郭
    pub focus: Color,
    /// カードの影の濃さ（黒の不透明度 0..=255。0 = 影なし。HC は 0）
    pub shadow_alpha: u8,
}

impl Palette {
    /// テーマの色の一式
    pub fn for_theme(kind: ThemeKind) -> Palette {
        match kind {
            ThemeKind::Light => Palette::light(),
            ThemeKind::Dark => Palette::dark(),
            ThemeKind::HighContrast => Palette::high_contrast(),
        }
    }

    fn light() -> Palette {
        Palette {
            kind: ThemeKind::Light,
            decorative: true,
            page_bg: Color::hex(0xF3F3F3),
            sidebar_bg: Color::hex(0xEBEBEB),
            card_bg: Color::hex(0xFFFFFF),
            card_border: Color::hex(0xDDDDDD),
            field_bg: Color::hex(0xFFFFFF),
            divider: Color::hex(0xE8E8E8),
            text: Color::hex(0x1A1A1A),
            text_secondary: Color::hex(0x585858),
            disabled_text: Color::hex(0x9A9A9A),
            error_text: Color::hex(0xB3261E),
            accent: Color::hex(0x0067C0),
            on_accent: Color::hex(0xFFFFFF),
            accent_hover: Color::hex(0x005AA8),
            accent_pressed: Color::hex(0x004E8F),
            accent_text: Color::hex(0x0067C0),
            nav_hover_bg: Color::hex(0xE0E0E0),
            nav_selected_bg: Color::hex(0xFFFFFF),
            nav_selected_text: Color::hex(0x1A1A1A),
            button_bg: [
                Color::hex(0xFBFBFB),
                Color::hex(0xF3F3F3),
                Color::hex(0xE8E8E8),
            ],
            button_border: Color::hex(0xC8C8C8),
            button_text: Color::hex(0x1A1A1A),
            danger: Color::hex(0xB3261E),
            toggle_off: Color::hex(0x6B6B6B),
            focus: Color::hex(0x1A1A1A),
            shadow_alpha: 22,
        }
    }

    fn dark() -> Palette {
        Palette {
            kind: ThemeKind::Dark,
            decorative: true,
            page_bg: Color::hex(0x1C1C1C),
            sidebar_bg: Color::hex(0x202020),
            card_bg: Color::hex(0x2B2B2B),
            card_border: Color::hex(0x3D3D3D),
            field_bg: Color::hex(0x333333),
            divider: Color::hex(0x383838),
            text: Color::hex(0xF2F2F2),
            text_secondary: Color::hex(0xB8B8B8),
            disabled_text: Color::hex(0x777777),
            error_text: Color::hex(0xFF99A4),
            accent: Color::hex(0x60CDFF),
            on_accent: Color::hex(0x000000),
            accent_hover: Color::hex(0x7AD6FF),
            accent_pressed: Color::hex(0x45B4E6),
            accent_text: Color::hex(0x60CDFF),
            nav_hover_bg: Color::hex(0x2A2A2A),
            nav_selected_bg: Color::hex(0x333333),
            nav_selected_text: Color::hex(0xF2F2F2),
            button_bg: [
                Color::hex(0x373737),
                Color::hex(0x3F3F3F),
                Color::hex(0x2F2F2F),
            ],
            button_border: Color::hex(0x555555),
            button_text: Color::hex(0xF2F2F2),
            danger: Color::hex(0xFF99A4),
            toggle_off: Color::hex(0xA0A0A0),
            focus: Color::hex(0xF2F2F2),
            shadow_alpha: 90,
        }
    }

    /// ハイコントラスト: システム色だけ。装飾（角丸・アクセント・影）は無い
    fn high_contrast() -> Palette {
        use SysColor::{ButtonFace, ButtonText, GrayText, Highlight, HighlightText, Window};
        use SysColor::{HotLight, WindowText};
        let s = Color::Sys;
        Palette {
            kind: ThemeKind::HighContrast,
            decorative: false,
            page_bg: s(Window),
            sidebar_bg: s(Window),
            card_bg: s(Window),
            card_border: s(WindowText),
            field_bg: s(Window),
            divider: s(GrayText),
            text: s(WindowText),
            text_secondary: s(WindowText),
            disabled_text: s(GrayText),
            error_text: s(WindowText),
            accent: s(Highlight),
            on_accent: s(HighlightText),
            accent_hover: s(HotLight),
            accent_pressed: s(HotLight),
            accent_text: s(HotLight),
            nav_hover_bg: s(Window),
            nav_selected_bg: s(Highlight),
            nav_selected_text: s(HighlightText),
            button_bg: [s(ButtonFace), s(ButtonFace), s(ButtonFace)],
            button_border: s(ButtonText),
            button_text: s(ButtonText),
            danger: s(ButtonText),
            toggle_off: s(WindowText),
            focus: s(WindowText),
            shadow_alpha: 0,
        }
    }

    /// バッジ（診断の結果・フッターのステータス）の色
    pub fn badge(&self, kind: BadgeKind) -> BadgeColors {
        if !self.decorative {
            return BadgeColors {
                fg: self.text,
                bg: self.card_bg,
            };
        }
        let (fg, bg) = match (self.kind, kind) {
            (ThemeKind::Dark, BadgeKind::Ok) => (0x7EDC6F, 0x1E3A22),
            (ThemeKind::Dark, BadgeKind::Warn) => (0xFCE100, 0x4A3F00),
            (ThemeKind::Dark, BadgeKind::Ng) => (0xFF99A4, 0x4A1F24),
            (ThemeKind::Dark, BadgeKind::Neutral) => (0xD0D0D0, 0x3A3A3A),
            (ThemeKind::Dark, BadgeKind::Info) => (0x8AD4FF, 0x1F3A4D),
            (_, BadgeKind::Ok) => (0x0E5A1E, 0xDFF6DD),
            (_, BadgeKind::Warn) => (0x6B4A00, 0xFFF4CE),
            (_, BadgeKind::Ng) => (0xA4262C, 0xFDE7E9),
            (_, BadgeKind::Neutral) => (0x424242, 0xE8E8E8),
            (_, BadgeKind::Info) => (0x004E8C, 0xDCEBFA),
        };
        BadgeColors {
            fg: Color::hex(fg),
            bg: Color::hex(bg),
        }
    }

    /// 検査する色の組 `(名前, 前景, 背景, 必要なコントラスト比)`。文字は 4.5、非文字（トグル・アクセント）は 3.0
    pub fn contrast_pairs(&self) -> Vec<(&'static str, Color, Color, f64)> {
        const TEXT: f64 = 4.5;
        const UI: f64 = 3.0;
        let mut v = vec![
            ("text/page", self.text, self.page_bg, TEXT),
            ("text/sidebar", self.text, self.sidebar_bg, TEXT),
            ("text/card", self.text, self.card_bg, TEXT),
            ("text/field", self.text, self.field_bg, TEXT),
            ("secondary/page", self.text_secondary, self.page_bg, TEXT),
            (
                "secondary/sidebar",
                self.text_secondary,
                self.sidebar_bg,
                TEXT,
            ),
            ("secondary/card", self.text_secondary, self.card_bg, TEXT),
            ("error/card", self.error_text, self.card_bg, TEXT),
            ("error/sidebar", self.error_text, self.sidebar_bg, TEXT),
            ("accent_text/card", self.accent_text, self.card_bg, TEXT),
            ("on_accent/accent", self.on_accent, self.accent, TEXT),
            ("on_accent/hover", self.on_accent, self.accent_hover, TEXT),
            (
                "on_accent/pressed",
                self.on_accent,
                self.accent_pressed,
                TEXT,
            ),
            (
                "nav/selected",
                self.nav_selected_text,
                self.nav_selected_bg,
                TEXT,
            ),
            ("nav/hover", self.text, self.nav_hover_bg, TEXT),
            ("danger/card", self.danger, self.card_bg, TEXT),
            ("focus/card", self.focus, self.card_bg, UI),
            ("focus/sidebar", self.focus, self.sidebar_bg, UI),
            ("toggle_off/card", self.toggle_off, self.card_bg, UI),
            ("accent/card", self.accent, self.card_bg, UI),
            // ページの見出しの下の線・節の見出しの印
            ("accent/page", self.accent, self.page_bg, UI),
            ("accent/sidebar", self.accent, self.sidebar_bg, UI),
        ];
        for (n, name) in ["normal", "hover", "pressed"].into_iter().enumerate() {
            let bg = self.button_bg[n];
            let label = match name {
                "normal" => ["button/normal", "danger/normal", "accent_text/normal"],
                "hover" => ["button/hover", "danger/hover", "accent_text/hover"],
                _ => ["button/pressed", "danger/pressed", "accent_text/pressed"],
            };
            v.push((label[0], self.button_text, bg, TEXT));
            v.push((label[1], self.danger, bg, TEXT));
            v.push((label[2], self.accent_text, bg, TEXT));
        }
        for kind in [
            BadgeKind::Ok,
            BadgeKind::Warn,
            BadgeKind::Ng,
            BadgeKind::Neutral,
            BadgeKind::Info,
        ] {
            let b = self.badge(kind);
            v.push(("badge", b.fg, b.bg, TEXT));
        }
        v
    }
}

// ---- 形（dp。実際の大きさは [`scale`] で px にする） ----

/// カードの角の丸み
pub const CARD_RADIUS: i32 = 8;
/// ボタンの角の丸み
pub const BUTTON_RADIUS: i32 = 4;
/// サイドバーの帯の角の丸み
pub const NAV_RADIUS: i32 = 6;
/// バッジの角の丸み
pub const BADGE_RADIUS: i32 = 4;
/// トグルスイッチのつまみの外側の余白
const TOGGLE_KNOB_PAD: i32 = 4;
/// トグルスイッチの幅
pub const TOGGLE_W: i32 = 40;
/// トグルスイッチの高さ
pub const TOGGLE_H: i32 = 20;
/// トグルスイッチと文字の間隔
pub const TOGGLE_GAP: i32 = 12;
/// 診断の結果行のバッジの幅
pub const BADGE_W: i32 = 72;
/// 診断の結果行のバッジとタイトルの間隔
pub const BADGE_GAP: i32 = 12;

/// 角の丸みの半径（px）。装飾なし（HC）は 0
pub fn radius_px(dp: i32, dpi: u32, decorative: bool) -> i32 {
    if decorative { scale(dp, dpi) } else { 0 }
}

/// トグルスイッチ 1 つの部品の位置（px）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToggleGeometry {
    /// 土台（角丸のカプセル）
    pub track: Rect,
    /// つまみ（円）
    pub knob: Rect,
    /// 文字の左端
    pub text_x: i32,
}

/// トグルスイッチの配置。`bounds` はコントロール全体、`line_h` は 1 行目の高さ（文字が複数行に
/// 折り返すとき、スイッチは 1 行目に揃える）。`on` でつまみが右に寄る。`hot`（押下・ホバー）で
/// つまみを少し大きくする
pub fn toggle_geometry(bounds: Rect, line_h: i32, on: bool, hot: bool, dpi: u32) -> ToggleGeometry {
    let w = scale(TOGGLE_W, dpi);
    let h = scale(TOGGLE_H, dpi);
    let line_h = line_h.max(h);
    let track = Rect {
        x: bounds.x,
        y: bounds.y + (line_h - h) / 2,
        w,
        h,
    };
    let pad = scale(TOGGLE_KNOB_PAD, dpi);
    let grow = if hot { scale(1, dpi) } else { 0 };
    let d = h - 2 * pad + grow;
    let knob_x = if on {
        track.x + w - pad - d + grow / 2
    } else {
        track.x + pad - grow / 2
    };
    ToggleGeometry {
        track,
        knob: Rect {
            x: knob_x,
            y: track.y + (h - d) / 2,
            w: d,
            h: d,
        },
        text_x: bounds.x + w + scale(TOGGLE_GAP, dpi),
    }
}

/// トグルスイッチの左の部分（スイッチ + 間隔）の幅（px）。ラベルの折り返し幅の計算に使う
pub fn toggle_label_offset(dpi: u32) -> i32 {
    scale(TOGGLE_W, dpi) + scale(TOGGLE_GAP, dpi)
}

/// トグルスイッチを含む行の最小の高さ（px）
pub fn toggle_row_min_height(dpi: u32) -> i32 {
    scale(TOGGLE_H + 4, dpi)
}

// ---- サイドバーのアイコン（Segoe Fluent Icons / Segoe MDL2 Assets の文字） ----

/// サイドバーの項目のアイコン。ページの識別（[`crate::native_ui_state::page_key`] と同じ文字列）と
/// 表示名から決める。分からなければ歯車
pub fn page_glyph(key: &str, title: &str) -> char {
    match key {
        "update" => '\u{E895}',
        "data" => '\u{E8B7}',
        "extra:diag" => '\u{E9D9}',
        _ => {
            if title.contains("入力") || title.contains("キー") {
                '\u{E765}'
            } else if title.contains("変換") {
                '\u{E8D2}'
            } else if title.contains("学習") {
                '\u{E7BE}'
            } else {
                '\u{E713}'
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs_pass(kind: ThemeKind) {
        let p = Palette::for_theme(kind);
        let mut checked = 0;
        for (name, fg, bg, need) in p.contrast_pairs() {
            let (Some(f), Some(b)) = (fg.rgb(), bg.rgb()) else {
                continue;
            };
            let ratio = contrast_ratio(f, b);
            assert!(
                ratio >= need,
                "{kind:?} {name}: {ratio:.2} < {need} ({f:?} on {b:?})"
            );
            checked += 1;
        }
        assert!(checked > 20, "検査が少なすぎる: {checked}");
    }

    #[test]
    fn light_palette_meets_contrast() {
        pairs_pass(ThemeKind::Light);
    }

    #[test]
    fn dark_palette_meets_contrast() {
        pairs_pass(ThemeKind::Dark);
    }

    #[test]
    fn high_contrast_uses_system_colors_only() {
        let p = Palette::for_theme(ThemeKind::HighContrast);
        assert!(!p.decorative);
        for (name, fg, bg, _) in p.contrast_pairs() {
            assert!(
                fg.rgb().is_none() && bg.rgb().is_none(),
                "HC に固定の色がある: {name}"
            );
        }
        // 装飾（角丸）は 0
        assert_eq!(radius_px(CARD_RADIUS, 144, p.decorative), 0);
        assert!(radius_px(CARD_RADIUS, 144, true) > 0);
        // エラー・バッジも色分けに頼らずシステム色
        assert!(p.error_text.rgb().is_none());
        assert!(p.badge(BadgeKind::Ng).fg.rgb().is_none());
    }

    #[test]
    fn contrast_ratio_matches_known_values() {
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
        // 順序によらない
        assert_eq!(contrast_ratio(black, white), contrast_ratio(white, black));
        // 灰色 #767676 と白はちょうど 4.5 付近（WCAG の有名な境界）
        let r = contrast_ratio(Rgb::hex(0x767676), white);
        assert!((4.5..4.6).contains(&r), "{r}");
    }

    #[test]
    fn colorref_and_argb_pack_channels() {
        let c = Rgb::hex(0x112233);
        assert_eq!(c.colorref(), 0x0033_2211);
        assert_eq!(c.argb(), 0xFF11_2233);
        assert_eq!(c.argb_alpha(0x40), 0x4011_2233);
        assert_eq!(c.argb_alpha(0), 0x0011_2233);
    }

    #[test]
    fn shadow_is_subtle_and_disabled_in_high_contrast() {
        let l = Palette::for_theme(ThemeKind::Light);
        let d = Palette::for_theme(ThemeKind::Dark);
        let h = Palette::for_theme(ThemeKind::HighContrast);
        // 見えるが目立たない（0 < alpha <= 25%）。暗い地では黒の影が見えにくいので濃くする
        assert!(l.shadow_alpha > 0 && l.shadow_alpha <= 64);
        assert!(d.shadow_alpha > l.shadow_alpha && d.shadow_alpha <= 128);
        assert_eq!(h.shadow_alpha, 0);
    }

    #[test]
    fn theme_decision_prefers_high_contrast() {
        assert_eq!(ThemeKind::decide(true, true), ThemeKind::HighContrast);
        assert_eq!(ThemeKind::decide(true, false), ThemeKind::HighContrast);
        assert_eq!(ThemeKind::decide(false, true), ThemeKind::Dark);
        assert_eq!(ThemeKind::decide(false, false), ThemeKind::Light);
    }

    #[test]
    fn light_and_dark_differ_in_every_surface() {
        let l = Palette::for_theme(ThemeKind::Light);
        let d = Palette::for_theme(ThemeKind::Dark);
        for (a, b) in [
            (l.page_bg, d.page_bg),
            (l.card_bg, d.card_bg),
            (l.text, d.text),
            (l.accent, d.accent),
        ] {
            assert_ne!(a, b);
        }
        // ダークの背景はライトより暗い
        let lum = |c: Color| c.rgb().map(Rgb::luminance).unwrap_or(0.0);
        assert!(lum(d.page_bg) < lum(l.page_bg));
        assert!(lum(d.card_bg) < lum(l.card_bg));
    }

    #[test]
    fn card_stands_out_from_the_page() {
        // カードは枠と背景で、ページと区別できる（ライト・ダーク）
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            let p = Palette::for_theme(kind);
            assert_ne!(p.card_bg, p.page_bg);
            assert_ne!(p.card_border, p.card_bg);
            assert_ne!(p.sidebar_bg, p.page_bg);
        }
    }

    #[test]
    fn button_styles_by_id() {
        use crate::native_model::ids;
        assert_eq!(button_style_for_id(ids::UPDATE_CHECK), ButtonStyle::Primary);
        assert_eq!(button_style_for_id(ids::DIAG_RUN_ALL), ButtonStyle::Primary);
        assert_eq!(
            button_style_for_id(ids::CLEAR_LEARNING),
            ButtonStyle::Danger
        );
        assert_eq!(button_style_for_id(ids::CLOSE), ButtonStyle::Normal);
        assert_eq!(button_style_for_id(ids::OPEN_CONFIG), ButtonStyle::Normal);
        assert_eq!(
            button_style_for_id(ids::DYNAMIC_BASE + 3),
            ButtonStyle::Normal
        );
    }

    #[test]
    fn status_kind_from_text() {
        assert_eq!(status_kind("保存しました", false), StatusKind::Success);
        assert_eq!(status_kind("保存しています…", false), StatusKind::Info);
        assert_eq!(status_kind("エラー: 失敗", true), StatusKind::Error);
        // エラーの指定は文言より優先
        assert_eq!(status_kind("保存しました", true), StatusKind::Error);
        // 始まっただけ（終わっていない）はお知らせ
        assert_eq!(status_kind("動作確認を始めました", false), StatusKind::Info);
        assert_eq!(status_kind("", false), StatusKind::Info);
        // 種類はアイコンでも分かる（色だけに頼らない）
        let g = [StatusKind::Info, StatusKind::Success, StatusKind::Error].map(StatusKind::glyph);
        assert!(g[0] != g[1] && g[1] != g[2] && g[0] != g[2]);
    }

    #[test]
    fn toggle_stays_inside_its_bounds_for_every_dpi() {
        for dpi in [96, 120, 144, 192, 240] {
            let bounds = Rect {
                x: 10,
                y: 20,
                w: scale(400, dpi),
                h: scale(40, dpi),
            };
            for (on, hot) in [(false, false), (true, false), (false, true), (true, true)] {
                let g = toggle_geometry(bounds, scale(24, dpi), on, hot, dpi);
                // つまみは土台の中
                assert!(g.knob.x >= g.track.x && g.knob.x + g.knob.w <= g.track.x + g.track.w);
                assert!(g.knob.y >= g.track.y && g.knob.y + g.knob.h <= g.track.y + g.track.h);
                // 土台は左端、文字は土台の右
                assert_eq!(g.track.x, bounds.x);
                assert!(g.text_x > g.track.x + g.track.w);
                assert_eq!(g.text_x - bounds.x, toggle_label_offset(dpi));
                // 1 行目の中央に揃う
                assert!(g.track.y >= bounds.y);
            }
            let off = toggle_geometry(bounds, 0, false, false, dpi);
            let on = toggle_geometry(bounds, 0, true, false, dpi);
            assert!(on.knob.x > off.knob.x, "オンはつまみが右");
            assert!(toggle_row_min_height(dpi) > off.track.h);
        }
    }

    #[test]
    fn glyphs_fall_back_to_the_gear() {
        assert_eq!(page_glyph("update", "更新"), '\u{E895}');
        assert_eq!(page_glyph("category:入力", "入力"), '\u{E765}');
        assert_eq!(page_glyph("category:変換", "変換"), '\u{E8D2}');
        assert_eq!(page_glyph("category:学習", "学習"), '\u{E7BE}');
        assert_eq!(page_glyph("category:未知", "未知"), '\u{E713}');
        // 文字はすべて私用領域（Segoe のアイコンフォント）
        for key in ["update", "data", "extra:diag", "category:x"] {
            let c = page_glyph(key, "x") as u32;
            assert!((0xE000..=0xF8FF).contains(&c));
        }
    }
}
