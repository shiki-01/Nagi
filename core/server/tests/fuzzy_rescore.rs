//! 曖昧入力の文脈つき再採点（F1）と高確信時の自動採用（F2）のテスト。
//! 変換モデルはモックで、`ConversionModel::score` / `ReadingScorer` の値を制御する。

mod common;

use common::dict_with_extra;
use nagi_config::Config;
use nagi_dict::Overlay;
use nagi_engine::TypingHint;
use nagi_infer::{ConversionModel, ConvertRequest, Hypothesis, InferError};
use nagi_server::fuzzy::{
    CHANNEL_COST_WEIGHT, FuzzyEngine, FuzzySettings, RESCORE_MAX, ReadingScorer, auto_apply_pick,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

const EXTRA: &str = "\
かんじ\t1\t1\t3000\t感じ
かんじ\t1\t1\t3500\t漢字
かじ\t1\t1\t3200\t火事
がっこう\t1\t1\t3000\t学校
";

const BUDGET: Duration = Duration::from_millis(150);

fn settings(rescore: bool) -> FuzzySettings {
    let mut config = Config::default();
    config.input.fuzzy.enabled = true;
    config.input.fuzzy.rescore = rescore;
    config.input.fuzzy.max_candidates = 5;
    FuzzySettings::from_config(&config.input.fuzzy)
}

/// 読みごとの対数確率を返すモックのスコアラー。呼び出された引数も記録する
struct MapScorer {
    by_reading: HashMap<&'static str, f32>,
    fail: bool,
    calls: Cell<usize>,
    last: RefCell<(String, Vec<(String, String)>)>,
}

impl MapScorer {
    fn new(pairs: &[(&'static str, f32)]) -> Self {
        MapScorer {
            by_reading: pairs.iter().copied().collect(),
            fail: false,
            calls: Cell::new(0),
            last: RefCell::new((String::new(), Vec::new())),
        }
    }
}

impl ReadingScorer for MapScorer {
    fn score(
        &self,
        left_context: &str,
        items: &[(String, String)],
        _budget: Duration,
    ) -> Option<Vec<f32>> {
        self.calls.set(self.calls.get() + 1);
        *self.last.borrow_mut() = (left_context.to_string(), items.to_vec());
        if self.fail {
            return None;
        }
        Some(
            items
                .iter()
                .map(|(r, _)| self.by_reading.get(r.as_str()).copied().unwrap_or(-100.0))
                .collect(),
        )
    }
}

fn suggest(
    settings: &FuzzySettings,
    scorer: Option<&MapScorer>,
) -> Vec<nagi_server::fuzzy::Suggestion> {
    let dict = dict_with_extra(EXTRA);
    FuzzyEngine::new().suggest_with(
        &dict,
        &Overlay::default(),
        "かmじ",
        &TypingHint::default(),
        settings,
        false,
        BUDGET,
        scorer.map(|s| (s as &dyn ReadingScorer, "左文脈")),
    )
}

#[test]
fn without_rescoring_the_dictionary_order_is_kept_and_the_scorer_is_not_called() {
    let scorer = MapScorer::new(&[("かじ", -1.0), ("かんじ", -50.0)]);
    // 設定が無効なら、スコアラーを渡しても呼ばない
    let off = suggest(&settings(false), Some(&scorer));
    assert_eq!(scorer.calls.get(), 0);
    assert!(off.iter().all(|s| s.lm_margin.is_none()));
    assert_eq!(off, suggest(&settings(false), None));
    assert!(off.iter().any(|s| s.surface == "感じ"), "{off:?}");
    assert!(off.iter().any(|s| s.surface == "火事"), "{off:?}");
}

#[test]
fn rescoring_reorders_by_lm_score_minus_channel_cost() {
    // LM は「かじ」（火事）を強く支持する
    let favor_kaji = MapScorer::new(&[("かんじ", -9.0), ("かじ", -1.0), ("かmじ", -30.0)]);
    let out = suggest(&settings(true), Some(&favor_kaji));
    assert_eq!(out[0].surface, "火事", "{out:?}");
    // 逆に「かんじ」を支持すれば感じが先頭
    let favor_kanji = MapScorer::new(&[("かんじ", -1.0), ("かじ", -9.0), ("かmじ", -30.0)]);
    let out = suggest(&settings(true), Some(&favor_kanji));
    assert_eq!(out[0].surface, "感じ", "{out:?}");
    // 差 = (補正の lm − 重み × チャネルコスト) − 入力そのままの lm
    let top = &out[0];
    let expected = -1.0 - CHANNEL_COST_WEIGHT * top.channel_cost - (-30.0);
    assert!((top.lm_margin.unwrap() - expected).abs() < 1e-4, "{top:?}");
    // 全件の差が降順
    let margins: Vec<f32> = out.iter().map(|s| s.lm_margin.unwrap()).collect();
    assert!(margins.windows(2).all(|w| w[0] >= w[1]), "{margins:?}");
}

#[test]
fn the_typed_reading_is_always_the_baseline_and_the_context_is_passed() {
    let scorer = MapScorer::new(&[("かんじ", -1.0)]);
    let out = suggest(&settings(true), Some(&scorer));
    let (left, items) = scorer.last.borrow().clone();
    assert_eq!(left, "左文脈");
    // 先頭は入力そのままの読み（辞書の最良変換 or 読みそのまま）
    assert_eq!(items[0].0, "かmじ");
    // 補正候補は上限まで、すべて別の読み
    assert!(items.len() <= RESCORE_MAX + 1);
    assert_eq!(items.len(), out.len() + 1);
    assert!(items[1..].iter().all(|(r, _)| r != "かmじ"));
}

#[test]
fn falls_back_to_the_dictionary_order_when_the_scorer_gives_up() {
    let mut scorer = MapScorer::new(&[("かじ", -1.0)]);
    scorer.fail = true;
    let out = suggest(&settings(true), Some(&scorer));
    assert_eq!(scorer.calls.get(), 1);
    assert_eq!(out, suggest(&settings(false), None));
    assert!(out.iter().all(|s| s.lm_margin.is_none()));
}

#[test]
fn clean_input_is_never_rescored() {
    let scorer = MapScorer::new(&[]);
    let dict = dict_with_extra(EXTRA);
    let out = FuzzyEngine::new().suggest_with(
        &dict,
        &Overlay::default(),
        "かんじ",
        &TypingHint::default(),
        &settings(true),
        false,
        BUDGET,
        Some((&scorer, "")),
    );
    assert!(out.is_empty());
    assert_eq!(scorer.calls.get(), 0, "辞書語はモデルを呼ばない");
}

#[test]
fn auto_apply_needs_the_flag_and_the_margin() {
    let scorer = MapScorer::new(&[("かんじ", -1.0), ("かじ", -9.0), ("かmじ", -30.0)]);
    let out = suggest(&settings(true), Some(&scorer));
    let margin = out[0].lm_margin.unwrap();
    let mut s = settings(true);
    // 既定ではオフ
    assert!(auto_apply_pick(&out, &s).is_none());
    s.auto_apply = true;
    s.auto_apply_margin = margin - 0.5;
    assert_eq!(auto_apply_pick(&out, &s).unwrap().surface, "感じ");
    s.auto_apply_margin = margin + 0.5;
    assert!(auto_apply_pick(&out, &s).is_none(), "閾値に届かない");
    // 再採点できていない提案（lm_margin なし）は採用しない
    s.auto_apply_margin = 1.0;
    let unscored = suggest(&settings(false), None);
    assert!(auto_apply_pick(&unscored, &s).is_none());
}

// ---- Pipeline・エンジン経由 ----

mod flow {
    use super::*;
    use nagi_engine::{Engine, EngineConfig, parse_key};
    use nagi_learn::{LearnConfig, Learner};
    use nagi_profile::SystemInfo;
    use nagi_protocol::view::CandidateSource;
    use nagi_server::{Pipeline, Runtime, Tunables};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// `score` が「読み → 対数確率」で答えるモデル。`convert` は使わない
    struct ScoreModel {
        by_reading: HashMap<String, f32>,
        scored: Arc<AtomicUsize>,
    }

    impl ConversionModel for ScoreModel {
        fn id(&self) -> &str {
            "score-mock"
        }

        fn convert(&mut self, _req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError> {
            Err(InferError::Other("未登録".into()))
        }

        fn score(
            &mut self,
            req: &ConvertRequest,
            candidates: &[String],
        ) -> Result<Vec<f32>, InferError> {
            self.scored.fetch_add(1, Ordering::SeqCst);
            let base = self.by_reading.get(&req.reading).copied().unwrap_or(-100.0);
            Ok(candidates.iter().map(|_| base).collect())
        }
    }

    fn engine(config: Config, scores: &[(&str, f32)]) -> (Engine, Arc<AtomicUsize>) {
        let runtime = Runtime::new(
            SystemInfo {
                total_memory_mb: 16384,
                cpu_cores: 8,
                has_gpu: false,
            },
            true,
            Tunables::from_config(&config),
            Learner::new(LearnConfig::default()),
            None,
        );
        let scored = Arc::new(AtomicUsize::new(0));
        let model = ScoreModel {
            by_reading: scores.iter().map(|(r, s)| (r.to_string(), *s)).collect(),
            scored: scored.clone(),
        };
        let pipeline = Pipeline::new(Some(dict_with_extra(EXTRA)), Some(Box::new(model)), runtime);
        (
            Engine::new(
                Arc::new(pipeline),
                EngineConfig {
                    fuzzy_input: config.input.fuzzy.enabled,
                    ..EngineConfig::default()
                },
            ),
            scored,
        )
    }

    fn config(rescore: bool, auto_apply: bool) -> Config {
        let mut c = Config::default();
        c.input.fuzzy.enabled = true;
        c.input.fuzzy.rescore = rescore;
        c.input.fuzzy.auto_apply = auto_apply;
        c.input.fuzzy.auto_apply_margin = 8;
        c
    }

    fn type_and_space(e: &mut Engine, romaji: &str) -> nagi_protocol::ProcessKeyResult {
        for c in romaji.chars() {
            e.process_key("s", &parse_key(&c.to_string()).unwrap());
        }
        e.process_key("s", &parse_key("Space").unwrap())
    }

    /// 「かんじ」を強く支持するモデル（入力そのままの「かmじ」は低い）
    const CONFIDENT: &[(&str, f32)] = &[("かんじ", -1.0), ("かじ", -20.0), ("かmじ", -40.0)];

    #[test]
    fn auto_apply_puts_the_correction_first_and_keeps_the_original_reachable() {
        let (mut e, scored) = engine(config(true, true), CONFIDENT);
        let r = type_and_space(&mut e, "kamji");
        assert!(scored.load(Ordering::SeqCst) > 0, "モデルで再採点している");
        let cands = r.candidates.expect("候補").items;
        assert_eq!(cands[0].surface, "感じ", "{cands:?}");
        assert_eq!(cands[0].sources, [CandidateSource::Fuzzy]);
        assert_eq!(cands[0].reading.as_deref(), Some("かんじ"));
        // 表示（プリエディット）も補正後の表記
        assert_eq!(r.preedit.expect("preedit").text, "感じ");
        // 元の読み（打った通り）に戻せる候補が一覧にある
        assert!(cands.iter().any(|c| c.surface == "かmじ"), "{cands:?}");
        // 元の候補を選べば打った通りで確定できる
        let index = cands.iter().position(|c| c.surface == "かmじ").unwrap();
        e.select_candidate("s", index as u32);
        let r = e.process_key("s", &parse_key("Enter").unwrap());
        assert_eq!(r.commit.as_deref(), Some("かmじ"));
    }

    #[test]
    fn auto_apply_is_off_by_default_even_with_rescoring() {
        let (mut e, _) = engine(config(true, false), CONFIDENT);
        let r = type_and_space(&mut e, "kamji");
        // 先頭（表示）は打った通りのまま
        assert_eq!(r.preedit.expect("preedit").text, "かmじ");
    }

    #[test]
    fn auto_apply_does_nothing_below_the_threshold() {
        // 補正の方が少し良い程度（差が閾値 8 に届かない）
        let weak = [("かんじ", -5.0), ("かじ", -20.0), ("かmじ", -6.0)];
        let (mut e, _) = engine(config(true, true), &weak);
        let r = type_and_space(&mut e, "kamji");
        assert_eq!(r.preedit.expect("preedit").text, "かmじ");
    }

    #[test]
    fn auto_apply_needs_rescoring() {
        // rescore = false なら、auto_apply = true でも判断材料が無いので採用しない
        let (mut e, scored) = engine(config(false, true), CONFIDENT);
        let r = type_and_space(&mut e, "kamji");
        assert_eq!(scored.load(Ordering::SeqCst), 0, "モデルを呼ばない");
        assert_eq!(r.preedit.expect("preedit").text, "かmじ");
    }
}
