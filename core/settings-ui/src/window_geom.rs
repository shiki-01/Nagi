//! ウィンドウの幾何（OS 非依存の純粋関数）。WebView2 のウィンドウの位置・大きさの計算に使う。
//!
//! 寸法は 96 DPI（100%）を基準とした dp で持ち、実際の DPI で px にする（[`scale`]）。

/// 長方形（px）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// 左
    pub x: i32,
    /// 上
    pub y: i32,
    /// 幅
    pub w: i32,
    /// 高さ
    pub h: i32,
}

/// dp → px（四捨五入）
pub fn scale(dp: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    i32::try_from((i64::from(dp) * dpi + 48).div_euclid(96)).unwrap_or(i32::MAX)
}

/// px → dp（[`scale`] の逆。四捨五入）。ウィンドウの大きさを DPI によらない形で保存するのに使う
pub fn unscale(px: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    i32::try_from((i64::from(px) * 96 + dpi / 2).div_euclid(dpi)).unwrap_or(i32::MAX)
}

/// 初期のクライアント領域の幅（dp）
pub const INITIAL_CLIENT_W: i32 = 960;
/// 初期のクライアント領域の高さ（dp）
pub const INITIAL_CLIENT_H: i32 = 680;

/// 初期のクライアント領域のサイズ（px）
pub fn initial_client_size(dpi: u32) -> (i32, i32) {
    (scale(INITIAL_CLIENT_W, dpi), scale(INITIAL_CLIENT_H, dpi))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DPIS: [u32; 5] = [96, 120, 144, 192, 240];

    #[test]
    fn scale_rounds_and_is_monotonic() {
        assert_eq!(scale(100, 96), 100);
        assert_eq!(scale(100, 144), 150);
        assert_eq!(scale(100, 192), 200);
        assert_eq!(scale(0, 144), 0);
        assert_eq!(scale(1, 96), 1);
        assert_eq!(scale(3, 144), 5); // 4.5 → 5
        let mut last = 0;
        for dpi in DPIS {
            let v = scale(37, dpi);
            assert!(v >= last);
            last = v;
        }
        let _ = scale(10, 0); // DPI 0 でも落ちない
    }

    #[test]
    fn unscale_inverts_scale() {
        for dpi in DPIS {
            for dp in [0, 1, 7, 100, 760, 880, 1000] {
                assert_eq!(unscale(scale(dp, dpi), dpi), dp, "dp={dp} dpi={dpi}");
            }
        }
        let _ = unscale(10, 0);
    }

    #[test]
    fn initial_size_scales_with_dpi() {
        assert_eq!(
            initial_client_size(96),
            (INITIAL_CLIENT_W, INITIAL_CLIENT_H)
        );
        let (w, h) = initial_client_size(192);
        assert_eq!((w, h), (INITIAL_CLIENT_W * 2, INITIAL_CLIENT_H * 2));
    }
}
