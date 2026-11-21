//! 候補ウィンドウの位置決めと表示保留（H-102・H-117・H-123）のステートマシン。OS 非依存。
//!
//! マウス位置は使わない。基準位置の優先順位（上ほど正確）は次のとおり。
//!
//! 1. 測れた位置（[`Source::Composition`] = composition の先頭・全体、[`Source::Selection`] = 開始前の
//!    挿入点または現在の選択範囲。どちらも `ITfContextView::GetTextExt`）
//! 2. [`Source::LastKnown`] = 直近（[`LAST_KNOWN_TTL`] 以内）に測れた位置。フォーカス移動で捨てる
//! 3. [`Source::GuiCaret`] = `GetGUIThreadInfo` の `rcCaret`（クラシックなアプリ）
//! 4. [`Source::FocusWindow`] = フォーカスウィンドウの左上付近（最終手段）。それも無ければ
//!    [`Source::Screen`] = プライマリモニターの左上付近
//!
//! 測れた位置が無い間は表示を保留する。2・3（暫定位置）は、再測定の完了を待つ [`PROVISIONAL_AFTER`]
//! だけ待ってから使う（再測定の予定が無ければ待たずに使う）。どれも無ければ [`MAX_DEFER`] で 4 を使う。
//! 暫定位置で出したあとに測れた位置が取れたときは、差が許容範囲（[`MOVE_TOLERANCE_DP`]）以内なら
//! 動かさない（動かすのは 1 回だけ）。
//!
//! 時刻は呼び出し側が単調な時計から `Duration` で渡す（テストで時間を進められる）。タイマーの仕掛けと
//! 描画は TSF 層が行い、ここは「待つか・出すか・どこに出すか」の判断だけを持つ。

use std::time::Duration;

use crate::candidates::{AnchorSource, Point, Rect, Size, scale_px};

/// 暫定位置（最後の既知位置・GUI キャレット）を使うまで、測れた位置を待つ時間
pub const PROVISIONAL_AFTER: Duration = Duration::from_millis(100);

/// 保留の上限。これを超えたら最終手段（フォーカスウィンドウ基準）で出す。マウス位置は使わない
pub const MAX_DEFER: Duration = Duration::from_millis(400);

/// 最後の既知位置の有効期間
pub const LAST_KNOWN_TTL: Duration = Duration::from_secs(10);

/// 暫定位置から測れた位置へ動かす差の許容（96 DPI 基準の dp。これ以下なら動かさない）
pub const MOVE_TOLERANCE_DP: i32 = 4;

/// 同じ内容の位置のログを繰り返さない間隔
pub const LOG_REPEAT: Duration = Duration::from_secs(10);

/// 基準位置の取得元（`tsf.log` の `[cand] anchor=…` の値）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// 選択範囲（composition 開始前の挿入点、または現在の選択範囲）
    Selection,
    /// composition の範囲
    Composition,
    /// 直近に測れた位置
    LastKnown,
    /// `GetGUIThreadInfo` のキャレット
    GuiCaret,
    /// フォーカスウィンドウの左上付近
    FocusWindow,
    /// プライマリモニターの左上付近（フォーカスウィンドウも分からないときの最後の手段）
    Screen,
}

impl Source {
    /// ログの表記
    pub const fn as_str(self) -> &'static str {
        match self {
            Source::Selection => "selection",
            Source::Composition => "composition",
            Source::LastKnown => "last-known",
            Source::GuiCaret => "gui-caret",
            Source::FocusWindow => "focus-window",
            Source::Screen => "screen",
        }
    }

    /// 文書（TSF）から測れた位置か（暫定位置ではない）
    pub const fn is_measured(self) -> bool {
        matches!(self, Source::Selection | Source::Composition)
    }

    /// `edit::measure_caret` の由来から取得元へ。何も測れていない（`Mouse`）は `None`。
    /// 同じ composition で直前に測れた矩形（`Previous`）は composition の位置として扱う
    pub const fn from_measure(source: AnchorSource) -> Option<Self> {
        match source {
            AnchorSource::Start | AnchorSource::Range | AnchorSource::Previous => {
                Some(Source::Composition)
            }
            AnchorSource::Caret | AnchorSource::PreCaret => Some(Source::Selection),
            AnchorSource::Mouse => None,
        }
    }
}

/// 使える矩形か（ゼロ矩形・高さのない矩形は取得失敗とみなす）
fn usable(r: &Rect) -> bool {
    !r.is_zero() && r.bottom > r.top
}

/// ウィンドウの左上付近に置く基準矩形（`GetGUIThreadInfo` でも位置が取れないアプリの最終手段）。
/// タイトルバーを避けて少し下・内側にずらし、行の高さ程度の矩形にする。ウィンドウが小さすぎれば `None`
pub fn window_corner_anchor(window: Rect) -> Option<Rect> {
    const INSET_X: i32 = 16;
    const INSET_Y: i32 = 48;
    const HEIGHT: i32 = 20;
    if window.width() < INSET_X * 2 || window.height() < INSET_Y + HEIGHT {
        return None;
    }
    Some(Rect::from_origin_size(
        Point {
            x: window.left + INSET_X,
            y: window.top + INSET_Y,
        },
        Size {
            width: 1,
            height: HEIGHT,
        },
    ))
}

/// 位置決めの材料。呼び出し側が集める（安い順に、必要になったものだけ集めてよい）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Anchors {
    /// 文書から測れた位置（取得元つき）
    pub measured: Option<(Source, Rect)>,
    /// 直近に測れた位置（[`LastKnown::get`]）
    pub last_known: Option<Rect>,
    /// `GetGUIThreadInfo` のキャレット
    pub gui_caret: Option<Rect>,
    /// フォーカスウィンドウ基準の位置（[`window_corner_anchor`]）
    pub focus_window: Option<Rect>,
    /// プライマリモニター基準の位置
    pub screen: Option<Rect>,
}

impl Anchors {
    /// 測れた位置（使えるものだけ）
    fn measured(&self) -> Option<(Source, Rect)> {
        self.measured.filter(|(_, r)| usable(r))
    }

    /// 暫定位置（最後の既知位置 → GUI キャレット）
    fn provisional(&self) -> Option<(Source, Rect)> {
        self.last_known
            .filter(usable)
            .map(|r| (Source::LastKnown, r))
            .or_else(|| self.gui_caret.filter(usable).map(|r| (Source::GuiCaret, r)))
    }

    /// 最終手段。どれも無ければ原点（候補が出なくなる退行を避ける）
    fn last_resort(&self) -> (Source, Rect) {
        self.focus_window
            .filter(usable)
            .map(|r| (Source::FocusWindow, r))
            .or_else(|| self.screen.filter(usable).map(|r| (Source::Screen, r)))
            .unwrap_or((
                Source::Screen,
                Rect::from_origin_size(
                    Point::default(),
                    Size {
                        width: 1,
                        height: 20,
                    },
                ),
            ))
    }
}

/// 最後の既知位置（フォーカス移動で捨てる。有効期間は [`LAST_KNOWN_TTL`]）
#[derive(Debug, Clone, Copy, Default)]
pub struct LastKnown {
    entry: Option<(Rect, Duration)>,
}

impl LastKnown {
    /// 空で作る
    pub fn new() -> Self {
        Self::default()
    }

    /// 測れた位置を覚える（使えない矩形は無視）
    pub fn remember(&mut self, now: Duration, rect: Rect) {
        if usable(&rect) {
            self.entry = Some((rect, now));
        }
    }

    /// 有効期間内なら位置を返す（時計が戻った場合は有効として扱う）
    pub fn get(&self, now: Duration) -> Option<Rect> {
        self.entry
            .filter(|(_, at)| now.saturating_sub(*at) <= LAST_KNOWN_TTL)
            .map(|(r, _)| r)
    }

    /// 捨てる（フォーカス移動）
    pub fn clear(&mut self) {
        self.entry = None;
    }
}

/// 位置のログの間引き（同じ取得元・測定の由来が [`LOG_REPEAT`] 以内に繰り返されたら出さない）
#[derive(Debug, Clone, Copy, Default)]
pub struct LogGate {
    last: Option<(Source, Duration)>,
}

impl LogGate {
    /// 空で作る
    pub fn new() -> Self {
        Self::default()
    }

    /// 出してよいか（出すなら記録する）
    pub fn should_log(&mut self, now: Duration, source: Source) -> bool {
        let repeat = matches!(
            self.last,
            Some((s, at)) if s == source && now.saturating_sub(at) < LOG_REPEAT
        );
        if !repeat {
            self.last = Some((source, now));
        }
        !repeat
    }
}

/// 候補ウィンドウを置く位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// 取得元
    pub source: Source,
    /// 基準矩形（スクリーン座標。暫定位置との差が小さいときは、すでに出している矩形のまま）
    pub rect: Rect,
    /// 直前に出していた位置から動かすか（最初の表示は `true`）
    pub moved: bool,
    /// 最初の表示のとき、保留を始めてから出すまでの ms（2 回目以降は `None`）
    pub first_ms: Option<u64>,
    /// 直前に出していた位置の取得元（最初の表示は `None`）
    pub from: Option<Source>,
}

impl Placement {
    /// ログに出すべき事象か（最初の表示、または取得元が変わったとき）
    pub fn is_event(&self) -> bool {
        self.first_ms.is_some() || self.from != Some(self.source)
    }
}

/// 表示要求への答え
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowDecision {
    /// 今出す（または位置を合わせる）
    Show(Placement),
    /// 保留する。`wake_after` 後に [`AnchorWait::on_timer`] を呼ぶこと
    Defer {
        /// 次の判断までの時間
        wake_after: Duration,
    },
}

/// タイマー満了時の答え
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerOutcome {
    /// 保留していない（すでに出した・隠した）。何もしない
    Idle,
    /// まだ早い（古いタイマーが先に鳴った・次の段階がある）。`wake_after` 後にもう一度呼ぶ
    Rearm {
        /// 次の判断までの時間
        wake_after: Duration,
    },
    /// 保留中の表示をこの位置で出す
    Show(Placement),
}

/// 出している位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    source: Source,
    rect: Rect,
}

/// 位置決め・表示保留のステートマシン
#[derive(Debug, Clone, Copy, Default)]
pub struct AnchorWait {
    /// 保留を始めた時刻（保留中のみ）
    since: Option<Duration>,
    /// いま出している位置（出していなければ `None`）
    shown: Option<Shown>,
}

/// 差が許容範囲内か（候補ウィンドウの位置に効く left / top / bottom だけを比べる）
fn close(a: &Rect, b: &Rect, tolerance_px: i32) -> bool {
    (a.left - b.left).abs() <= tolerance_px
        && (a.top - b.top).abs() <= tolerance_px
        && (a.bottom - b.bottom).abs() <= tolerance_px
}

enum Resolution {
    Show(Placement),
    Defer(Duration),
}

impl AnchorWait {
    /// 保留していない・何も出していない状態で作る
    pub fn new() -> Self {
        Self::default()
    }

    /// 保留中か
    pub fn is_waiting(&self) -> bool {
        self.since.is_some()
    }

    /// 位置を出しているか
    pub fn is_shown(&self) -> bool {
        self.shown.is_some()
    }

    /// 候補の表示要求。`remeasure_pending` は再測定（非同期の編集セッション）の完了待ちか、
    /// `tolerance_px` は暫定位置から動かさない差（[`MOVE_TOLERANCE_DP`] を DPI で換算したもの）。
    pub fn on_show_request(
        &mut self,
        now: Duration,
        anchors: &Anchors,
        remeasure_pending: bool,
        tolerance_px: i32,
    ) -> ShowDecision {
        match self.resolve(now, anchors, remeasure_pending, tolerance_px) {
            Resolution::Show(p) => ShowDecision::Show(p),
            Resolution::Defer(wake_after) => ShowDecision::Defer { wake_after },
        }
    }

    /// タイマーが鳴った
    pub fn on_timer(
        &mut self,
        now: Duration,
        anchors: &Anchors,
        remeasure_pending: bool,
        tolerance_px: i32,
    ) -> TimerOutcome {
        if !self.is_waiting() {
            return TimerOutcome::Idle;
        }
        match self.resolve(now, anchors, remeasure_pending, tolerance_px) {
            Resolution::Show(p) => TimerOutcome::Show(p),
            Resolution::Defer(wake_after) => TimerOutcome::Rearm { wake_after },
        }
    }

    /// 再測定が完了した。保留中なら出せるかを判断し（出せなければ `None` で保留を続ける）、
    /// 出している最中なら位置を動かすべきときだけ `Some`（動かさないなら `None`）
    pub fn on_remeasured(
        &mut self,
        now: Duration,
        anchors: &Anchors,
        tolerance_px: i32,
    ) -> Option<Placement> {
        if !self.is_waiting() && !self.is_shown() {
            return None;
        }
        match self.resolve(now, anchors, false, tolerance_px) {
            Resolution::Show(p) if p.moved || p.first_ms.is_some() => Some(p),
            _ => None,
        }
    }

    /// 候補を隠した・フォーカスが外れた。保留も出している位置も捨てる
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn resolve(
        &mut self,
        now: Duration,
        anchors: &Anchors,
        remeasure_pending: bool,
        tolerance_px: i32,
    ) -> Resolution {
        if let Some((source, rect)) = anchors.measured() {
            return Resolution::Show(self.place(now, source, rect, tolerance_px));
        }
        // 測れないが出している: 動かさない（測れた位置が一瞬取れないだけで飛ばない）
        if let Some(shown) = self.shown {
            self.since = None;
            return Resolution::Show(Placement {
                source: shown.source,
                rect: shown.rect,
                moved: false,
                first_ms: None,
                from: Some(shown.source),
            });
        }
        let since = *self.since.get_or_insert(now);
        let elapsed = now.saturating_sub(since);
        let provisional = anchors.provisional();
        if let Some((source, rect)) = provisional
            && (!remeasure_pending || elapsed >= PROVISIONAL_AFTER)
        {
            return Resolution::Show(self.place(now, source, rect, tolerance_px));
        }
        if elapsed >= MAX_DEFER {
            let (source, rect) = anchors.last_resort();
            return Resolution::Show(self.place(now, source, rect, tolerance_px));
        }
        let next = if provisional.is_some() && remeasure_pending {
            PROVISIONAL_AFTER
        } else {
            MAX_DEFER
        };
        Resolution::Defer(next.saturating_sub(elapsed).max(Duration::from_millis(1)))
    }

    fn place(&mut self, now: Duration, source: Source, rect: Rect, tolerance_px: i32) -> Placement {
        let previous = self.shown;
        let first_ms = previous.is_none().then(|| {
            let waited = self.since.map_or(Duration::ZERO, |s| now.saturating_sub(s));
            u64::try_from(waited.as_millis()).unwrap_or(u64::MAX)
        });
        // 差が小さいときは、すでに出している位置のまま（ちらつき・ジッターの防止）
        let rect = match previous {
            Some(p) if close(&p.rect, &rect, tolerance_px) => p.rect,
            _ => rect,
        };
        let moved = previous.is_none_or(|p| p.rect != rect);
        self.shown = Some(Shown { source, rect });
        self.since = None;
        Placement {
            source,
            rect,
            moved,
            first_ms,
            from: previous.map(|p| p.source),
        }
    }
}

/// 暫定位置の許容（px）。`dpi` は候補ウィンドウのモニターの DPI
pub fn tolerance_px(dpi: u32) -> i32 {
    scale_px(MOVE_TOLERANCE_DP, dpi)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn rect(left: i32, top: i32) -> Rect {
        Rect {
            left,
            top,
            right: left + 1,
            bottom: top + 20,
        }
    }

    const TOL: i32 = 4;

    fn measured(source: Source, r: Rect) -> Anchors {
        Anchors {
            measured: Some((source, r)),
            ..Anchors::default()
        }
    }

    fn show(d: ShowDecision) -> Placement {
        match d {
            ShowDecision::Show(p) => p,
            other => panic!("Show のはずが {other:?}"),
        }
    }

    #[test]
    fn measured_position_shows_immediately_even_while_remeasure_is_pending() {
        let mut w = AnchorWait::new();
        let p =
            show(w.on_show_request(ms(0), &measured(Source::Selection, rect(5, 10)), true, TOL));
        assert_eq!((p.source, p.rect), (Source::Selection, rect(5, 10)));
        assert_eq!(p.first_ms, Some(0));
        assert!(p.moved && p.is_event());
        assert!(!w.is_waiting());
    }

    #[test]
    fn unusable_measured_rect_is_ignored() {
        let mut w = AnchorWait::new();
        let a = measured(Source::Composition, Rect::default());
        assert!(matches!(
            w.on_show_request(ms(0), &a, true, TOL),
            ShowDecision::Defer { .. }
        ));
        let flat = measured(
            Source::Composition,
            Rect {
                left: 1,
                top: 5,
                right: 2,
                bottom: 5,
            },
        );
        assert!(matches!(
            w.on_show_request(ms(1), &flat, true, TOL),
            ShowDecision::Defer { .. }
        ));
    }

    #[test]
    fn nothing_known_defers_until_max_then_uses_focus_window_never_the_mouse() {
        let mut w = AnchorWait::new();
        let a = Anchors {
            focus_window: Some(rect(100, 200)),
            screen: Some(rect(0, 0)),
            ..Anchors::default()
        };
        assert_eq!(
            w.on_show_request(ms(1000), &a, true, TOL),
            ShowDecision::Defer {
                wake_after: MAX_DEFER
            }
        );
        // 保留中に次のキーが来ても期限は延びない
        assert_eq!(
            w.on_show_request(ms(1300), &a, true, TOL),
            ShowDecision::Defer {
                wake_after: ms(100)
            }
        );
        assert_eq!(
            w.on_timer(ms(1200), &a, true, TOL),
            TimerOutcome::Rearm {
                wake_after: ms(200)
            }
        );
        let TimerOutcome::Show(p) = w.on_timer(ms(1400), &a, true, TOL) else {
            panic!("期限で出るはず");
        };
        assert_eq!((p.source, p.rect), (Source::FocusWindow, rect(100, 200)));
        assert_eq!(p.first_ms, Some(400));
        assert!(!w.is_waiting());
        assert_eq!(w.on_timer(ms(5000), &a, true, TOL), TimerOutcome::Idle);
    }

    #[test]
    fn without_even_a_focus_window_the_screen_corner_is_used() {
        let mut w = AnchorWait::new();
        let a = Anchors {
            screen: Some(rect(64, 64)),
            ..Anchors::default()
        };
        w.on_show_request(ms(0), &a, false, TOL);
        let TimerOutcome::Show(p) = w.on_timer(MAX_DEFER, &a, false, TOL) else {
            panic!();
        };
        assert_eq!(p.source, Source::Screen);
        // 何も無くても出す（候補が出なくなる退行を避ける）
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), &Anchors::default(), false, TOL);
        assert!(matches!(
            w.on_timer(MAX_DEFER, &Anchors::default(), false, TOL),
            TimerOutcome::Show(Placement {
                source: Source::Screen,
                ..
            })
        ));
    }

    #[test]
    fn provisional_positions_are_used_in_priority_order() {
        let both = Anchors {
            last_known: Some(rect(1, 1)),
            gui_caret: Some(rect(2, 2)),
            focus_window: Some(rect(3, 3)),
            ..Anchors::default()
        };
        let mut w = AnchorWait::new();
        // 再測定の予定が無ければ待たずに最後の既知位置
        assert_eq!(
            show(w.on_show_request(ms(0), &both, false, TOL)).source,
            Source::LastKnown
        );
        // 最後の既知位置が無ければ GUI キャレット
        let mut w = AnchorWait::new();
        let gui = Anchors {
            last_known: None,
            ..both
        };
        assert_eq!(
            show(w.on_show_request(ms(0), &gui, false, TOL)).source,
            Source::GuiCaret
        );
    }

    #[test]
    fn provisional_waits_for_the_remeasure_then_gives_up_after_the_grace_period() {
        let a = Anchors {
            gui_caret: Some(rect(7, 7)),
            ..Anchors::default()
        };
        let mut w = AnchorWait::new();
        assert_eq!(
            w.on_show_request(ms(0), &a, true, TOL),
            ShowDecision::Defer {
                wake_after: PROVISIONAL_AFTER
            }
        );
        assert_eq!(
            w.on_timer(ms(30), &a, true, TOL),
            TimerOutcome::Rearm { wake_after: ms(70) }
        );
        let TimerOutcome::Show(p) = w.on_timer(PROVISIONAL_AFTER, &a, true, TOL) else {
            panic!();
        };
        assert_eq!(p.source, Source::GuiCaret);
        assert_eq!(p.first_ms, Some(100));
    }

    #[test]
    fn remeasure_completion_without_a_position_uses_provisional_or_keeps_waiting() {
        let mut w = AnchorWait::new();
        let none = Anchors::default();
        w.on_show_request(ms(0), &none, true, TOL);
        // 何も無い: 保留を続ける（タイマーが最終手段で出す）
        assert_eq!(w.on_remeasured(ms(20), &none, TOL), None);
        assert!(w.is_waiting());
        // 暫定位置がある: 再測定が失敗に終わったので使う
        let mut w = AnchorWait::new();
        let gui = Anchors {
            gui_caret: Some(rect(9, 9)),
            ..Anchors::default()
        };
        w.on_show_request(ms(0), &Anchors::default(), true, TOL);
        let p = w.on_remeasured(ms(20), &gui, TOL).expect("出す");
        assert_eq!(p.source, Source::GuiCaret);
    }

    #[test]
    fn remeasure_completion_with_a_measured_position_shows_the_deferred_window() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), &Anchors::default(), true, TOL);
        let p = w
            .on_remeasured(ms(16), &measured(Source::Composition, rect(40, 50)), TOL)
            .expect("出す");
        assert_eq!((p.source, p.rect), (Source::Composition, rect(40, 50)));
        assert_eq!(p.first_ms, Some(16));
        assert!(!w.is_waiting());
    }

    #[test]
    fn small_difference_from_a_provisional_position_does_not_move() {
        let mut w = AnchorWait::new();
        let gui = Anchors {
            gui_caret: Some(rect(100, 100)),
            ..Anchors::default()
        };
        let first = show(w.on_show_request(ms(0), &gui, false, TOL));
        assert_eq!(first.source, Source::GuiCaret);
        // 4px 以内 → 動かさない（取得元だけ測れた位置に変わる）
        let p = w.on_remeasured(ms(30), &measured(Source::Composition, rect(103, 104)), TOL);
        assert_eq!(p, None);
        let again = show(w.on_show_request(
            ms(40),
            &measured(Source::Composition, rect(103, 104)),
            false,
            TOL,
        ));
        assert_eq!(again.rect, rect(100, 100));
        assert!(!again.moved);
        assert_eq!(again.source, Source::Composition);
        assert!(!again.is_event() || again.from != Some(Source::Composition));
    }

    #[test]
    fn large_difference_moves_exactly_once() {
        let mut w = AnchorWait::new();
        let gui = Anchors {
            gui_caret: Some(rect(100, 100)),
            ..Anchors::default()
        };
        show(w.on_show_request(ms(0), &gui, false, TOL));
        let m = measured(Source::Composition, rect(160, 130));
        let p = w.on_remeasured(ms(30), &m, TOL).expect("動かす");
        assert_eq!(p.rect, rect(160, 130));
        assert!(p.moved);
        assert_eq!(p.from, Some(Source::GuiCaret));
        assert!(p.is_event());
        // 同じ位置が続くなら、もう動かさない
        assert_eq!(w.on_remeasured(ms(60), &m, TOL), None);
        let p = show(w.on_show_request(ms(70), &m, false, TOL));
        assert!(!p.moved && !p.is_event());
    }

    #[test]
    fn losing_the_measured_position_while_shown_keeps_the_window_still() {
        let mut w = AnchorWait::new();
        show(w.on_show_request(
            ms(0),
            &measured(Source::Composition, rect(10, 10)),
            false,
            TOL,
        ));
        let gui = Anchors {
            gui_caret: Some(rect(500, 500)),
            ..Anchors::default()
        };
        let p = show(w.on_show_request(ms(5), &gui, true, TOL));
        assert_eq!(
            (p.source, p.rect, p.moved),
            (Source::Composition, rect(10, 10), false)
        );
        assert!(!w.is_waiting());
    }

    #[test]
    fn reset_forgets_the_shown_position_and_the_wait() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), &Anchors::default(), true, TOL);
        w.reset();
        assert!(!w.is_waiting() && !w.is_shown());
        assert_eq!(
            w.on_timer(ms(900), &Anchors::default(), true, TOL),
            TimerOutcome::Idle
        );
        // 新しい保留は新しい時計で始まる
        assert_eq!(
            w.on_show_request(ms(2000), &Anchors::default(), true, TOL),
            ShowDecision::Defer {
                wake_after: MAX_DEFER
            }
        );
        assert_eq!(w.on_remeasured(ms(1), &Anchors::default(), TOL), None);
    }

    #[test]
    fn clock_going_backwards_is_treated_as_just_started() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(500), &Anchors::default(), true, TOL);
        assert_eq!(
            w.on_timer(ms(100), &Anchors::default(), true, TOL),
            TimerOutcome::Rearm {
                wake_after: MAX_DEFER
            }
        );
    }

    #[test]
    fn on_remeasured_without_a_window_does_nothing() {
        let mut w = AnchorWait::new();
        assert_eq!(
            w.on_remeasured(ms(0), &measured(Source::Composition, rect(1, 1)), TOL),
            None
        );
    }

    #[test]
    fn last_known_expires_and_clears() {
        let mut k = LastKnown::new();
        assert_eq!(k.get(ms(0)), None);
        k.remember(ms(1000), rect(5, 5));
        assert_eq!(k.get(ms(1000) + LAST_KNOWN_TTL), Some(rect(5, 5)));
        assert_eq!(k.get(ms(1001) + LAST_KNOWN_TTL), None);
        k.remember(ms(2000), Rect::default());
        assert_eq!(k.get(ms(2000)), Some(rect(5, 5)), "使えない矩形は覚えない");
        k.clear();
        assert_eq!(k.get(ms(2000)), None);
    }

    #[test]
    fn log_gate_suppresses_repeats_of_the_same_source_only() {
        let mut g = LogGate::new();
        assert!(g.should_log(ms(0), Source::Selection));
        assert!(!g.should_log(ms(1000), Source::Selection));
        assert!(g.should_log(ms(2000), Source::GuiCaret));
        assert!(g.should_log(ms(3000), Source::Selection));
        assert!(g.should_log(ms(3000) + LOG_REPEAT, Source::Selection));
    }

    #[test]
    fn source_names_and_measure_mapping() {
        let names = [
            (Source::Selection, "selection"),
            (Source::Composition, "composition"),
            (Source::LastKnown, "last-known"),
            (Source::GuiCaret, "gui-caret"),
            (Source::FocusWindow, "focus-window"),
            (Source::Screen, "screen"),
        ];
        for (s, n) in names {
            assert_eq!(s.as_str(), n);
        }
        assert!(Source::Selection.is_measured() && Source::Composition.is_measured());
        assert!(!Source::LastKnown.is_measured() && !Source::GuiCaret.is_measured());
        assert_eq!(
            Source::from_measure(AnchorSource::Start),
            Some(Source::Composition)
        );
        assert_eq!(
            Source::from_measure(AnchorSource::Range),
            Some(Source::Composition)
        );
        assert_eq!(
            Source::from_measure(AnchorSource::Previous),
            Some(Source::Composition)
        );
        assert_eq!(
            Source::from_measure(AnchorSource::Caret),
            Some(Source::Selection)
        );
        assert_eq!(
            Source::from_measure(AnchorSource::PreCaret),
            Some(Source::Selection)
        );
        assert_eq!(Source::from_measure(AnchorSource::Mouse), None);
    }

    #[test]
    fn window_corner_anchor_is_inside_the_window_and_rejects_tiny_windows() {
        let win = Rect {
            left: 100,
            top: 200,
            right: 900,
            bottom: 700,
        };
        let a = window_corner_anchor(win).expect("使える");
        assert!(a.left > win.left && a.top > win.top && a.bottom < win.bottom);
        assert!(usable(&a));
        assert_eq!(
            window_corner_anchor(Rect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10
            }),
            None
        );
        assert_eq!(window_corner_anchor(Rect::default()), None);
    }

    #[test]
    fn tolerance_scales_with_dpi() {
        assert_eq!(tolerance_px(96), 4);
        assert_eq!(tolerance_px(192), 8);
    }
}
