//! 認識: なぞり軌跡 → 読みの候補（SHARK2 系）。
//!
//! 処理（Zhai & Kristensson 2004 の SHARK2 と、FlorisBoard の統計的グライド分類器の構成を参考にした）:
//!
//! 1. 前処理: 近すぎる点を間引き、軌跡が短すぎれば（タップ）認識しない
//! 2. 始点・終点の近くのキーで索引の箱を絞り込む（始点 × 終点のキー）
//! 3. 軌跡から各キーまでの最短距離を 26 キー分だけ先に求めておき、箱の各項目を表引きだけで枝刈りする:
//!    軌跡が通っていないキー（`max_key_miss` より遠い）を含む読み、軌跡の長さと理想経路（キー中心を
//!    つないだ折れ線）の長さの比が合わない読みを捨て、残りを「各キーへの近さ」と事前確率で並べて上位を残す
//! 4. 粗い照合（32 点）で上位を残し、本照合（200 点）で順位付けする。照合は 2 つの距離の組:
//!    - 形状: 両方をバウンディングボックスの長辺 1 に正規化し、対応する点の距離の平均
//!    - 位置: キー単位の座標のまま、対応する点の距離の平均
//!
//!    それぞれをガウス（σ 付き）の対数尤度にして足し、語頻度の事前確率（辞書の語コスト）を重みを付けて足す
//! 5. 同じ読みは最良のスコアだけ残し、上位を返す
//!
//! 同じキーが続く読み（`きっぷ` の `pp`、`かんい` の `nn`）は、ループなし・ありの 2 通りの
//! 理想経路を照合して良い方を採る。綴りの揺れ（`si` / `shi` など）は、代表の綴りとヘボン式の
//! 両方を照合する。

use crate::geom::{
    GlideLayout, Pt, TracePoint, mean_distance, path_length, resample, shape_normalize, thin,
};
use crate::index::{GlideIndex, LOOP_FLAG};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// 1 回の認識で受け付ける点の最大数（超えたら等間隔に間引く）
pub const MAX_INPUT_POINTS: usize = 2048;

/// 語コストを対数確率に直す尺度（Mozc の語コストは `-500 ln p` 相当）
const COST_SCALE: f32 = 500.0;

/// 認識のパラメータ
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// 本照合の再サンプリング点数
    pub resample_points: usize,
    /// 粗い照合の再サンプリング点数
    pub coarse_points: usize,
    /// これ未満の間隔の点は捨てる（キー単位）
    pub min_point_distance: f32,
    /// これ未満の長さの軌跡はタップとして認識しない（キー単位）
    pub min_length: f32,
    /// 始点・終点の近傍キーとみなす中心間距離（キー単位）
    pub end_radius: f32,
    /// 始点・終点それぞれの近傍キーの最大数
    pub max_end_keys: usize,
    /// 軌跡の長さ / 理想経路の長さ の許容範囲（下限, 上限）
    pub length_ratio: (f32, f32),
    /// 軌跡から最短距離がこれを超えるキーを含む読みは捨てる（キー単位）
    pub max_key_miss: f32,
    /// 順番を守った割り当て（簡易 DTW）での、軌跡から各キーまでの距離の二乗平均平方根がこれを超える読みは捨てる（キー単位）
    pub max_rms_miss: f32,
    /// 表引きの枝刈り（順番を守った割り当て + 事前確率）に使う軌跡の最小の点数。キーの多い読みには
    /// この 2 倍・32 点の表も使う（[`align_resolutions`]）
    pub align_points: usize,
    /// 表引きの枝刈りで事前確率の重みに掛ける倍率（1.0 で最終順位と同じ。小さいほど珍しい語を残しやすい）
    pub stage1_prior_scale: f32,
    /// 表引きの枝刈りから粗い照合へ進める候補数
    pub stage1_keep: usize,
    /// 順番を守った割り当て（重い）を計算する読みの数の上限。箱は始点・終点に近い順、箱の中は
    /// 語コストの小さい（よく使う）順に調べるので、上限に達すると珍しい読みから諦める（所要時間の上限）
    pub max_aligned: usize,
    /// 粗い照合から本照合へ進める候補数
    pub coarse_keep: usize,
    /// 形状距離のガウスの σ（バウンディングボックスの長辺を 1 とする）
    pub sigma_shape: f32,
    /// 位置距離のガウスの σ（キー単位）
    pub sigma_location: f32,
    /// 語頻度の事前確率の重み（0 で無視）
    pub prior_weight: f32,
    /// 最終候補の位置距離の上限（キー単位）。これを超える読みは、なぞりと合っていないとして捨てる
    pub max_location: f32,
    /// 最終候補の形状距離の上限。これを超える読みは捨てる
    pub max_shape: f32,
    /// ループの理想経路の半径（キー単位）
    pub loop_radius: f32,
    /// 返す読みの最大数
    pub max_results: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            resample_points: 200,
            coarse_points: 32,
            min_point_distance: 0.08,
            min_length: 1.0,
            end_radius: 1.3,
            max_end_keys: 4,
            length_ratio: (0.5, 1.8),
            max_key_miss: 1.0,
            max_rms_miss: 1.0,
            align_points: 12,
            stage1_prior_scale: 0.3,
            stage1_keep: 1500,
            max_aligned: 12000,
            coarse_keep: 96,
            sigma_shape: 0.07,
            sigma_location: 0.5,
            prior_weight: 0.2,
            max_location: 1.0,
            max_shape: 0.25,
            loop_radius: 0.2,
            max_results: 8,
        }
    }
}

/// 認識結果の読み 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct GlideMatch {
    /// 読み（ひらがな）
    pub reading: String,
    /// 総合スコア（対数尤度 + 事前。大きいほど良い）
    pub score: f32,
    /// 形状距離（正規化後の平均距離）
    pub shape: f32,
    /// 位置距離（キー単位の平均距離）
    pub location: f32,
    /// 語コスト
    pub cost: u16,
}

/// 1 回の認識の内訳（評価・チューニング用）
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    /// 走査した索引の項目数（始点 × 終点キーの箱の合計）
    pub scanned: usize,
    /// 長さ・キー近接の枝刈りを通った候補数
    pub candidates: usize,
    /// 候補収集に要した時間（マイクロ秒）
    pub collect_us: u64,
    /// 粗い照合に要した時間（マイクロ秒）
    pub coarse_us: u64,
    /// 本照合に要した時間（マイクロ秒）
    pub full_us: u64,
}

/// グライド認識器（キー配置とパラメータを持つ。索引は呼び出しごとに渡す）
#[derive(Debug, Clone)]
pub struct Recognizer {
    layout: GlideLayout,
    params: Params,
    /// キー中心どうしの距離（キー単位。配置に無いキーは無限大）
    key_distance: Box<[[f32; 26]; 26]>,
}

/// 1 回の認識で、軌跡側から先に求めておく表（段階 1 の表引き用）
struct StrokeTables {
    /// 軌跡から各キーまでの最短距離（キー単位）
    key_miss: [f32; 26],
    /// 軌跡の点それぞれから各キーまでの二乗距離（`26 × 点数`）。点数が `align_resolutions` の順に違う表
    key_sq: Vec<Vec<f32>>,
    /// 軌跡の長さ（キー単位）
    user_len: f32,
    /// ループ 1 つ分の長さ（キー単位）
    loop_len: f32,
}

/// 理想経路 1 つ（ループなし / あり）
struct Template {
    plain: Vec<Pt>,
    looped: Option<Vec<Pt>>,
}

impl Recognizer {
    /// 既定のパラメータで作る
    pub fn new(layout: GlideLayout) -> Self {
        Self::with_params(layout, Params::default())
    }

    /// パラメータを指定して作る
    pub fn with_params(layout: GlideLayout, params: Params) -> Self {
        let mut key_distance = Box::new([[f32::INFINITY; 26]; 26]);
        for a in 0..26u8 {
            for b in 0..26u8 {
                if let (Some(pa), Some(pb)) = (layout.center(a), layout.center(b)) {
                    key_distance[a as usize][b as usize] = pa.dist(pb);
                }
            }
        }
        Recognizer {
            layout,
            params,
            key_distance,
        }
    }

    /// キー配置
    pub fn layout(&self) -> &GlideLayout {
        &self.layout
    }

    /// パラメータ
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// 軌跡から読みの候補を求める（良い順）。タップ相当・始点または終点の近くにキーが無い・
    /// 候補が無ければ空
    pub fn recognize(&self, index: &GlideIndex, trace: &[TracePoint]) -> Vec<GlideMatch> {
        self.recognize_stats(index, trace).0
    }

    /// [`Recognizer::recognize`] に、処理の内訳（走査数・候補数・段階ごとの時間）を添えたもの
    pub fn recognize_stats(
        &self,
        index: &GlideIndex,
        trace: &[TracePoint],
    ) -> (Vec<GlideMatch>, Stats) {
        let mut stats = Stats::default();
        let t0 = std::time::Instant::now();
        let p = &self.params;
        let Some(points) = self.preprocess(trace) else {
            return (Vec::new(), stats);
        };
        let user_len = path_length(&points);
        let starts = self
            .layout
            .nearest_keys(points[0], p.end_radius, p.max_end_keys);
        let ends = self.layout.nearest_keys(
            *points.last().expect("2 点以上"),
            p.end_radius,
            p.max_end_keys,
        );
        if starts.is_empty() || ends.is_empty() {
            return (Vec::new(), stats);
        }

        // 軌跡から各キーまでの最短距離（枝刈りは表引きだけで済ませる）
        let mut key_miss = [f32::INFINITY; 26];
        for (k, miss) in key_miss.iter_mut().enumerate() {
            if let Some(c) = self.layout.center(k as u8) {
                *miss = points
                    .windows(2)
                    .map(|w| point_segment_distance(c, w[0], w[1]))
                    .fold(f32::INFINITY, f32::min);
            }
        }

        // 軌跡の点それぞれから各キーまでの二乗距離（順番を守った割り当ての表）。キー数の多い読みは
        // 点が少ないと割り当てが苦しくなるので、点数の違う表を用意して読みの長さで使い分ける
        let mut sample = Vec::new();
        let mut key_sq: Vec<Vec<f32>> = Vec::new();
        for n in align_resolutions(p.align_points) {
            resample(&points, n, &mut sample);
            let mut table = vec![f32::INFINITY; 26 * n];
            for k in 0..26u8 {
                if let Some(c) = self.layout.center(k) {
                    for (j, q) in sample.iter().enumerate() {
                        let d = c.dist(*q);
                        table[k as usize * n + j] = d * d;
                    }
                }
            }
            key_sq.push(table);
        }
        let tables = StrokeTables {
            key_miss,
            key_sq,
            user_len,
            loop_len: std::f32::consts::TAU * p.loop_radius,
        };

        // 段階 1: 箱の項目を表引きで枝刈りして並べる
        // 上位 `stage1_keep` 件だけを残す（最低点を下回る候補は割り当てを省く）
        let mut heap: BinaryHeap<Reverse<Scored>> = BinaryHeap::with_capacity(p.stage1_keep + 1);
        // 箱は始点・終点に近い順（近い方が正解の確率が高い）
        let mut buckets: Vec<(usize, u8, u8)> = starts
            .iter()
            .enumerate()
            .flat_map(|(si, &s)| ends.iter().enumerate().map(move |(ei, &e)| (si + ei, s, e)))
            .collect();
        buckets.sort_by_key(|&(rank, ..)| rank);
        let mut aligned = 0usize;
        for &(_, s, e) in &buckets {
            {
                let range = index.bucket(s, e);
                stats.scanned += range.len();
                for entry in range {
                    if aligned >= p.max_aligned {
                        break;
                    }
                    let floor = match heap.peek() {
                        Some(Reverse(worst)) if heap.len() >= p.stage1_keep => worst.0,
                        _ => f32::NEG_INFINITY,
                    };
                    if let Some(score) =
                        self.stage1_score(index, entry, &tables, floor, &mut aligned)
                    {
                        stats.candidates += 1;
                        heap.push(Reverse(Scored(score, entry)));
                        if heap.len() > p.stage1_keep {
                            heap.pop();
                        }
                    }
                }
            }
        }
        let stage1: Vec<(f32, usize)> = heap.into_iter().map(|Reverse(s)| (s.0, s.1)).collect();
        stats.collect_us = t0.elapsed().as_micros() as u64;
        if stage1.is_empty() {
            return (Vec::new(), stats);
        }

        // 軌跡側の準備（粗い照合と本照合の 2 解像度）
        let t1 = std::time::Instant::now();
        let (mut coarse, mut full) = (Vec::new(), Vec::new());
        resample(&points, p.coarse_points, &mut coarse);
        resample(&points, p.resample_points, &mut full);
        let (mut coarse_shape, mut full_shape) = (Vec::new(), Vec::new());
        shape_normalize(&coarse, &mut coarse_shape);
        shape_normalize(&full, &mut full_shape);

        // 段階 2: 粗い照合
        let mut scratch = Scratch::default();
        let mut scored: Vec<(f32, usize)> = Vec::with_capacity(stage1.len());
        let mut templates: Vec<(usize, Template)> = Vec::with_capacity(stage1.len());
        for &(_, entry) in &stage1 {
            let Some(template) = self.template(index.path(entry)) else {
                continue;
            };
            let (ll, ..) = self.likelihood(
                &template,
                &coarse,
                &coarse_shape,
                p.coarse_points,
                &mut scratch,
            );
            scored.push((ll + self.prior(index, entry), templates.len()));
            templates.push((entry, template));
        }
        keep_top(&mut scored, p.coarse_keep);
        stats.coarse_us = t1.elapsed().as_micros() as u64;

        // 段階 3: 本照合
        let t2 = std::time::Instant::now();
        let mut results: Vec<GlideMatch> = Vec::with_capacity(scored.len());
        for (_, i) in scored {
            let (entry, template) = &templates[i];
            let (ll, shape, location) = self.likelihood(
                template,
                &full,
                &full_shape,
                p.resample_points,
                &mut scratch,
            );
            let m = GlideMatch {
                reading: index.reading(*entry).to_string(),
                score: ll + self.prior(index, *entry),
                shape,
                location,
                cost: index.cost(*entry),
            };
            match results.iter_mut().find(|r| r.reading == m.reading) {
                Some(existing) if existing.score >= m.score => {}
                Some(existing) => *existing = m,
                None => results.push(m),
            }
        }
        results.retain(|m| m.location <= p.max_location && m.shape <= p.max_shape);
        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.reading.cmp(&b.reading))
        });
        results.truncate(p.max_results);
        stats.full_us = t2.elapsed().as_micros() as u64;
        (results, stats)
    }

    /// 語頻度の事前確率（対数）
    fn prior(&self, index: &GlideIndex, entry: usize) -> f32 {
        -self.params.prior_weight * f32::from(index.cost(entry)) / COST_SCALE
    }

    /// 段階 1 のスコア（表引きのみ）。軌跡が通っていないキーを含む・長さの比が許容外・
    /// 配置に無いキーを含むなら `None`
    fn stage1_score(
        &self,
        index: &GlideIndex,
        entry: usize,
        tables: &StrokeTables,
        floor: f32,
        aligned: &mut usize,
    ) -> Option<f32> {
        let path = index.path(entry);
        if path.len() < 2 {
            return None;
        }
        let p = &self.params;
        let mut plain_len = 0.0f32;
        let mut loops = 0usize;
        let mut miss_sq = 0.0f32;
        let mut prev: Option<usize> = None;
        for &b in path {
            let k = (b & !LOOP_FLAG) as usize;
            if k >= 26 {
                return None;
            }
            if b & LOOP_FLAG != 0 {
                loops += 1;
            }
            let miss = tables.key_miss[k];
            if miss.is_nan() || miss > p.max_key_miss {
                return None;
            }
            miss_sq += miss * miss;
            if let Some(q) = prev {
                plain_len += self.key_distance[q][k];
            }
            prev = Some(k);
        }

        let looped_len = plain_len + loops as f32 * tables.loop_len;
        let (lo, hi) = p.length_ratio;
        if !plain_len.is_finite()
            || plain_len <= f32::EPSILON
            || tables.user_len < lo * plain_len.min(looped_len)
            || tables.user_len > hi * plain_len.max(looped_len)
        {
            return None;
        }
        // 割り当て後の距離は各キーへの最短距離より小さくならないので、ここまでの値が上限を与える。
        // すでに残す候補の最低点に届かないなら、割り当て（重い）を省く
        let sigma = p.sigma_location;
        let prior = p.stage1_prior_scale * self.prior(index, entry);
        if -0.5 * (miss_sq / path.len() as f32) / (sigma * sigma) + prior < floor {
            return None;
        }
        *aligned += 1;
        // 順番を守った割り当て: キーを経路の順に、軌跡上の点へ（前進する向きだけで）割り当てたときの
        // 二乗距離の最小和（簡易な DTW）。同じキーを 2 回通る読みや、逆順の読みも正しく扱える
        // キー数の 2 倍以上の点がある最小の表（無ければ最大の表）
        let key_sq = tables
            .key_sq
            .iter()
            .find(|t| t.len() / 26 >= 2 * path.len())
            .or(tables.key_sq.last())?;
        let n = key_sq.len() / 26;
        // これを超えたら（以降の和は増える一方なので）打ち切る
        let limit = p.max_rms_miss * p.max_rms_miss * path.len() as f32;
        let mut dp = [0.0f32; 64];
        let dp = &mut dp[..n.min(64)];
        for (i, &b) in path.iter().enumerate() {
            let row = &key_sq[(b & !LOOP_FLAG) as usize * n..][..dp.len()];
            if i == 0 {
                dp.copy_from_slice(row);
                // 前から見て最小を取り込んだ形（j までの最小）にしておく
                for j in 1..dp.len() {
                    dp[j] = dp[j].min(dp[j - 1]);
                }
            } else {
                let mut run = f32::INFINITY;
                // 新しい dp[j] = min_{j' <= j} 旧 dp[j'] + row[j]
                for j in 0..dp.len() {
                    run = run.min(dp[j]);
                    dp[j] = run + row[j];
                }
                for j in 1..dp.len() {
                    dp[j] = dp[j].min(dp[j - 1]);
                }
            }
            if dp.last().is_none_or(|&best| best > limit) {
                return None;
            }
        }
        let aligned = dp.last().copied().unwrap_or(f32::INFINITY) / path.len() as f32;
        Some(-0.5 * aligned / (sigma * sigma) + prior)
    }

    /// 軌跡をキー単位にして間引く。短すぎる（タップ）・点が足りなければ `None`
    fn preprocess(&self, trace: &[TracePoint]) -> Option<Vec<Pt>> {
        let stride = trace.len().div_ceil(MAX_INPUT_POINTS).max(1);
        let mut units: Vec<Pt> = Vec::with_capacity(trace.len() / stride + 1);
        for (i, t) in trace.iter().enumerate() {
            if !(t.x.is_finite() && t.y.is_finite()) {
                continue;
            }
            if i % stride == 0 || i + 1 == trace.len() {
                units.push(self.layout.to_units(t.x, t.y));
            }
        }
        let points = thin(&units, self.params.min_point_distance);
        (points.len() >= 2 && path_length(&points) >= self.params.min_length).then_some(points)
    }

    /// 索引の経路から理想経路（折れ線）を作る。配置に無いキーを含めば `None`
    fn template(&self, path: &[u8]) -> Option<Template> {
        let mut centers: Vec<Pt> = Vec::with_capacity(path.len());
        for &b in path {
            centers.push(self.layout.center(b & !LOOP_FLAG)?);
        }
        let has_loops = path.iter().any(|b| b & LOOP_FLAG != 0);
        let looped = has_loops.then(|| {
            let r = self.params.loop_radius;
            let mut pts = Vec::with_capacity(centers.len() + 8);
            for (c, &b) in centers.iter().zip(path) {
                pts.push(*c);
                if b & LOOP_FLAG != 0 {
                    // キーの中心から出て、真上に半径 r の円を 1 周して戻る小さなループ
                    for step in 1..=8 {
                        let a = std::f32::consts::TAU * step as f32 / 8.0;
                        pts.push(Pt {
                            x: c.x + r * a.sin(),
                            y: c.y - r + r * a.cos(),
                        });
                    }
                }
            }
            pts
        });
        Some(Template {
            plain: centers,
            looped,
        })
    }

    /// 理想経路（ループなし / ありの良い方）と軌跡の照合: (対数尤度, 形状距離, 位置距離)
    fn likelihood(
        &self,
        template: &Template,
        user: &[Pt],
        user_shape: &[Pt],
        n: usize,
        scratch: &mut Scratch,
    ) -> (f32, f32, f32) {
        let mut best = (f32::MIN, 0.0, 0.0);
        let variants = std::iter::once(&template.plain).chain(template.looped.iter());
        for poly in variants {
            resample(poly, n, &mut scratch.t);
            shape_normalize(&scratch.t, &mut scratch.ts);
            let shape = mean_distance(user_shape, &scratch.ts);
            let location = mean_distance(user, &scratch.t);
            let ll = -0.5
                * ((shape / self.params.sigma_shape).powi(2)
                    + (location / self.params.sigma_location).powi(2));
            if ll > best.0 {
                best = (ll, shape, location);
            }
        }
        best
    }
}

/// 割り当ての表の点数（短い読み用から順に。最小は `base`、その 2 倍、32 点）
fn align_resolutions(base: usize) -> [usize; 3] {
    let b = base.clamp(4, 16);
    [b, b * 2, 32]
}

/// 点数の順序を持つ候補（`BinaryHeap` 用。`f32` の全順序は `total_cmp`）
#[derive(Debug, Clone, Copy)]
struct Scored(f32, usize);

impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Scored {}

impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Scored {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 同点は項目番号の大きい方を「小さい」とする（決定的にするため）
        self.0.total_cmp(&other.0).then(other.1.cmp(&self.1))
    }
}

#[derive(Default)]
struct Scratch {
    t: Vec<Pt>,
    ts: Vec<Pt>,
}

/// スコアの大きい上位 `n` 件だけを残す（順序は不定）
fn keep_top(v: &mut Vec<(f32, usize)>, n: usize) {
    if v.len() > n {
        v.select_nth_unstable_by(n, |a, b| b.0.total_cmp(&a.0));
        v.truncate(n);
    }
}

/// 点 `c` から線分 `ab` までの距離
fn point_segment_distance(c: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((c.x - a.x) * dx + (c.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    c.dist(Pt {
        x: a.x + dx * t,
        y: a.y + dy * t,
    })
}
