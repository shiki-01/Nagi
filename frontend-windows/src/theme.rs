//! 候補ウィンドウ・トーストの配色。OS からの値の取得は TSF 層が行い、ここは
//! 「テーマ設定 → パレット」の選択だけを持つ（OS 非依存・テスト対象）。

/// 8bit RGB（`COLORREF` に依存しない）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// GDI の `COLORREF` の値（0x00BBGGRR）
    pub fn to_colorref(self) -> u32 {
        u32::from(self.0) | (u32::from(self.1) << 8) | (u32::from(self.2) << 16)
    }

    /// WCAG の相対輝度
    fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// WCAG のコントラスト比（1.0〜21.0）
    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

/// 描画に使う色一式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// 候補ウィンドウの背景
    pub background: Rgb,
    /// 候補ウィンドウの枠線
    pub border: Rgb,
    /// 本文（候補の表層）
    pub text: Rgb,
    /// 補助テキスト（番号・出所ラベル・フッター）
    pub sub_text: Rgb,
    /// 選択行の背景
    pub selected_bg: Rgb,
    /// 選択行の本文
    pub selected_text: Rgb,
    /// 選択行の補助テキスト
    pub selected_sub_text: Rgb,
    /// トーストの背景
    pub toast_bg: Rgb,
    /// トーストの文字
    pub toast_text: Rgb,
}

/// ハイコントラスト時に使うシステム色（`GetSysColor` の値を TSF 側で渡す）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemColors {
    /// COLOR_WINDOW
    pub window: Rgb,
    /// COLOR_WINDOWTEXT
    pub window_text: Rgb,
    /// COLOR_HIGHLIGHT
    pub highlight: Rgb,
    /// COLOR_HIGHLIGHTTEXT
    pub highlight_text: Rgb,
    /// COLOR_GRAYTEXT
    pub gray_text: Rgb,
}

/// OS から得たテーマ情報
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThemeInput {
    /// `AppsUseLightTheme`（0 = ダーク）。読めなければ `None`（ライト扱い）
    pub apps_use_light_theme: Option<u32>,
    /// ハイコントラスト有効時のシステム色
    pub high_contrast: Option<SystemColors>,
}

/// ライトのパレット
pub const LIGHT: Palette = Palette {
    background: Rgb(255, 255, 255),
    border: Rgb(160, 160, 160),
    text: Rgb(0, 0, 0),
    sub_text: Rgb(96, 96, 96),
    selected_bg: Rgb(0, 102, 204),
    selected_text: Rgb(255, 255, 255),
    selected_sub_text: Rgb(255, 255, 255),
    toast_bg: Rgb(43, 43, 48),
    toast_text: Rgb(255, 255, 255),
};

/// ダークのパレット
pub const DARK: Palette = Palette {
    background: Rgb(43, 43, 43),
    border: Rgb(85, 85, 85),
    text: Rgb(255, 255, 255),
    sub_text: Rgb(170, 170, 170),
    selected_bg: Rgb(0, 95, 184),
    selected_text: Rgb(255, 255, 255),
    selected_sub_text: Rgb(255, 255, 255),
    toast_bg: Rgb(64, 64, 68),
    toast_text: Rgb(255, 255, 255),
};

/// ハイコントラスト時のパレット（システム色をそのまま使う）
pub fn high_contrast_palette(c: SystemColors) -> Palette {
    Palette {
        background: c.window,
        border: c.window_text,
        text: c.window_text,
        sub_text: c.window_text,
        selected_bg: c.highlight,
        selected_text: c.highlight_text,
        selected_sub_text: c.highlight_text,
        toast_bg: c.window,
        toast_text: c.window_text,
    }
}

/// テーマ情報からパレットを選ぶ。ハイコントラストが最優先、次にダーク、既定はライト
pub fn select_palette(input: ThemeInput) -> Palette {
    if let Some(c) = input.high_contrast {
        return high_contrast_palette(c);
    }
    match input.apps_use_light_theme {
        Some(0) => DARK,
        _ => LIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_bgr() {
        assert_eq!(Rgb(0x2B, 0x2B, 0x30).to_colorref(), 0x0030_2B2B);
        assert_eq!(Rgb(1, 2, 3).to_colorref(), 0x0003_0201);
    }

    #[test]
    fn default_is_light() {
        assert_eq!(select_palette(ThemeInput::default()), LIGHT);
        let light = ThemeInput {
            apps_use_light_theme: Some(1),
            high_contrast: None,
        };
        assert_eq!(select_palette(light), LIGHT);
    }

    #[test]
    fn zero_is_dark() {
        let dark = ThemeInput {
            apps_use_light_theme: Some(0),
            high_contrast: None,
        };
        assert_eq!(select_palette(dark), DARK);
    }

    #[test]
    fn contrast_ratio_sanity() {
        assert!((Rgb(0, 0, 0).contrast(Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((Rgb(9, 9, 9).contrast(Rgb(9, 9, 9)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn palettes_are_readable() {
        for p in [LIGHT, DARK] {
            assert!(p.text.contrast(p.background) >= 4.5);
            assert!(p.sub_text.contrast(p.background) >= 4.5);
            assert!(p.selected_text.contrast(p.selected_bg) >= 4.5);
            assert!(p.selected_sub_text.contrast(p.selected_bg) >= 4.5);
            assert!(p.toast_text.contrast(p.toast_bg) >= 4.5);
        }
    }

    #[test]
    fn dark_selection_is_distinct_from_background() {
        assert!(DARK.selected_bg.contrast(DARK.background) >= 1.5);
        assert!(DARK.border.contrast(DARK.background) > 1.0);
    }

    #[test]
    fn high_contrast_uses_system_colors_even_in_dark() {
        let c = SystemColors {
            window: Rgb(0, 0, 0),
            window_text: Rgb(255, 255, 255),
            highlight: Rgb(0, 255, 255),
            highlight_text: Rgb(0, 0, 0),
            gray_text: Rgb(0, 255, 0),
        };
        let p = select_palette(ThemeInput {
            apps_use_light_theme: Some(0),
            high_contrast: Some(c),
        });
        assert_eq!(p.background, c.window);
        assert_eq!(p.text, c.window_text);
        assert_eq!(p.selected_bg, c.highlight);
        assert_eq!(p.selected_text, c.highlight_text);
        assert_eq!(p.toast_bg, c.window);
    }
}
