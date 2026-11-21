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
//!   文字は「カバレッジマスク（size × size の u8）」として受け取って合成する純粋関数（[`compose_status`]）。
//!   マスクの供給元は、本番では TSF 層が GDI でシステムの日本語 UI フォントから描いたもの（[`fit_coverage`] で縮める）、
//!   フォントが無い・GDI が失敗したときと Linux のテストでは手描きストローク（[`stroke_mask`]。H-122）。
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

/// 手描きストロークのフォールバック用の図形（白。外接の四角は 1024 座標の (100, 100)〜(900, 900) 付近）。
/// 線の太さは字形の外接の約 14%（小さい大きさでも潰れない太さ）。位置と大きさは [`fit_coverage`] が決める
fn stroke_shapes(glyph: ModeGlyph) -> Vec<Shape> {
    glyph_strokes(glyph, 100.0, 100.0, 800.0, 800.0)
        .into_iter()
        .map(|pts| Shape::new(pts, 130.0, WHITE, 1.0))
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

/// 文字のカバレッジマスクの作り直し（縮小）に使う、フォントでの描画の倍率（1 辺）。
/// 供給元（GDI）はこの倍率の大きさで文字を描き、[`fit_coverage`] が `size` px に縮める
pub const HIRES_SCALE: u32 = 8;

/// 文字の外接の四角が背景（`size` px 四方）に占める割合の上限（長い方の辺）。
/// カラーは角丸の背景の内側に余白を残し、モノクロは背景がないので少し大きく使う
pub fn fill_ratio(tone: Tone) -> f32 {
    match tone {
        Tone::Color => 0.74,
        Tone::Mono(_) => 0.78,
    }
}

/// 文字の外接を求めるときに、字形とみなすカバレッジの下限（アンチエイリアスの薄い雑音を無視する）
const BBOX_THRESHOLD: u8 = 16;

/// 大きなカバレッジマスク（`width` × `height`、行は上から下）の字形を、`size` px 四方の
/// カバレッジマスクに縮める。字形の外接の四角（[`BBOX_THRESHOLD`] 以上の画素）で位置を決める:
/// 縦横比を保ったまま、長い方の辺が `size * fill` になるよう縮め、四角の中央に置く。
/// 縮小は面積平均（ボックスフィルタ）。字形が無ければ全て 0
pub fn fit_coverage(hires: &[u8], width: u32, height: u32, size: u32, fill: f32) -> Vec<u8> {
    let (w, h, n) = (width as usize, height as usize, size as usize);
    let mut out = vec![0u8; n * n];
    if hires.len() < w * h || n == 0 {
        return out;
    }
    // 字形の外接（半開区間）
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if hires[y * w + x] >= BBOX_THRESHOLD {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return out;
    }
    let (bw, bh) = ((x1 - x0) as f32, (y1 - y0) as f32);
    // 元の 1 画素が出力で何画素になるか
    let scale = size as f32 * fill / bw.max(bh);
    let offset_x = (size as f32 - bw * scale) / 2.0;
    let offset_y = (size as f32 - bh * scale) / 2.0;
    // 出力の画素 [p, p + 1) に対応する元の区間
    let source_range = |p: usize, offset: f32, origin: usize| {
        (
            origin as f32 + (p as f32 - offset) / scale,
            origin as f32 + (p as f32 + 1.0 - offset) / scale,
        )
    };
    // 元の画素 i の区間 [i, i + 1) と [lo, hi) の重なり
    let overlap = |i: usize, lo: f32, hi: f32| (hi.min(i as f32 + 1.0) - lo.max(i as f32)).max(0.0);
    for py in 0..n {
        let (sy0, sy1) = source_range(py, offset_y, y0);
        let (iy0, iy1) = (sy0.floor().max(0.0) as usize, (sy1.ceil() as usize).min(h));
        for px in 0..n {
            let (sx0, sx1) = source_range(px, offset_x, x0);
            let (ix0, ix1) = (sx0.floor().max(0.0) as usize, (sx1.ceil() as usize).min(w));
            let mut acc = 0.0f32;
            for iy in iy0..iy1 {
                let wy = overlap(iy, sy0, sy1);
                for ix in ix0..ix1 {
                    acc += f32::from(hires[iy * w + ix]) * wy * overlap(ix, sx0, sx1);
                }
            }
            let area = (sx1 - sx0) * (sy1 - sy0);
            out[py * n + px] = (acc / area).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// 手描きストロークの字形のカバレッジマスク（`size` × `size`）。フォントが使えないときのフォールバックと、
/// Linux 上のテストの供給元。[`fit_coverage`] を通すので、位置・大きさの規則はフォント版と同じ
pub fn stroke_mask(size: u32, glyph: ModeGlyph, tone: Tone) -> Vec<u8> {
    let big = size * HIRES_SCALE / 2;
    let alpha: Vec<u8> = rasterize(big, false, &stroke_shapes(glyph))
        .chunks_exact(4)
        .map(|p| p[3])
        .collect();
    fit_coverage(&alpha, big, big, size, fill_ratio(tone))
}

/// 文字のカバレッジマスク（`size` × `size`、0〜255）から、モード表示つきアイコンの RGBA
/// （行は上から下、非乗算）を作る。カラーは藍のグラデーションの角丸の背景に生成り色の文字、
/// モノクロは背景なしで `Tone::Mono` の単色（濃淡はアルファだけで表す）
pub fn compose_status(size: u32, tone: Tone, mask: &[u8]) -> Vec<u8> {
    let pixels = (size * size) as usize;
    let mut out = match tone {
        Tone::Color => rasterize(size, true, &[]),
        Tone::Mono(_) => vec![0u8; pixels * 4],
    };
    for (px, &cov) in out
        .chunks_exact_mut(4)
        .zip(mask.iter().chain(std::iter::repeat(&0)))
    {
        let a = f32::from(cov) / 255.0;
        match tone {
            Tone::Color => {
                if cov == 0 {
                    continue;
                }
                // 背景（非乗算の色 + アルファ）の上に、文字の色をカバレッジで重ねる
                let bg_a = f32::from(px[3]) / 255.0;
                let out_a = a + bg_a * (1.0 - a);
                for (c, fg) in px[..3].iter_mut().zip(SUN) {
                    let v = (f32::from(fg) * a + f32::from(*c) * bg_a * (1.0 - a)) / out_a;
                    *c = v.round().clamp(0.0, 255.0) as u8;
                }
                px[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            Tone::Mono(rgb) => {
                px[..3].copy_from_slice(&rgb);
                px[3] = cov;
            }
        }
    }
    out
}

/// モード表示つきアイコンの `size` px 四方の RGBA（行は上から下、非乗算）。手描きストロークの字形で描く
/// （フォールバック）。実際のフォントの字形を使うときは、供給元が [`fit_coverage`] で作ったマスクを
/// [`compose_status`] に渡す。言語バー・入力インジケーター用で、`size` はシステムの小アイコンの大きさ（16〜）
pub fn render_status(size: u32, glyph: ModeGlyph, tone: Tone) -> Vec<u8> {
    compose_status(size, tone, &stroke_mask(size, glyph, tone))
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

    /// `min` 以上の画素の外接（半開区間）。無ければ `None`
    fn mask_bbox(mask: &[u8], size: u32, min: u8) -> Option<(u32, u32, u32, u32)> {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for (i, &v) in mask.iter().enumerate() {
            if v < min {
                continue;
            }
            let (x, y) = (i as u32 % size, i as u32 / size);
            b = Some(match b {
                None => (x, y, x + 1, y + 1),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
            });
        }
        b
    }

    /// 大きなマスクの `(x, y, w, h)` の長方形を 255 で塗ったもの（400 四方）
    fn hires_rect(x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
        let mut m = vec![0u8; 400 * 400];
        for yy in y..y + h {
            for xx in x..x + w {
                m[yy * 400 + xx] = 255;
            }
        }
        m
    }

    #[test]
    fn fit_centers_the_glyph_by_its_bounding_box() {
        // 左上に偏った縦長の字形でも、外接の四角が中央に来る。長い方の辺が size * fill
        for &size in &[16u32, 20, 24, 32] {
            let hires = hires_rect(20, 40, 60, 160);
            let mask = fit_coverage(&hires, 400, 400, size, 0.75);
            let (x0, y0, x1, y1) = mask_bbox(&mask, size, 64).expect("字形がある");
            // 左右・上下の余白は（半画素の誤差で）等しい
            assert!(x0.abs_diff(size - x1) <= 1, "{size}: 左右 {x0} {x1}");
            assert!(y0.abs_diff(size - y1) <= 1, "{size}: 上下 {y0} {y1}");
            let h = (y1 - y0) as f32;
            assert!(
                (h - size as f32 * 0.75).abs() <= 1.5,
                "{size}: 高さ {h} は {} 付近",
                size as f32 * 0.75
            );
            // 縦横比は保たれる（60:160）
            let w = (x1 - x0) as f32;
            assert!((w / h - 0.375).abs() < 0.2, "{size}: 比 {}", w / h);
        }
    }

    #[test]
    fn fit_never_overflows_the_tile() {
        // 全面が字形の四角でも、外周に余白が残る（はみ出さない）
        for &size in &[16u32, 20, 24, 32, 48] {
            for tone in [Tone::Color, Tone::Mono([255; 3])] {
                let hires = vec![255u8; 400 * 400];
                let mask = fit_coverage(&hires, 400, 400, size, fill_ratio(tone));
                let (x0, y0, x1, y1) = mask_bbox(&mask, size, 64).unwrap();
                let side = (x1 - x0).max(y1 - y0) as f32 / size as f32;
                assert!(
                    side <= fill_ratio(tone) + 1.5 / size as f32,
                    "{size}: {side}"
                );
                assert!(side >= 0.70, "{size} {tone:?}: 小さすぎる {side}");
                assert!(
                    x0 >= 1 && y0 >= 1 && x1 < size && y1 < size,
                    "{size}: 外周に触れる"
                );
            }
        }
    }

    #[test]
    fn fit_of_an_empty_mask_is_empty() {
        let mask = fit_coverage(&[0u8; 100], 10, 10, 16, 0.75);
        assert_eq!(mask, vec![0u8; 256]);
        // 長さが足りない入力でも落ちない
        assert_eq!(fit_coverage(&[1, 2], 10, 10, 16, 0.75), vec![0u8; 256]);
    }

    #[test]
    fn stroke_fallback_fits_the_same_rule() {
        for &size in &[16u32, 20, 24, 32] {
            for glyph in [ModeGlyph::Hiragana, ModeGlyph::Alphanumeric] {
                for tone in [Tone::Color, Tone::Mono([0; 3])] {
                    let mask = stroke_mask(size, glyph, tone);
                    assert_eq!(mask.len(), (size * size) as usize);
                    let (x0, y0, x1, y1) = mask_bbox(&mask, size, 64).unwrap();
                    let side = (x1 - x0).max(y1 - y0) as f32 / size as f32;
                    assert!(
                        (0.68..=0.78 + 1.5 / size as f32).contains(&side),
                        "{size} {glyph:?} {tone:?}: {side}"
                    );
                    let (a, b, c, d) = mask_bbox(&mask, size, 1).unwrap();
                    assert!(a >= 1 && b >= 1 && c < size && d < size);
                }
            }
        }
    }

    #[test]
    fn compose_color_blends_the_mask_over_the_tile() {
        let size = 32;
        let tile = rasterize(size, true, &[]);
        // マスクが空なら背景そのまま
        assert_eq!(compose_status(size, Tone::Color, &[0; 1024]), tile);
        // 全面 255 なら、不透明な背景の部分は文字の色になる（角の透明は文字色の不透明になる）
        let full = compose_status(size, Tone::Color, &[255; 1024]);
        assert_eq!(pixel(&full, size, 16, 16), [0xFF, 0xF3, 0xD6, 0xFF]);
        // 半分のカバレッジは背景と文字色の中間
        let half = compose_status(size, Tone::Color, &[128; 1024]);
        let (h, t) = (pixel(&half, size, 16, 4), pixel(&tile, size, 16, 4));
        assert_eq!(h[3], 255);
        assert!(h[0] > t[0] && h[0] < 0xFF, "{h:?} {t:?}");
    }

    #[test]
    fn compose_mono_is_single_color_with_mask_as_alpha() {
        let size = 16;
        let mask: Vec<u8> = (0..256).map(|i| i as u8).collect();
        for fg in [[0xFF, 0xFF, 0xFF], [0, 0, 0]] {
            let px = compose_status(size, Tone::Mono(fg), &mask);
            for (p, &m) in px.chunks_exact(4).zip(&mask) {
                assert_eq!(p[3], m);
                assert_eq!([p[0], p[1], p[2]], fg);
            }
        }
    }

    /// 目視確認用: `NAGI_ICON_FONT_DIR` に `<名前>.gray`（先頭 8 バイトが幅・高さの u32 LE、続いて 8bit グレースケール）が
    /// あれば、[`fit_coverage`] → [`compose_status`] を通して `<名前>_<トーン>_<大きさ>.rgba` を `NAGI_ICON_DUMP_DIR` へ書く
    /// （フォントで描いた字形の縮小と合成の確認。本番の供給元は TSF 層の GDI）
    #[test]
    fn dump_font_glyphs_for_visual_check() {
        let (Some(src), Some(dst)) = (
            std::env::var_os("NAGI_ICON_FONT_DIR"),
            std::env::var_os("NAGI_ICON_DUMP_DIR"),
        ) else {
            return;
        };
        let dst = std::path::PathBuf::from(dst);
        std::fs::create_dir_all(&dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "gray") {
                continue;
            }
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let data = std::fs::read(&path).unwrap();
            let (w, h) = (u32_at(&data, 0), u32_at(&data, 4));
            for &size in &[16u32, 20, 24, 32] {
                for (tone_name, tone) in [
                    ("color", Tone::Color),
                    ("white", Tone::Mono([255; 3])),
                    ("black", Tone::Mono([0; 3])),
                ] {
                    let mask = fit_coverage(&data[8..], w, h, size, fill_ratio(tone));
                    std::fs::write(
                        dst.join(format!("{name}_{tone_name}_{size}.rgba")),
                        compose_status(size, tone, &mask),
                    )
                    .unwrap();
                }
            }
        }
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
