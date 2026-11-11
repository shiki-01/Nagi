//! かな言語モデルの学習（`cargo xtask build-kana-lm` が使う）。
//!
//! 学習データは辞書だけ（評価データは使わない）:
//!
//! 1. 語内の連鎖: 辞書の各エントリの読みを `exp(-cost/500)` で重み付けして数える（同じ読みは合算）。
//!    重みの総和は [`KanaLmParams::dict_mass`] に正規化する
//! 2. 語をまたぐ連鎖: 連接コストを使った疑似文を乱数で作って数える。前の語の右 ID `r` から次の語の
//!    左 ID `L` への遷移確率は `exp(-conn(r, L)/500) × W(L)`（`W(L)` は左 ID が `L` の語の重みの合計）に比例し、
//!    `L` の中では語の重みに比例して語を選ぶ。文末は連接コスト `conn(r, 0)` に基づく終端として選ぶ。
//!    乱数は固定シードで決定的

use crate::builder::DictionaryBuilder;
use crate::kanalm::{DEFAULT_ORDER, KanaLm, KanaLmTrainer, SYM_UNK, symbol};
use std::collections::HashMap;

/// 重みの尺度（Mozc のコストは約 500 で 1 の対数確率）
const COST_SCALE: f64 = 500.0;

/// 学習の設定
#[derive(Debug, Clone)]
pub struct KanaLmParams {
    /// 次数
    pub order: usize,
    /// 疑似文の数
    pub sentences: usize,
    /// 辞書の語の重みの総和をこの値に正規化して数える（疑似文 1 件を 1 とした相対量）
    pub dict_mass: f64,
    /// 次数 3 以上の n-gram を捨てる頻度の下限（`dict_mass` と同じ尺度）
    pub prune_min_count: f64,
    /// 文末へ進む重みの、全左 ID の重み総和に対する比（大きいほど文が短い）
    pub eos_weight: f64,
    /// 疑似文の語数の上限
    pub max_words: usize,
    /// 乱数の種
    pub seed: u64,
}

impl Default for KanaLmParams {
    fn default() -> Self {
        Self {
            order: DEFAULT_ORDER,
            sentences: 2_000_000,
            dict_mass: 1_000_000.0,
            prune_min_count: 7.0,
            eos_weight: 0.0001,
            max_words: 12,
            seed: 20_261_111,
        }
    }
}

/// 学習の統計
#[derive(Debug, Clone, Default)]
pub struct KanaLmStats {
    /// 辞書の異なる読みの数
    pub dict_readings: usize,
    /// 作った疑似文の数
    pub sentences: usize,
    /// 疑似文 1 件あたりの平均語数
    pub avg_words: f64,
    /// 疑似文の平均文字数
    pub avg_chars: f64,
}

/// 決定的な乱数（splitmix64）
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// [0, 1)
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// `cum`（単調増加の累積）から `u * 総和` に当たる位置を選ぶ
fn pick(cum: &[f64], u: f64) -> usize {
    let target = u * cum.last().copied().unwrap_or(0.0);
    cum.partition_point(|&x| x <= target).min(cum.len() - 1)
}

/// 辞書（構築前のビルダ）からかな言語モデルを学習する
pub fn train_from_builder(
    builder: &DictionaryBuilder,
    params: &KanaLmParams,
) -> (KanaLm, KanaLmStats) {
    // 語: (読みの記号列, 左ID, 右ID, 重み)
    let mut words: Vec<(Vec<u8>, u16, u16, f64)> = Vec::new();
    for (reading, lid, rid, cost) in builder.training_entries() {
        let syms: Vec<u8> = reading.chars().map(symbol).collect();
        // かな以外（記号・数字・英字）を含む読みは、かなの連鎖の学習から外す
        if syms.is_empty() || syms.contains(&SYM_UNK) {
            continue;
        }
        let w = (-f64::from(cost) / COST_SCALE).exp();
        words.push((syms, lid, rid, w));
    }
    let total_w: f64 = words.iter().map(|w| w.3).sum();
    let mut trainer = KanaLmTrainer::new(params.order);

    // 1. 語内: 同じ読みの重みを合算して数える
    let mut by_reading: HashMap<&[u8], f64> = HashMap::new();
    for (syms, _, _, w) in &words {
        *by_reading.entry(syms.as_slice()).or_insert(0.0) += w;
    }
    let scale = params.dict_mass / total_w.max(f64::MIN_POSITIVE);
    let mut readings: Vec<(&[u8], f64)> = by_reading.into_iter().collect();
    // HashMap の順序に結果が依存しないよう並べる（浮動小数点の加算順を固定する）
    readings.sort_unstable_by(|a, b| a.0.cmp(b.0));
    for (syms, w) in &readings {
        trainer.add_symbols(syms, w * scale);
    }
    let dict_readings = readings.len();
    drop(readings);

    // 2. 語間: 疑似文
    let n_ids = words
        .iter()
        .map(|w| usize::from(w.1.max(w.2)) + 1)
        .max()
        .unwrap_or(1);
    let mut by_lid: Vec<Vec<u32>> = vec![Vec::new(); n_ids];
    let mut cum_by_lid: Vec<Vec<f64>> = vec![Vec::new(); n_ids];
    let mut lid_w = vec![0.0f64; n_ids];
    for (i, w) in words.iter().enumerate() {
        let l = usize::from(w.1);
        lid_w[l] += w.3;
        by_lid[l].push(i as u32);
        cum_by_lid[l].push(lid_w[l]);
    }
    // 左 ID 0 は文頭・文末の特殊 ID なので語としては選ばない
    lid_w[0] = 0.0;
    let all_w: f64 = lid_w.iter().sum();
    let eos_w = all_w * params.eos_weight;
    // 前の語の右 ID ごとの、左 ID の累積分布（末尾が文末）。初めて使うときに作る
    let mut cdf: Vec<Option<Vec<f64>>> = vec![None; n_ids];
    let conn = |r: usize, l: usize| f64::from(builder.connection_cost(r as u16, l as u16));

    let mut rng = Rng(params.seed);
    let (mut total_words, mut total_chars) = (0usize, 0usize);
    let mut seq: Vec<u8> = Vec::new();
    for _ in 0..params.sentences {
        seq.clear();
        let mut prev = 0usize; // 文頭
        let mut n_words = 0;
        while n_words < params.max_words {
            let table = cdf[prev].get_or_insert_with(|| {
                let mut acc = 0.0;
                let mut v = Vec::with_capacity(n_ids + 1);
                for (l, &w) in lid_w.iter().enumerate() {
                    if w > 0.0 {
                        acc += (-conn(prev, l) / COST_SCALE).exp() * w;
                    }
                    v.push(acc);
                }
                v.push(acc + (-conn(prev, 0) / COST_SCALE).exp() * eos_w);
                v
            });
            // 先頭で文末を選ばない（空の文を作らない）
            let usable = if n_words == 0 {
                &table[..n_ids]
            } else {
                &table[..]
            };
            if usable.last().copied().unwrap_or(0.0) <= 0.0 {
                break;
            }
            let l = pick(usable, rng.unit());
            if l >= n_ids {
                break;
            }
            let ws = &by_lid[l];
            if ws.is_empty() {
                break;
            }
            let wi = ws[pick(&cum_by_lid[l], rng.unit())] as usize;
            let (syms, _, rid, _) = &words[wi];
            seq.extend_from_slice(syms);
            prev = usize::from(*rid);
            n_words += 1;
        }
        if seq.is_empty() {
            continue;
        }
        total_words += n_words;
        total_chars += seq.len();
        trainer.add_symbols(&seq, 1.0);
    }
    let stats = KanaLmStats {
        dict_readings,
        sentences: params.sentences,
        avg_words: total_words as f64 / params.sentences.max(1) as f64,
        avg_chars: total_chars as f64 / params.sentences.max(1) as f64,
    };
    (trainer.finish(params.prune_min_count), stats)
}

/// 検証用: `readings` の 1 文字あたり対数確率の平均と、各読みにランダムな 1 文字の置換または
/// 隣接 2 文字の入れ替えを入れた読みの平均、元の方が高かった読みの割合を返す。固定シードで決定的
pub fn corruption_check(lm: &KanaLm, readings: &[String], seed: u64) -> CorruptionReport {
    let kana: Vec<char> = ('ぁ'..='ん').collect();
    let mut rng = Rng(seed);
    let (mut orig, mut bad, mut wins, mut n) = (0.0f64, 0.0f64, 0usize, 0usize);
    for r in readings {
        let chars: Vec<char> = r.chars().collect();
        if chars.len() < 2 {
            continue;
        }
        let mut c = chars.clone();
        let i = (rng.next_u64() % c.len() as u64) as usize;
        if rng.next_u64() % 2 == 0 {
            // 置換（元と違う文字に）
            loop {
                let x = kana[(rng.next_u64() % kana.len() as u64) as usize];
                if x != c[i] {
                    c[i] = x;
                    break;
                }
            }
        } else {
            // 隣接入れ替え（同じ文字しか並んでいなければ置換に落とす）
            let j = if i + 1 < c.len() { i + 1 } else { i - 1 };
            if c[i] == c[j] {
                c[i] = if c[i] == 'あ' { 'い' } else { 'あ' };
            } else {
                c.swap(i, j);
            }
        }
        let corrupted: String = c.into_iter().collect();
        let a = f64::from(lm.log_prob_per_char(r));
        let b = f64::from(lm.log_prob_per_char(&corrupted));
        orig += a;
        bad += b;
        wins += usize::from(a > b);
        n += 1;
    }
    let d = n.max(1) as f64;
    CorruptionReport {
        n,
        original_mean: orig / d,
        corrupted_mean: bad / d,
        win_rate: wins as f64 / d,
    }
}

/// [`corruption_check`] の結果
#[derive(Debug, Clone, Copy)]
pub struct CorruptionReport {
    /// 比べた読みの数
    pub n: usize,
    /// 元の読みの 1 文字あたり対数確率の平均
    pub original_mean: f64,
    /// 壊した読みの 1 文字あたり対数確率の平均
    pub corrupted_mean: f64,
    /// 元の方が高かった読みの割合
    pub win_rate: f64,
}

/// `evaluation.tsv` の入力（読み）列を取り出す。評価専用で、学習には渡さない
pub fn evaluation_readings(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split('\t').nth(1))
        .filter(|r| !r.is_empty() && r.chars().all(|c| symbol(c) != 0))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trains_from_small_dictionary() {
        let mut b = DictionaryBuilder::new();
        b.set_connection(3, vec![0; 9]);
        b.add_entry("わたし", "わたし", 1, 1, 1, 1000);
        b.add_entry("は", "は", 2, 2, 2, 500);
        b.add_entry("がくせい", "がくせい", 1, 1, 1, 2000);
        b.add_entry("です", "です", 2, 2, 2, 800);
        let p = KanaLmParams {
            order: 3,
            sentences: 2000,
            ..KanaLmParams::default()
        };
        let (lm1, stats) = train_from_builder(&b, &p);
        let (lm2, _) = train_from_builder(&b, &p);
        assert_eq!(lm1.to_bytes(), lm2.to_bytes(), "決定的");
        assert!(stats.avg_words >= 1.0);
        assert!(lm1.log_prob("わたしは") > lm1.log_prob("わしたは"));
    }

    #[test]
    fn evaluation_readings_skips_comments_and_non_kana() {
        let t = "# h\nOK:\tこれは\t表記\tx\ty\tz\nOK:\tabc\t表記\tx\ty\tz\n";
        assert_eq!(evaluation_readings(t), vec!["これは".to_string()]);
    }
}
