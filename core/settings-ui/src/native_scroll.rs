//! ネイティブ設定画面のスクロールの OS 非依存部分（純粋な関数。Linux でテストする）。
//!
//! - [`SmoothScroll`]: マウスホイールの補間（1 ノッチを数フレームで動かす。精密タッチパッドの小さな量は即追従）
//! - [`backdrop_fits`] / [`needs_rebuild`]: ページ全体の背景キャッシュを持てる大きさか・作り直すか

/// ホイール 1 ノッチの `delta`（`WHEEL_DELTA`）
pub const WHEEL_NOTCH: i32 = 120;
/// 補間の時定数（ms）。この時間で残りの約 63%、3 倍の時間で約 95% まで近づく
pub const TAU_MS: f64 = 55.0;
/// 補間を回す間隔（ms。タイマー）
pub const FRAME_MS: u32 = 16;
/// 背景キャッシュに使うピクセル数の上限（約 48MB。超えるページは毎回描く）
pub const MAX_BACKDROP_PIXELS: i64 = 12_000_000;

/// ホイールの補間状態。位置は `f64`、表示は四捨五入
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmoothScroll {
    pos: f64,
    target: f64,
}

impl SmoothScroll {
    /// 位置 `pos` で止まった状態
    pub fn new(pos: i32) -> Self {
        Self {
            pos: f64::from(pos),
            target: f64::from(pos),
        }
    }

    /// いまの表示位置（px）
    pub fn pos(&self) -> i32 {
        self.pos.round() as i32
    }

    /// 動いている途中か
    pub fn animating(&self) -> bool {
        (self.target - self.pos).abs() >= 0.5
    }

    /// 外から位置が変わった（スクロールバーのドラッグ・キー・ページ切替）とき、補間をやめてそこに合わせる
    pub fn jump(&mut self, pos: i32) {
        self.pos = f64::from(pos);
        self.target = f64::from(pos);
    }

    /// ホイールの 1 イベント。`delta` は `WM_MOUSEWHEEL` の値（上向きが正）、`step` は 1 ノッチの移動量（px）。
    /// ノッチ単位以上の量は目標を動かして補間し、それより小さい量（精密タッチパッド）は即座に反映する。
    /// 戻り値は補間が必要か（タイマーを動かすか）
    pub fn wheel(&mut self, delta: i32, step: i32, max: i32) -> bool {
        let px = -f64::from(delta) * f64::from(step) / f64::from(WHEEL_NOTCH);
        let max = f64::from(max.max(0));
        if delta.abs() < WHEEL_NOTCH {
            // 精密タッチパッド: 指に追従させる（補間すると遅れて見える）。補間中ならその分も目標に足す
            self.pos = (self.pos + px).clamp(0.0, max);
            self.target = (self.target + px).clamp(0.0, max);
        } else {
            self.target = (self.target + px).clamp(0.0, max);
        }
        self.pos = self.pos.clamp(0.0, max);
        self.animating()
    }

    /// `dt_ms` だけ進める。戻り値は新しい表示位置（px）
    pub fn step(&mut self, dt_ms: f64) -> i32 {
        let dt = dt_ms.clamp(0.0, 100.0);
        let k = 1.0 - (-dt / TAU_MS).exp();
        self.pos += (self.target - self.pos) * k;
        if (self.target - self.pos).abs() < 0.5 {
            self.pos = self.target;
        }
        self.pos()
    }

    /// 範囲（内容の高さの変化）に合わせて丸める
    pub fn clamp(&mut self, max: i32) {
        let max = f64::from(max.max(0));
        self.pos = self.pos.clamp(0.0, max);
        self.target = self.target.clamp(0.0, max);
    }
}

/// 幅 × 高さのページ全体の背景をビットマップに持てる大きさか
pub fn backdrop_fits(width: i32, height: i32) -> bool {
    width > 0 && height > 0 && i64::from(width) * i64::from(height) <= MAX_BACKDROP_PIXELS
}

/// キャッシュするビットマップの高さ（ページの高さと表示領域の高さの大きいほう）
pub fn backdrop_height(content: i32, viewport: i32) -> i32 {
    content.max(viewport).max(1)
}

/// 背景キャッシュを作り直す必要があるか（`Key` は幅・高さ・DPI・世代）
pub fn needs_rebuild<K: PartialEq>(cached: Option<&K>, wanted: &K) -> bool {
    cached != Some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notch_animates_to_target_and_stops() {
        let mut s = SmoothScroll::new(0);
        assert!(s.wheel(-120, 48, 1000));
        assert_eq!(s.pos(), 0);
        let mut last = 0;
        for _ in 0..40 {
            let p = s.step(16.0);
            assert!(p >= last);
            last = p;
        }
        assert_eq!(last, 48);
        assert!(!s.animating());
    }

    #[test]
    fn notches_accumulate() {
        let mut s = SmoothScroll::new(0);
        s.wheel(-120, 48, 1000);
        s.step(16.0);
        s.wheel(-240, 48, 1000);
        for _ in 0..60 {
            s.step(16.0);
        }
        assert_eq!(s.pos(), 144);
    }

    #[test]
    fn precision_touchpad_is_immediate() {
        let mut s = SmoothScroll::new(100);
        let animating = s.wheel(-30, 48, 1000);
        assert!(!animating);
        assert_eq!(s.pos(), 112);
        s.wheel(30, 48, 1000);
        assert_eq!(s.pos(), 100);
    }

    #[test]
    fn clamps_to_range() {
        let mut s = SmoothScroll::new(10);
        s.wheel(120 * 10, 48, 1000);
        for _ in 0..100 {
            s.step(16.0);
        }
        assert_eq!(s.pos(), 0);
        s.wheel(-120 * 100, 48, 200);
        for _ in 0..200 {
            s.step(16.0);
        }
        assert_eq!(s.pos(), 200);
    }

    #[test]
    fn jump_cancels_animation() {
        let mut s = SmoothScroll::new(0);
        s.wheel(-120 * 5, 48, 1000);
        s.step(16.0);
        s.jump(500);
        assert!(!s.animating());
        assert_eq!(s.step(16.0), 500);
    }

    #[test]
    fn frame_rate_independent() {
        let mut a = SmoothScroll::new(0);
        let mut b = SmoothScroll::new(0);
        a.wheel(-120 * 3, 48, 1000);
        b.wheel(-120 * 3, 48, 1000);
        for _ in 0..4 {
            a.step(16.0);
        }
        b.step(64.0);
        assert!((a.pos() - b.pos()).abs() <= 1);
    }

    #[test]
    fn backdrop_size_limit() {
        assert!(backdrop_fits(900, 3000));
        assert!(!backdrop_fits(1200, 20000));
        assert!(!backdrop_fits(0, 100));
        assert_eq!(backdrop_height(300, 500), 500);
        assert_eq!(backdrop_height(900, 500), 900);
    }

    #[test]
    fn rebuild_decision() {
        assert!(needs_rebuild(None, &(1, 2)));
        assert!(!needs_rebuild(Some(&(1, 2)), &(1, 2)));
        assert!(needs_rebuild(Some(&(1, 2)), &(1, 3)));
    }
}
