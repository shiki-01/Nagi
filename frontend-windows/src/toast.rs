//! 通知パネル（トースト）の配置と表示時間（OS 非依存）。描画は Win32 層が行う。
//!
//! macOS 版は画面の右上に小パネルを出すが、Windows の通知は右下が慣例なので作業領域の右下に置く
//! （decisions.md H-41）。

use std::time::Duration;

use nagi_protocol::InputMode;

use crate::candidates::{Point, Rect, Size, place_window};

/// 通知を出しておく時間
pub const TOAST_DURATION: Duration = Duration::from_secs(4);

/// 通知の幅の上限（96 DPI 基準。超える文言は折り返す）
pub const TOAST_MAX_WIDTH: i32 = 420;

/// 作業領域の端からの余白（96 DPI 基準）
pub const TOAST_MARGIN: i32 = 16;

/// `work`（作業領域）の右下に `size` の通知を置く左上位置。収まらない場合は作業領域の左上に寄せる
pub fn place_toast(work: Rect, size: Size, margin: i32) -> Point {
    place_in_corner(work, size, margin, ToastPosition::BottomRight)
}

/// 通知・モード表示を置く作業領域の隅（設定 `windows.toast_position` / `mode_popup_position`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastPosition {
    /// 右下（既定）
    #[default]
    BottomRight,
    /// 左下
    BottomLeft,
    /// 右上
    TopRight,
    /// 左上
    TopLeft,
    /// 下の中央
    BottomCenter,
    /// 上の中央
    TopCenter,
}

impl ToastPosition {
    /// 設定値（`bottom_right` など）から。知らない値は `None`
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "bottom_right" => Self::BottomRight,
            "bottom_left" => Self::BottomLeft,
            "top_right" => Self::TopRight,
            "top_left" => Self::TopLeft,
            "bottom_center" => Self::BottomCenter,
            "top_center" => Self::TopCenter,
            _ => return None,
        })
    }
}

/// `work`（作業領域）の `position` の隅に `size` を置く左上位置。端から `margin` あける。
/// 収まらない場合は作業領域の左上に寄せる
pub fn place_in_corner(work: Rect, size: Size, margin: i32, position: ToastPosition) -> Point {
    let left = work.left + margin;
    let right = work.right - margin - size.width;
    let center = work.left + (work.width() - size.width) / 2;
    let top = work.top + margin;
    let bottom = work.bottom - margin - size.height;
    let (x, y) = match position {
        ToastPosition::BottomRight => (right, bottom),
        ToastPosition::BottomLeft => (left, bottom),
        ToastPosition::TopRight => (right, top),
        ToastPosition::TopLeft => (left, top),
        ToastPosition::BottomCenter => (center, bottom),
        ToastPosition::TopCenter => (center, top),
    };
    Point {
        x: x.max(work.left),
        y: y.max(work.top),
    }
}

/// モード切替ポップアップ（MS-IME の「あ / A」）を出しておく時間
pub const MODE_POPUP_DURATION: Duration = Duration::from_secs(1);

/// モード切替ポップアップの有効・無効。設定項目（案: `[ui] mode_popup`）はまだ無いので既定で有効に固定する
/// （コアの設定型は変えない方針。docs/decisions.md の追記待ち）
pub const MODE_POPUP_ENABLED: bool = true;

/// モードが変わった原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeChangeCause {
    /// 利用者の操作（半角/全角・Ctrl+Caps などのキー、言語バーのクリック・メニュー）
    User,
    /// 自動の同期（フォーカス取得・エンジンの再起動・アプリや他の IME による変更・復元）
    Automatic,
}

/// ポップアップを出すかの判断材料
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModePopupInput {
    /// 設定で有効か
    pub enabled: bool,
    /// 変わる前のモード（未確定なら `None`）
    pub previous: Option<InputMode>,
    /// 変わった後のモード
    pub mode: InputMode,
    /// 変わった原因
    pub cause: ModeChangeCause,
    /// このセッションが前面でフォーカスを持っているか
    pub focused: bool,
    /// アプリが候補を自前で描く（UILess）モードか。そのアプリには出さない
    pub ui_less: bool,
}

/// モード切替ポップアップを出すか。利用者の操作で、既知のモードから実際に変わったときだけ
pub fn should_show_mode_popup(input: &ModePopupInput) -> bool {
    input.enabled
        && input.cause == ModeChangeCause::User
        && input.focused
        && !input.ui_less
        && input.previous.is_some_and(|p| p != input.mode)
}

/// ポップアップの文字（言語バーのボタンと同じ「あ」「A」）
pub fn mode_popup_text(mode: InputMode) -> &'static str {
    crate::status::mode_indicator(mode)
}

/// ポップアップの基準位置。キャレット矩形（全成分 0 は未取得）、無ければマウス位置の点、
/// どちらも無ければ `None`（出さない）
pub fn mode_popup_anchor(caret: Option<Rect>, mouse: Option<Point>) -> Option<Rect> {
    match caret {
        Some(r) if !r.is_zero() => Some(r),
        _ => mouse.map(|p| Rect::from_origin_size(p, Size::default())),
    }
}

/// `anchor` の下（収まらなければ上）に `size` のポップアップを置く左上位置。
/// 候補ウィンドウと同じ配置（キャレットを含むモニターの作業領域内に収める）
pub fn place_mode_popup(anchor: Rect, size: Size, work_areas: &[Rect]) -> Point {
    place_window(anchor, size, work_areas).origin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_right_with_margin() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let p = place_toast(
            work,
            Size {
                width: 300,
                height: 60,
            },
            16,
        );
        assert_eq!(p, Point { x: 1604, y: 964 });
    }

    #[test]
    fn corners_and_centers() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };
        let size = Size {
            width: 100,
            height: 50,
        };
        let at = |p| place_in_corner(work, size, 10, p);
        assert_eq!(at(ToastPosition::BottomRight), Point { x: 890, y: 740 });
        assert_eq!(at(ToastPosition::BottomLeft), Point { x: 10, y: 740 });
        assert_eq!(at(ToastPosition::TopRight), Point { x: 890, y: 10 });
        assert_eq!(at(ToastPosition::TopLeft), Point { x: 10, y: 10 });
        assert_eq!(at(ToastPosition::BottomCenter), Point { x: 450, y: 740 });
        assert_eq!(at(ToastPosition::TopCenter), Point { x: 450, y: 10 });
        assert_eq!(
            ToastPosition::parse("top_center"),
            Some(ToastPosition::TopCenter)
        );
        assert_eq!(ToastPosition::parse("middle"), None);
    }

    #[test]
    fn oversized_is_clamped_to_work_area() {
        let work = Rect {
            left: 100,
            top: 50,
            right: 200,
            bottom: 100,
        };
        let p = place_toast(
            work,
            Size {
                width: 500,
                height: 500,
            },
            16,
        );
        assert_eq!(p, Point { x: 100, y: 50 });
    }

    fn input() -> ModePopupInput {
        ModePopupInput {
            enabled: true,
            previous: Some(InputMode::Hiragana),
            mode: InputMode::Alphanumeric,
            cause: ModeChangeCause::User,
            focused: true,
            ui_less: false,
        }
    }

    #[test]
    fn popup_shown_for_user_change() {
        assert!(should_show_mode_popup(&input()));
    }

    #[test]
    fn popup_not_shown_for_automatic_or_unknown_or_same() {
        let mut i = input();
        i.cause = ModeChangeCause::Automatic;
        assert!(!should_show_mode_popup(&i));
        let mut i = input();
        i.previous = None;
        assert!(!should_show_mode_popup(&i));
        let mut i = input();
        i.previous = Some(i.mode);
        assert!(!should_show_mode_popup(&i));
    }

    #[test]
    fn popup_not_shown_when_disabled_unfocused_or_ui_less() {
        for tweak in [
            (|i: &mut ModePopupInput| i.enabled = false) as fn(&mut ModePopupInput),
            |i| i.focused = false,
            |i| i.ui_less = true,
        ] {
            let mut i = input();
            tweak(&mut i);
            assert!(!should_show_mode_popup(&i));
        }
    }

    #[test]
    fn popup_text_matches_language_bar() {
        assert_eq!(mode_popup_text(InputMode::Hiragana), "あ");
        assert_eq!(mode_popup_text(InputMode::Alphanumeric), "A");
    }

    #[test]
    fn anchor_prefers_caret_then_mouse_then_none() {
        let caret = Rect {
            left: 10,
            top: 20,
            right: 12,
            bottom: 40,
        };
        let mouse = Point { x: 500, y: 600 };
        assert_eq!(mode_popup_anchor(Some(caret), Some(mouse)), Some(caret));
        // ゼロ矩形は未取得扱い
        assert_eq!(
            mode_popup_anchor(Some(Rect::default()), Some(mouse)),
            Some(Rect::from_origin_size(mouse, Size::default()))
        );
        assert_eq!(
            mode_popup_anchor(None, Some(mouse)),
            Some(Rect::from_origin_size(mouse, Size::default()))
        );
        assert_eq!(mode_popup_anchor(None, None), None);
        assert_eq!(mode_popup_anchor(Some(Rect::default()), None), None);
    }

    #[test]
    fn popup_is_placed_below_caret_and_flips_above_at_bottom() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let size = Size {
            width: 40,
            height: 40,
        };
        let caret = Rect {
            left: 100,
            top: 200,
            right: 102,
            bottom: 220,
        };
        let p = place_mode_popup(caret, size, &[work]);
        assert_eq!(p.x, 100);
        assert!(p.y >= caret.bottom);
        let low = Rect {
            top: 1000,
            bottom: 1020,
            ..caret
        };
        let p = place_mode_popup(low, size, &[work]);
        assert!(p.y + size.height <= low.top);
    }

    #[test]
    fn popup_lasts_about_a_second() {
        assert_eq!(MODE_POPUP_DURATION, Duration::from_secs(1));
    }
}
