//! 曖昧入力（タイプミス補正）の評価用部品（`nagi-fuzzy-eval` と単体テストが使う）。
//!
//! Mozc の `evaluation.tsv` の読みに、再現可能（シード固定）なタイプミスを注入するための乱数・
//! 注入器と、正解率の集計器。注入器は補正側（`nagi-fuzzy`）と同じ隣接表から作るので、実際の
//! 打ち間違いに対する値より楽観的になる（`docs/plan-fuzzy-and-glide.md` F0）。
//!
//! - [`add_noise`]: 隣接キー置換・脱落・挿入・転置（QWERTY の打鍵列）
//! - [`tap`]: 狙ったキーの中心からのガウス乱数の位置（H-25 のタッチモデル）で打つ
//! - [`flick_noise`]: 12 キーのフリック方向の誤り（隣り合う方向・中央との取り違え）と、その角度

use crate::fuzzy::Suggestion;
use nagi_engine::touch::{DakutenOrder, key_table};
use nagi_fuzzy::{Adjacency, KeyLayout, Stroke};
use nagi_romaji::{Emit, RomajiConverter, RomajiTable};
use std::sync::Arc;

/// 決定的な擬似乱数（SplitMix64）
pub struct Rng(u64);

impl Rng {
    /// シードから作る
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    /// 次の 64 ビット
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[0, 1)` の一様乱数
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `[0, n)` の整数
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// 標準正規分布の乱数（Box-Muller）
    pub fn gauss(&mut self) -> f64 {
        let u1 = self.unit().max(1e-12);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

/// 打鍵列にノイズを入れる。1 打鍵ごとに確率 `p` で、置換・脱落・挿入・転置のいずれかを注入する
pub fn add_noise(typed: &str, p: f64, adj: &Adjacency, rng: &mut Rng) -> String {
    let t: Vec<char> = typed.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < t.len() {
        let c = t[i];
        if !c.is_ascii_lowercase() || rng.unit() >= p {
            out.push(c);
            i += 1;
            continue;
        }
        match rng.below(4) {
            // 置換: 隣のキーを打つ
            0 => {
                let nb = adj.neighbors(c);
                if nb.is_empty() {
                    out.push(c);
                } else {
                    out.push(nb[rng.below(nb.len())].label);
                }
                i += 1;
            }
            // 脱落: 打ち忘れ
            1 => i += 1,
            // 挿入: 直前の文字の重複か、隣のキーが余計に入る
            2 => {
                out.push(c);
                let nb = adj.neighbors(c);
                if nb.is_empty() || rng.below(2) == 0 {
                    out.push(c);
                } else {
                    out.push(nb[rng.below(nb.len())].label);
                }
                i += 1;
            }
            // 転置
            _ => {
                if i + 1 < t.len() && t[i + 1] != c {
                    out.push(t[i + 1]);
                    out.push(c);
                    i += 2;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
        }
    }
    out
}

/// 打鍵列を Composer と同じ規則で読みにする（変換できずに残った英字はそのまま残す）
pub fn to_reading(typed: &str, table: &Arc<RomajiTable>) -> String {
    let mut conv = RomajiConverter::new(table.clone());
    let mut out = String::new();
    let mut push = |emits: Vec<Emit>| {
        for e in emits {
            match e {
                Emit::Converted(s) => out.push_str(&s),
                Emit::Raw(c) => out.push(c),
            }
        }
    };
    for c in typed.chars() {
        push(conv.feed(c));
    }
    push(conv.flush());
    out
}

/// 打鍵列を、狙ったキーの中心からのガウス乱数の位置（標準偏差 `sigma`、キー間隔が 1）で打つ。
/// 押されるキーは中心が最も近いキー。座標はそのキーの矩形内の相対位置（0..1）
pub fn tap(typed: &str, layout: &KeyLayout, sigma: f64, rng: &mut Rng) -> Vec<Stroke> {
    typed
        .chars()
        .map(|c| {
            let Some(k) = layout.key(c) else {
                return Stroke::key(c);
            };
            let (cx, cy) = (k.x + k.w / 2.0, k.y + k.h / 2.0);
            let x = cx as f64 + rng.gauss() * sigma;
            let y = cy as f64 + rng.gauss() * sigma;
            let nearest = layout
                .keys
                .iter()
                .min_by(|a, b| {
                    let da = (a.x as f64 + a.w as f64 / 2.0 - x).powi(2)
                        + (a.y as f64 + a.h as f64 / 2.0 - y).powi(2);
                    let db = (b.x as f64 + b.w as f64 / 2.0 - x).powi(2)
                        + (b.y as f64 + b.h as f64 / 2.0 - y).powi(2);
                    da.total_cmp(&db)
                })
                .unwrap_or(k);
            let rx = ((x - nearest.x as f64) / nearest.w as f64).clamp(0.0, 1.0);
            let ry = ((y - nearest.y as f64) / nearest.h as f64).clamp(0.0, 1.0);
            Stroke {
                ch: nearest.label,
                touch: Some((rx as f32, ry as f32)),
                flick: None,
            }
        })
        .collect()
}

/// 12 キーの各キーの文字 `[中央, 左, 上, 右, 下]`（engine の `key_table` と同じ表）
pub fn flick_rows() -> Vec<[char; 5]> {
    let table = key_table(DakutenOrder::SmallFirst);
    let first = |s: &str| s.chars().next().unwrap_or(' ');
    table
        .keys
        .iter()
        .map(|k| {
            [
                first(&k.center),
                first(&k.left),
                first(&k.up),
                first(&k.right),
                first(&k.down),
            ]
        })
        .collect()
}

fn is_kana(c: char) -> bool {
    ('ぁ'..='ん').contains(&c)
}

/// 行内の方向の添字（中央 0・左 1・上 2・右 3・下 4）のフリック角度（度。右 0、上 90、左 180、下 270）
fn direction_angle(dir: usize) -> Option<f64> {
    match dir {
        1 => Some(180.0),
        2 => Some(90.0),
        3 => Some(0.0),
        4 => Some(270.0),
        _ => None,
    }
}

/// 隣り合う 2 方向の境界の角度（度）と、`from` から `to` へ向かう符号（反時計回りなら +1）
fn boundary(from: usize, to: usize) -> Option<(f64, f64)> {
    let (a, b) = (direction_angle(from)?, direction_angle(to)?);
    let mid = (a.to_radians().sin() + b.to_radians().sin())
        .atan2(a.to_radians().cos() + b.to_radians().cos())
        .to_degrees();
    let diff = (b - a + 540.0).rem_euclid(360.0) - 180.0;
    Some((mid, diff.signum()))
}

/// フリック方向の誤りを注入した結果
#[derive(Debug, Clone, PartialEq)]
pub struct FlickSample {
    /// 実際に出た読み
    pub noisy: String,
    /// 読みの各文字を出したフリックの（角度[度]、距離）。タップ（中央）は `None`
    pub flicks: Vec<Option<(f32, f32)>>,
}

/// かなの読みに、12 キーのフリック方向の誤りを注入する。1 文字ごとに確率 `p` で、同じキーの
/// 隣の方向（左-上-右-下の周回で隣、または中央⇔各方向）の文字に取り違える。取り違えたときの
/// 角度は、意図した方向との境界のすぐ向こう側（ガウスのずれ）に置く。行に無い文字（濁点つき・
/// 小書き）は誤りを入れない
pub fn flick_noise(reading: &str, p: f64, rows: &[[char; 5]], rng: &mut Rng) -> FlickSample {
    let mut noisy = String::new();
    let mut flicks: Vec<Option<(f32, f32)>> = Vec::new();
    for c in reading.chars() {
        let found = rows
            .iter()
            .find_map(|row| row.iter().position(|&x| x == c).map(|d| (row, d)));
        let Some((row, d)) = found else {
            noisy.push(c);
            flicks.push(None);
            continue;
        };
        // 取り違えの候補（かなである隣の方向）
        let mut near: Vec<usize> = if d == 0 {
            vec![1, 2, 3, 4]
        } else {
            vec![0, d % 4 + 1, (d + 2) % 4 + 1]
        };
        near.retain(|&j| is_kana(row[j]));
        if !is_kana(c) || near.is_empty() || rng.unit() >= p {
            noisy.push(c);
            flicks.push(direction_angle(d).map(|a| ((a + rng.gauss() * 12.0) as f32, 0.8)));
            continue;
        }
        let to = near[rng.below(near.len())];
        noisy.push(row[to]);
        let flick = match (boundary(d, to), direction_angle(to)) {
            // 方向どうし: 意図した方向との境界のすぐ向こう側
            (Some((mid, sign)), _) => Some(((mid + sign * rng.gauss().abs() * 8.0) as f32, 0.8)),
            // 中央 → 方向: 方向の中心付近
            (None, Some(a)) => Some(((a + rng.gauss() * 12.0) as f32, 0.8)),
            // 方向 → 中央: タップ
            (None, None) => None,
        };
        flicks.push(flick);
    }
    FlickSample { noisy, flicks }
}

/// 百分位（`sorted` は昇順）
pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx]
}

/// 読みがひらがな・長音だけか
pub fn is_kana_reading(s: &str) -> bool {
    s.chars().all(|c| ('ぁ'..='ん').contains(&c) || c == 'ー')
}

/// 補正候補の正解率の集計（正解 = 補正前の読みの最良変換と同じ表記）
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    /// ノイズ入りの件数
    pub n: usize,
    /// 1 位が正解
    pub hit1: usize,
    /// 上位 N 件に正解
    pub hit_n: usize,
    /// 何か出た
    pub any: usize,
}

impl Tally {
    /// 1 件の結果を足す
    pub fn add(&mut self, s: &[Suggestion], surface: &str) {
        self.n += 1;
        if !s.is_empty() {
            self.any += 1;
        }
        if s.first().is_some_and(|x| x.surface == surface) {
            self.hit1 += 1;
        }
        if s.iter().any(|x| x.surface == surface) {
            self.hit_n += 1;
        }
    }
}

/// 自動採用の集計（閾値ごと）。ノイズ入りの入力と、クリーンな入力で別々に持つ
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdoptionTally {
    /// 件数
    pub n: usize,
    /// 採用した（先頭の提案の `lm_margin` が閾値以上）
    pub adopted: usize,
    /// 採用して、それが正解だった
    pub adopted_correct: usize,
}

impl AdoptionTally {
    /// 1 件を足す。`expected` は正解の表記（クリーンな入力では `None` = 何も採用しないのが正解）
    pub fn add(&mut self, first: Option<&Suggestion>, expected: Option<&str>, margin: f32) {
        self.n += 1;
        if let Some(s) = first.filter(|s| s.lm_margin.is_some_and(|m| m >= margin)) {
            self.adopted += 1;
            if expected == Some(s.surface.as_str()) {
                self.adopted_correct += 1;
            }
        }
    }
}
