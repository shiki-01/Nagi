//! 幾何: キー配置（キー単位への正規化）、軌跡の間引き・再サンプリング・正規化。
//!
//! 座標は「キー単位」で扱う: フロントのキー矩形（`set_layout_geometry` と同じ座標系）から求めた
//! キー間隔（ピッチ。横・縦別）で割った値。これで端末・向き・キーの大きさに依存せず、
//! 位置のばらつき（σ）をキー幅の比で決められる。

/// フロントから届く軌跡の点 1 つ（`set_layout_geometry` のキー矩形と同じ座標系）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TracePoint {
    /// 横
    pub x: f64,
    /// 縦
    pub y: f64,
    /// 時刻（ミリ秒。ストローク内で単調増加。原点は任意）。現状の認識では使わない
    pub t: f64,
}

/// キー単位に直した点
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    /// 横
    pub x: f32,
    /// 縦
    pub y: f32,
}

impl Pt {
    /// 2 点間の距離
    pub fn dist(self, o: Pt) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// キー矩形 1 つ（QWERTY の英字キー）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyRect {
    /// キーの文字（小文字の `a`〜`z`）
    pub label: char,
    /// 左端
    pub x: f32,
    /// 上端
    pub y: f32,
    /// 幅
    pub w: f32,
    /// 高さ
    pub h: f32,
}

/// 配置に必要な英字キーの最小数（これ未満の配置はグライドに使えない。12 キーのかななど）
pub const MIN_LETTER_KEYS: usize = 20;

/// キー配置。各英字キーの中心をキー単位で持つ
#[derive(Debug, Clone, PartialEq)]
pub struct GlideLayout {
    /// `a`〜`z` の中心（キー単位）。無いキーは `None`
    centers: [Option<Pt>; 26],
    /// 横・縦のキー間隔（フロントの座標系での値）
    pitch: (f32, f32),
}

impl GlideLayout {
    /// キー矩形から作る。英字キーが [`MIN_LETTER_KEYS`] 個未満なら `None`。同じ文字のキーは最初のものだけ使う
    pub fn new(rects: &[KeyRect]) -> Option<GlideLayout> {
        let mut keys: Vec<KeyRect> = Vec::new();
        for r in rects {
            if r.label.is_ascii_lowercase()
                && r.w > 0.0
                && r.h > 0.0
                && !keys.iter().any(|k| k.label == r.label)
            {
                keys.push(*r);
            }
        }
        if keys.len() < MIN_LETTER_KEYS {
            return None;
        }
        let pitch = pitch_of(&keys);
        let mut centers = [None; 26];
        for k in &keys {
            centers[(k.label as u8 - b'a') as usize] = Some(Pt {
                x: (k.x + k.w / 2.0) / pitch.0,
                y: (k.y + k.h / 2.0) / pitch.1,
            });
        }
        Some(GlideLayout { centers, pitch })
    }

    /// 標準 QWERTY（ANSI の段のずれ 0 / 0.25 / 0.75、キー幅 = 間隔 = 1）。テスト・評価用
    pub fn qwerty() -> GlideLayout {
        Self::qwerty_phone()
    }

    /// スマートフォン風の QWERTY: キーは縦長（幅 0.9、高さ 1.3 に対し間隔は幅 1、高さ 1.45）で、
    /// 段のずれはなく 2 段目のみ半キーずれる。実機のキーボードに近い縦横比を確認する用途
    pub fn qwerty_phone() -> GlideLayout {
        const ROWS: [(&str, f32); 3] = [("qwertyuiop", 0.0), ("asdfghjkl", 0.5), ("zxcvbnm", 1.5)];
        let mut rects = Vec::new();
        for (row, (letters, offset)) in ROWS.iter().enumerate() {
            for (col, label) in letters.chars().enumerate() {
                rects.push(KeyRect {
                    label,
                    x: offset + col as f32 + 0.05,
                    y: row as f32 * 1.45 + 0.075,
                    w: 0.9,
                    h: 1.3,
                });
            }
        }
        GlideLayout::new(&rects).expect("26 キー")
    }

    /// 文字のキーの中心（キー単位）
    pub fn center(&self, letter: u8) -> Option<Pt> {
        self.centers.get(letter as usize).copied().flatten()
    }

    /// フロントの座標をキー単位に直す
    pub fn to_units(&self, x: f64, y: f64) -> Pt {
        Pt {
            x: x as f32 / self.pitch.0,
            y: y as f32 / self.pitch.1,
        }
    }

    /// キー単位の座標をフロントの座標に戻す
    pub fn from_units(&self, p: Pt) -> (f64, f64) {
        (f64::from(p.x * self.pitch.0), f64::from(p.y * self.pitch.1))
    }

    /// `p` に近いキー（文字の番号 0..26）を近い順に、中心間距離が `radius` 以内のものを最大 `max` 個
    pub fn nearest_keys(&self, p: Pt, radius: f32, max: usize) -> Vec<u8> {
        let mut found: Vec<(f32, u8)> = self
            .centers
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let d = c.as_ref()?.dist(p);
                (d <= radius).then_some((d, i as u8))
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        found.truncate(max);
        found.into_iter().map(|(_, i)| i).collect()
    }
}

/// 横・縦のキー間隔（隣のキーの中心までの距離。隙間を含む）。横は同じ段で隣り合うキーの中心間距離の
/// 中央値、縦は段の中心の間隔の中央値。求められなければキーの平均の幅・高さ
fn pitch_of(keys: &[KeyRect]) -> (f32, f32) {
    let n = keys.len().max(1) as f32;
    let mean_w = keys.iter().map(|k| k.w).sum::<f32>() / n;
    let mean_h = keys.iter().map(|k| k.h).sum::<f32>() / n;
    let cx = |k: &KeyRect| k.x + k.w / 2.0;
    let cy = |k: &KeyRect| k.y + k.h / 2.0;
    let mut order: Vec<&KeyRect> = keys.iter().collect();
    order.sort_by(|a, b| cy(a).total_cmp(&cy(b)));
    let mut rows: Vec<Vec<&KeyRect>> = Vec::new();
    for k in order {
        match rows.last_mut() {
            Some(row) if (cy(k) - cy(row[0])).abs() < mean_h / 2.0 => row.push(k),
            _ => rows.push(vec![k]),
        }
    }
    let mut xs: Vec<f32> = Vec::new();
    for row in &mut rows {
        row.sort_by(|a, b| cx(a).total_cmp(&cx(b)));
        for pair in row.windows(2) {
            let gap = cx(pair[1]) - cx(pair[0]);
            if gap > 0.0 && gap < mean_w * 1.6 {
                xs.push(gap);
            }
        }
    }
    let row_y: Vec<f32> = rows
        .iter()
        .map(|r| r.iter().map(|k| cy(k)).sum::<f32>() / r.len() as f32)
        .collect();
    let mut ys: Vec<f32> = row_y.windows(2).map(|w| w[1] - w[0]).collect();
    ys.retain(|g| *g > 0.0 && *g < mean_h * 1.6);
    let median = |v: &mut Vec<f32>, default: f32| {
        if v.is_empty() {
            default
        } else {
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        }
    };
    (median(&mut xs, mean_w), median(&mut ys, mean_h))
}

/// 折れ線の長さ
pub fn path_length(points: &[Pt]) -> f32 {
    points.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// 前に残した点から `min_dist` 未満の点を捨てる（最後の点は必ず残す）
pub fn thin(points: &[Pt], min_dist: f32) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        match out.last() {
            Some(last) if last.dist(*p) < min_dist => {
                if i + 1 == points.len() && out.len() > 1 {
                    // 最後の点は、近すぎる直前の点と入れ替えて残す
                    *out.last_mut().expect("非空") = *p;
                } else if i + 1 == points.len() {
                    out.push(*p);
                }
            }
            _ => out.push(*p),
        }
    }
    out
}

/// 折れ線を弧長に沿って等間隔の `n` 点に再サンプリングする（`n >= 2`）。
/// 長さ 0 の折れ線は同じ点を `n` 個並べる
pub fn resample(points: &[Pt], n: usize, out: &mut Vec<Pt>) {
    out.clear();
    if points.is_empty() || n == 0 {
        return;
    }
    let total = path_length(points);
    if points.len() == 1 || total <= f32::EPSILON || n == 1 {
        out.extend(std::iter::repeat_n(points[0], n));
        return;
    }
    let step = total / (n - 1) as f32;
    out.push(points[0]);
    let mut seg = 0usize;
    // 現在の線分の始点からの位置
    let mut walked = 0.0f32;
    for k in 1..n - 1 {
        let target = step * k as f32;
        loop {
            let len = points[seg].dist(points[seg + 1]);
            if seg + 2 >= points.len() || walked + len >= target {
                let t = if len > 0.0 {
                    ((target - walked) / len).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let a = points[seg];
                let b = points[seg + 1];
                out.push(Pt {
                    x: a.x + (b.x - a.x) * t,
                    y: a.y + (b.y - a.y) * t,
                });
                break;
            }
            walked += len;
            seg += 1;
        }
    }
    out.push(*points.last().expect("非空"));
}

/// 形状の正規化: バウンディングボックスの中心を原点にし、長辺が 1 になるよう拡縮する（縦横比は保つ）。
/// 位置・大きさに依らない「形」だけを比べるため（SHARK2 の形状チャネル）
pub fn shape_normalize(points: &[Pt], out: &mut Vec<Pt>) {
    out.clear();
    let Some(first) = points.first() else {
        return;
    };
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
    for p in points {
        min_x = min_x.min(p.x);
        max_x = max_x.max(p.x);
        min_y = min_y.min(p.y);
        max_y = max_y.max(p.y);
    }
    let side = (max_x - min_x).max(max_y - min_y);
    let scale = if side > f32::EPSILON { 1.0 / side } else { 1.0 };
    let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    out.extend(points.iter().map(|p| Pt {
        x: (p.x - cx) * scale,
        y: (p.y - cy) * scale,
    }));
}

/// 対応する点どうしの距離の平均（同じ長さの 2 列）
pub fn mean_distance(a: &[Pt], b: &[Pt]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return f32::MAX;
    }
    a.iter().zip(b).map(|(p, q)| p.dist(*q)).sum::<f32>() / n as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }

    #[test]
    fn resample_is_equidistant_and_keeps_ends() {
        let line = [p(0.0, 0.0), p(1.0, 0.0), p(1.0, 3.0)];
        let mut out = Vec::new();
        resample(&line, 9, &mut out);
        assert_eq!(out.len(), 9);
        assert_eq!(out[0], p(0.0, 0.0));
        assert_eq!(out[8], p(1.0, 3.0));
        // 全長 4 を 8 等分 → 隣の弧長間隔は 0.5
        assert!((out[2].dist(p(1.0, 0.0))) < 1e-5);
        assert!((out[4].y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn resample_handles_degenerate_input() {
        let mut out = Vec::new();
        resample(&[p(2.0, 2.0)], 5, &mut out);
        assert_eq!(out, vec![p(2.0, 2.0); 5]);
        resample(&[p(1.0, 1.0), p(1.0, 1.0)], 3, &mut out);
        assert_eq!(out, vec![p(1.0, 1.0); 3]);
    }

    #[test]
    fn thin_drops_close_points_but_keeps_last() {
        let pts = [p(0.0, 0.0), p(0.01, 0.0), p(0.5, 0.0), p(0.51, 0.0)];
        let out = thin(&pts, 0.1);
        assert_eq!(out.first(), Some(&p(0.0, 0.0)));
        assert_eq!(out.last(), Some(&p(0.51, 0.0)));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn shape_normalize_is_scale_and_shift_invariant() {
        let a = [p(0.0, 0.0), p(2.0, 0.0), p(2.0, 1.0)];
        let b = [p(10.0, 5.0), p(14.0, 5.0), p(14.0, 7.0)];
        let (mut na, mut nb) = (Vec::new(), Vec::new());
        shape_normalize(&a, &mut na);
        shape_normalize(&b, &mut nb);
        assert!(mean_distance(&na, &nb) < 1e-6);
    }

    #[test]
    fn layout_pitch_matches_key_spacing() {
        let l = GlideLayout::qwerty_phone();
        // q と w の中心間隔が横 1 キー分になる（キー単位で 1.0）
        let (q, w) = (
            l.center(b'q' - b'a').unwrap(),
            l.center(b'w' - b'a').unwrap(),
        );
        assert!((w.x - q.x - 1.0).abs() < 1e-4);
        // 段の間隔も 1.0
        let a = l.center(0).unwrap();
        assert!((a.y - q.y - 1.0).abs() < 1e-4);
    }

    #[test]
    fn layout_rejects_small_layouts() {
        let rects: Vec<KeyRect> = "abcdefghij"
            .chars()
            .enumerate()
            .map(|(i, label)| KeyRect {
                label,
                x: i as f32,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            })
            .collect();
        assert!(GlideLayout::new(&rects).is_none());
    }

    #[test]
    fn nearest_keys_sorted_and_limited() {
        let l = GlideLayout::qwerty_phone();
        let g = l.center(b'g' - b'a').unwrap();
        let near = l.nearest_keys(g, 1.2, 3);
        assert_eq!(near[0], b'g' - b'a');
        assert!(near.len() <= 3);
    }
}
