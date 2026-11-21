//! 変換 UI（候補ウィンドウ・通知・モード表示・入力中の下線）の見た目の解決（OS 非依存）。
//!
//! 設定 `windows.candidate_*` などは「テンプレート（`candidate_theme`）が決める既定値」と「項目ごとの明示指定」
//! からなり、明示指定が常にテンプレートに優先する（CLAUDE.md の設定方針と同じ）。ここは
//! 「設定項目 → テンプレートの値 + 明示値 → 範囲に丸めた実効値 [`CandidateStyle`]」の合成と、
//! 実効値から描画に渡す数値（DPI 換算・フォントの大きさ・配色）を求める純粋関数だけを持つ。
//! 設定値の取得（コアの `get_settings`）と GDI の描画は TSF 層が行う（decisions.md H-119）。
//!
//! 長さの単位 dp は 96 DPI 基準の px（[`scale_px`] で画面の DPI に拡大する）。

use std::time::{Duration, Instant};

use nagi_protocol::{SettingItem, SettingValue};

use crate::candidates::{FontRole, Metrics, scale_px};
use crate::preedit_style::UnderlineKind;
use crate::theme::{self, Palette, Rgb, ThemeInput};
use crate::toast::ToastPosition;

// ---- 設定キーと範囲（`nagi_config::windows_ui` と同じ値。テストが食い違いを検出する）----

/// 設定項目のキー（`config.toml` の `[windows]`）
pub mod key {
    /// テンプレート
    pub const THEME: &str = "windows.candidate_theme";
    /// 文字の大きさの倍率（%）
    pub const FONT_SCALE: &str = "windows.candidate_font_scale";
    /// 行の高さ
    pub const ROW_HEIGHT: &str = "windows.candidate_row_height";
    /// 左右の余白
    pub const PADDING: &str = "windows.candidate_padding";
    /// 角丸
    pub const CORNER_RADIUS: &str = "windows.candidate_corner_radius";
    /// 影
    pub const SHADOW: &str = "windows.candidate_shadow";
    /// 番号ラベル
    pub const NUMBER: &str = "windows.candidate_number";
    /// 注釈
    pub const ANNOTATION: &str = "windows.candidate_annotation";
    /// 選択行の強調
    pub const SELECTION: &str = "windows.candidate_selection";
    /// 背景の不透明度
    pub const OPACITY: &str = "windows.candidate_opacity";
    /// 配色
    pub const COLOR: &str = "windows.candidate_color";
    /// 最大幅
    pub const MAX_WIDTH: &str = "windows.candidate_max_width";
    /// ページ送りの表示
    pub const PAGE_INDICATOR: &str = "windows.candidate_page_indicator";
    /// 入力位置からの距離
    pub const GAP: &str = "windows.candidate_gap";
    /// 入力中の下線
    pub const UNDERLINE: &str = "windows.preedit_underline";
    /// 通知の表示時間（秒）
    pub const TOAST_DURATION: &str = "windows.toast_duration_sec";
    /// 通知の位置
    pub const TOAST_POSITION: &str = "windows.toast_position";
    /// モード表示の有無
    pub const MODE_POPUP: &str = "windows.mode_popup";
    /// モード表示の時間（ミリ秒）
    pub const MODE_POPUP_DURATION: &str = "windows.mode_popup_duration_ms";
    /// モード表示の位置
    pub const MODE_POPUP_POSITION: &str = "windows.mode_popup_position";
}

/// 値の範囲 `(最小, 最大)`
pub type Bounds = (i32, i32);

/// 文字の倍率（%）
pub const FONT_SCALE: Bounds = (60, 200);
/// 行の高さ（dp）
pub const ROW_HEIGHT: Bounds = (16, 64);
/// 左右の余白（dp）
pub const PADDING: Bounds = (0, 32);
/// 角丸（dp）
pub const CORNER_RADIUS: Bounds = (0, 16);
/// 不透明度（%）
pub const OPACITY: Bounds = (30, 100);
/// 最大幅（dp）
pub const MAX_WIDTH: Bounds = (120, 1200);
/// 入力位置からの距離（dp）
pub const CARET_GAP: Bounds = (0, 40);
/// 通知の表示時間（秒）
pub const TOAST_DURATION_SEC: Bounds = (1, 30);
/// モード表示の表示時間（ミリ秒）
pub const MODE_POPUP_DURATION_MS: Bounds = (200, 5000);

// ---- 選択肢 ----

/// テンプレート
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// 標準（従来の見た目）
    #[default]
    Standard,
    /// コンパクト
    Compact,
    /// ゆったり
    Spacious,
    /// ミニマル（番号と候補のみ）
    Minimal,
    /// くっきり（高コントラスト寄り）
    Contrast,
}

impl Theme {
    /// すべてのテンプレート（設定画面の並び順）
    pub const ALL: [Theme; 5] = [
        Theme::Standard,
        Theme::Compact,
        Theme::Spacious,
        Theme::Minimal,
        Theme::Contrast,
    ];

    /// 設定値（`standard` など）から。知らない値は `None`
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "standard" => Theme::Standard,
            "compact" => Theme::Compact,
            "spacious" => Theme::Spacious,
            "minimal" => Theme::Minimal,
            "contrast" => Theme::Contrast,
            _ => return None,
        })
    }

    /// 設定値
    pub const fn as_str(self) -> &'static str {
        match self {
            Theme::Standard => "standard",
            Theme::Compact => "compact",
            Theme::Spacious => "spacious",
            Theme::Minimal => "minimal",
            Theme::Contrast => "contrast",
        }
    }
}

/// 番号ラベルの表示と位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberPlacement {
    /// 候補の左
    Left,
    /// 右端
    Right,
    /// 表示しない
    Hidden,
}

/// 注釈の表示
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation {
    /// 出所と、曖昧入力で補正した読み
    Full,
    /// 出所のみ
    Source,
    /// 表示しない
    None,
}

/// 選択行の強調
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStyle {
    /// 行を塗りつぶす
    Fill,
    /// 左にバー
    Bar,
    /// 枠線
    Outline,
}

/// 配色の決め方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    /// システムのライト/ダークに従う
    System,
    /// ライト固定
    Light,
    /// ダーク固定
    Dark,
    /// テンプレート固有の配色で固定
    Template,
}

/// ページ送りの表示
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageIndicator {
    /// 下部に「2/5」
    Footer,
    /// 表示しない
    None,
}

/// モード表示の位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModePopupPosition {
    /// キャレットの近く
    NearCaret,
    /// 作業領域の隅
    Corner(ToastPosition),
}

// ---- テンプレートの値 ----

/// テンプレートが決める値（明示指定が無いときに使う）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Template {
    font_percent: i32,
    row_height: i32,
    padding: i32,
    corner_radius: i32,
    shadow: bool,
    number: NumberPlacement,
    annotation: Annotation,
    selection: SelectionStyle,
    opacity: i32,
    color: ColorScheme,
    max_width: i32,
    page_indicator: PageIndicator,
    caret_gap: i32,
    underline: UnderlineKind,
    /// 枠線の太さ（dp。設定項目ではなくテンプレート固有）
    border: i32,
    toast_corner: i32,
    toast_pad_x: i32,
    toast_pad_y: i32,
}

/// 「標準」。値は従来の固定値（`Metrics::default()` と `toast_window` の定数）と同じ
const STANDARD: Template = Template {
    font_percent: 100,
    row_height: 24,
    padding: 8,
    corner_radius: 0,
    shadow: false,
    number: NumberPlacement::Left,
    annotation: Annotation::Source,
    selection: SelectionStyle::Fill,
    opacity: 100,
    color: ColorScheme::System,
    max_width: 800,
    page_indicator: PageIndicator::Footer,
    caret_gap: 2,
    underline: UnderlineKind::Solid,
    border: 1,
    toast_corner: 12,
    toast_pad_x: 14,
    toast_pad_y: 10,
};

impl Theme {
    fn template(self) -> Template {
        match self {
            Theme::Standard => STANDARD,
            Theme::Compact => Template {
                font_percent: 90,
                row_height: 20,
                padding: 6,
                max_width: 480,
                caret_gap: 1,
                toast_corner: 8,
                toast_pad_x: 10,
                toast_pad_y: 6,
                ..STANDARD
            },
            Theme::Spacious => Template {
                font_percent: 115,
                row_height: 32,
                padding: 12,
                corner_radius: 8,
                shadow: true,
                annotation: Annotation::Full,
                max_width: 900,
                caret_gap: 4,
                toast_corner: 14,
                toast_pad_x: 18,
                toast_pad_y: 12,
                ..STANDARD
            },
            Theme::Minimal => Template {
                annotation: Annotation::None,
                page_indicator: PageIndicator::None,
                max_width: 600,
                ..STANDARD
            },
            Theme::Contrast => Template {
                font_percent: 110,
                row_height: 28,
                padding: 10,
                border: 2,
                toast_corner: 4,
                toast_pad_x: 16,
                toast_pad_y: 12,
                ..STANDARD
            },
        }
    }

    /// 「テンプレートの配色で固定」のときの配色
    fn fixed_palette(self) -> Palette {
        match self {
            Theme::Contrast => CONTRAST_DARK,
            _ => theme::LIGHT,
        }
    }

    /// ライト/ダークそれぞれの配色
    fn palette(self, dark: bool) -> Palette {
        match (self, dark) {
            (Theme::Contrast, false) => CONTRAST_LIGHT,
            (Theme::Contrast, true) => CONTRAST_DARK,
            (_, false) => theme::LIGHT,
            (_, true) => theme::DARK,
        }
    }
}

/// 「くっきり」のライト: 白地に黒の太い枠・濃い青の選択
pub const CONTRAST_LIGHT: Palette = Palette {
    background: Rgb(255, 255, 255),
    border: Rgb(0, 0, 0),
    text: Rgb(0, 0, 0),
    sub_text: Rgb(51, 51, 51),
    selected_bg: Rgb(0, 0, 153),
    selected_text: Rgb(255, 255, 255),
    selected_sub_text: Rgb(255, 255, 255),
    toast_bg: Rgb(0, 0, 0),
    toast_text: Rgb(255, 255, 255),
};

/// 「くっきり」のダーク（テンプレート固定の配色でもある）: 黒地に白の枠・黄色の選択
pub const CONTRAST_DARK: Palette = Palette {
    background: Rgb(0, 0, 0),
    border: Rgb(255, 255, 255),
    text: Rgb(255, 255, 255),
    sub_text: Rgb(217, 217, 217),
    selected_bg: Rgb(255, 255, 0),
    selected_text: Rgb(0, 0, 0),
    selected_sub_text: Rgb(0, 0, 0),
    toast_bg: Rgb(0, 0, 0),
    toast_text: Rgb(255, 255, 255),
};

// ---- 明示指定 ----

/// 項目ごとの明示指定（`None` はテンプレートに従う）。範囲の検査・丸めは [`resolve`] が行う
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// 文字の倍率（%）
    pub font_percent: Option<i32>,
    /// 行の高さ（dp）
    pub row_height: Option<i32>,
    /// 左右の余白（dp）
    pub padding: Option<i32>,
    /// 角丸（dp）
    pub corner_radius: Option<i32>,
    /// 影
    pub shadow: Option<bool>,
    /// 番号ラベル
    pub number: Option<NumberPlacement>,
    /// 注釈
    pub annotation: Option<Annotation>,
    /// 選択行の強調
    pub selection: Option<SelectionStyle>,
    /// 不透明度（%）
    pub opacity: Option<i32>,
    /// 配色
    pub color: Option<ColorScheme>,
    /// 最大幅（dp）
    pub max_width: Option<i32>,
    /// ページ送りの表示
    pub page_indicator: Option<PageIndicator>,
    /// 入力位置からの距離（dp）
    pub caret_gap: Option<i32>,
    /// 入力中の下線
    pub underline: Option<UnderlineKind>,
}

// ---- 実効値 ----

/// 実効値。テンプレートと明示指定を合成し、範囲に丸めたもの。長さは dp（96 DPI 基準）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateStyle {
    /// テンプレート
    pub theme: Theme,
    /// 文字の倍率（%）
    pub font_percent: i32,
    /// 行の高さ（dp。文字が収まる高さに引き上げ済み）
    pub row_height: i32,
    /// 左右の余白（dp）
    pub padding: i32,
    /// 角丸（dp。0 で角丸なし）
    pub corner_radius: i32,
    /// 影
    pub shadow: bool,
    /// 番号ラベル
    pub number: NumberPlacement,
    /// 注釈
    pub annotation: Annotation,
    /// 選択行の強調
    pub selection: SelectionStyle,
    /// 不透明度（%）
    pub opacity: i32,
    /// 配色
    pub color: ColorScheme,
    /// 最大幅（dp）
    pub max_width: i32,
    /// ページ送りの表示
    pub page_indicator: PageIndicator,
    /// 入力位置からの距離（dp）
    pub caret_gap: i32,
    /// 入力中の文字列の下線（通常の文節）
    pub underline: UnderlineKind,
    /// 枠線の太さ（dp）
    pub border: i32,
    /// 通知・モード表示の角丸（dp）
    pub toast_corner: i32,
    /// 通知・モード表示の左右の余白（dp）
    pub toast_pad_x: i32,
    /// 通知・モード表示の上下の余白（dp）
    pub toast_pad_y: i32,
    /// 通知を出しておく時間
    pub toast_duration: Duration,
    /// 通知の位置
    pub toast_position: ToastPosition,
    /// モード切替の表示をするか
    pub mode_popup: bool,
    /// モード表示を出しておく時間
    pub mode_popup_duration: Duration,
    /// モード表示の位置
    pub mode_popup_position: ModePopupPosition,
}

impl Default for CandidateStyle {
    fn default() -> Self {
        resolve(Theme::Standard, &Overrides::default(), &Settings::default()).0
    }
}

/// テンプレートに依らない設定（通知・モード表示）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// 通知の表示時間（秒）
    pub toast_duration_sec: i32,
    /// 通知の位置
    pub toast_position: ToastPosition,
    /// モード切替の表示
    pub mode_popup: bool,
    /// モード表示の時間（ミリ秒）
    pub mode_popup_duration_ms: i32,
    /// モード表示の位置
    pub mode_popup_position: ModePopupPosition,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            toast_duration_sec: 4,
            toast_position: ToastPosition::BottomRight,
            mode_popup: true,
            mode_popup_duration_ms: 1000,
            mode_popup_position: ModePopupPosition::NearCaret,
        }
    }
}

fn clamp(value: i32, (min, max): Bounds) -> i32 {
    value.clamp(min, max)
}

/// 基準の文字の大きさ（96 DPI の px。倍率 100% のとき）
const BASE_SURFACE_PX: i32 = 15;
const BASE_NUMBER_PX: i32 = 13;
const BASE_LABEL_PX: i32 = 11;

/// 文字の大きさ（dp）。倍率を掛けて四捨五入し、読めない小ささ（8 未満）にはしない
pub fn font_px(role: FontRole, percent: i32) -> i32 {
    let base = match role {
        FontRole::Surface => BASE_SURFACE_PX,
        FontRole::Number => BASE_NUMBER_PX,
        FontRole::Label => BASE_LABEL_PX,
    };
    ((base * percent + 50) / 100).max(8)
}

/// 文字が収まる行の高さの下限（dp）。候補の文字の高さの 1.4 倍
pub fn min_row_height(percent: i32) -> i32 {
    (font_px(FontRole::Surface, percent) * 14 + 9) / 10
}

/// テンプレートと明示指定から実効値を作る。範囲外の値は範囲に丸め、そのことを `notes` に書く
/// （日本語。設定値の内容だけで入力内容は含まない）
pub fn resolve(theme: Theme, o: &Overrides, s: &Settings) -> (CandidateStyle, Vec<String>) {
    let t = theme.template();
    let mut notes = Vec::new();
    let mut ranged = |name: &str, value: Option<i32>, default: i32, bounds: Bounds| -> i32 {
        match value {
            None => default,
            Some(v) if (bounds.0..=bounds.1).contains(&v) => v,
            Some(v) => {
                let c = clamp(v, bounds);
                notes.push(format!(
                    "{name} は {} から {} の間で指定してください（{v} は {c} として扱います）",
                    bounds.0, bounds.1
                ));
                c
            }
        }
    };
    let font_percent = ranged(key::FONT_SCALE, o.font_percent, t.font_percent, FONT_SCALE);
    let row_height = ranged(key::ROW_HEIGHT, o.row_height, t.row_height, ROW_HEIGHT);
    let padding = ranged(key::PADDING, o.padding, t.padding, PADDING);
    let corner_radius = ranged(
        key::CORNER_RADIUS,
        o.corner_radius,
        t.corner_radius,
        CORNER_RADIUS,
    );
    let opacity = ranged(key::OPACITY, o.opacity, t.opacity, OPACITY);
    let max_width = ranged(key::MAX_WIDTH, o.max_width, t.max_width, MAX_WIDTH);
    let caret_gap = ranged(key::GAP, o.caret_gap, t.caret_gap, CARET_GAP);
    let toast_sec = ranged(
        key::TOAST_DURATION,
        Some(s.toast_duration_sec),
        STANDARD_TOAST_SEC,
        TOAST_DURATION_SEC,
    );
    let mode_ms = ranged(
        key::MODE_POPUP_DURATION,
        Some(s.mode_popup_duration_ms),
        STANDARD_MODE_MS,
        MODE_POPUP_DURATION_MS,
    );
    let style = CandidateStyle {
        theme,
        font_percent,
        // 文字が収まらない高さにはしない
        row_height: row_height.max(min_row_height(font_percent)),
        padding,
        corner_radius,
        shadow: o.shadow.unwrap_or(t.shadow),
        number: o.number.unwrap_or(t.number),
        annotation: o.annotation.unwrap_or(t.annotation),
        selection: o.selection.unwrap_or(t.selection),
        opacity,
        color: o.color.unwrap_or(t.color),
        max_width,
        page_indicator: o.page_indicator.unwrap_or(t.page_indicator),
        caret_gap,
        underline: o.underline.unwrap_or(t.underline),
        border: t.border,
        toast_corner: t.toast_corner,
        toast_pad_x: t.toast_pad_x,
        toast_pad_y: t.toast_pad_y,
        toast_duration: Duration::from_secs(u64::from(toast_sec.unsigned_abs())),
        toast_position: s.toast_position,
        mode_popup: s.mode_popup,
        mode_popup_duration: Duration::from_millis(u64::from(mode_ms.unsigned_abs())),
        mode_popup_position: s.mode_popup_position,
    };
    (style, notes)
}

const STANDARD_TOAST_SEC: i32 = 4;
const STANDARD_MODE_MS: i32 = 1000;

impl CandidateStyle {
    /// 画面の DPI に合わせた寸法（px）
    pub fn metrics(&self, dpi: u32) -> Metrics {
        let base = Metrics::default();
        let s = |dp: i32| scale_px(dp, dpi);
        Metrics {
            row_height: s(self.row_height),
            padding: s(self.padding),
            // 注釈との間隔・番号欄の余白・フッターの高さは文字の大きさに比例させる
            gap: s(base.gap * self.font_percent / 100),
            number_extra: s(base.number_extra * self.font_percent / 100),
            footer_height: s(base.footer_height * self.font_percent / 100),
            top_inset: s(self.border.max(base.top_inset)),
            bottom_inset: s(self.border.max(base.bottom_inset)),
            min_width: s(base.min_width),
            max_width: s(self.max_width),
        }
    }

    /// 画面の DPI に合わせた文字の大きさ（px）
    pub fn font_px(&self, role: FontRole, dpi: u32) -> i32 {
        scale_px(font_px(role, self.font_percent), dpi)
    }

    /// 枠線の太さ（px。1 以上）
    pub fn border_px(&self, dpi: u32) -> i32 {
        scale_px(self.border, dpi).max(1)
    }

    /// 通知・モード表示の文字の大きさ（候補の本文と同じ倍率）
    pub fn toast_font_px(&self, dpi: u32) -> i32 {
        self.font_px(FontRole::Surface, dpi)
    }

    /// ハイコントラスト（システムの色を最優先）のときの実効値。半透明と角丸は使わない
    pub fn for_high_contrast(mut self) -> Self {
        self.opacity = 100;
        self.corner_radius = 0;
        self.toast_corner = 0;
        self.shadow = false;
        self
    }

    /// 配色。ハイコントラストが最優先、次に `color` の指定、`system` なら OS のライト/ダーク
    pub fn palette(&self, input: ThemeInput) -> Palette {
        if let Some(c) = input.high_contrast {
            return theme::high_contrast_palette(c);
        }
        let dark = match self.color {
            ColorScheme::Template => return self.theme.fixed_palette(),
            ColorScheme::Light => false,
            ColorScheme::Dark => true,
            ColorScheme::System => input.apps_use_light_theme == Some(0),
        };
        self.theme.palette(dark)
    }

    /// 影を付けるか。半透明のウィンドウ（レイヤードウィンドウ）にはクラスの影が付かない
    pub fn has_shadow(&self) -> bool {
        self.shadow && self.opacity >= 100
    }

    /// 不透明度（0〜255 のアルファ値）
    pub fn alpha(&self) -> u8 {
        u8::try_from((self.opacity.clamp(0, 100) * 255 + 50) / 100).unwrap_or(255)
    }
}

/// 1 行の色（選択の強調の仕方で変わる）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowColors {
    /// 行を塗る色
    pub fill: Option<Rgb>,
    /// 左のバーの色
    pub bar: Option<Rgb>,
    /// 枠線の色
    pub outline: Option<Rgb>,
    /// 候補の文字色
    pub main: Rgb,
    /// 番号・注釈の文字色
    pub sub: Rgb,
}

/// 行の色。`Fill` の選択行だけ文字色も反転し、`Bar` / `Outline` は文字色を変えない
pub fn row_colors(selection: SelectionStyle, selected: bool, pal: &Palette) -> RowColors {
    let plain = RowColors {
        fill: None,
        bar: None,
        outline: None,
        main: pal.text,
        sub: pal.sub_text,
    };
    if !selected {
        return plain;
    }
    match selection {
        SelectionStyle::Fill => RowColors {
            fill: Some(pal.selected_bg),
            main: pal.selected_text,
            sub: pal.selected_sub_text,
            ..plain
        },
        SelectionStyle::Bar => RowColors {
            bar: Some(pal.selected_bg),
            ..plain
        },
        SelectionStyle::Outline => RowColors {
            outline: Some(pal.selected_bg),
            ..plain
        },
    }
}

// ---- 設定項目からの読み取り ----

fn value<'a>(items: &'a [SettingItem], key: &str) -> Option<&'a SettingValue> {
    items
        .iter()
        .find(|i| i.key == key)
        .and_then(|i| i.value.as_ref())
}

fn text<'a>(items: &'a [SettingItem], key: &str) -> Option<&'a str> {
    match value(items, key)? {
        SettingValue::Text(t) => Some(t),
        _ => None,
    }
}

fn int(items: &[SettingItem], key: &str) -> Option<i32> {
    match value(items, key)? {
        SettingValue::Int(v) => Some(*v),
        _ => None,
    }
}

/// 選択肢の項目。未指定は `None`、知らない値は `None` にして理由を `notes` に足す
fn pick<T>(
    items: &[SettingItem],
    key: &str,
    notes: &mut Vec<String>,
    parse: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    let raw = text(items, key)?;
    let parsed = parse(raw);
    if parsed.is_none() {
        notes.push(format!(
            "{key} の値 \"{raw}\" は使えないため、テンプレートの値にします"
        ));
    }
    parsed
}

impl CandidateStyle {
    /// `get_settings` の項目一覧から実効値を作る。項目が無い・読めない・知らない値の項目は
    /// 既定（テンプレートの値）に戻し、その理由を返す。範囲外の数値は範囲に丸める
    pub fn from_items(items: &[SettingItem]) -> (Self, Vec<String>) {
        let mut notes = Vec::new();
        let theme = pick(items, key::THEME, &mut notes, Theme::parse).unwrap_or_default();
        let o = Overrides {
            font_percent: int(items, key::FONT_SCALE),
            row_height: int(items, key::ROW_HEIGHT),
            padding: int(items, key::PADDING),
            corner_radius: int(items, key::CORNER_RADIUS),
            shadow: pick(items, key::SHADOW, &mut notes, |v| match v {
                "on" => Some(true),
                "off" => Some(false),
                _ => None,
            }),
            number: pick(items, key::NUMBER, &mut notes, |v| match v {
                "left" => Some(NumberPlacement::Left),
                "right" => Some(NumberPlacement::Right),
                "hidden" => Some(NumberPlacement::Hidden),
                _ => None,
            }),
            annotation: pick(items, key::ANNOTATION, &mut notes, |v| match v {
                "full" => Some(Annotation::Full),
                "source" => Some(Annotation::Source),
                "none" => Some(Annotation::None),
                _ => None,
            }),
            selection: pick(items, key::SELECTION, &mut notes, |v| match v {
                "fill" => Some(SelectionStyle::Fill),
                "bar" => Some(SelectionStyle::Bar),
                "outline" => Some(SelectionStyle::Outline),
                _ => None,
            }),
            opacity: int(items, key::OPACITY),
            color: pick(items, key::COLOR, &mut notes, |v| match v {
                "system" => Some(ColorScheme::System),
                "light" => Some(ColorScheme::Light),
                "dark" => Some(ColorScheme::Dark),
                "template" => Some(ColorScheme::Template),
                _ => None,
            }),
            max_width: int(items, key::MAX_WIDTH),
            page_indicator: pick(items, key::PAGE_INDICATOR, &mut notes, |v| match v {
                "footer" => Some(PageIndicator::Footer),
                "none" => Some(PageIndicator::None),
                _ => None,
            }),
            caret_gap: int(items, key::GAP),
            underline: pick(items, key::UNDERLINE, &mut notes, |v| match v {
                "solid" => Some(UnderlineKind::Solid),
                "dot" => Some(UnderlineKind::Dot),
                "dash" => Some(UnderlineKind::Dash),
                _ => None,
            }),
        };
        let defaults = Settings::default();
        let s = Settings {
            toast_duration_sec: int(items, key::TOAST_DURATION)
                .unwrap_or(defaults.toast_duration_sec),
            toast_position: pick(items, key::TOAST_POSITION, &mut notes, ToastPosition::parse)
                .unwrap_or(defaults.toast_position),
            mode_popup: match value(items, key::MODE_POPUP) {
                Some(SettingValue::Bool(b)) => *b,
                _ => defaults.mode_popup,
            },
            mode_popup_duration_ms: int(items, key::MODE_POPUP_DURATION)
                .unwrap_or(defaults.mode_popup_duration_ms),
            mode_popup_position: pick(items, key::MODE_POPUP_POSITION, &mut notes, |v| {
                if v == "caret" {
                    Some(ModePopupPosition::NearCaret)
                } else {
                    ToastPosition::parse(v).map(ModePopupPosition::Corner)
                }
            })
            .unwrap_or(defaults.mode_popup_position),
        };
        let (style, range_notes) = resolve(theme, &o, &s);
        notes.extend(range_notes);
        (style, notes)
    }
}

/// 設定値を読み直す間隔の判断（取得に失敗しても試した時刻を残し、エンジンが落ちている間に
/// 何度も待たない）
#[derive(Debug, Clone, Copy, Default)]
pub struct RefreshGate {
    last_attempt: Option<Instant>,
}

impl RefreshGate {
    /// まだ一度も試していない状態
    pub fn new() -> Self {
        Self::default()
    }

    /// 前回の試行から `min_interval` 以上たったか（一度も試していなければ true）
    pub fn due(&self, now: Instant, min_interval: Duration) -> bool {
        self.last_attempt
            .is_none_or(|last| now.saturating_duration_since(last) >= min_interval)
    }

    /// 試行を記録する
    pub fn record(&mut self, now: Instant) {
        self.last_attempt = Some(now);
    }
}

/// フォーカスを得たときに読み直す最小の間隔（設定画面で変えて戻ってくる操作に追従しつつ、
/// フォーカスの往復で毎回 RPC しない）
pub const FOCUS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// 候補表示のたびに読み直しを試みる最小の間隔（まだ一度も読めていないときの救済）
pub const SHOW_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::{SettingKind, SettingLevel, SettingPlatforms};

    fn item(key: &str, value: Option<SettingValue>) -> SettingItem {
        SettingItem {
            key: key.into(),
            group: "g".into(),
            label: "l".into(),
            description: "d".into(),
            kind: SettingKind::Bool,
            value,
            default: None,
            explicit: true,
            requires_restart: false,
            level: SettingLevel::Detail,
            platforms: SettingPlatforms::Desktop,
        }
    }

    fn txt(key: &str, v: &str) -> SettingItem {
        item(key, Some(SettingValue::Text(v.into())))
    }

    fn num(key: &str, v: i32) -> SettingItem {
        item(key, Some(SettingValue::Int(v)))
    }

    fn style_of(theme: Theme) -> CandidateStyle {
        resolve(theme, &Overrides::default(), &Settings::default()).0
    }

    #[test]
    fn standard_template_keeps_the_legacy_look() {
        let s = CandidateStyle::default();
        let m = s.metrics(96);
        let legacy = Metrics::default();
        assert_eq!(m.row_height, legacy.row_height);
        assert_eq!(m.padding, legacy.padding);
        assert_eq!(m.gap, legacy.gap);
        assert_eq!(m.number_extra, legacy.number_extra);
        assert_eq!(m.footer_height, legacy.footer_height);
        assert_eq!(m.top_inset, legacy.top_inset);
        assert_eq!(m.bottom_inset, legacy.bottom_inset);
        assert_eq!(m.min_width, legacy.min_width);
        assert_eq!(s.font_px(FontRole::Surface, 96), 15);
        assert_eq!(s.font_px(FontRole::Number, 96), 13);
        assert_eq!(s.font_px(FontRole::Label, 96), 11);
        assert_eq!(s.caret_gap, crate::candidates::CARET_GAP);
        assert_eq!(s.toast_duration, crate::toast::TOAST_DURATION);
        assert_eq!(s.mode_popup_duration, crate::toast::MODE_POPUP_DURATION);
        assert_eq!(s.toast_position, ToastPosition::BottomRight);
        assert_eq!(s.toast_corner, 12);
        assert_eq!((s.toast_pad_x, s.toast_pad_y), (14, 10));
        assert_eq!(s.border_px(96), 1);
        assert!(!s.has_shadow());
        assert_eq!(s.alpha(), 255);
        assert_eq!(s.underline, UnderlineKind::Solid);
        assert_eq!(s.palette(ThemeInput::default()), theme::LIGHT);
        assert_eq!(
            s.palette(ThemeInput {
                apps_use_light_theme: Some(0),
                high_contrast: None
            }),
            theme::DARK
        );
    }

    #[test]
    fn every_template_is_within_ranges_and_fits_its_text() {
        for theme in Theme::ALL {
            let s = style_of(theme);
            let ok = |v: i32, (min, max): Bounds| (min..=max).contains(&v);
            assert!(ok(s.font_percent, FONT_SCALE), "{theme:?}");
            assert!(ok(s.row_height, ROW_HEIGHT), "{theme:?}");
            assert!(ok(s.padding, PADDING), "{theme:?}");
            assert!(ok(s.corner_radius, CORNER_RADIUS), "{theme:?}");
            assert!(ok(s.opacity, OPACITY), "{theme:?}");
            assert!(ok(s.max_width, MAX_WIDTH), "{theme:?}");
            assert!(ok(s.caret_gap, CARET_GAP), "{theme:?}");
            assert!(s.row_height >= min_row_height(s.font_percent), "{theme:?}");
            assert_eq!(Theme::parse(theme.as_str()), Some(theme));
            // どのテンプレートも既定では注釈・番号の領域を含めても最小幅に収まる範囲
            assert!(s.max_width > s.metrics(96).min_width, "{theme:?}");
        }
    }

    #[test]
    fn templates_differ_where_expected() {
        let std = style_of(Theme::Standard);
        let compact = style_of(Theme::Compact);
        let spacious = style_of(Theme::Spacious);
        let minimal = style_of(Theme::Minimal);
        let contrast = style_of(Theme::Contrast);
        assert!(compact.row_height < std.row_height && std.row_height < spacious.row_height);
        assert!(compact.font_percent < std.font_percent);
        assert!(spacious.shadow && spacious.corner_radius > 0);
        assert_eq!(spacious.annotation, Annotation::Full);
        assert_eq!(minimal.annotation, Annotation::None);
        assert_eq!(minimal.page_indicator, PageIndicator::None);
        assert_eq!(minimal.number, NumberPlacement::Left);
        assert!(contrast.border > std.border);
        assert_eq!(contrast.palette(ThemeInput::default()), CONTRAST_LIGHT);
    }

    #[test]
    fn explicit_values_win_over_the_template() {
        let (s, notes) = resolve(
            Theme::Spacious,
            &Overrides {
                font_percent: Some(100),
                shadow: Some(false),
                number: Some(NumberPlacement::Hidden),
                annotation: Some(Annotation::None),
                row_height: Some(30),
                ..Overrides::default()
            },
            &Settings::default(),
        );
        assert!(notes.is_empty());
        assert_eq!(s.font_percent, 100);
        assert!(!s.shadow);
        assert_eq!(s.number, NumberPlacement::Hidden);
        assert_eq!(s.annotation, Annotation::None);
        assert_eq!(s.row_height, 30);
        // 指定していない項目はテンプレートのまま
        assert_eq!(s.padding, 12);
        assert_eq!(s.corner_radius, 8);
    }

    #[test]
    fn out_of_range_values_are_clamped_with_a_note() {
        let (s, notes) = resolve(
            Theme::Standard,
            &Overrides {
                font_percent: Some(1000),
                row_height: Some(-5),
                opacity: Some(0),
                max_width: Some(5),
                caret_gap: Some(400),
                ..Overrides::default()
            },
            &Settings {
                toast_duration_sec: 0,
                mode_popup_duration_ms: 99_999,
                ..Settings::default()
            },
        );
        assert_eq!(s.font_percent, FONT_SCALE.1);
        // 行の高さは範囲の下限に丸めたうえで、文字（200%）が収まる高さまで広がる
        assert_eq!(s.row_height, min_row_height(FONT_SCALE.1));
        assert_eq!(s.opacity, OPACITY.0);
        assert_eq!(s.max_width, MAX_WIDTH.0);
        assert_eq!(s.caret_gap, CARET_GAP.1);
        assert_eq!(s.toast_duration, Duration::from_secs(1));
        assert_eq!(s.mode_popup_duration, Duration::from_millis(5000));
        assert_eq!(notes.len(), 7, "{notes:?}");
        assert!(notes.iter().all(|n| n.contains("windows.")));
    }

    #[test]
    fn row_height_never_cuts_the_text() {
        for percent in [FONT_SCALE.0, 100, 150, FONT_SCALE.1] {
            let (s, _) = resolve(
                Theme::Compact,
                &Overrides {
                    font_percent: Some(percent),
                    row_height: Some(ROW_HEIGHT.0),
                    ..Overrides::default()
                },
                &Settings::default(),
            );
            assert!(s.row_height >= font_px(FontRole::Surface, percent));
        }
    }

    #[test]
    fn unset_items_follow_the_template_and_invalid_ones_fall_back() {
        let items = [
            txt(key::THEME, "spacious"),
            item(key::FONT_SCALE, None),
            num(key::PADDING, 20),
            txt(key::NUMBER, "sideways"),
            txt(key::SHADOW, "off"),
            txt(key::TOAST_POSITION, "top_left"),
            txt(key::MODE_POPUP_POSITION, "bottom_center"),
            item(key::MODE_POPUP, Some(SettingValue::Bool(false))),
        ];
        let (s, notes) = CandidateStyle::from_items(&items);
        assert_eq!(s.theme, Theme::Spacious);
        assert_eq!(s.font_percent, 115);
        assert_eq!(s.padding, 20);
        assert!(!s.shadow);
        assert_eq!(s.number, NumberPlacement::Left, "不正値はテンプレートへ");
        assert_eq!(s.toast_position, ToastPosition::TopLeft);
        assert_eq!(
            s.mode_popup_position,
            ModePopupPosition::Corner(ToastPosition::BottomCenter)
        );
        assert!(!s.mode_popup);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains(key::NUMBER));
        // 項目が無ければ標準
        assert_eq!(CandidateStyle::from_items(&[]).0, CandidateStyle::default());
        let bad_theme = [txt(key::THEME, "neon")];
        let (s, notes) = CandidateStyle::from_items(&bad_theme);
        assert_eq!(s.theme, Theme::Standard);
        assert_eq!(notes.len(), 1);
        // 型違いの値は無視（テンプレートに従う）
        let wrong_type = [txt(key::PADDING, "wide"), num(key::SHADOW, 1)];
        assert_eq!(
            CandidateStyle::from_items(&wrong_type).0,
            CandidateStyle::default()
        );
    }

    #[test]
    fn color_scheme_selection_and_high_contrast_priority() {
        let dark = ThemeInput {
            apps_use_light_theme: Some(0),
            high_contrast: None,
        };
        let light = ThemeInput {
            apps_use_light_theme: Some(1),
            high_contrast: None,
        };
        let pal = |color, theme, input| {
            resolve(
                theme,
                &Overrides {
                    color: Some(color),
                    ..Overrides::default()
                },
                &Settings::default(),
            )
            .0
            .palette(input)
        };
        assert_eq!(pal(ColorScheme::System, Theme::Standard, dark), theme::DARK);
        assert_eq!(
            pal(ColorScheme::System, Theme::Standard, light),
            theme::LIGHT
        );
        assert_eq!(pal(ColorScheme::Light, Theme::Standard, dark), theme::LIGHT);
        assert_eq!(pal(ColorScheme::Dark, Theme::Standard, light), theme::DARK);
        assert_eq!(
            pal(ColorScheme::Template, Theme::Standard, dark),
            theme::LIGHT
        );
        assert_eq!(
            pal(ColorScheme::Template, Theme::Contrast, light),
            CONTRAST_DARK
        );
        // ハイコントラストはどの指定よりも優先
        let sys = theme::SystemColors {
            window: Rgb(0, 0, 0),
            window_text: Rgb(255, 255, 255),
            highlight: Rgb(0, 255, 255),
            highlight_text: Rgb(0, 0, 0),
            gray_text: Rgb(0, 255, 0),
        };
        let hc = ThemeInput {
            apps_use_light_theme: Some(1),
            high_contrast: Some(sys),
        };
        for color in [
            ColorScheme::System,
            ColorScheme::Light,
            ColorScheme::Dark,
            ColorScheme::Template,
        ] {
            for theme in Theme::ALL {
                assert_eq!(pal(color, theme, hc).background, sys.window);
                assert_eq!(pal(color, theme, hc).selected_bg, sys.highlight);
            }
        }
        // ハイコントラストでは半透明・角丸・影を使わない
        let (s, _) = resolve(
            Theme::Spacious,
            &Overrides {
                opacity: Some(50),
                ..Overrides::default()
            },
            &Settings::default(),
        );
        let hc_style = s.for_high_contrast();
        assert_eq!(hc_style.opacity, 100);
        assert_eq!(hc_style.corner_radius, 0);
        assert!(!hc_style.has_shadow());
    }

    #[test]
    fn all_palettes_are_readable() {
        for theme in Theme::ALL {
            for dark in [false, true] {
                let p = theme.palette(dark);
                let min = if theme == Theme::Contrast { 7.0 } else { 4.5 };
                assert!(p.text.contrast(p.background) >= min, "{theme:?} {dark}");
                assert!(p.sub_text.contrast(p.background) >= min, "{theme:?} {dark}");
                assert!(p.selected_text.contrast(p.selected_bg) >= min, "{theme:?}");
                assert!(p.selected_sub_text.contrast(p.selected_bg) >= min);
                assert!(p.toast_text.contrast(p.toast_bg) >= min);
                // バー・枠線の強調色が背景から見分けられる
                assert!(
                    p.selected_bg.contrast(p.background) >= 1.5,
                    "{theme:?} {dark}"
                );
            }
        }
        let fixed = Theme::Contrast.fixed_palette();
        assert!(fixed.text.contrast(fixed.background) >= 7.0);
    }

    #[test]
    fn opacity_disables_the_class_shadow() {
        let (s, _) = resolve(Theme::Spacious, &Overrides::default(), &Settings::default());
        assert!(s.has_shadow());
        let (s, _) = resolve(
            Theme::Spacious,
            &Overrides {
                opacity: Some(80),
                ..Overrides::default()
            },
            &Settings::default(),
        );
        assert!(!s.has_shadow());
        assert_eq!(s.alpha(), 204);
    }

    #[test]
    fn dpi_scales_every_length() {
        for theme in Theme::ALL {
            let s = style_of(theme);
            let (a, b) = (s.metrics(96), s.metrics(192));
            assert_eq!(b.row_height, a.row_height * 2, "{theme:?}");
            assert_eq!(b.padding, a.padding * 2, "{theme:?}");
            assert_eq!(b.max_width, a.max_width * 2, "{theme:?}");
            assert_eq!(
                s.font_px(FontRole::Surface, 192),
                s.font_px(FontRole::Surface, 96) * 2
            );
            assert!(s.metrics(144).row_height > a.row_height);
            assert!(s.border_px(96) >= 1);
        }
    }

    #[test]
    fn selection_styles_pick_colors() {
        let p = theme::LIGHT;
        let plain = row_colors(SelectionStyle::Fill, false, &p);
        assert_eq!((plain.fill, plain.bar, plain.outline), (None, None, None));
        assert_eq!((plain.main, plain.sub), (p.text, p.sub_text));
        let fill = row_colors(SelectionStyle::Fill, true, &p);
        assert_eq!(fill.fill, Some(p.selected_bg));
        assert_eq!(fill.main, p.selected_text);
        let bar = row_colors(SelectionStyle::Bar, true, &p);
        assert_eq!((bar.fill, bar.bar), (None, Some(p.selected_bg)));
        assert_eq!(bar.main, p.text);
        let outline = row_colors(SelectionStyle::Outline, true, &p);
        assert_eq!(outline.outline, Some(p.selected_bg));
        assert_eq!(outline.main, p.text);
    }

    #[test]
    fn refresh_gate_throttles_attempts() {
        let t0 = Instant::now();
        let mut gate = RefreshGate::new();
        assert!(gate.due(t0, FOCUS_REFRESH_INTERVAL));
        gate.record(t0);
        assert!(!gate.due(t0 + Duration::from_millis(500), FOCUS_REFRESH_INTERVAL));
        assert!(gate.due(t0 + Duration::from_secs(1), FOCUS_REFRESH_INTERVAL));
        assert!(!gate.due(t0 + Duration::from_secs(4), SHOW_REFRESH_INTERVAL));
        assert!(gate.due(t0 + Duration::from_secs(5), SHOW_REFRESH_INTERVAL));
    }

    /// 選択肢・範囲・既定値が `nagi_config`（設定の正）と食い違っていないこと
    #[test]
    fn matches_nagi_config() {
        use nagi_config::windows_ui as cfg;
        let ints: [(&str, Bounds, (i32, i32, i32)); 9] = [
            (key::FONT_SCALE, FONT_SCALE, cfg::FONT_SCALE),
            (key::ROW_HEIGHT, ROW_HEIGHT, cfg::ROW_HEIGHT),
            (key::PADDING, PADDING, cfg::PADDING),
            (key::CORNER_RADIUS, CORNER_RADIUS, cfg::CORNER_RADIUS),
            (key::OPACITY, OPACITY, cfg::OPACITY),
            (key::MAX_WIDTH, MAX_WIDTH, cfg::MAX_WIDTH),
            (key::GAP, CARET_GAP, cfg::CARET_GAP),
            (
                key::TOAST_DURATION,
                TOAST_DURATION_SEC,
                cfg::TOAST_DURATION_SEC,
            ),
            (
                key::MODE_POPUP_DURATION,
                MODE_POPUP_DURATION_MS,
                cfg::MODE_POPUP_DURATION_MS,
            ),
        ];
        for (name, ours, theirs) in ints {
            assert_eq!(ours, (theirs.0, theirs.1), "{name}");
        }
        // 選択肢は、すべての値をこちらが解釈できる（不正値の note が出ない）
        let choice_keys: [(&str, nagi_config::windows_ui::Choices); 11] = [
            (key::THEME, cfg::THEMES),
            (key::SHADOW, cfg::TOGGLE),
            (key::NUMBER, cfg::NUMBER),
            (key::ANNOTATION, cfg::ANNOTATION),
            (key::SELECTION, cfg::SELECTION),
            (key::COLOR, cfg::COLOR),
            (key::PAGE_INDICATOR, cfg::PAGE_INDICATOR),
            (key::UNDERLINE, cfg::UNDERLINE),
            (key::TOAST_POSITION, cfg::TOAST_POSITION),
            (key::MODE_POPUP_POSITION, cfg::MODE_POPUP_POSITION),
            (key::MODE_POPUP_POSITION, cfg::TOAST_POSITION),
        ];
        for (name, choices) in choice_keys {
            for (value, _) in choices {
                let (_, notes) = CandidateStyle::from_items(&[txt(name, value)]);
                assert!(notes.is_empty(), "{name}={value}: {notes:?}");
            }
        }
        assert_eq!(
            cfg::THEMES.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
            Theme::ALL.iter().map(|t| t.as_str()).collect::<Vec<_>>()
        );
        // 既定値: コアの既定の設定 → こちらの標準と一致する
        let w = nagi_config::WindowsSection::default();
        let d = Settings::default();
        assert_eq!(w.toast_duration_sec, d.toast_duration_sec);
        assert_eq!(w.mode_popup_duration_ms, d.mode_popup_duration_ms);
        assert_eq!(w.mode_popup, d.mode_popup);
        assert_eq!(w.candidate_theme, Theme::Standard.as_str());
        assert_eq!(
            ToastPosition::parse(&w.toast_position),
            Some(d.toast_position)
        );
        assert_eq!(w.mode_popup_position, "caret");
    }

    /// 設定画面の記述子（`nagi_config::settings::definitions`）の項目を、そのまま読める
    #[test]
    fn reads_the_config_definitions() {
        let states = nagi_config::settings::read(None).unwrap();
        let items: Vec<SettingItem> = states
            .iter()
            .filter(|s| s.def.key.starts_with("windows."))
            .map(|s| {
                item(
                    s.def.key,
                    s.value.as_ref().map(|v| match v {
                        nagi_config::settings::SettingValue::Bool(b) => SettingValue::Bool(*b),
                        nagi_config::settings::SettingValue::Int(i) => SettingValue::Int(*i),
                        nagi_config::settings::SettingValue::Float(f) => SettingValue::Float(*f),
                        nagi_config::settings::SettingValue::Text(t) => {
                            SettingValue::Text(t.clone())
                        }
                    }),
                )
            })
            .collect();
        assert!(items.len() >= 21);
        let (s, notes) = CandidateStyle::from_items(&items);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(s, CandidateStyle::default());
    }
}
