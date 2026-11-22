//! WebView2 ウィンドウの背景色とテーマの種類（OS 非依存）。

/// RGB（各 0..=255）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `0xRRGGBB` から作る
    pub const fn hex(v: u32) -> Self {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_splits_the_channels() {
        assert_eq!(Rgb::hex(0x12_34_56), Rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn high_contrast_wins_over_dark() {
        assert_eq!(ThemeKind::decide(true, true), ThemeKind::HighContrast);
        assert_eq!(ThemeKind::decide(false, true), ThemeKind::Dark);
        assert_eq!(ThemeKind::decide(false, false), ThemeKind::Light);
    }
}
