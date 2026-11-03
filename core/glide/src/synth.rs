//! 合成軌跡ジェネレータ（G0）: 読みの理想経路にノイズを加えて、人がなぞったような軌跡を作る。
//!
//! 実機の軌跡が無い段階で認識の精度・速度を測るためのもの。実際の指の挙動より楽観的な部分があるので
//! （ノイズの種類が限られる、利用者ごとの癖が無い）、数値は相対比較と回帰検知に使い、実機で取り直すこと。
//! 乱数はシード固定（SplitMix64）で、同じシードなら同じ軌跡になる。
//!
//! 生成の流れ:
//! 1. 読みの綴り（代表 / ヘボン式をヘボン率で選ぶ）→ キー経路
//! 2. 各キーの中心に、狙いのずれ（ガウス、キーごと）と全体の偏り（ガウス、1 ストローク 1 回）を加える。
//!    同じキーが続く所は、ループ率で小さな円を描く
//! 3. 経由点を Catmull-Rom 曲線でなめらかにつなぐ（角が丸くなる）
//! 4. 区間ごとの所要時間にばらつきを持たせ、減速・加速の速度プロファイルで一定の周期（Hz）でサンプリングする
//!    （遅い所は点が密、速い所は疎になる = 速度ムラ）
//! 5. 位置にごく小さなジッタを加え、点を一定の割合で間引く（始点・終点は残す）

use crate::geom::{GlideLayout, Pt, TracePoint};
use crate::spell::{KeyPath, Style, key_path};

/// 再現可能な乱数（SplitMix64）
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
    spare: Option<f32>,
}

impl Rng {
    /// シードから作る
    pub fn new(seed: u64) -> Self {
        Rng {
            state: seed,
            spare: None,
        }
    }

    /// 次の 64 ビット
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0 以上 1 未満
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// 標準正規分布（Box-Muller 法）
    pub fn normal(&mut self) -> f32 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let u1 = self.unit().max(1e-7);
        let u2 = self.unit();
        let r = (-2.0 * u1.ln()).sqrt();
        let (s, c) = (std::f32::consts::TAU * u2).sin_cos();
        self.spare = Some(r * s);
        r * c
    }

    /// `0..n` の整数（`n >= 1`）
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

/// 軌跡の崩れ方
#[derive(Debug, Clone, PartialEq)]
pub struct SynthParams {
    /// 各キーの狙いのずれの標準偏差（キー単位）
    pub aim_sigma: f32,
    /// ストローク全体の偏り（指の当たり方の癖）の標準偏差（キー単位）
    pub bias_sigma: f32,
    /// 点ごとのジッタの標準偏差（キー単位）
    pub jitter_sigma: f32,
    /// 区間の所要時間のばらつき（変動係数）
    pub speed_var: f32,
    /// サンプリング周期（Hz）
    pub sample_hz: f32,
    /// 点を間引く確率（始点・終点は残す）
    pub drop_rate: f32,
    /// 同じキーが続く所で小さなループを描く確率
    pub loop_prob: f32,
    /// ヘボン式の綴りでなぞる確率
    pub hepburn_prob: f32,
}

impl SynthParams {
    /// ほぼ理想: 狙いのずれが小さい（回帰検知・下限の確認用）
    pub fn clean() -> Self {
        SynthParams {
            aim_sigma: 0.12,
            bias_sigma: 0.05,
            jitter_sigma: 0.02,
            speed_var: 0.2,
            sample_hz: 90.0,
            drop_rate: 0.1,
            loop_prob: 0.5,
            hepburn_prob: 0.3,
        }
    }

    /// 標準: 日常的な、キーをおおまかに通るなぞり
    pub fn normal() -> Self {
        SynthParams {
            aim_sigma: 0.22,
            bias_sigma: 0.1,
            jitter_sigma: 0.04,
            speed_var: 0.3,
            sample_hz: 90.0,
            drop_rate: 0.2,
            loop_prob: 0.4,
            hepburn_prob: 0.3,
        }
    }

    /// 粗い: 急いで雑になぞる（ずれが大きく、点も少ない）
    pub fn noisy() -> Self {
        SynthParams {
            aim_sigma: 0.35,
            bias_sigma: 0.18,
            jitter_sigma: 0.07,
            speed_var: 0.45,
            sample_hz: 60.0,
            drop_rate: 0.35,
            loop_prob: 0.3,
            hepburn_prob: 0.3,
        }
    }
}

impl Default for SynthParams {
    fn default() -> Self {
        Self::normal()
    }
}

/// 読みをなぞった合成軌跡を作る（フロントの座標系）。読みがキー経路にできない、またはキー配置に
/// 無いキーを含めば `None`
pub fn generate(
    layout: &GlideLayout,
    reading: &str,
    params: &SynthParams,
    rng: &mut Rng,
) -> Option<Vec<TracePoint>> {
    let style = if rng.unit() < params.hepburn_prob {
        Style::Hepburn
    } else {
        Style::Canon
    };
    let path = key_path(reading, style).or_else(|| key_path(reading, Style::Canon))?;
    generate_path(layout, &path, params, rng)
}

/// キー経路をなぞった合成軌跡を作る
pub fn generate_path(
    layout: &GlideLayout,
    path: &KeyPath,
    params: &SynthParams,
    rng: &mut Rng,
) -> Option<Vec<TracePoint>> {
    let bias = Pt {
        x: rng.normal() * params.bias_sigma,
        y: rng.normal() * params.bias_sigma,
    };
    // 経由点
    let mut way: Vec<Pt> = Vec::new();
    for (&k, &has_loop) in path.keys.iter().zip(&path.loops) {
        let c = layout.center(k)?;
        let aim = Pt {
            x: c.x + bias.x + rng.normal() * params.aim_sigma,
            y: c.y + bias.y + rng.normal() * params.aim_sigma,
        };
        way.push(aim);
        if has_loop && rng.unit() < params.loop_prob {
            // 小さな円。大きさと向きは毎回ばらつかせる
            let r = 0.15 + 0.2 * rng.unit();
            let dir = std::f32::consts::TAU * rng.unit();
            for step in 1..=4 {
                let a = std::f32::consts::TAU * step as f32 / 4.0;
                let (sa, ca) = a.sin_cos();
                // 中心 aim から dir 方向へ r 離れた点を中心とする円を、aim から出て aim に戻る
                let (dx, dy) = (dir.cos(), dir.sin());
                way.push(Pt {
                    x: aim.x + r * dx * (1.0 - ca) - r * dy * sa,
                    y: aim.y + r * dy * (1.0 - ca) + r * dx * sa,
                });
            }
        }
    }
    if way.len() < 2 {
        return None;
    }

    // 区間ごとにサンプリング（Catmull-Rom 曲線 × 速度プロファイル）
    let dt_ms = 1000.0 / params.sample_hz.max(10.0);
    let mut pts: Vec<(f32, Pt)> = Vec::new(); // (時刻 ms, 位置)
    let mut t_base = 0.0f32;
    let mut next_t = 0.0f32;
    let n = way.len();
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (
            way[i.saturating_sub(1)],
            way[i],
            way[i + 1],
            way[(i + 2).min(n - 1)],
        );
        let len = p1.dist(p2).max(0.05);
        // 所要時間: 距離の 0.6 乗に比例（長い移動ほど速い）+ ばらつき
        let jitter = (1.0 + params.speed_var * rng.normal()).clamp(0.4, 2.0);
        let dur = (60.0 + 90.0 * len.powf(0.6)) * jitter;
        while next_t < t_base + dur {
            let tau = (next_t - t_base) / dur;
            // 区間の端で少し減速する速度プロファイル（完全には止まらない）
            let eased = tau + 0.35 * (tau * tau * (3.0 - 2.0 * tau) - tau);
            pts.push((next_t, catmull_rom(p0, p1, p2, p3, eased)));
            next_t += dt_ms * (0.85 + 0.3 * rng.unit());
        }
        t_base += dur;
    }
    pts.push((t_base, *way.last().expect("2 点以上")));

    // ジッタと間引き（始点・終点は残す）
    let last = pts.len() - 1;
    let mut out: Vec<TracePoint> = Vec::with_capacity(pts.len());
    for (i, (t, p)) in pts.into_iter().enumerate() {
        if i != 0 && i != last && rng.unit() < params.drop_rate {
            continue;
        }
        let q = Pt {
            x: p.x + rng.normal() * params.jitter_sigma,
            y: p.y + rng.normal() * params.jitter_sigma,
        };
        let (x, y) = layout.from_units(q);
        out.push(TracePoint {
            x,
            y,
            t: f64::from(t),
        });
    }
    (out.len() >= 2).then_some(out)
}

fn catmull_rom(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f32) -> Pt {
    let (t2, t3) = (t * t, t * t * t);
    let f = |a: f32, b: f32, c: f32, d: f32| {
        0.5 * (2.0 * b
            + (-a + c) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
            + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    Pt {
        x: f(p0.x, p1.x, p2.x, p3.x),
        y: f(p0.y, p1.y, p2.y, p3.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_roughly_normal() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        assert_eq!(a.next_u64(), b.next_u64());
        let mut r = Rng::new(1);
        let n = 20000;
        let xs: Vec<f32> = (0..n).map(|_| r.normal()).collect();
        let mean = xs.iter().sum::<f32>() / n as f32;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n as f32;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!((var - 1.0).abs() < 0.08, "var {var}");
    }

    #[test]
    fn generated_trace_starts_and_ends_near_the_keys() {
        let layout = GlideLayout::qwerty_phone();
        let mut rng = Rng::new(3);
        let t = generate(&layout, "さくら", &SynthParams::clean(), &mut rng).unwrap();
        assert!(t.len() > 5);
        let first = layout.to_units(t[0].x, t[0].y);
        let last = layout.to_units(t.last().unwrap().x, t.last().unwrap().y);
        assert!(first.dist(layout.center(b's' - b'a').unwrap()) < 0.8);
        assert!(last.dist(layout.center(b'a' - b'a').unwrap()) < 0.8);
        // 時刻は単調増加
        assert!(t.windows(2).all(|w| w[1].t >= w[0].t));
    }

    #[test]
    fn same_seed_gives_same_trace_and_noise_changes_it() {
        let layout = GlideLayout::qwerty_phone();
        let make = |seed, p: &SynthParams| {
            generate(&layout, "ありがとう", p, &mut Rng::new(seed)).unwrap()
        };
        let a = make(5, &SynthParams::normal());
        assert_eq!(a, make(5, &SynthParams::normal()));
        assert_ne!(a, make(6, &SynthParams::normal()));
    }

    #[test]
    fn unspellable_readings_give_none() {
        let layout = GlideLayout::qwerty_phone();
        assert!(generate(&layout, "漢字", &SynthParams::clean(), &mut Rng::new(1)).is_none());
    }
}
