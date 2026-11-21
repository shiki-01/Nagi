//! Windows の「変換 UI」（候補ウィンドウ・通知・モード表示・下線）の設定の選択肢と範囲。
//!
//! コアは値を保持して検証し `get_settings` で返すだけで、見た目への解釈（テンプレートが決める既定値と
//! 明示指定の合成）は Windows フロントエンド（`nagi_windows::candidate_style`）が行う。
//! 選択肢の値と範囲はここが正で、設定画面の記述子（`settings.rs`）と検証（`Config::validate`）が共有する。
//! フロントエンド側にも同じ値の表があるので、変えるときは両方（`nagi-windows` のテストが食い違いを検出する）。

use crate::WindowsSection;

/// 選択肢の一覧（`(TOML に書く値, 表示名)`）
pub type Choices = &'static [(&'static str, &'static str)];

/// 整数項目の範囲（`(最小, 最大, UI の刻み)`）
pub type Range = (i32, i32, i32);

/// 候補ウィンドウのテンプレート。各テンプレートが下の詳細項目の既定値を決める
pub const THEMES: Choices = &[
    ("standard", "標準"),
    ("compact", "コンパクト"),
    ("spacious", "ゆったり"),
    ("minimal", "ミニマル（番号と候補のみ）"),
    ("contrast", "くっきり（高コントラスト寄り）"),
];

/// あり / なし
pub const TOGGLE: Choices = &[("on", "あり"), ("off", "なし")];

/// 番号ラベルの表示と位置
pub const NUMBER: Choices = &[
    ("left", "候補の左"),
    ("right", "候補の右端"),
    ("hidden", "表示しない"),
];

/// 注釈（出所・読み）の表示
pub const ANNOTATION: Choices = &[
    ("full", "出所と読み"),
    ("source", "出所のみ"),
    ("none", "表示しない"),
];

/// 選択行の強調
pub const SELECTION: Choices = &[
    ("fill", "塗りつぶし"),
    ("bar", "左にバー"),
    ("outline", "枠線"),
];

/// 配色
pub const COLOR: Choices = &[
    ("system", "システムに従う（ライト/ダーク）"),
    ("light", "ライト"),
    ("dark", "ダーク"),
    ("template", "テンプレートの配色で固定"),
];

/// ページ送りの表示
pub const PAGE_INDICATOR: Choices = &[("footer", "下部に「2/5」"), ("none", "表示しない")];

/// 入力中の文字列の下線（通常の文節・ライブ変換・予測）
pub const UNDERLINE: Choices = &[("solid", "実線"), ("dot", "点線"), ("dash", "破線")];

/// 通知の表示位置（作業領域の隅）
pub const TOAST_POSITION: Choices = &[
    ("bottom_right", "右下"),
    ("bottom_left", "左下"),
    ("top_right", "右上"),
    ("top_left", "左上"),
    ("bottom_center", "下の中央"),
    ("top_center", "上の中央"),
];

/// モード表示の位置（キャレット付近、または通知と同じ隅）
pub const MODE_POPUP_POSITION: Choices = &[
    ("caret", "キャレットの近く"),
    ("bottom_right", "右下"),
    ("bottom_left", "左下"),
    ("top_right", "右上"),
    ("top_left", "左上"),
    ("bottom_center", "下の中央"),
    ("top_center", "上の中央"),
];

/// フォントの倍率（%）
pub const FONT_SCALE: Range = (60, 200, 5);
/// 行の高さ（dp。96 DPI 基準の px）
pub const ROW_HEIGHT: Range = (16, 64, 1);
/// 左右の余白（dp）
pub const PADDING: Range = (0, 32, 1);
/// 角丸の半径（dp。0 = 角丸なし）
pub const CORNER_RADIUS: Range = (0, 16, 1);
/// 背景の不透明度（%）
pub const OPACITY: Range = (30, 100, 5);
/// 候補ウィンドウの最大幅（dp）
pub const MAX_WIDTH: Range = (120, 1200, 20);
/// キャレット（入力中の文字列）と候補ウィンドウの間隔（dp）
pub const CARET_GAP: Range = (0, 40, 1);
/// 通知の表示時間（秒）
pub const TOAST_DURATION_SEC: Range = (1, 30, 1);
/// モード表示の表示時間（ミリ秒）
pub const MODE_POPUP_DURATION_MS: Range = (200, 5000, 100);

/// `value` が選択肢に含まれるか
pub fn is_choice(choices: Choices, value: &str) -> bool {
    choices.iter().any(|(v, _)| *v == value)
}

fn check_range(errors: &mut Vec<String>, key: &str, range: Range, value: Option<i32>) {
    if let Some(v) = value
        && !(range.0..=range.1).contains(&v)
    {
        errors.push(format!(
            "windows.{key} は {} から {} の間である必要があります: {v}",
            range.0, range.1
        ));
    }
}

fn check_choice(errors: &mut Vec<String>, key: &str, choices: Choices, value: Option<&str>) {
    if let Some(v) = value
        && !is_choice(choices, v)
    {
        let all: Vec<&str> = choices.iter().map(|(v, _)| *v).collect();
        errors.push(format!(
            "windows.{key} は {} のいずれかである必要があります: {v}",
            all.join(" / ")
        ));
    }
}

/// `[windows]` の変換 UI 関連の値を検証し、エラー文のリストを返す（メッセージの先頭が `windows.<キー>`）
pub fn validate(w: &WindowsSection) -> Vec<String> {
    let mut e = Vec::new();
    check_choice(&mut e, "candidate_theme", THEMES, Some(&w.candidate_theme));
    check_range(
        &mut e,
        "candidate_font_scale",
        FONT_SCALE,
        w.candidate_font_scale,
    );
    check_range(
        &mut e,
        "candidate_row_height",
        ROW_HEIGHT,
        w.candidate_row_height,
    );
    check_range(&mut e, "candidate_padding", PADDING, w.candidate_padding);
    check_range(
        &mut e,
        "candidate_corner_radius",
        CORNER_RADIUS,
        w.candidate_corner_radius,
    );
    check_choice(
        &mut e,
        "candidate_shadow",
        TOGGLE,
        w.candidate_shadow.as_deref(),
    );
    check_choice(
        &mut e,
        "candidate_number",
        NUMBER,
        w.candidate_number.as_deref(),
    );
    check_choice(
        &mut e,
        "candidate_annotation",
        ANNOTATION,
        w.candidate_annotation.as_deref(),
    );
    check_choice(
        &mut e,
        "candidate_selection",
        SELECTION,
        w.candidate_selection.as_deref(),
    );
    check_range(&mut e, "candidate_opacity", OPACITY, w.candidate_opacity);
    check_choice(
        &mut e,
        "candidate_color",
        COLOR,
        w.candidate_color.as_deref(),
    );
    check_range(
        &mut e,
        "candidate_max_width",
        MAX_WIDTH,
        w.candidate_max_width,
    );
    check_choice(
        &mut e,
        "candidate_page_indicator",
        PAGE_INDICATOR,
        w.candidate_page_indicator.as_deref(),
    );
    check_range(&mut e, "candidate_gap", CARET_GAP, w.candidate_gap);
    check_choice(
        &mut e,
        "preedit_underline",
        UNDERLINE,
        w.preedit_underline.as_deref(),
    );
    check_range(
        &mut e,
        "toast_duration_sec",
        TOAST_DURATION_SEC,
        Some(w.toast_duration_sec),
    );
    check_choice(
        &mut e,
        "toast_position",
        TOAST_POSITION,
        Some(&w.toast_position),
    );
    check_range(
        &mut e,
        "mode_popup_duration_ms",
        MODE_POPUP_DURATION_MS,
        Some(w.mode_popup_duration_ms),
    );
    check_choice(
        &mut e,
        "mode_popup_position",
        MODE_POPUP_POSITION,
        Some(&w.mode_popup_position),
    );
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_section_is_valid() {
        assert_eq!(validate(&WindowsSection::default()), Vec::<String>::new());
    }

    #[test]
    fn out_of_range_and_unknown_choice_are_reported_with_key_first() {
        let w = WindowsSection {
            candidate_theme: "nope".into(),
            candidate_font_scale: Some(500),
            candidate_number: Some("up".into()),
            toast_duration_sec: 0,
            ..Default::default()
        };
        let errors = validate(&w);
        assert_eq!(errors.len(), 4, "{errors:?}");
        assert!(errors[0].starts_with("windows.candidate_theme "));
        assert!(errors[1].starts_with("windows.candidate_font_scale "));
        assert!(
            errors
                .iter()
                .any(|m| m.starts_with("windows.candidate_number "))
        );
        assert!(
            errors
                .iter()
                .any(|m| m.starts_with("windows.toast_duration_sec "))
        );
    }

    #[test]
    fn ranges_are_well_formed() {
        for (min, max, step) in [
            FONT_SCALE,
            ROW_HEIGHT,
            PADDING,
            CORNER_RADIUS,
            OPACITY,
            MAX_WIDTH,
            CARET_GAP,
            TOAST_DURATION_SEC,
            MODE_POPUP_DURATION_MS,
        ] {
            assert!(min < max && step >= 1 && step <= max - min);
        }
    }
}
