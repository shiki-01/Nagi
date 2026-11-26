//! 英字入力の予測候補・スペル訂正（`docs/decisions.md` H-146）。
//!
//! 英字で打っている途中の語（またはスペースの直前の語）に対して、次の 3 種類の候補を返す。
//!
//! - 打ったままの語（先頭。必ず含める）
//! - 訂正: 語彙・学習に無い語に近い語（[`crate::spell`]）と、修正表（[`Fixes`]）の直し先
//! - 補完: 打った語で始まる語（学習した語 → 語彙の一般的な語の順）
//!
//! あわせて、自信の高い訂正を「自動修正の対象」（[`Assist::autocorrect`]）として返す。適用するかどうかは
//! フロントの設定（モバイルの自動修正）で決める。デスクトップは候補として出すだけで自動では置き換えない。
//!
//! 大文字小文字は打った語の形（先頭だけ大文字・全部大文字）を候補へ写す。語彙は小文字だけなので、
//! 固有の大文字表記（I・Monday など）は修正表の直し先で補う。
//!
//! 状態は持たない。学習した語は呼び出し側が [`LearnedWords`] で渡す。

use crate::lexicon::Lexicon;
use crate::spell::{SpellCosts, suggest_from};
use std::collections::HashMap;

/// 同梱の修正表（`config/english_fixes.tsv`）
const BUILTIN_FIXES: &str = include_str!("../../../config/english_fixes.tsv");

/// 同梱のよく使う語の表（`config/english_common.txt`。補完の並べ替えに使う）
const BUILTIN_COMMON: &str = include_str!("../../../config/english_common.txt");

/// よく使う語の順位（小さいほど先）。表に無ければ `None`
pub fn common_rank(lower: &str) -> Option<usize> {
    static RANKS: std::sync::OnceLock<HashMap<String, usize>> = std::sync::OnceLock::new();
    RANKS
        .get_or_init(|| {
            let mut m = HashMap::new();
            for line in BUILTIN_COMMON.lines() {
                let w = line.trim();
                if w.is_empty() || w.starts_with('#') {
                    continue;
                }
                let n = m.len();
                m.entry(w.to_ascii_lowercase()).or_insert(n);
            }
            m
        })
        .get(lower)
        .copied()
}

/// 語彙から補完の候補を取る件数（よく使う語の表で並べ替える前の数）
const LEXICON_COMPLETION_POOL: usize = 64;

/// 候補を求める語の最大の長さ（これより長い語は打ったままだけを返す）
pub const MAX_WORD_LEN: usize = 32;

/// 訂正候補を探す最小の文字数（3 文字の語は小さな誤りだけ自動修正する）
pub const MIN_CORRECT_LEN: usize = 3;

/// 学習した語を「正しい語」とみなす回数（1 回だけ打った語は打ち間違いかもしれないので訂正の対象に残す）
pub const LEARNED_KNOWN_COUNT: u32 = 2;

/// 自動修正する訂正の重みの上限（1 回の編集まで。隣のキーの置換・入れ替え・重ねはそれより安い）
pub const AUTOCORRECT_MAX_COST: f32 = 1.0;

/// 自動修正するには、2 番目の訂正とこれだけ得点の差が要る（拮抗するときは直さない）
pub const AUTOCORRECT_MARGIN: f32 = 0.25;

/// 自動修正の直し先にしてよい語彙の段階の上限（稀な語へは直さない）
pub const AUTOCORRECT_MAX_TIER: u8 = 50;

/// 打った語で始まる語があるとき（入力途中かもしれない）に自動修正してよい誤りの重みの上限
/// （入れ替え 0.7・隣のキーの置換 0.6・重ね 0.5 まで）
pub const AUTOCORRECT_IN_PROGRESS_MAX_COST: f32 = 0.7;

/// 打った語で始まる語があるとき、訂正先がその語より何段階一般的なら自動修正するか
pub const AUTOCORRECT_IN_PROGRESS_TIER_GAP: u8 = 20;

/// 候補の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// 打ったままの語
    Typed,
    /// 補完（打った語で始まる語）
    Completion,
    /// 訂正（打ち間違いの直し先）
    Correction,
}

/// 候補 1 件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 表記（打った語の大文字小文字の形を写したもの）
    pub text: String,
    /// 種類
    pub kind: ItemKind,
}

/// 候補の一覧と、自動修正の対象
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assist {
    /// 候補（先頭は打ったままの語。語が空なら空）
    pub items: Vec<Item>,
    /// 自信の高い訂正（自動修正の対象）。無ければ `None`
    pub autocorrect: Option<String>,
}

/// 学習した語（利用者が実際に入力した英単語）の参照
pub trait LearnedWords {
    /// 小文字の語 `lower` の `(表記, 回数)`。学習に無ければ `None`
    fn get(&self, lower: &str) -> Option<(String, u32)>;
    /// 小文字の `prefix` で始まる語の `(表記, 回数)`（回数の多い順、最大 `limit` 件。`prefix` 自体は除く）
    fn with_prefix(&self, prefix: &str, limit: usize) -> Vec<(String, u32)>;
}

/// 学習した語が無いとき
pub struct NoLearned;

impl LearnedWords for NoLearned {
    fn get(&self, _: &str) -> Option<(String, u32)> {
        None
    }
    fn with_prefix(&self, _: &str, _: usize) -> Vec<(String, u32)> {
        Vec::new()
    }
}

/// 修正表（打った語 → 決まった直し先）
#[derive(Debug, Clone, Default)]
pub struct Fixes {
    /// 打った語（小文字） → 直し先
    map: HashMap<String, String>,
    /// 直し先の小文字 → 直し先（`don't` を正しい語とみなす・`monday` の補完を `Monday` にする）
    targets: HashMap<String, String>,
}

impl Fixes {
    /// 同梱の修正表
    pub fn builtin() -> &'static Fixes {
        static FIXES: std::sync::OnceLock<Fixes> = std::sync::OnceLock::new();
        FIXES.get_or_init(|| Fixes::parse(BUILTIN_FIXES))
    }

    /// TSV を読む（不正な行は読み飛ばす）
    pub fn parse(text: &str) -> Fixes {
        let mut out = Fixes::default();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cols = line.split('\t');
            let (Some(typed), Some(fixed)) = (cols.next(), cols.next()) else {
                continue;
            };
            let typed = typed.trim().to_ascii_lowercase();
            let fixed = fixed.trim();
            if !is_word(&typed) || !is_word(fixed) || typed == fixed {
                continue;
            }
            out.targets
                .insert(fixed.to_ascii_lowercase(), fixed.to_string());
            out.map.entry(typed).or_insert_with(|| fixed.to_string());
        }
        out
    }

    /// 打った語（小文字）の直し先
    pub fn get(&self, lower: &str) -> Option<&str> {
        self.map.get(lower).map(String::as_str)
    }

    /// 小文字の語が修正表の直し先なら、その表記（`monday` → `Monday`）
    pub fn target(&self, lower: &str) -> Option<&str> {
        self.targets.get(lower).map(String::as_str)
    }

    /// 件数
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// 英字・アポストロフィ・ハイフンだけで、英字を 1 つ以上含むか
pub fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_WORD_LEN
        && s.bytes().any(|b| b.is_ascii_alphabetic())
        && s.bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'\'' || b == b'-')
}

/// 打った語の大文字小文字の形
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasePattern {
    /// 全部小文字
    Lower,
    /// 先頭だけ大文字（1 文字の大文字も含む）
    Capitalized,
    /// 2 文字以上で全部大文字
    Upper,
    /// それ以外（`iPhone` など）
    Mixed,
}

impl CasePattern {
    /// `word` の形
    pub fn of(word: &str) -> CasePattern {
        let letters: Vec<u8> = word.bytes().filter(|b| b.is_ascii_alphabetic()).collect();
        let Some((&first, rest)) = letters.split_first() else {
            return CasePattern::Lower;
        };
        let rest_lower = rest.iter().all(|b| b.is_ascii_lowercase());
        let rest_upper = rest.iter().all(|b| b.is_ascii_uppercase());
        if first.is_ascii_lowercase() && rest_lower {
            CasePattern::Lower
        } else if first.is_ascii_uppercase() && rest.is_empty() {
            CasePattern::Capitalized
        } else if first.is_ascii_uppercase() && rest_upper {
            CasePattern::Upper
        } else if first.is_ascii_uppercase() && rest_lower {
            CasePattern::Capitalized
        } else {
            CasePattern::Mixed
        }
    }

    /// 候補 `candidate` にこの形を写す。`candidate` が固有の大文字を含む（`I'm`・`Monday`・学習した
    /// `iPhone`）なら、全部大文字のとき以外はそのまま
    pub fn apply(self, candidate: &str) -> String {
        let has_upper = candidate.bytes().any(|b| b.is_ascii_uppercase());
        match self {
            CasePattern::Upper => candidate.to_ascii_uppercase(),
            _ if has_upper => candidate.to_string(),
            CasePattern::Capitalized => {
                let mut out = String::with_capacity(candidate.len());
                let mut done = false;
                for c in candidate.chars() {
                    if !done && c.is_ascii_alphabetic() {
                        out.push(c.to_ascii_uppercase());
                        done = true;
                    } else {
                        out.push(c);
                    }
                }
                out
            }
            CasePattern::Lower | CasePattern::Mixed => candidate.to_string(),
        }
    }
}

/// 候補を組み立てる材料
pub struct Assistant<'a> {
    /// 語彙（無ければ補完・訂正は学習と修正表だけ）
    pub lexicon: Option<&'a Lexicon>,
    /// 修正表
    pub fixes: &'a Fixes,
    /// 学習した語
    pub learned: &'a dyn LearnedWords,
    /// 編集の重み（キーの近さ）
    pub costs: &'a SpellCosts<'a>,
}

impl Assistant<'_> {
    /// 小文字の語 `lower` を正しい語とみなすか（語彙・修正表の直し先・2 回以上入力した学習語）
    pub fn is_known(&self, lower: &str) -> bool {
        let bare: String = lower.chars().filter(|c| *c != '\'').collect();
        self.lexicon.is_some_and(|l| l.tier(lower).is_some())
            || self.fixes.target(lower).is_some()
            || self
                .learned
                .get(lower)
                .is_some_and(|(_, n)| n >= LEARNED_KNOWN_COUNT)
            || (bare != lower && self.lexicon.is_some_and(|l| l.tier(&bare).is_some()))
    }

    /// 補完・訂正の表記（修正表の固有の表記 → 語彙の表記の形 → 小文字の順で決め、打った語の形を写す）
    fn completion_text(&self, lower: &str, case: CasePattern) -> String {
        if let Some(t) = self.fixes.target(lower) {
            return case.apply(t);
        }
        match self.lexicon.and_then(|l| l.case_of(lower)) {
            Some(c @ (crate::lexicon::WordCase::Capitalized | crate::lexicon::WordCase::Upper)) => {
                case.apply(&c.render(lower))
            }
            _ => case.apply(lower),
        }
    }

    /// `word` の候補を最大 `limit` 件（先頭の打ったままの語を含む）
    pub fn assist(&self, word: &str, limit: usize) -> Assist {
        if word.is_empty() || limit == 0 {
            return Assist::default();
        }
        let mut items = vec![Item {
            text: word.to_string(),
            kind: ItemKind::Typed,
        }];
        if !is_word(word) {
            return Assist {
                items,
                autocorrect: None,
            };
        }
        let lower = word.to_ascii_lowercase();
        let case = CasePattern::of(word);
        let known = self.is_known(&lower);

        // 補完（学習 → 語彙）。語彙の補完の最も一般的な段階も覚える（入力途中かの判断に使う）
        let mut completions: Vec<String> = Vec::new();
        let mut best_completion_tier: Option<u8> = None;
        let want = limit.saturating_mul(2).max(8);
        for (display, n) in self.learned.with_prefix(&lower, want) {
            let dl = display.to_ascii_lowercase();
            let tier = self.lexicon.and_then(|l| l.tier(&dl));
            if n >= LEARNED_KNOWN_COUNT || tier.is_some() {
                completions.push(case.apply(&display));
                // 何度も使った学習語は、一般的な語と同じくらい「入力途中」の根拠になる
                let t = tier.unwrap_or(0);
                best_completion_tier = Some(best_completion_tier.map_or(t, |b| b.min(t)));
            }
        }
        if let Some(lex) = self.lexicon {
            let mut pool = lex.complete(&lower, LEXICON_COMPLETION_POOL.max(want));
            // 段階が同じ語の中では、よく使う語の表にある語を表の順で先にする（安定ソート）
            pool.sort_by_key(|(w, _)| common_rank(w).unwrap_or(usize::MAX));
            for (w, tier) in pool.into_iter().take(want) {
                completions.push(self.completion_text(&w, case));
                best_completion_tier = Some(best_completion_tier.map_or(tier, |b| b.min(tier)));
            }
        }

        // 訂正（修正表 → 編集距離）
        let mut corrections: Vec<String> = Vec::new();
        let mut autocorrect = None;
        if let Some(fixed) = self.fixes.get(&lower) {
            let text = case.apply(fixed);
            if text != word {
                corrections.push(text.clone());
                // 学習で「打ったまま」を選び直した語（2 回以上）は直さない
                let learned_typed = self
                    .learned
                    .get(&lower)
                    .is_some_and(|(_, n)| n >= LEARNED_KNOWN_COUNT);
                if !learned_typed && case != CasePattern::Mixed {
                    autocorrect = Some(text);
                }
            }
        } else if !known
            && !lower.contains('\'')
            && !lower.contains('-')
            && let Some(lex) = self.lexicon
        {
            let found = suggest_from(lex, &lower, self.costs, 4, MIN_CORRECT_LEN);
            // 打った語で始まる語があるなら入力途中かもしれない。訂正先の方がずっと一般的な語で、
            // 誤りも小さい（入れ替え・重ね・隣のキー）ときだけ直す（teh → the。comp は直さない）
            let in_progress_ok = |b: &crate::spell::Suggestion| match best_completion_tier {
                None => true,
                Some(t) => {
                    b.cost <= AUTOCORRECT_IN_PROGRESS_MAX_COST + 1e-4
                        && u16::from(b.tier) + u16::from(AUTOCORRECT_IN_PROGRESS_TIER_GAP)
                            <= u16::from(t)
                }
            };
            if let Some(best) = found.first()
                && (lower.len() > MIN_CORRECT_LEN
                    || best.cost <= AUTOCORRECT_IN_PROGRESS_MAX_COST + 1e-4)
                && matches!(case, CasePattern::Lower | CasePattern::Capitalized)
                && best.cost <= AUTOCORRECT_MAX_COST + 1e-4
                && best.tier <= AUTOCORRECT_MAX_TIER
                && in_progress_ok(best)
                && found
                    .get(1)
                    .is_none_or(|second| second.score() - best.score() >= AUTOCORRECT_MARGIN)
            {
                autocorrect = Some(self.completion_text(&best.word, case));
            }
            corrections.extend(found.iter().map(|s| self.completion_text(&s.word, case)));
        }

        // 正しい語・入力途中の語は補完を先に、それ以外（打ち間違い）は訂正を先に並べる
        let completions_first = known || !completions.is_empty();
        let (first, second, first_kind, second_kind) = if completions_first {
            (
                completions,
                corrections,
                ItemKind::Completion,
                ItemKind::Correction,
            )
        } else {
            (
                corrections,
                completions,
                ItemKind::Correction,
                ItemKind::Completion,
            )
        };
        // 自動修正の対象は打ったままの語の次に置く
        let mut ordered: Vec<(String, ItemKind)> = Vec::new();
        if let Some(a) = &autocorrect {
            ordered.push((a.clone(), ItemKind::Correction));
        }
        ordered.extend(first.into_iter().map(|t| (t, first_kind)));
        ordered.extend(second.into_iter().map(|t| (t, second_kind)));
        for (text, kind) in ordered {
            if items.len() >= limit {
                break;
            }
            if items.iter().any(|i| i.text.eq_ignore_ascii_case(&text)) {
                continue;
            }
            items.push(Item { text, kind });
        }
        Assist { items, autocorrect }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::LexiconBuilder;
    use std::collections::BTreeMap;

    fn lexicon(words: &[(&str, u8)]) -> Lexicon {
        let mut b = LexiconBuilder::new();
        for (w, t) in words {
            b.add(w, *t);
        }
        Lexicon::from_vec(b.build().unwrap()).unwrap()
    }

    struct Learned(BTreeMap<String, (String, u32)>);

    impl Learned {
        fn of(words: &[(&str, u32)]) -> Learned {
            Learned(
                words
                    .iter()
                    .map(|(w, n)| (w.to_ascii_lowercase(), (w.to_string(), *n)))
                    .collect(),
            )
        }
    }

    impl LearnedWords for Learned {
        fn get(&self, lower: &str) -> Option<(String, u32)> {
            self.0.get(lower).cloned()
        }
        fn with_prefix(&self, prefix: &str, limit: usize) -> Vec<(String, u32)> {
            let mut v: Vec<(String, u32)> = self
                .0
                .range(prefix.to_string()..)
                .take_while(|(k, _)| k.starts_with(prefix))
                .filter(|(k, _)| k.as_str() != prefix)
                .map(|(_, v)| v.clone())
                .collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.truncate(limit);
            v
        }
    }

    fn texts(a: &Assist) -> Vec<&str> {
        a.items.iter().map(|i| i.text.as_str()).collect()
    }

    fn sample() -> Lexicon {
        lexicon(&[
            ("the", 10),
            ("there", 10),
            ("these", 20),
            ("therefore", 35),
            ("thermal", 50),
            ("hello", 10),
            ("help", 10),
            ("receive", 20),
            ("monday", 35),
            ("because", 10),
            ("form", 20),
            ("from", 10),
            ("dont", 70),
        ])
    }

    fn assist(lex: &Lexicon, learned: &dyn LearnedWords, word: &str) -> Assist {
        let costs = SpellCosts::default();
        Assistant {
            lexicon: Some(lex),
            fixes: Fixes::builtin(),
            learned,
            costs: &costs,
        }
        .assist(word, 6)
    }

    #[test]
    fn completes_by_frequency_then_length() {
        let lex = sample();
        let a = assist(&lex, &NoLearned, "the");
        assert_eq!(texts(&a), ["the", "there", "these", "therefore", "thermal"]);
        assert_eq!(a.items[1].kind, ItemKind::Completion);
        assert_eq!(a.autocorrect, None);
    }

    #[test]
    fn corrects_and_autocorrects_clear_typos() {
        let lex = sample();
        let a = assist(&lex, &NoLearned, "recieve");
        assert_eq!(a.autocorrect.as_deref(), Some("receive"));
        assert_eq!(a.items[0].text, "recieve");
        assert_eq!(a.items[1].text, "receive");
        assert_eq!(a.items[1].kind, ItemKind::Correction);
        let a = assist(&lex, &NoLearned, "becuase");
        assert_eq!(a.autocorrect.as_deref(), Some("because"));
    }

    #[test]
    fn prefix_of_common_word_is_in_progress() {
        let lex = lexicon(&[
            ("computer", 20),
            ("company", 35),
            ("camp", 35),
            ("the", 10),
            ("tehran", 60),
        ]);
        let a = assist(&lex, &NoLearned, "comp");
        assert_eq!(a.autocorrect, None, "{a:?}");
        // よく使う語の表にある company が先
        assert_eq!(texts(&a)[..3], ["comp", "company", "computer"]);
        // 入れ替えで、ずっと一般的な語へ直せるなら直す（tehran の入力途中より the）
        let a = assist(&lex, &NoLearned, "teh");
        assert_eq!(a.autocorrect.as_deref(), Some("the"), "{a:?}");
    }

    #[test]
    fn keeps_case_of_typed_word() {
        let lex = sample();
        assert_eq!(
            assist(&lex, &NoLearned, "Recieve").autocorrect.as_deref(),
            Some("Receive")
        );
        assert_eq!(texts(&assist(&lex, &NoLearned, "The"))[1], "There");
        // 全部大文字は略語かもしれないので自動修正しない（候補には出す）
        let a = assist(&lex, &NoLearned, "RECIEVE");
        assert_eq!(a.autocorrect, None);
        assert!(texts(&a).contains(&"RECEIVE"));
    }

    #[test]
    fn fixes_table_handles_i_and_contractions() {
        let lex = sample();
        assert_eq!(
            assist(&lex, &NoLearned, "i").autocorrect.as_deref(),
            Some("I")
        );
        assert_eq!(
            assist(&lex, &NoLearned, "dont").autocorrect.as_deref(),
            Some("don't")
        );
        assert_eq!(
            assist(&lex, &NoLearned, "Dont").autocorrect.as_deref(),
            Some("Don't")
        );
        // 直し先の表記は正しい語として扱う
        assert_eq!(assist(&lex, &NoLearned, "don't").autocorrect, None);
        // 固有の大文字表記で補完する
        assert!(texts(&assist(&lex, &NoLearned, "mon")).contains(&"Monday"));
    }

    #[test]
    fn known_words_are_not_corrected() {
        let lex = sample();
        let a = assist(&lex, &NoLearned, "form");
        assert_eq!(a.autocorrect, None);
        assert!(a.items.iter().all(|i| i.kind != ItemKind::Correction));
    }

    #[test]
    fn learned_words_are_known_and_completed_first() {
        let lex = sample();
        let learned = Learned::of(&[("kubernetes", 3), ("kubectl", 1), ("helm", 2)]);
        let a = assist(&lex, &learned, "kubernetes");
        assert_eq!(a.autocorrect, None);
        let a = assist(&lex, &learned, "kub");
        // 1 回だけの語は補完に出さない
        assert_eq!(texts(&a), ["kub", "kubernetes"]);
        let a = assist(&lex, &learned, "hel");
        assert_eq!(texts(&a)[..2], ["hel", "helm"]);
    }

    #[test]
    fn learned_typed_word_disables_fix() {
        let lex = sample();
        let learned = Learned::of(&[("im", 2)]);
        assert_eq!(assist(&lex, &learned, "im").autocorrect, None);
    }

    #[test]
    fn ambiguous_typo_is_not_autocorrected() {
        // bate → bake / bale（どちらも 1 文字の置換）は拮抗するので直さない（候補には出す）
        let lex = lexicon(&[("bake", 10), ("bale", 10)]);
        let a = assist(&lex, &NoLearned, "bate");
        assert_eq!(a.autocorrect, None, "{a:?}");
        assert_eq!(texts(&a), ["bate", "bake", "bale"]);
    }

    #[test]
    fn non_words_return_only_typed() {
        let lex = sample();
        let a = assist(&lex, &NoLearned, "a1b2");
        assert_eq!(texts(&a), ["a1b2"]);
        assert_eq!(a.autocorrect, None);
        assert!(assist(&lex, &NoLearned, "").items.is_empty());
    }

    #[test]
    fn case_pattern_of_and_apply() {
        assert_eq!(CasePattern::of("hello"), CasePattern::Lower);
        assert_eq!(CasePattern::of("Hello"), CasePattern::Capitalized);
        assert_eq!(CasePattern::of("A"), CasePattern::Capitalized);
        assert_eq!(CasePattern::of("HELLO"), CasePattern::Upper);
        assert_eq!(CasePattern::of("iPhone"), CasePattern::Mixed);
        assert_eq!(CasePattern::Capitalized.apply("don't"), "Don't");
        assert_eq!(CasePattern::Lower.apply("I'm"), "I'm");
        assert_eq!(CasePattern::Upper.apply("I'm"), "I'M");
    }

    #[test]
    fn common_words_come_first_and_lexicon_case_is_used() {
        let mut b = LexiconBuilder::new();
        for (w, t) in [
            ("tax", 10),
            ("tea", 10),
            ("the", 10),
            ("that", 10),
            ("to", 10),
        ] {
            b.add(w, t);
        }
        b.add_cased("Tehran", 50);
        b.add_cased("NASA", 40);
        let lex = Lexicon::from_vec(b.build().unwrap()).unwrap();
        let a = assist(&lex, &NoLearned, "t");
        assert_eq!(texts(&a)[..4], ["t", "the", "to", "that"]);
        assert!(texts(&assist(&lex, &NoLearned, "teh")).contains(&"Tehran"));
        assert!(texts(&assist(&lex, &NoLearned, "na")).contains(&"NASA"));
        assert_eq!(common_rank("the"), Some(0));
    }

    #[test]
    fn builtin_fixes_parse() {
        let f = Fixes::builtin();
        assert!(f.len() > 40);
        assert_eq!(f.get("i"), Some("I"));
        assert_eq!(f.target("monday"), Some("Monday"));
        // 正しい英単語を左に載せていない
        for w in ["its", "were", "well", "id", "ill", "lets", "hell", "shed"] {
            assert_eq!(f.get(w), None, "{w}");
        }
    }
}
