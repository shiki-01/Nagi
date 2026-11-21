//! ネイティブ設定ウィンドウのレイアウト計算（OS 非依存の純粋関数）。
//!
//! 寸法は 96 DPI（100%）を基準とした dp で持ち、実際の DPI で px にする（[`scale`]）。
//! 文字の折り返しの高さはウィンドウ側が実測して渡す（ここは文字を測らない）。

/// 長方形（クライアント座標。px）
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

/// 最小のクライアント領域の幅（dp）
pub const MIN_CLIENT_W: i32 = 700;
/// 最小のクライアント領域の高さ（dp）
pub const MIN_CLIENT_H: i32 = 400;
/// 初期のクライアント領域の幅（dp）
pub const INITIAL_CLIENT_W: i32 = 820;
/// 初期のクライアント領域の高さ（dp）
pub const INITIAL_CLIENT_H: i32 = 600;

const MARGIN: i32 = 12;
const LIST_W: i32 = 188;
const TOP_BAR_H: i32 = 32;
const STATUS_H: i32 = 26;
const PANEL_PAD: i32 = 16;
const ROW_GAP: i32 = 20;
const LINE_GAP: i32 = 4;
/// 入力欄・ボタンの高さ
pub const CONTROL_H: i32 = 26;
const CONTROL_W: i32 = 200;
const RESET_W: i32 = 96;
const HINT_GAP: i32 = 10;
const SCROLL_STEP: i32 = 48;

/// ウィンドウ全体の部品の配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayout {
    /// 左のカテゴリ一覧
    pub list: Rect,
    /// 「詳細設定を表示」
    pub detail_check: Rect,
    /// 右のスクロールパネル
    pub panel: Rect,
    /// ステータス行
    pub status: Rect,
}

/// クライアント領域（px）から部品の配置を求める。小さすぎるときも幅・高さは 0 以上
pub fn frame_layout(client_w: i32, client_h: i32, dpi: u32) -> FrameLayout {
    let m = scale(MARGIN, dpi);
    let list_w = scale(LIST_W, dpi);
    let top = scale(TOP_BAR_H, dpi);
    let status_h = scale(STATUS_H, dpi);
    let right_x = m + list_w + m;
    let right_w = (client_w - right_x - m).max(0);
    let body_y = m + top;
    let status_y = (client_h - m - status_h).max(body_y);
    let body_h = (status_y - m / 2 - body_y).max(0);
    FrameLayout {
        list: Rect {
            x: m,
            y: body_y,
            w: list_w,
            h: body_h,
        },
        detail_check: Rect {
            x: right_x,
            y: m,
            w: right_w,
            h: top,
        },
        panel: Rect {
            x: right_x,
            y: body_y,
            w: right_w,
            h: body_h,
        },
        status: Rect {
            x: m,
            y: status_y,
            w: (client_w - 2 * m).max(0),
            h: status_h,
        },
    }
}

/// パネル内の列の位置（パネルのクライアント幅から。スクロールバーを除いた幅を渡す）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    /// 内容の左端
    pub x: i32,
    /// 内容の幅（ラベル・説明・エラーの折り返し幅）
    pub width: i32,
    /// 入力欄の幅
    pub control_w: i32,
    /// 範囲の案内の左端（入力欄の右）
    pub hint_x: i32,
    /// 範囲の案内の幅（「既定に戻す」の左まで）
    pub hint_w: i32,
    /// 「既定に戻す」の左端
    pub reset_x: i32,
    /// 「既定に戻す」の幅
    pub reset_w: i32,
}

/// 列を求める
pub fn columns(panel_client_w: i32, dpi: u32) -> Columns {
    let pad = scale(PANEL_PAD, dpi);
    let width = (panel_client_w - 2 * pad).max(scale(120, dpi));
    let control_w = scale(CONTROL_W, dpi).min(width);
    let reset_w = scale(RESET_W, dpi);
    let gap = scale(HINT_GAP, dpi);
    let hint_x = pad + control_w + gap;
    let reset_x = (pad + width - reset_w).max(hint_x);
    Columns {
        x: pad,
        width,
        control_w,
        hint_x,
        hint_w: (reset_x - gap - hint_x).max(0),
        reset_x,
        reset_w,
    }
}

/// 1 行（項目 1 つ）の部品の高さ（px。無ければ 0）。折り返しの高さはウィンドウ側が測って渡す
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowSpec {
    /// ラベル（チェックボックスのときはそれ全体）の高さ
    pub title_h: i32,
    /// 説明の高さ
    pub desc_h: i32,
    /// 入力欄・ボタンの行の高さ
    pub control_h: i32,
    /// エラーの高さ
    pub error_h: i32,
}

/// 1 行の配置（内容座標。スクロール前の y）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowLayout {
    /// 行の上端
    pub top: i32,
    /// ラベルの上端
    pub title_y: i32,
    /// 説明の上端
    pub desc_y: i32,
    /// 入力欄の行の上端
    pub control_y: i32,
    /// エラーの上端
    pub error_y: i32,
    /// 行の高さ（下の余白を含まない）
    pub height: i32,
}

/// パネル全体の配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelLayout {
    /// 各行
    pub rows: Vec<RowLayout>,
    /// 内容の高さ（上下の余白を含む。スクロール範囲の元）
    pub content_height: i32,
}

/// 行を縦に積む。部品が無い（高さ 0 の）段は詰め、段の間に [`LINE_GAP`] を入れる
pub fn layout_rows(rows: &[RowSpec], dpi: u32) -> PanelLayout {
    let pad = scale(PANEL_PAD, dpi);
    let row_gap = scale(ROW_GAP, dpi);
    let line_gap = scale(LINE_GAP, dpi);
    let mut y = pad;
    let mut out = Vec::with_capacity(rows.len());
    for (n, spec) in rows.iter().enumerate() {
        if n > 0 {
            y += row_gap;
        }
        let top = y;
        let mut place = |h: i32| -> i32 {
            if h <= 0 {
                return y;
            }
            let at = y;
            y += h + line_gap;
            at
        };
        let title_y = place(spec.title_h);
        let desc_y = place(spec.desc_h);
        let control_y = place(spec.control_h);
        let error_y = place(spec.error_h);
        let any = [spec.title_h, spec.desc_h, spec.control_h, spec.error_h]
            .iter()
            .any(|h| *h > 0);
        if any {
            y -= line_gap; // 最後の段の下の隙間は行の高さに含めない
        }
        out.push(RowLayout {
            top,
            title_y,
            desc_y,
            control_y,
            error_y,
            height: y - top,
        });
    }
    PanelLayout {
        rows: out,
        content_height: y + pad,
    }
}

/// スクロール位置の最大値
pub fn max_scroll(content_height: i32, viewport_height: i32) -> i32 {
    (content_height - viewport_height).max(0)
}

/// スクロール位置を範囲内にする
pub fn clamp_scroll(pos: i32, content_height: i32, viewport_height: i32) -> i32 {
    pos.clamp(0, max_scroll(content_height, viewport_height))
}

/// マウスホイール 1 ノッチ分の移動量（px）
pub fn wheel_step(dpi: u32) -> i32 {
    scale(SCROLL_STEP, dpi)
}

/// 内容座標の `[top, bottom)` が見えるように、スクロール位置を最小限動かす
pub fn scroll_into_view(
    scroll: i32,
    viewport_height: i32,
    top: i32,
    bottom: i32,
    content_height: i32,
    margin: i32,
) -> i32 {
    let mut pos = scroll;
    if top - margin < pos {
        pos = top - margin;
    } else if bottom + margin > pos + viewport_height {
        pos = bottom + margin - viewport_height;
    }
    clamp_scroll(pos, content_height, viewport_height)
}

/// ウィンドウの最小サイズ（クライアント領域。px）
pub fn min_client_size(dpi: u32) -> (i32, i32) {
    (scale(MIN_CLIENT_W, dpi), scale(MIN_CLIENT_H, dpi))
}

/// 初期のクライアント領域のサイズ（px）
pub fn initial_client_size(dpi: u32) -> (i32, i32) {
    (scale(INITIAL_CLIENT_W, dpi), scale(INITIAL_CLIENT_H, dpi))
}

/// 入力欄・ボタンの高さ（px）
pub fn control_height(dpi: u32) -> i32 {
    scale(CONTROL_H, dpi)
}

/// ボタンを横に並べる（左から。幅は `widths_dp`、間隔は一定）。各ボタンの `(x, w)`
pub fn button_row(origin_x: i32, widths_dp: &[i32], dpi: u32) -> Vec<(i32, i32)> {
    let gap = scale(8, dpi);
    let mut x = origin_x;
    widths_dp
        .iter()
        .map(|dp| {
            let w = scale(*dp, dpi);
            let out = (x, w);
            x += w + gap;
            out
        })
        .collect()
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
    fn frame_regions_fit_inside_the_client_area() {
        for dpi in DPIS {
            let (min_w, min_h) = min_client_size(dpi);
            for (w, h) in [(min_w, min_h), (scale(900, dpi), scale(700, dpi))] {
                let f = frame_layout(w, h, dpi);
                for r in [f.list, f.detail_check, f.panel, f.status] {
                    assert!(r.w >= 0 && r.h >= 0, "{r:?}");
                    assert!(r.x >= 0 && r.x + r.w <= w, "{r:?} w={w}");
                    assert!(r.y >= 0 && r.y + r.h <= h, "{r:?} h={h}");
                }
                assert!(f.list.x + f.list.w <= f.panel.x);
                assert!(f.panel.y + f.panel.h <= f.status.y);
                assert!(f.panel.w > scale(300, dpi));
                assert!(f.panel.h > scale(150, dpi));
            }
        }
    }

    #[test]
    fn frame_layout_survives_a_tiny_window() {
        let f = frame_layout(10, 10, 96);
        assert!(f.panel.w >= 0 && f.panel.h >= 0 && f.list.h >= 0);
    }

    #[test]
    fn columns_are_ordered_and_fit_the_width() {
        for dpi in DPIS {
            let (min_w, _) = min_client_size(dpi);
            let panel_w = frame_layout(min_w, scale(500, dpi), dpi).panel.w;
            let c = columns(panel_w, dpi);
            assert!(c.x > 0);
            assert!(c.x + c.width <= panel_w);
            assert!(c.control_w <= c.width);
            assert!(c.hint_x >= c.x + c.control_w);
            assert!(c.reset_x >= c.hint_x);
            assert!(c.reset_x + c.reset_w <= c.x + c.width + 1);
            assert!(c.hint_x + c.hint_w <= c.reset_x);
        }
        // 広いパネルでは案内の幅が広がる
        assert!(columns(900, 96).hint_w > columns(500, 96).hint_w);
    }

    fn sample_rows(dpi: u32) -> Vec<RowSpec> {
        let line = scale(16, dpi);
        vec![
            RowSpec {
                title_h: line,
                desc_h: line * 2,
                control_h: control_height(dpi),
                error_h: 0,
            },
            RowSpec {
                title_h: line,
                desc_h: line,
                control_h: 0,
                error_h: line,
            },
            RowSpec::default(),
            RowSpec {
                title_h: line,
                ..RowSpec::default()
            },
        ]
    }

    #[test]
    fn rows_stack_without_overlap() {
        for dpi in DPIS {
            let rows = sample_rows(dpi);
            let layout = layout_rows(&rows, dpi);
            assert_eq!(layout.rows.len(), rows.len());
            let mut prev_bottom = 0;
            for (spec, row) in rows.iter().zip(&layout.rows) {
                assert!(row.top >= prev_bottom);
                let mut cursor = row.top;
                for (h, y) in [
                    (spec.title_h, row.title_y),
                    (spec.desc_h, row.desc_y),
                    (spec.control_h, row.control_y),
                    (spec.error_h, row.error_y),
                ] {
                    if h > 0 {
                        assert!(y >= cursor, "dpi={dpi}");
                        cursor = y + h;
                    }
                }
                assert_eq!(row.top + row.height, cursor.max(row.top));
                prev_bottom = row.top + row.height;
            }
            assert!(layout.content_height > prev_bottom);
        }
    }

    #[test]
    fn content_height_grows_with_dpi_and_rows() {
        let mut last = 0;
        for dpi in DPIS {
            let h = layout_rows(&sample_rows(dpi), dpi).content_height;
            assert!(h > last, "dpi={dpi} h={h} last={last}");
            last = h;
        }
        let empty = layout_rows(&[], 96);
        assert_eq!(empty.content_height, 2 * scale(PANEL_PAD, 96));
        let one = layout_rows(&sample_rows(96)[..1], 96).content_height;
        let two = layout_rows(&sample_rows(96)[..2], 96).content_height;
        assert!(two > one);
    }

    #[test]
    fn scroll_range_and_clamping() {
        assert_eq!(max_scroll(500, 300), 200);
        assert_eq!(max_scroll(200, 300), 0);
        assert_eq!(clamp_scroll(-5, 500, 300), 0);
        assert_eq!(clamp_scroll(250, 500, 300), 200);
        assert_eq!(clamp_scroll(100, 500, 300), 100);
        for dpi in DPIS {
            let layout = layout_rows(&sample_rows(dpi), dpi);
            let viewport = scale(100, dpi);
            let max = max_scroll(layout.content_height, viewport);
            assert_eq!(max, layout.content_height - viewport);
            assert!(wheel_step(dpi) > 0);
        }
        assert!(wheel_step(192) > wheel_step(96));
    }

    #[test]
    fn scroll_into_view_moves_the_minimum() {
        // 見えているなら動かない
        assert_eq!(scroll_into_view(100, 300, 150, 200, 1000, 8), 100);
        // 下に隠れている → 下端が見える位置へ
        assert_eq!(scroll_into_view(0, 300, 400, 430, 1000, 8), 138);
        // 上に隠れている → 上端が見える位置へ
        assert_eq!(scroll_into_view(500, 300, 300, 330, 1000, 8), 292);
        // 範囲を超えない
        assert_eq!(scroll_into_view(0, 300, 990, 1000, 1000, 8), 700);
        assert_eq!(scroll_into_view(50, 300, 0, 20, 1000, 8), 0);
    }

    #[test]
    fn button_row_places_buttons_left_to_right() {
        let b = button_row(16, &[100, 120, 90], 144);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0], (16, 150));
        assert!(b[1].0 >= b[0].0 + b[0].1);
        assert!(b[2].0 >= b[1].0 + b[1].1);
        assert_eq!(b[1].1, 180);
    }

    #[test]
    fn initial_size_is_not_smaller_than_the_minimum() {
        for dpi in DPIS {
            let (iw, ih) = initial_client_size(dpi);
            let (mw, mh) = min_client_size(dpi);
            assert!(iw >= mw && ih >= mh);
        }
    }
}
