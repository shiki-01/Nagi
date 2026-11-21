//! Nagi のロゴ（`web/static/icon.svg`）の描画と `.ico` の生成（OS 非依存）。
//!
//! 絵柄の正は SVG（藍のグラデーションの角丸の四角に、太陽の円・水平線・波）。ここではその形を
//! コードで再現し（超サンプリングのアンチエイリアス）、次の 2 系統を作る（decisions.md H-41・H-115）。
//!
//! - **ロゴ**（[`render_logo`] / [`encode_ico`]）: 入力方式の一覧（`RegisterProfile` の `.ico`）・設定アプリ・
//!   セットアップ・ショートカットなど、モードを持たない場所のアイコン。16〜256 px。
//!   小さい大きさ（24 px 以下）では、線と波を太くして波の振れ幅を大きくする（SVG のままだと 16 px で潰れる）。
//! - **モード表示つきのアイコン**（[`render_status`]）: タスクバーの入力インジケーター・言語バー用。
//!   現在のモードの文字（あ / A）だけを大きく描く（水平線・波は描かない）。カラー（ロゴの色の背景に生成り色）と、
//!   システムのテーマに合わせて白または黒で描くモノクロ（背景なし）がある。
//!
//! ファイルに書き出した `frontend-windows/assets/nagi.ico` はコミットしてあり（設定アプリ・imserver の
//! exe リソースとセットアップ exe が参照する）、テストが [`encode_ico`] の出力と一致することを確かめる。
//! 絵柄を変えたら `NAGI_UPDATE_ICO=1 cargo test -p nagi-windows icon` で書き直す。

/// `.ico` に含める大きさ（px）
pub const ICON_SIZES: [u32; 6] = [16, 20, 24, 32, 48, 256];

/// 8bit RGB
type Rgb = [u8; 3];

/// 点（SVG と同じ 1024 四方の座標系。y は下向き）
type Pt = (f32, f32);

/// 描画の座標系の 1 辺（SVG の viewBox）
const VIEW: f32 = 1024.0;

/// 背景のグラデーションの上端・下端（SVG の `linearGradient`）
const GRADIENT_TOP: Rgb = [0x5B, 0x78, 0xE8];
const GRADIENT_BOTTOM: Rgb = [0x2B, 0x43, 0xB0];
/// 太陽の色（SVG の `#fff3d6`）
const SUN: Rgb = [0xFF, 0xF3, 0xD6];
const WHITE: Rgb = [0xFF, 0xFF, 0xFF];
/// 角丸の半径（SVG の `rx` = 224）
const CORNER_RADIUS: f32 = 224.0;

/// 小さいアイコン（この大きさ以下）では、潰れないよう形を太く・大きくする
const SMALL_MAX: u32 = 24;

/// モード表示の文字
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeGlyph {
    /// ひらがな（あ）
    Hiragana,
    /// 英数（A）
    Alphanumeric,
}

/// モード表示つきアイコンの配色
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// ロゴの色（藍のグラデーションの角丸の四角に、生成り色の文字）
    Color,
    /// 背景なしの単色（図形を `Rgb` で描く。濃淡は透明度で表す）
    Mono(Rgb),
}

/// 1 つの図形
#[derive(Debug, Clone)]
struct Shape {
    /// 折れ線（点 1 つなら円）。線は丸い端で、`width` は線の太さ（円なら直径）
    points: Vec<Pt>,
    width: f32,
    color: Rgb,
    opacity: f32,
    /// 外接の四角（余白込み。判定の足切り用）
    bounds: (f32, f32, f32, f32),
}

impl Shape {
    fn new(points: Vec<Pt>, width: f32, color: Rgb, opacity: f32) -> Self {
        let r = width / 2.0;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in &points {
            x0 = x0.min(x - r);
            y0 = y0.min(y - r);
            x1 = x1.max(x + r);
            y1 = y1.max(y + r);
        }
        Shape {
            points,
            width,
            color,
            opacity,
            bounds: (x0, y0, x1, y1),
        }
    }

    fn disc(center: Pt, radius: f32, color: Rgb) -> Self {
        Shape::new(vec![center], radius * 2.0, color, 1.0)
    }

    fn contains(&self, p: Pt) -> bool {
        let (x0, y0, x1, y1) = self.bounds;
        if p.0 < x0 || p.0 > x1 || p.1 < y0 || p.1 > y1 {
            return false;
        }
        let r = self.width / 2.0;
        if let [only] = self.points.as_slice() {
            return dist2(p, *only) <= r * r;
        }
        self.points
            .windows(2)
            .any(|w| dist_to_segment2(p, w[0], w[1]) <= r * r)
    }
}

fn dist2(a: Pt, b: Pt) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// 点と線分の距離の 2 乗
fn dist_to_segment2(p: Pt, a: Pt, b: Pt) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0)
    };
    dist2(p, (a.0 + abx * t, a.1 + aby * t))
}

/// 2 次ベジェを折れ線にして `out` へ足す（始点は含めない）
fn flatten_quad(out: &mut Vec<Pt>, p0: Pt, c: Pt, p1: Pt) {
    const STEPS: u32 = 10;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        out.push((
            u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0,
            u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1,
        ));
    }
}

/// 3 次ベジェを折れ線にして `out` へ足す（始点は含めない）
fn flatten_cubic(out: &mut Vec<Pt>, p0: Pt, c0: Pt, c1: Pt, p1: Pt) {
    const STEPS: u32 = 14;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push((
            a * p0.0 + b * c0.0 + c * c1.0 + d * p1.0,
            a * p0.1 + b * c0.1 + c * c1.1 + d * p1.1,
        ));
    }
}

/// SVG の `M x y q dx -a 2dx 0 t ...` の波（`segments` 個の半波。奇数番目は上、偶数番目は下へ振れる）
fn wave(start: Pt, half: f32, amplitude: f32, segments: u32) -> Vec<Pt> {
    let mut out = vec![start];
    let mut from = start;
    for i in 0..segments {
        let sign = if i % 2 == 0 { -1.0 } else { 1.0 };
        // q は制御点が半波の中央・高さが `amplitude` だけずれる（SVG の `q69-40 138 0`）
        let control = (from.0 + half / 2.0, from.1 + sign * amplitude);
        let end = (from.0 + half, from.1);
        flatten_quad(&mut out, from, control, end);
        from = end;
    }
    out
}

/// 水平な線（丸い端）
fn line(a: Pt, b: Pt, width: f32, color: Rgb, opacity: f32) -> Shape {
    Shape::new(vec![a, b], width, color, opacity)
}

/// ロゴの図形（太陽・水平線・波）。`small` のときは太く・大きくする
fn logo_shapes(small: bool, tone: Tone) -> Vec<Shape> {
    let paint = |c: Rgb| match tone {
        Tone::Color => c,
        Tone::Mono(m) => m,
    };
    if small {
        vec![
            Shape::disc((512.0, 330.0), 178.0, paint(SUN)),
            line((130.0, 548.0), (894.0, 548.0), 92.0, paint(WHITE), 1.0),
            Shape::new(
                wave((190.0, 740.0), 161.0, 120.0, 5),
                84.0,
                paint(WHITE),
                0.8,
            ),
        ]
    } else {
        vec![
            Shape::disc((512.0, 392.0), 124.0, paint(SUN)),
            line((170.0, 566.0), (854.0, 566.0), 44.0, paint(WHITE), 1.0),
            Shape::new(
                wave((236.0, 692.0), 138.0, 40.0, 4),
                40.0,
                paint(WHITE),
                0.7,
            ),
            Shape::new(
                wave((376.0, 810.0), 136.0, 38.0, 2),
                40.0,
                paint(WHITE),
                0.42,
            ),
        ]
    }
}

/// 文字の外接の四角（1024 座標）に文字を描く折れ線の組を返す。文字側の座標は 0〜1 の正方形
fn glyph_strokes(glyph: ModeGlyph, x: f32, y: f32, w: f32, h: f32) -> Vec<Vec<Pt>> {
    let map = |p: Pt| (x + p.0 * w, y + p.1 * h);
    let strokes: Vec<Vec<Pt>> = match glyph {
        ModeGlyph::Alphanumeric => vec![
            vec![(0.06, 1.0), (0.5, 0.0), (0.94, 1.0)],
            vec![(0.24, 0.68), (0.76, 0.68)],
        ],
        ModeGlyph::Hiragana => {
            // あ: 上の横画・縦画（下で左へ曲がる）・左から出て右へ回り込む大きな曲線の 3 画
            let bar = vec![(0.14, 0.30), (0.80, 0.24)];
            let mut stem = vec![(0.50, 0.02)];
            flatten_cubic(
                &mut stem,
                (0.50, 0.02),
                (0.50, 0.30),
                (0.46, 0.55),
                (0.40, 0.78),
            );
            let mut loop_ = vec![(0.22, 0.52)];
            flatten_cubic(
                &mut loop_,
                (0.22, 0.52),
                (0.46, 0.34),
                (0.92, 0.34),
                (0.90, 0.66),
            );
            flatten_cubic(
                &mut loop_,
                (0.90, 0.66),
                (0.88, 0.94),
                (0.52, 1.04),
                (0.24, 0.84),
            );
            vec![bar, stem, loop_]
        }
    };
    strokes
        .into_iter()
        .map(|s| s.into_iter().map(map).collect())
        .collect()
}

/// モード表示つきアイコンの図形（文字だけ。水平線・波は描かない）。
/// 文字は外接の四角（線の太さを含めて）を背景いっぱいに広げる。カラーは角丸の背景の内側に収め、
/// モノクロは背景がないので端まで使う
fn status_shapes(glyph: ModeGlyph, small: bool, tone: Tone) -> Vec<Shape> {
    let (color, (gx, gy, gw, gh), stroke) = match (tone, small) {
        (Tone::Color, true) => (SUN, (200.0, 190.0, 624.0, 640.0), 100.0),
        (Tone::Color, false) => (SUN, (200.0, 190.0, 624.0, 640.0), 88.0),
        (Tone::Mono(m), true) => (m, (130.0, 120.0, 764.0, 780.0), 110.0),
        (Tone::Mono(m), false) => (m, (130.0, 120.0, 764.0, 780.0), 96.0),
    };
    glyph_strokes(glyph, gx, gy, gw, gh)
        .into_iter()
        .map(|pts| Shape::new(pts, stroke, color, 1.0))
        .collect()
}

/// 角丸の四角の内側か（1024 座標）
fn in_rounded_square(p: Pt) -> bool {
    let cx = p.0.clamp(CORNER_RADIUS, VIEW - CORNER_RADIUS);
    let cy = p.1.clamp(CORNER_RADIUS, VIEW - CORNER_RADIUS);
    dist2(p, (cx, cy)) <= CORNER_RADIUS * CORNER_RADIUS
}

fn gradient(y: f32) -> [f32; 3] {
    let t = (y / VIEW).clamp(0.0, 1.0);
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = f32::from(GRADIENT_TOP[i]) * (1.0 - t) + f32::from(GRADIENT_BOTTOM[i]) * t;
    }
    out
}

/// 超サンプリングの 1 辺の分割数
fn samples_for(size: u32) -> u32 {
    if size >= 128 { 3 } else { 4 }
}

/// `size` px 四方の RGBA（行は上から下、非乗算）に描く。`tile` は背景（グラデーションの角丸の四角）の有無
fn rasterize(size: u32, tile: bool, shapes: &[Shape]) -> Vec<u8> {
    let samples = samples_for(size);
    let count = (samples * samples) as f32;
    let scale = VIEW / size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            // 乗算済みアルファで平均する（acc = [r, g, b, a]）
            let mut acc = [0.0f32; 4];
            for sy in 0..samples {
                for sx in 0..samples {
                    let p = (
                        (x as f32 + (sx as f32 + 0.5) / samples as f32) * scale,
                        (y as f32 + (sy as f32 + 0.5) / samples as f32) * scale,
                    );
                    // 乗算済みの色（rgb）と不透明度（alpha）
                    let (mut rgb, mut alpha) = if tile {
                        if !in_rounded_square(p) {
                            continue;
                        }
                        (gradient(p.1), 1.0f32)
                    } else {
                        ([0.0f32; 3], 0.0f32)
                    };
                    for shape in shapes.iter().filter(|s| s.contains(p)) {
                        let o = shape.opacity;
                        for (c, v) in rgb.iter_mut().zip(shape.color) {
                            *c = *c * (1.0 - o) + f32::from(v) * o;
                        }
                        alpha = alpha * (1.0 - o) + o;
                    }
                    acc[0] += rgb[0];
                    acc[1] += rgb[1];
                    acc[2] += rgb[2];
                    acc[3] += alpha;
                }
            }
            if acc[3] <= 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            for c in &acc[..3] {
                out.push((c / acc[3]).round().clamp(0.0, 255.0) as u8);
            }
            out.push((acc[3] / count * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    out
}

/// ロゴ（モードなし）の `size` px 四方の RGBA（行は上から下、非乗算）
pub fn render_logo(size: u32) -> Vec<u8> {
    rasterize(size, true, &logo_shapes(size <= SMALL_MAX, Tone::Color))
}

/// モード表示つきアイコンの `size` px 四方の RGBA（行は上から下、非乗算）。
/// 言語バー・入力インジケーター用で、`size` はシステムの小アイコンの大きさ（16〜）
pub fn render_status(size: u32, glyph: ModeGlyph, tone: Tone) -> Vec<u8> {
    let small = size <= SMALL_MAX;
    let shapes = status_shapes(glyph, small, tone);
    rasterize(size, matches!(tone, Tone::Color), &shapes)
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// 1 枚分の DIB（BITMAPINFOHEADER + 下から上の BGRA + AND マスク）
fn dib(size: u32) -> Vec<u8> {
    let rgba = render_logo(size);
    let mut out = Vec::new();
    push_u32(&mut out, 40);
    push_u32(&mut out, size);
    push_u32(&mut out, size * 2); // 高さは XOR + AND の 2 倍
    push_u16(&mut out, 1);
    push_u16(&mut out, 32);
    push_u32(&mut out, 0); // BI_RGB
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    for row in (0..size).rev() {
        for col in 0..size {
            let i = ((row * size + col) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    // AND マスク（1bpp、各行 4 バイト境界）。透明度は alpha で表すので 0
    let mask_row = size.div_ceil(32) * 4;
    out.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    out
}

/// ロゴの `.ico` ファイルの内容（[`ICON_SIZES`] の各大きさ。32bpp の DIB 形式）
pub fn encode_ico() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICON_SIZES.iter().map(|&s| (s, dib(s))).collect();
    let mut out = Vec::new();
    push_u16(&mut out, 0);
    push_u16(&mut out, 1); // ICO
    push_u16(&mut out, images.len() as u16);
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        // 256 は 0 と書く（ICO の規則）
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(0); // パレット無し
        out.push(0);
        push_u16(&mut out, 1); // planes
        push_u16(&mut out, 32);
        push_u32(&mut out, data.len() as u32);
        push_u32(&mut out, offset);
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        out.extend_from_slice(data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }
    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    fn pixel(px: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn ico_directory_is_consistent() {
        let ico = encode_ico();
        assert_eq!(u16_at(&ico, 0), 0);
        assert_eq!(u16_at(&ico, 2), 1);
        let count = usize::from(u16_at(&ico, 4));
        assert_eq!(count, ICON_SIZES.len());
        assert_eq!(ICON_SIZES, [16, 20, 24, 32, 48, 256]);
        for (i, expected_size) in ICON_SIZES.iter().enumerate() {
            let e = 6 + 16 * i;
            let size = if ico[e] == 0 { 256 } else { u32::from(ico[e]) };
            assert_eq!(size, *expected_size);
            assert_eq!(u16_at(&ico, e + 6), 32, "bit count");
            let len = u32_at(&ico, e + 8) as usize;
            let off = u32_at(&ico, e + 12) as usize;
            assert!(off + len <= ico.len());
            // DIB ヘッダ
            assert_eq!(u32_at(&ico, off), 40);
            assert_eq!(u32_at(&ico, off + 4), size);
            assert_eq!(u32_at(&ico, off + 8), size * 2);
            let expected =
                40 + (size * size * 4) as usize + (size.div_ceil(32) * 4 * size) as usize;
            assert_eq!(len, expected);
        }
        assert_eq!(
            ico.len(),
            6 + 16 * count
                + (0..count)
                    .map(|i| u32_at(&ico, 6 + 16 * i + 8) as usize)
                    .sum::<usize>()
        );
    }

    #[test]
    fn logo_matches_the_svg_layout() {
        let size = 256;
        let px = render_logo(size);
        assert_eq!(px.len(), (size * size * 4) as usize);
        // 角は角丸の外側で透明
        assert_eq!(pixel(&px, size, 0, 0)[3], 0);
        // 左上寄りの内側は不透明な藍（グラデーションの上端に近い）
        let top = pixel(&px, size, 128, 8);
        assert_eq!(top[3], 255);
        assert!(top[2] > top[0], "藍: B が R より強い {top:?}");
        // 太陽の中心（512, 392 → 128, 98）は生成りの色
        assert_eq!(pixel(&px, size, 128, 98), [0xFF, 0xF3, 0xD6, 0xFF]);
        // 水平線の中央（y = 566 → 141.5）は白
        assert_eq!(pixel(&px, size, 128, 141), [0xFF, 0xFF, 0xFF, 0xFF]);
        // 下端は上端より暗い（縦のグラデーション）
        let bottom = pixel(&px, size, 128, 248);
        assert!(bottom[0] < top[0] && bottom[1] < top[1]);
    }

    #[test]
    fn every_size_has_opaque_center_and_transparent_corners() {
        for &size in &ICON_SIZES {
            let px = render_logo(size);
            assert_eq!(px.len(), (size * size * 4) as usize, "{size}");
            assert_eq!(pixel(&px, size, 0, 0)[3], 0, "{size}: 角");
            assert_eq!(pixel(&px, size, size - 1, size - 1)[3], 0, "{size}: 角");
            assert_eq!(pixel(&px, size, size / 2, 1)[3], 255, "{size}: 上辺の中央");
        }
    }

    /// 白（明るい）画素が 1 つでもあるか
    fn has_bright(px: &[u8]) -> bool {
        px.chunks_exact(4)
            .any(|p| p[3] == 255 && p[0] > 240 && p[1] > 240 && p[2] > 200)
    }

    #[test]
    fn status_icons_differ_by_mode_at_every_size() {
        for size in [16u32, 20, 24, 32, 48] {
            for tone in [Tone::Color, Tone::Mono([0, 0, 0])] {
                let a = render_status(size, ModeGlyph::Hiragana, tone);
                let b = render_status(size, ModeGlyph::Alphanumeric, tone);
                assert_eq!(a.len(), (size * size * 4) as usize);
                assert_ne!(a, b, "{size} {tone:?}: あ と A が同じ絵");
                // モードごとの違いが十分ある（16px で数画素以上）
                let diff = a
                    .chunks_exact(4)
                    .zip(b.chunks_exact(4))
                    .filter(|(p, q)| p != q)
                    .count();
                assert!(diff >= 6, "{size} {tone:?}: 差が小さい ({diff} 画素)");
            }
        }
    }

    #[test]
    fn color_status_icon_keeps_the_logo_tile() {
        let size = 32;
        let px = render_status(size, ModeGlyph::Hiragana, Tone::Color);
        assert_eq!(pixel(&px, size, 0, 0)[3], 0, "角は透明");
        let top = pixel(&px, size, 16, 0);
        assert_eq!(top[3], 255);
        assert!(top[2] > top[0]);
        assert!(has_bright(&px), "文字が明るい色で描かれている");
    }

    #[test]
    fn mono_status_icon_is_single_color_on_transparent() {
        for (fg, name) in [([0xFF, 0xFF, 0xFF], "white"), ([0, 0, 0], "black")] {
            for &size in &[16u32, 24, 32] {
                let px = render_status(size, ModeGlyph::Alphanumeric, Tone::Mono(fg));
                assert_eq!(pixel(&px, size, 0, 0)[3], 0, "{name} {size}: 背景は透明");
                let mut opaque = 0;
                for p in px.chunks_exact(4) {
                    if p[3] > 0 {
                        // 半透明の画素も含め、色は常に指定色（乗算なしの RGBA）
                        assert_eq!([p[0], p[1], p[2]], fg, "{name} {size}");
                    }
                    if p[3] == 255 {
                        opaque += 1;
                    }
                }
                assert!(opaque >= 10, "{name} {size}: 不透明な画素が少なすぎる");
            }
        }
    }

    #[test]
    fn small_sizes_are_legible_not_blank() {
        // 16px でも文字の分だけ、明るい画素が十分にある
        let px = render_status(16, ModeGlyph::Hiragana, Tone::Color);
        let bright = px
            .chunks_exact(4)
            .filter(|p| p[3] == 255 && p[0] > 200 && p[1] > 200)
            .count();
        assert!(bright >= 20, "16px の明るい画素 {bright}");
    }

    /// 書き出し済みの `assets/nagi.ico` が、いまの絵柄から生成したものと一致する。
    /// `NAGI_UPDATE_ICO=1` を付けて実行すると、ファイルを書き直す
    #[test]
    fn committed_ico_matches_the_generated_one() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/nagi.ico");
        let generated = encode_ico();
        if std::env::var_os("NAGI_UPDATE_ICO").is_some() {
            std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).unwrap();
            std::fs::write(path, &generated).unwrap();
        }
        let committed =
            std::fs::read(path).expect("assets/nagi.ico が無い（NAGI_UPDATE_ICO=1 で作る）");
        assert!(
            committed == generated,
            "assets/nagi.ico が古い。NAGI_UPDATE_ICO=1 cargo test -p nagi-windows icon で書き直す"
        );
    }

    /// 目視確認用: `NAGI_ICON_DUMP_DIR` があれば、各絵柄を RGBA の生データ（`<名前>_<大きさ>.rgba`）で書き出す
    #[test]
    fn dump_for_visual_check() {
        let Some(dir) = std::env::var_os("NAGI_ICON_DUMP_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for &size in &[16u32, 20, 24, 32, 48, 256] {
            std::fs::write(dir.join(format!("logo_{size}.rgba")), render_logo(size)).unwrap();
            for (name, glyph) in [
                ("hira", ModeGlyph::Hiragana),
                ("alnum", ModeGlyph::Alphanumeric),
            ] {
                for (tone_name, tone) in [
                    ("color", Tone::Color),
                    ("white", Tone::Mono([255, 255, 255])),
                    ("black", Tone::Mono([0, 0, 0])),
                ] {
                    std::fs::write(
                        dir.join(format!("{name}_{tone_name}_{size}.rgba")),
                        render_status(size, glyph, tone),
                    )
                    .unwrap();
                }
            }
        }
    }
}
