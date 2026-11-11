//! 曖昧入力（タイプミス補正の「もしかして」候補。M1: キー近接 + 辞書）。
//!
//! 展開そのもの（`nagi-fuzzy`）は辞書の trie を問い合わせるだけの純粋な処理で、ここでは
//! 辞書に接続し、展開した補正読みを表記に変換して、**元の読みより辞書の Viterbi で良くなる**
//! ものだけを候補にする（雑音のある通信路のモデル: 事前の「その語らしさ」＋「その打ち間違いらしさ」）。
//!
//! - 読みが辞書語として完全一致する（またはユーザー辞書・学習にある）ときは何も出さない
//!   （クリーンな入力を誤って補正しない）
//! - 入力中の読みは変えない。候補に混ぜるだけで、自動では置き換えない

use nagi_config::FuzzySection;
use nagi_dict::{Dictionary, Overlay, PathInfo};
use nagi_engine::TypingHint;
use nagi_engine::touch::{DakutenOrder, key_table};
use nagi_fuzzy::{
    Correction, EditKind, EditKinds, Fuzzy, KanaTable, Key, KeyLayout, Params, ReadingIndex,
    covered,
};
use nagi_protocol::SetLayoutGeometryParams;
use nagi_romaji::RomajiTable;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 編集コスト 1.0 を Mozc 辞書のコスト尺度に換算する係数。
///
/// Mozc の語コストは `-500 * ln(出現確率)` 相当なので、編集コスト（負の対数尤度風）× 500 で
/// 同じ尺度になる。大きくすると補正が出にくく（保守的）、小さくすると出やすくなる。
/// 評価（`docs/decisions.md` H-24）で調整する前提の初期値
pub const LEXICON_COST_PER_EDIT_COST: f32 = 500.0;

/// 元の読みが辞書の語の連なりとして成り立つ（covered）場合の採否の余裕（マージン）の基本値。
/// 補正が濁点・半濁点・小書きの付け忘れ/過剰（Voicing）だけのとき。
///
/// 採用条件は `score + margin < typed_cost`。マージンは補正の種類・編集コスト・元の読みの長さで
/// 連続的に変える（かつての一律 5 倍の係数より、正解を出せた入力が多く誤表示が少なかった）。
/// 根拠は 2 つの乱数 seed の合算による診断分析（正解を出せた入力 316 → 413、クリーン入力の
/// 誤表示 0/782 → 2/782）。クリーンの誤表示 0.5% 以下を採る基準で選んだ値で、音韻ノイズの
/// 重みは仮の値であり実測ではない。評価データへの過学習の恐れがあるので、実利用の
/// データで再評価する前提
pub const MARGIN_VOICING: f32 = 1000.0;

/// covered の読みで、Voicing 以外の操作（置換・脱落・挿入・転置など）を含む補正のマージンの基本値。
/// 根拠は [`MARGIN_VOICING`] と同じ（Voicing は打ち間違いとして起きやすいので余裕を小さくする）
pub const MARGIN_OTHER: f32 = 2500.0;

/// マージンに足す、編集コスト 1.0 あたりの係数（`LEXICON_COST_PER_EDIT_COST` に対する比）。
/// 編集コストが大きい補正ほど余裕を多く求める。根拠は [`MARGIN_VOICING`] と同じ
pub const MARGIN_PER_EDIT_COST: f32 = 0.5;

/// 短い読みは偶然別の語に一致しやすいので、元の読みが [`SHORT_READING_CHARS`] 文字に満たない
/// 分だけ、1 文字あたりこの値をマージンに足す。根拠は [`MARGIN_VOICING`] と同じ
pub const MARGIN_PER_SHORT_CHAR: f32 = 200.0;

/// 「短い読み」とみなす文字数の境界（これ未満の分がマージンに加わる）
pub const SHORT_READING_CHARS: usize = 6;

/// 展開に使う時間の上限の目安（ミリ秒）。同期予算 `sync_budget` が小さければそちらに合わせる
/// （debug ビルドは release の 10 倍ほど遅く、2ms ではテストが不安定になるので緩める）
pub const EXPANSION_BUDGET_MS: u64 = if cfg!(debug_assertions) { 50 } else { 2 };

/// 辞書に問い合わせる補正読みの最大数（展開は上位をこれだけ返す。各々 Viterbi を 1 回走らせる）
const MAX_EXPANDED: usize = 30;

/// 再採点（LLM）に渡す補正候補の最大数（入力そのままの読みは別に 1 件加える）。
/// 1 件ずつ prefill が要るので、同期予算に収めるための上限
pub const RESCORE_MAX: usize = 8;

/// 再採点で、打ち間違いのコスト（`nagi-fuzzy` の編集コスト）を LM の対数確率（自然対数）に
/// 換算する係数。編集コストは置換 2.0〜3.0 など「負の対数尤度風の相対値」で、azooKey の
/// `ZenzaiTypoCandidateGenerator`（`score = lmScore - channelCost`、置換 2.0・脱落 3.0・転置 2.0）と
/// 近い尺度なので 1.0 とする。F0 の評価（`nagi-fuzzy-eval --rescore`）で調整する前提の初期値
pub const CHANNEL_COST_WEIGHT: f32 = 1.0;

/// 設定から決まる、曖昧入力の実行時パラメータ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuzzySettings {
    /// 有効か（`input.fuzzy.enabled`）
    pub enabled: bool,
    /// 累積コストの上限（`input.fuzzy.strength`）
    pub max_cost: f32,
    /// 候補に混ぜる最大数（`input.fuzzy.max_candidates`）
    pub max_candidates: usize,
    /// 有効な補正の種類（`input.fuzzy.kinds`）
    pub kinds: EditKinds,
    /// タッチ座標（キー内の位置・フリックの角度）を補正のコストに使うか（`input.fuzzy.use_touch_coords`）
    pub use_touch: bool,
    /// タッチ位置のばらつき（標準偏差。キー間隔を 1 とする）。設定項目ではなく評価用の調整値
    pub touch_sigma: f32,
    /// 補正候補を文脈つきの LM で再採点するか（`input.fuzzy.rescore`）
    pub rescore: bool,
    /// 高確信の補正を Space 変換の第 1 候補に自動採用するか（`input.fuzzy.auto_apply`）
    pub auto_apply: bool,
    /// 自動採用の閾値（再採点後のスコア差。自然対数の単位。`input.fuzzy.auto_apply_margin`）
    pub auto_apply_margin: f32,
    /// 展開の時間上限を外す（ノード数の上限だけにする）。設定項目ではなく評価用の値で、
    /// 時間による打ち切りで結果が実行ごとに揺れるのを防ぎ、前後比較を決定的にするために使う
    pub no_time_limit: bool,
    /// 採否の規則を従来の式（covered なら編集コストを一律 5 倍、マージンなし）に戻す。
    /// 設定項目ではなく評価用の値で、新旧の比較のために残す
    pub legacy_penalty: bool,
}

impl Default for FuzzySettings {
    fn default() -> Self {
        Self::from_config(&FuzzySection::default())
    }
}

impl FuzzySettings {
    /// 設定から作る
    pub fn from_config(section: &FuzzySection) -> Self {
        FuzzySettings {
            enabled: section.enabled,
            max_cost: section.strength.max_cost(),
            max_candidates: section.max_candidates,
            kinds: EditKinds::parse(&section.kinds),
            use_touch: section.use_touch_coords,
            touch_sigma: nagi_fuzzy::DEFAULT_TOUCH_SIGMA,
            rescore: section.rescore,
            auto_apply: section.auto_apply,
            auto_apply_margin: section.auto_apply_margin as f32,
            no_time_limit: false,
            legacy_penalty: false,
        }
    }
}

/// 補正の提案 1 件
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// 補正後の読み
    pub reading: String,
    /// 補正後の読みの変換結果（表記）
    pub surface: String,
    /// 並べ替え用の総合コスト（辞書コスト尺度。小さいほど良い）
    pub score: f32,
    /// 展開時の編集コスト（`nagi-fuzzy` の尺度。チャネルコスト）
    pub channel_cost: f32,
    /// 再採点（LM）の結果: 補正候補の `lmScore - channelCost` から、入力そのままの読みの
    /// `lmScore` を引いた差（大きいほど補正が有利）。再採点しなかった・できなかったときは `None`
    pub lm_margin: Option<f32>,
}

/// 補正の診断（[`FuzzyEngine::diagnose`]）の 1 行: 採否ゲートを通す前の補正 1 件
#[derive(Debug, Clone, PartialEq)]
pub struct DiagRow {
    /// 補正後の読み
    pub reading: String,
    /// 補正後の読みの最良の表記
    pub surface: String,
    /// 補正後の読みの Viterbi 総コスト
    pub corrected_cost: i32,
    /// 編集コスト（`nagi-fuzzy` の尺度）
    pub channel_cost: f32,
    /// 補正に使った編集操作の種類（出現順に `+` でつないだ名前。例 `Substitute+Voicing`）
    pub kinds: String,
}

/// 補正の診断結果。採否の判定の係数を決めるための材料で、採否の挙動には使わない
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnosis {
    /// 読みが辞書語（システム辞書・ユーザー辞書・スニペット）として完全一致するか
    /// （`collect` が何も出さなくなる条件と同じ）
    pub exact: bool,
    /// 読みが辞書の語の連なりとして成立するか
    pub covered: bool,
    /// 元の読みの Viterbi 最良経路の要約
    pub typed: Option<PathInfo>,
    /// 展開された補正（表記の重複は除く。採否のゲートは通さない）
    pub rows: Vec<DiagRow>,
}

/// 文脈つきで「読み → 表記」の尤もらしさ（対数確率。大きいほど良い）を求めるもの。
/// 実体は `nagi-infer` の [`nagi_infer::ConversionModel::score`]（Pipeline が包む）。
/// テストではモックに差し替える
pub trait ReadingScorer {
    /// `items` の各 `(読み, 表記)` の対数確率を返す。`budget` 内に求められない・モデルが無い・
    /// 失敗したときは `None`（呼び出し側が辞書のスコアにフォールバックする）
    fn score(
        &self,
        left_context: &str,
        items: &[(String, String)],
        budget: Duration,
    ) -> Option<Vec<f32>>;
}

/// 再採点の結果、補正を自動採用してよいか。`suggestions` は [`FuzzyEngine::suggest_with`] の結果
/// （先頭が最良）。先頭の `lm_margin` が閾値以上ならその提案を返す
pub fn auto_apply_pick<'a>(
    suggestions: &'a [Suggestion],
    settings: &FuzzySettings,
) -> Option<&'a Suggestion> {
    if !settings.auto_apply {
        return None;
    }
    suggestions
        .first()
        .filter(|s| s.lm_margin.is_some_and(|m| m >= settings.auto_apply_margin))
}

/// `nagi-dict` の辞書を `nagi-fuzzy` の索引として見せる
pub(crate) struct DictIndex<'a>(pub(crate) &'a Dictionary);

impl ReadingIndex for DictIndex<'_> {
    fn word_prefix_lens(&self, text: &str) -> Vec<usize> {
        self.0.reading_prefix_lens(text)
    }

    fn has_reading_prefix(&self, prefix: &str) -> bool {
        self.0.has_reading_prefix(prefix)
    }
}

/// 曖昧入力の提案器（隣接表などを 1 度だけ作って使い回す）
pub struct FuzzyEngine {
    /// 内蔵の QWERTY 配置
    fuzzy: Fuzzy,
    kana: KanaTable,
    /// フロントが渡したキー矩形から作った配置（直近の 1 つ）。同じ内容なら作り直さない
    custom: Mutex<Option<(SetLayoutGeometryParams, Arc<Fuzzy>)>>,
}

/// フロントのキー矩形から配置を作る（ラベルが 1 文字の英字のキーだけ。同じ文字は最初のキー）
fn layout_from_geometry(geometry: &SetLayoutGeometryParams) -> KeyLayout {
    let mut keys: Vec<Key> = Vec::new();
    for (label, k) in geometry.letter_keys() {
        if keys.iter().any(|x| x.label == label) {
            continue;
        }
        keys.push(Key {
            id: k.id.clone(),
            label,
            x: k.x as f32,
            y: k.y as f32,
            w: k.w as f32,
            h: k.h as f32,
        });
    }
    KeyLayout { keys }
}

impl Default for FuzzyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyEngine {
    /// 標準 QWERTY の隣接表と、12 キーの表（engine の `key_table`）から作る
    pub fn new() -> Self {
        let table = key_table(DakutenOrder::SmallFirst);
        let first = |s: &str| s.chars().next().unwrap_or(' ');
        let rows: Vec<[char; 5]> = table
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
            .collect();
        let cycles: Vec<Vec<char>> = table
            .variants
            .iter()
            .map(|c| c.iter().map(|s| first(s)).collect())
            .collect();
        let kana = KanaTable::new(&rows, &cycles);
        FuzzyEngine {
            fuzzy: Fuzzy::qwerty(kana.clone()),
            kana,
            custom: Mutex::new(None),
        }
    }

    /// 使う配置: フロントが渡したキー矩形（使えるものなら）、無ければ内蔵の QWERTY
    fn fuzzy_for(&self, geometry: Option<&SetLayoutGeometryParams>) -> Arc<Fuzzy> {
        let Some(geometry) = geometry.filter(|g| g.is_usable_for_fuzzy()) else {
            return Arc::new(self.fuzzy.clone());
        };
        let mut cache = self.custom.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached, fuzzy)) = cache.as_ref()
            && cached == geometry
        {
            return fuzzy.clone();
        }
        let fuzzy = Arc::new(Fuzzy::new(
            &layout_from_geometry(geometry),
            self.kana.clone(),
            RomajiTable::standard(),
        ));
        *cache = Some((geometry.clone(), fuzzy.clone()));
        fuzzy
    }

    /// 補正読みの展開だけ（辞書の枝刈り付き。表記への変換・採否の判定はしない）
    pub fn expand(
        &self,
        dict: &Dictionary,
        reading: &str,
        typing: &TypingHint<'_>,
        settings: &FuzzySettings,
        budget: Duration,
    ) -> Vec<Correction> {
        let params = Params {
            max_cost: settings.max_cost,
            max_results: MAX_EXPANDED,
            deadline: if settings.no_time_limit {
                None
            } else {
                Some(budget.min(Duration::from_millis(EXPANSION_BUDGET_MS)))
            },
            kinds: settings.kinds,
            use_touch: settings.use_touch,
            touch_sigma: settings.touch_sigma,
            ..Params::default()
        };
        self.fuzzy_for(typing.geometry).corrections_with(
            reading,
            typing.strokes,
            typing.flicks,
            &DictIndex(dict),
            &params,
        )
    }

    /// `reading`（入力中の読み）の補正の提案を、良い順に最大 `settings.max_candidates` 件返す
    /// （辞書のスコアだけ。再採点はしない）。
    ///
    /// `known` は、学習・ユーザー辞書などで読みがすでに意味を持つとき（クリーンとみなして何も出さない）。
    /// `budget` は展開に使える時間の上限
    #[allow(clippy::too_many_arguments)] // 辞書・overlay・読み・打鍵・設定・既知・予算は別々の出所
    pub fn suggest(
        &self,
        dict: &Dictionary,
        overlay: &Overlay,
        reading: &str,
        typing: &TypingHint<'_>,
        settings: &FuzzySettings,
        known: bool,
        budget: Duration,
    ) -> Vec<Suggestion> {
        let mut out = self
            .collect(dict, overlay, reading, typing, settings, known, budget)
            .map(|c| c.suggestions)
            .unwrap_or_default();
        out.truncate(settings.max_candidates);
        out
    }

    /// [`Self::suggest`] に、文脈つきの LM による再採点（`settings.rescore`）を足したもの。
    ///
    /// 辞書のスコアで絞った上位 [`RESCORE_MAX`] 件の補正読みと、入力そのままの読みを `scorer` に
    /// 評価させ、`lmScore - CHANNEL_COST_WEIGHT * channelCost` の降順に並べ替える（入力そのままは
    /// channelCost 0 の基準）。`scorer` が無い・`None` を返した（予算超過・モデルなし）ときは
    /// 辞書のスコアの順のまま `lm_margin` を `None` にして返す。
    /// `rescore_budget` は再採点に使える時間の上限
    #[allow(clippy::too_many_arguments)] // `suggest` の引数に再採点の道具・文脈・予算が加わる
    pub fn suggest_with(
        &self,
        dict: &Dictionary,
        overlay: &Overlay,
        reading: &str,
        typing: &TypingHint<'_>,
        settings: &FuzzySettings,
        known: bool,
        budget: Duration,
        rescoring: Option<(&dyn ReadingScorer, &str)>,
    ) -> Vec<Suggestion> {
        let Some(mut collected) =
            self.collect(dict, overlay, reading, typing, settings, known, budget)
        else {
            return Vec::new();
        };
        if let Some((scorer, left_context)) = rescoring.filter(|_| settings.rescore) {
            collected.suggestions.truncate(RESCORE_MAX);
            rerank(
                &mut collected.suggestions,
                reading,
                &collected.typed_surface,
                scorer,
                left_context,
                budget,
            );
        }
        collected.suggestions.truncate(settings.max_candidates);
        collected.suggestions
    }

    /// 補正の診断: 採否のゲート（Viterbi コストの比較）を通さず、展開された補正を全部返す。
    /// 読みが辞書語として完全一致していても `rows` を返す（`exact` で区別できる）。
    /// 読みが空なら `None`
    pub fn diagnose(
        &self,
        dict: &Dictionary,
        overlay: &Overlay,
        reading: &str,
        typing: &TypingHint<'_>,
        settings: &FuzzySettings,
        budget: Duration,
    ) -> Option<Diagnosis> {
        if reading.is_empty() {
            return None;
        }
        let exact = !dict.lookup(reading).is_empty()
            || !overlay.lookup(reading).is_empty()
            || !overlay.snippets(reading).is_empty();
        let covered = covered(&DictIndex(dict), reading);
        let typed = dict.best_path_info(reading, Some(overlay));
        let mut rows: Vec<DiagRow> = Vec::new();
        for c in self.expand(dict, reading, typing, settings, budget) {
            let Some(info) = dict.best_path_info(&c.reading, Some(overlay)) else {
                continue;
            };
            if rows.iter().any(|r| r.surface == info.surface) {
                continue;
            }
            let mut kinds: Vec<String> = Vec::new();
            for op in &c.ops {
                let name = format!("{:?}", op.kind);
                if !kinds.contains(&name) {
                    kinds.push(name);
                }
            }
            rows.push(DiagRow {
                reading: c.reading,
                surface: info.surface,
                corrected_cost: info.total_cost,
                channel_cost: c.cost,
                kinds: kinds.join("+"),
            });
        }
        Some(Diagnosis {
            exact,
            covered,
            typed,
            rows,
        })
    }

    /// 補正の提案を辞書のスコアの良い順に全部集める（個数の上限は掛けない）。クリーンな入力・
    /// 補正なしなら `None`
    #[allow(clippy::too_many_arguments)]
    fn collect(
        &self,
        dict: &Dictionary,
        overlay: &Overlay,
        reading: &str,
        typing: &TypingHint<'_>,
        settings: &FuzzySettings,
        known: bool,
        budget: Duration,
    ) -> Option<Collected> {
        if !settings.enabled || known || settings.max_candidates == 0 || reading.is_empty() {
            return None;
        }
        // クリーンな入力: 読みが辞書語（システム辞書・ユーザー辞書・スニペット）として完全一致する
        if !dict.lookup(reading).is_empty()
            || !overlay.lookup(reading).is_empty()
            || !overlay.snippets(reading).is_empty()
        {
            return None;
        }
        let corrections = self.expand(dict, reading, typing, settings, budget);
        if corrections.is_empty() {
            return None;
        }
        // 元の読みの Viterbi コスト（辞書に無い文字は大きなコストになる）。補正はこれより良くなければ捨てる
        let typed = best_path(dict, overlay, reading);
        let typed_cost = typed.as_ref().map_or(f32::MAX, |(_, c)| *c);
        let typed_surface = typed.map_or_else(|| reading.to_string(), |(s, _)| s);
        // 元の読みがすでに辞書の語の連なりとして成り立っている（かなの打ち間違いが別の語になった
        // 可能性より、打った通りである可能性が高い）ときは、補正に強い証拠（マージン）を求める
        let is_covered = covered(&DictIndex(dict), reading);
        let reading_chars = reading.chars().count();
        let mut out: Vec<Suggestion> = Vec::new();
        for c in corrections {
            let Some((surface, cost)) = best_path(dict, overlay, &c.reading) else {
                continue;
            };
            let (score, margin) = if settings.legacy_penalty {
                let per_edit = if is_covered {
                    LEXICON_COST_PER_EDIT_COST * LEGACY_VALID_READING_PENALTY
                } else {
                    LEXICON_COST_PER_EDIT_COST
                };
                (cost + c.cost * per_edit, 0.0)
            } else {
                let score = cost + c.cost * LEXICON_COST_PER_EDIT_COST;
                let margin = if is_covered {
                    covered_margin(&c, reading_chars)
                } else {
                    0.0
                };
                (score, margin)
            };
            if score + margin >= typed_cost || out.iter().any(|s| s.surface == surface) {
                continue;
            }
            out.push(Suggestion {
                reading: c.reading,
                surface,
                score,
                channel_cost: c.cost,
                lm_margin: None,
            });
        }
        if out.is_empty() {
            return None;
        }
        out.sort_by(|a, b| {
            a.score
                .total_cmp(&b.score)
                .then_with(|| a.reading.cmp(&b.reading))
        });
        Some(Collected {
            suggestions: out,
            typed_surface,
        })
    }
}

/// 従来方式（`legacy_penalty`）で covered の読みの編集コストに掛ける係数
const LEGACY_VALID_READING_PENALTY: f32 = 5.0;

/// covered の読みに対する補正の採否のマージン（辞書コスト尺度）。
/// `reading_chars` は元の読みの文字数
pub fn covered_margin(c: &Correction, reading_chars: usize) -> f32 {
    let voicing_only = !c.ops.is_empty() && c.ops.iter().all(|op| op.kind == EditKind::Voicing);
    let base = if voicing_only {
        MARGIN_VOICING
    } else {
        MARGIN_OTHER
    };
    let short = SHORT_READING_CHARS.saturating_sub(reading_chars) as f32;
    base + MARGIN_PER_EDIT_COST * c.cost * LEXICON_COST_PER_EDIT_COST
        + MARGIN_PER_SHORT_CHAR * short
}

/// [`FuzzyEngine::collect`] の結果
struct Collected {
    /// 辞書のスコアの良い順
    suggestions: Vec<Suggestion>,
    /// 入力そのままの読みの最良の表記（辞書に無ければ読みそのまま）
    typed_surface: String,
}

/// `suggestions` を `lmScore - CHANNEL_COST_WEIGHT * channelCost` の降順に並べ替え、`lm_margin` を入れる。
/// 評価できなければ（`scorer` が `None` を返す・件数が合わない）何も変えない
fn rerank(
    suggestions: &mut [Suggestion],
    reading: &str,
    typed_surface: &str,
    scorer: &dyn ReadingScorer,
    left_context: &str,
    budget: Duration,
) {
    // LM が -inf を返す（文脈長の超過など）と比較できないので、十分低い有限値に丸める
    const FLOOR: f32 = -1000.0;
    let mut items: Vec<(String, String)> = Vec::with_capacity(suggestions.len() + 1);
    items.push((reading.to_string(), typed_surface.to_string()));
    items.extend(
        suggestions
            .iter()
            .map(|s| (s.reading.clone(), s.surface.clone())),
    );
    let Some(scores) = scorer.score(left_context, &items, budget) else {
        return;
    };
    if scores.len() != items.len() {
        return;
    }
    let clean = |x: f32| if x.is_finite() { x.max(FLOOR) } else { FLOOR };
    let base = clean(scores[0]);
    for (s, lm) in suggestions.iter_mut().zip(&scores[1..]) {
        s.lm_margin = Some(clean(*lm) - CHANNEL_COST_WEIGHT * s.channel_cost - base);
    }
    // 安定ソート: 差が同じなら辞書のスコアの順を保つ
    suggestions.sort_by(|a, b| {
        b.lm_margin
            .unwrap_or(f32::MIN)
            .total_cmp(&a.lm_margin.unwrap_or(f32::MIN))
    });
}

/// 読みの最良の変換（表記）と Viterbi の総コスト
fn best_path(dict: &Dictionary, overlay: &Overlay, reading: &str) -> Option<(String, f32)> {
    dict.segment_candidates_with(reading, 1, Some(overlay))
        .into_iter()
        .next()
        .map(|c| (c.surface, c.cost as f32))
}
