//! 候補ウィンドウの表示保留（H-102）のタイムアウト付きステートマシン。OS 非依存。
//!
//! 最初の候補表示はキャレット位置が測れるまで保留し、再測定の完了で出す。しかし非同期の編集セッションが
//! 走らない・位置を返さないアプリでは再測定が完了せず、候補がいつまでも出ない。そこで保留に期限
//! （[`DEFER_TIMEOUT`]）を設け、期限を過ぎたら最後に測れた位置（無ければ `None` = マウス位置）で出す。
//!
//! 時刻は呼び出し側が単調な時計から `Duration` で渡す（テストで時間を進められる）。タイマーの仕掛けと
//! 描画は TSF 層が行い、ここは「待つか・出すか・どこに出すか」の判断だけを持つ。

use std::time::Duration;

use crate::candidates::Rect;

/// 保留の期限。長すぎると候補が遅れて見え、短すぎると再測定が間に合わずマウス位置に出てから動く
/// （実機での調整対象。H-102 の再測定は通常 1 フレーム程度で終わる想定）
pub const DEFER_TIMEOUT: Duration = Duration::from_millis(150);

/// 保留の状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    /// 保留していない
    #[default]
    Idle,
    /// `since`（保留を始めた時刻）から保留中
    Waiting { since: Duration },
}

/// 表示要求への答え
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowDecision {
    /// 今出す。`Some` はその矩形の位置、`None` は呼び出し側のフォールバック（マウス位置）
    Show(Option<Rect>),
    /// 保留する。`wake_after` 後に [`AnchorWait::on_timer`] を呼ぶこと
    Defer {
        /// 期限までの残り
        wake_after: Duration,
    },
}

/// タイマー満了時の答え
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerOutcome {
    /// 保留していない（すでに出した・隠した）。何もしない
    Idle,
    /// まだ期限前（古いタイマーが先に鳴った）。`wake_after` 後にもう一度呼ぶ
    Rearm {
        /// 期限までの残り
        wake_after: Duration,
    },
    /// 期限切れ。保留中の表示をこの位置で出す（`None` はマウス位置）
    Show(Option<Rect>),
}

/// 表示保留のステートマシン
#[derive(Debug, Clone, Copy, Default)]
pub struct AnchorWait {
    state: State,
}

impl AnchorWait {
    /// 保留していない状態で作る
    pub fn new() -> Self {
        Self::default()
    }

    /// 保留中か
    pub fn is_waiting(&self) -> bool {
        matches!(self.state, State::Waiting { .. })
    }

    /// 候補の表示要求。`caret` は測れた基準位置（`None` は未確定）、`remeasure_pending` は再測定待ちか、
    /// `last_known` は最後に測れた位置（フォーカスや composition を跨いで古くなったものは呼び出し側が捨てる）。
    ///
    /// - 位置が測れている、または再測定の予定が無い → 保留を終えて今出す（従来どおり）
    /// - 測れておらず再測定待ち → 保留（初回は時刻を記録）。期限を過ぎていれば `last_known` で出す
    pub fn on_show_request(
        &mut self,
        now: Duration,
        caret: Option<Rect>,
        remeasure_pending: bool,
        last_known: Option<Rect>,
    ) -> ShowDecision {
        if caret.is_some() || !remeasure_pending {
            self.state = State::Idle;
            return ShowDecision::Show(caret);
        }
        let since = match self.state {
            State::Waiting { since } => since,
            State::Idle => {
                self.state = State::Waiting { since: now };
                now
            }
        };
        match remaining(since, now) {
            Some(wake_after) => ShowDecision::Defer { wake_after },
            None => {
                self.state = State::Idle;
                ShowDecision::Show(last_known)
            }
        }
    }

    /// タイマーが鳴った。`last_known` は [`on_show_request`](Self::on_show_request) と同じ
    pub fn on_timer(&mut self, now: Duration, last_known: Option<Rect>) -> TimerOutcome {
        let State::Waiting { since } = self.state else {
            return TimerOutcome::Idle;
        };
        match remaining(since, now) {
            Some(wake_after) => TimerOutcome::Rearm { wake_after },
            None => {
                self.state = State::Idle;
                TimerOutcome::Show(last_known)
            }
        }
    }

    /// 保留を終える（再測定が完了して位置を更新した・候補を隠した・フォーカスが外れた）
    pub fn finish(&mut self) {
        self.state = State::Idle;
    }
}

/// 期限までの残り。期限に達していれば `None`（時計が戻った場合は保留開始直後として扱う）
fn remaining(since: Duration, now: Duration) -> Option<Duration> {
    let elapsed = now.saturating_sub(since);
    DEFER_TIMEOUT.checked_sub(elapsed).filter(|d| !d.is_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn rect(left: i32) -> Rect {
        Rect {
            left,
            top: 10,
            right: left + 1,
            bottom: 30,
        }
    }

    #[test]
    fn known_caret_shows_immediately() {
        let mut w = AnchorWait::new();
        assert_eq!(
            w.on_show_request(ms(0), Some(rect(5)), true, None),
            ShowDecision::Show(Some(rect(5)))
        );
        assert!(!w.is_waiting());
    }

    #[test]
    fn no_remeasure_means_show_at_mouse_now() {
        let mut w = AnchorWait::new();
        assert_eq!(
            w.on_show_request(ms(0), None, false, Some(rect(1))),
            ShowDecision::Show(None)
        );
        assert!(!w.is_waiting());
    }

    #[test]
    fn unknown_caret_with_pending_remeasure_defers_for_the_full_timeout() {
        let mut w = AnchorWait::new();
        assert_eq!(
            w.on_show_request(ms(100), None, true, None),
            ShowDecision::Defer {
                wake_after: DEFER_TIMEOUT
            }
        );
        assert!(w.is_waiting());
    }

    #[test]
    fn repeated_requests_keep_the_original_start() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), None, true, None);
        // 保留中に次のキーが来ても期限は延びない
        assert_eq!(
            w.on_show_request(ms(100), None, true, None),
            ShowDecision::Defer { wake_after: ms(50) }
        );
        assert_eq!(
            w.on_show_request(ms(150), None, true, Some(rect(7))),
            ShowDecision::Show(Some(rect(7)))
        );
        assert!(!w.is_waiting());
    }

    #[test]
    fn timer_before_deadline_rearms() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), None, true, None);
        assert_eq!(
            w.on_timer(ms(40), None),
            TimerOutcome::Rearm {
                wake_after: ms(110)
            }
        );
        assert!(w.is_waiting());
    }

    #[test]
    fn timer_at_deadline_shows_at_last_known() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), None, true, Some(rect(3)));
        assert_eq!(
            w.on_timer(DEFER_TIMEOUT, Some(rect(3))),
            TimerOutcome::Show(Some(rect(3)))
        );
        assert!(!w.is_waiting());
        // 一度出したら次のタイマーは何もしない
        assert_eq!(w.on_timer(ms(1000), Some(rect(3))), TimerOutcome::Idle);
    }

    #[test]
    fn timer_without_last_known_falls_back_to_mouse() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), None, true, None);
        assert_eq!(w.on_timer(ms(500), None), TimerOutcome::Show(None));
    }

    #[test]
    fn finish_cancels_the_wait() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), None, true, None);
        w.finish();
        assert!(!w.is_waiting());
        assert_eq!(w.on_timer(ms(500), None), TimerOutcome::Idle);
    }

    #[test]
    fn a_new_wait_after_finish_starts_a_new_clock() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(0), None, true, None);
        w.finish();
        assert_eq!(
            w.on_show_request(ms(1000), None, true, None),
            ShowDecision::Defer {
                wake_after: DEFER_TIMEOUT
            }
        );
    }

    #[test]
    fn clock_going_backwards_is_treated_as_just_started() {
        let mut w = AnchorWait::new();
        w.on_show_request(ms(500), None, true, None);
        assert_eq!(
            w.on_timer(ms(100), None),
            TimerOutcome::Rearm {
                wake_after: DEFER_TIMEOUT
            }
        );
    }
}
