//! かな文字 n-gram 言語モデル（`KanaLm`）。
//!
//! 読み（ひらがな列）の「自然さ」を対数確率で測る。曖昧入力の補正で、補正前後の読みを比べる
//! 特徴量に使う。モデルは文字（ひらがな・長音符・その他 1 クラス）の n-gram で、平滑は
//! 補間つき Witten-Bell（各文脈で `P(w|h) = c(h,w)/(c(h)+N(h)) + N(h)/(c(h)+N(h)) * P(w|h')`。
//! `N(h)` は文脈 `h` の後に現れた異なる文字の数。頻度 1 未満の文字は頻度ぶんの端数で数える）。どの文脈でも確率の和は 1 になる。
//!
//! データ資産なので同梱せず、`cargo xtask build-kana-lm` で辞書から作る（[`crate::kanalm_train`]）。
//! 読み込めないときは [`KanaLm::load`] が `None` を返し、呼び出し側は特徴量なしで従来どおり動く。
//!
//! ファイル形式（リトルエンディアン）: マジック 8 バイト `NAGIKLM\0` / バージョン u32 / 次数 u32 /
//! 語彙数 u32 / 次数ぶんの段（段 k は長さ k の文脈。文脈数 u64・n-gram 数 u64・文脈キー（5 バイト）×文脈数・
//! gamma f32×文脈数・n-gram キー（5 バイト）×n-gram 数・確率 f32×n-gram 数）/ 末尾にそれまでの
//! 全バイトの FNV-1a 64 チェックサム。

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::Path;

/// 既定の次数（n-gram の n）
pub const DEFAULT_ORDER: usize = 5;

const MAGIC: &[u8; 8] = b"NAGIKLM\0";
const VERSION: u32 = 1;
/// 1 記号あたりのビット数（語彙数 128 未満）
const SYM_BITS: u32 = 7;
/// 次数の上限（キーを 8 バイトに収めるため）
const MAX_ORDER: usize = 8;

/// 未知文字（ひらがな・長音符以外）
pub const SYM_UNK: u8 = 0;
/// 文末
pub const SYM_EOS: u8 = 1;
/// 文頭（文脈にだけ現れ、予測はされない）
pub const SYM_BOS: u8 = 2;
const HIRA_FIRST: u32 = 0x3041;
const HIRA_LAST: u32 = 0x3096;
const SYM_HIRA_BASE: u8 = 3;
/// 長音符「ー」
const SYM_PROLONG: u8 = SYM_HIRA_BASE + (HIRA_LAST - HIRA_FIRST + 1) as u8;
/// 記号の種類数
pub const VOCAB: usize = SYM_PROLONG as usize + 1;

/// 文字を記号に写す。カタカナはひらがなに畳む。それ以外は未知
pub fn symbol(c: char) -> u8 {
    let mut u = c as u32;
    if (0x30A1..=0x30F6).contains(&u) {
        u -= 0x60;
    }
    if (HIRA_FIRST..=HIRA_LAST).contains(&u) {
        SYM_HIRA_BASE + (u - HIRA_FIRST) as u8
    } else if u == 0x30FC {
        SYM_PROLONG
    } else {
        SYM_UNK
    }
}

/// 記号の列をキー（記号あたり 7 ビット）にまとめる
fn pack(syms: &[u8]) -> u64 {
    syms.iter()
        .fold(0u64, |k, &s| (k << SYM_BITS) | u64::from(s))
}

/// 1 つの次数の表
#[derive(Debug, Clone, Default)]
struct Level {
    /// 文脈キー（昇順）
    ctxs: Vec<u64>,
    /// 文脈ごとのバックオフ重み `N/(c+N)`
    gammas: Vec<f32>,
    /// n-gram キー（文脈キー + 予測記号。昇順）
    grams: Vec<u64>,
    /// 割引後の確率 `c(h,w)/(c(h)+N(h))`
    probs: Vec<f32>,
}

/// かな文字 n-gram 言語モデル
#[derive(Debug, Clone)]
pub struct KanaLm {
    order: usize,
    levels: Vec<Level>,
}

impl KanaLm {
    /// 次数
    pub fn order(&self) -> usize {
        self.order
    }

    /// 全段の n-gram 数
    pub fn gram_count(&self) -> usize {
        self.levels.iter().map(|l| l.grams.len()).sum()
    }

    /// 文脈 `ctx`（長さ `order - 1`）の後に `w` が来る確率
    fn prob(&self, ctx: &[u8], w: u8) -> f64 {
        // 最下層は予測されうる記号（文頭を除く）の一様分布
        let mut p = 1.0 / (VOCAB - 1) as f64;
        for (k, level) in self.levels.iter().enumerate() {
            let ckey = pack(&ctx[ctx.len() - k..]);
            let Ok(ci) = level.ctxs.binary_search(&ckey) else {
                continue;
            };
            let disc = level
                .grams
                .binary_search(&((ckey << SYM_BITS) | u64::from(w)))
                .map_or(0.0, |i| f64::from(level.probs[i]));
            p = disc + f64::from(level.gammas[ci]) * p;
        }
        p
    }

    /// 読み全体の対数確率（自然対数。文頭・文末つき。文末の予測を含む）。
    /// ひらがな以外の文字は未知として扱う
    pub fn log_prob(&self, reading: &str) -> f32 {
        let n = self.order;
        let mut hist: Vec<u8> = vec![SYM_BOS; n - 1];
        hist.extend(reading.chars().map(symbol));
        hist.push(SYM_EOS);
        let mut total = 0.0f64;
        for i in n - 1..hist.len() {
            total += self.prob(&hist[i + 1 - n..i], hist[i]).ln();
        }
        total as f32
    }

    /// 1 文字あたりの対数確率（`log_prob` を文字数で割る。文字数 0 のときは割らない）。
    /// `log_prob_per_char × 文字数 = log_prob`
    pub fn log_prob_per_char(&self, reading: &str) -> f32 {
        let n = reading.chars().count().max(1);
        self.log_prob(reading) / n as f32
    }

    /// バイト列へ書き出す
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.order as u32).to_le_bytes());
        out.extend_from_slice(&(VOCAB as u32).to_le_bytes());
        for l in &self.levels {
            out.extend_from_slice(&(l.ctxs.len() as u64).to_le_bytes());
            out.extend_from_slice(&(l.grams.len() as u64).to_le_bytes());
            for k in &l.ctxs {
                out.extend_from_slice(&k.to_le_bytes()[..5]);
            }
            for g in &l.gammas {
                out.extend_from_slice(&g.to_le_bytes());
            }
            for k in &l.grams {
                out.extend_from_slice(&k.to_le_bytes()[..5]);
            }
            for p in &l.probs {
                out.extend_from_slice(&p.to_le_bytes());
            }
        }
        let sum = fnv1a(&out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    /// バイト列から読む。マジック・バージョン・チェックサム・長さが合わなければ `None`
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 28 + 8 {
            return None;
        }
        let (body, tail) = bytes.split_at(bytes.len() - 8);
        if fnv1a(body) != u64::from_le_bytes(tail.try_into().ok()?) {
            return None;
        }
        let mut r = Reader { b: body, pos: 0 };
        if r.take(8)? != MAGIC {
            return None;
        }
        if r.u32()? != VERSION {
            return None;
        }
        let order = r.u32()? as usize;
        if !(1..=MAX_ORDER).contains(&order) || r.u32()? as usize != VOCAB {
            return None;
        }
        let mut levels = Vec::with_capacity(order);
        for _ in 0..order {
            let nc = usize::try_from(r.u64()?).ok()?;
            let ng = usize::try_from(r.u64()?).ok()?;
            let ctxs = r.keys(nc)?;
            let gammas = r.f32s(nc)?;
            let grams = r.keys(ng)?;
            let probs = r.f32s(ng)?;
            levels.push(Level {
                ctxs,
                gammas,
                grams,
                probs,
            });
        }
        if r.pos != body.len() {
            return None;
        }
        Some(Self { order, levels })
    }

    /// ファイルから読む。無い・壊れている・形式違いは `None`
    pub fn load(path: &Path) -> Option<Self> {
        Self::from_bytes(&std::fs::read(path).ok()?)
    }

    /// ファイルへ書く（一時ファイル → rename。使用中のファイルを壊さない）
    pub fn write_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_bytes())?;
        std::fs::rename(&tmp, path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.b.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn keys(&mut self, n: usize) -> Option<Vec<u64>> {
        let raw = self.take(n.checked_mul(5)?)?;
        Some(
            raw.chunks_exact(5)
                .map(|c| {
                    let mut b = [0u8; 8];
                    b[..5].copy_from_slice(c);
                    u64::from_le_bytes(b)
                })
                .collect(),
        )
    }
    fn f32s(&mut self, n: usize) -> Option<Vec<f32>> {
        let raw = self.take(n.checked_mul(4)?)?;
        Some(
            raw.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        )
    }
}

/// u64 キー用の軽いハッシャ
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn write_u64(&mut self, v: u64) {
        let x = (v ^ (v >> 29)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        self.0 = x ^ (x >> 32);
    }
}

type CountMap = HashMap<u64, f64, BuildHasherDefault<KeyHasher>>;

/// n-gram の（重みつき）頻度を数えて [`KanaLm`] にするもの
pub struct KanaLmTrainer {
    order: usize,
    counts: Vec<CountMap>,
}

impl KanaLmTrainer {
    /// 次数 `order`（1〜8）の学習器を作る
    pub fn new(order: usize) -> Self {
        let order = order.clamp(1, MAX_ORDER);
        Self {
            order,
            counts: (0..order).map(|_| CountMap::default()).collect(),
        }
    }

    /// 次数
    pub fn order(&self) -> usize {
        self.order
    }

    /// 読み 1 件（文頭・文末つきで）を重み `weight` で数える
    pub fn add_reading(&mut self, reading: &str, weight: f64) {
        let syms: Vec<u8> = reading.chars().map(symbol).collect();
        self.add_symbols(&syms, weight);
    }

    /// 記号列（文頭・文末は含めない）を重み `weight` で数える
    pub fn add_symbols(&mut self, syms: &[u8], weight: f64) {
        let n = self.order;
        let mut hist: Vec<u8> = Vec::with_capacity(syms.len() + n);
        hist.resize(n - 1, SYM_BOS);
        hist.extend_from_slice(syms);
        hist.push(SYM_EOS);
        for i in n - 1..hist.len() {
            for k in 1..=n {
                let key = pack(&hist[i + 1 - k..=i]);
                *self.counts[k - 1].entry(key).or_insert(0.0) += weight;
            }
        }
    }

    /// 数え終えてモデルにする。次数 3 以上の n-gram は頻度が `min_count` 未満のものを捨てる
    /// （捨てた分を除いて正規化するので、確率の和は崩れない）
    pub fn finish(self, min_count: f64) -> KanaLm {
        let order = self.order;
        let mut levels = Vec::with_capacity(order);
        for (k, map) in self.counts.into_iter().enumerate() {
            let mut entries: Vec<(u64, f64)> = map
                .into_iter()
                .filter(|&(_, c)| k < 2 || c >= min_count)
                .collect();
            entries.sort_unstable_by_key(|e| e.0);
            let mut level = Level::default();
            let mut i = 0;
            while i < entries.len() {
                let ctx = entries[i].0 >> SYM_BITS;
                let mut j = i;
                let mut total = 0.0;
                // 異なる文字の数は、頻度が 1 未満の文字を頻度ぶんだけ（端数で）数える。重みつきの頻度では
                // 極めて稀な文字が大量にあっても、文脈の確率質量を食い潰さないようにするため
                let mut types = 0.0;
                while j < entries.len() && entries[j].0 >> SYM_BITS == ctx {
                    total += entries[j].1;
                    types += entries[j].1.min(1.0);
                    j += 1;
                }
                let denom = total + types;
                level.ctxs.push(ctx);
                level.gammas.push((types / denom) as f32);
                for e in &entries[i..j] {
                    level.grams.push(e.0);
                    level.probs.push((e.1 / denom) as f32);
                }
                i = j;
            }
            levels.push(level);
        }
        KanaLm { order, levels }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> KanaLm {
        let mut t = KanaLmTrainer::new(3);
        for _ in 0..50 {
            t.add_reading("ありがとう", 1.0);
            t.add_reading("こんにちは", 1.0);
        }
        t.add_reading("あり", 5.0);
        t.finish(0.0)
    }

    /// 任意の文脈で、予測されうる記号の確率の和が 1 になる
    #[test]
    fn probabilities_are_normalized() {
        let lm = tiny();
        let ctxs: Vec<Vec<u8>> = vec![
            vec![SYM_BOS, SYM_BOS],
            vec![SYM_BOS, symbol('あ')],
            vec![symbol('あ'), symbol('り')],
            vec![symbol('ん'), symbol('ん')], // 未見の文脈
            vec![SYM_UNK, symbol('と')],
        ];
        for c in ctxs {
            let sum: f64 = (0..VOCAB as u8)
                .filter(|&w| w != SYM_BOS)
                .map(|w| lm.prob(&c, w))
                .sum();
            assert!((sum - 1.0).abs() < 1e-4, "{c:?}: {sum}");
        }
    }

    #[test]
    fn seen_reading_beats_scrambled() {
        let lm = tiny();
        assert!(lm.log_prob("ありがとう") > lm.log_prob("りあとがう"));
        assert!(lm.log_prob_per_char("ありがとう") > lm.log_prob_per_char("うとがりあ"));
    }

    #[test]
    fn unknown_chars_and_empty_are_finite() {
        let lm = tiny();
        for s in ["", "漢字", "abc", "あ漢り", "ー"] {
            let lp = lm.log_prob(s);
            assert!(lp.is_finite() && lp < 0.0, "{s:?}: {lp}");
        }
        // カタカナはひらがなと同じ
        assert_eq!(lm.log_prob("アリガトウ"), lm.log_prob("ありがとう"));
    }

    /// 文末の確率が効く（語の途中で終わる読みは、完結した読みより低い）
    #[test]
    fn eos_is_modeled() {
        let lm = tiny();
        assert!(lm.log_prob("ありがとう") > lm.log_prob("ありがと"));
    }

    #[test]
    fn per_char_times_len_is_total() {
        let lm = tiny();
        let s = "ありがとう";
        let total = lm.log_prob_per_char(s) * 5.0;
        assert!((total - lm.log_prob(s)).abs() < 1e-4);
    }

    #[test]
    fn roundtrip_and_corruption() {
        let lm = tiny();
        let bytes = lm.to_bytes();
        let back = KanaLm::from_bytes(&bytes).expect("読める");
        assert_eq!(back.log_prob("ありがとう"), lm.log_prob("ありがとう"));
        let mut bad = bytes.clone();
        bad[20] ^= 1;
        assert!(KanaLm::from_bytes(&bad).is_none());
        assert!(KanaLm::from_bytes(&bytes[..bytes.len() - 3]).is_none());
        assert!(KanaLm::from_bytes(b"").is_none());
        assert!(KanaLm::load(Path::new("/nonexistent/nagi-kana-lm.bin")).is_none());
    }
}
