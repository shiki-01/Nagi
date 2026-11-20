//! 自動終了のタイマー。時刻は呼び出し側が渡す（テストで進められるように）。
//!
//! - 操作（認証済みのリクエスト）が [`IDLE_LIMIT`] 無ければ終了
//! - ページが閉じられた（`/bye`）後、[`BYE_GRACE`] 以内に次のリクエストが来なければ終了。
//!   再読み込み（`pagehide` → すぐ新しいページの読み込み）で誤って終了しないよう、来たら取り消す

use std::time::{Duration, Instant};

/// 操作が無いまま待つ最長時間
pub const IDLE_LIMIT: Duration = Duration::from_secs(15 * 60);
/// `/bye` のあと、次のリクエストを待つ猶予
pub const BYE_GRACE: Duration = Duration::from_secs(5);

/// 終了の判定
#[derive(Debug, Clone)]
pub struct Lifecycle {
    idle_limit: Duration,
    bye_grace: Duration,
    last_activity: Instant,
    bye_at: Option<Instant>,
}

impl Lifecycle {
    /// 既定の時間で作る
    pub fn new(now: Instant) -> Self {
        Self::with_limits(now, IDLE_LIMIT, BYE_GRACE)
    }

    /// 時間を指定して作る
    pub fn with_limits(now: Instant, idle_limit: Duration, bye_grace: Duration) -> Self {
        Self {
            idle_limit,
            bye_grace,
            last_activity: now,
            bye_at: None,
        }
    }

    /// 認証済みのリクエストが来た（`/bye` の取り消しも兼ねる）
    pub fn touch(&mut self, now: Instant) {
        self.last_activity = now;
        self.bye_at = None;
    }

    /// ページが閉じられた
    pub fn bye(&mut self, now: Instant) {
        self.bye_at = Some(now);
    }

    /// 終了すべきか
    pub fn should_exit(&self, now: Instant) -> bool {
        if let Some(at) = self.bye_at
            && now.duration_since(at) >= self.bye_grace
        {
            return true;
        }
        now.duration_since(self.last_activity) >= self.idle_limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn exits_after_idle_limit_and_touch_resets_it() {
        let t0 = Instant::now();
        let mut l = Lifecycle::new(t0);
        assert!(!l.should_exit(t0 + secs(14 * 60 + 59)));
        l.touch(t0 + secs(14 * 60));
        assert!(!l.should_exit(t0 + secs(15 * 60 + 30)));
        assert!(l.should_exit(t0 + secs(29 * 60 + 1)));
    }

    #[test]
    fn bye_exits_after_grace_unless_a_request_arrives() {
        let t0 = Instant::now();
        let mut l = Lifecycle::new(t0);
        l.bye(t0 + secs(10));
        assert!(!l.should_exit(t0 + secs(14)));
        assert!(l.should_exit(t0 + secs(15)));

        // 再読み込み: 猶予の間に API が来たら取り消す
        let mut l = Lifecycle::new(t0);
        l.bye(t0 + secs(10));
        l.touch(t0 + secs(12));
        assert!(!l.should_exit(t0 + secs(100)));
    }
}
