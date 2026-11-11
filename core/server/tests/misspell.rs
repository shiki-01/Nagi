//! 誤り辞書（`misspellings.tsv`）による「もしかして」候補の統合テスト。

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_dict::Overlay;
use nagi_engine::TypingHint;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::view::CandidateSource;
use nagi_server::fuzzy::{FuzzyEngine, FuzzySettings, Suggestion};
use nagi_server::misspell::Misspellings;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::sync::Arc;
use std::time::Duration;

const EXTRA: &str = "\
かん\t1\t1\t3000\t感
し\t1\t1\t3000\t氏
かんじ\t1\t1\t3000\t漢字
しんぶん\t1\t1\t3000\t新聞
がっこう\t1\t1\t3000\t学校
";

fn misspellings(tsv: &str) -> Arc<Misspellings> {
    let mut issues = Vec::new();
    let m = Misspellings::parse("t.tsv", tsv, &mut issues);
    assert!(issues.is_empty(), "{issues:?}");
    Arc::new(m)
}

fn settings() -> FuzzySettings {
    FuzzySettings {
        enabled: true,
        no_time_limit: true,
        ..FuzzySettings::default()
    }
}

fn suggest(
    engine: &FuzzyEngine,
    reading: &str,
    settings: &FuzzySettings,
    known: bool,
) -> Vec<Suggestion> {
    let dict = dict_with_extra(EXTRA);
    engine.suggest(
        &dict,
        &Overlay::default(),
        reading,
        &TypingHint::default(),
        settings,
        known,
        Duration::from_millis(150),
    )
}

#[test]
fn covered_reading_gets_the_misspelling_candidate() {
    let engine = FuzzyEngine::new();
    // 「かんし」は「かん」+「し」の連なり（covered）で、語としては完全一致しない
    engine.set_misspellings(misspellings("かんし\tかんじ\n"));
    let out = suggest(&engine, "かんし", &settings(), false);
    assert_eq!(out.first().map(|s| s.reading.as_str()), Some("かんじ"));
    assert_eq!(out[0].surface, "漢字");
    assert_eq!(out[0].channel_cost, 0.0);
    assert_eq!(out[0].lm_margin, None);
}

#[test]
fn exact_dictionary_reading_still_gets_the_candidate() {
    let engine = FuzzyEngine::new();
    // 「しんぶん」は辞書語だが、誤り辞書に載っていれば出る（通常の補正は出ない）
    engine.set_misspellings(misspellings("しんぶん\tがっこう\n"));
    let out = suggest(&engine, "しんぶん", &settings(), false);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].surface, "学校");
    // 誤り辞書が空なら、辞書語は何も出さない
    assert!(
        suggest(&FuzzyEngine::new(), "しんぶん", &settings(), false).is_empty(),
        "既定の空の誤り辞書では従来どおり"
    );
}

#[test]
fn same_surface_as_the_typed_reading_is_dropped() {
    let engine = FuzzyEngine::new();
    // 元の読み「しんぶ」の最良の表記が「新聞」で、正しい読み「しんぶん」の表記も「新聞」なら出さない
    let dict = dict_with_extra(&format!("{EXTRA}しんぶ\t1\t1\t100\t新聞\n"));
    engine.set_misspellings(misspellings("しんぶ\tしんぶん\n"));
    let out = engine.suggest(
        &dict,
        &Overlay::default(),
        "しんぶ",
        &TypingHint::default(),
        &settings(),
        false,
        Duration::from_millis(150),
    );
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn disabled_or_known_gives_nothing() {
    let engine = FuzzyEngine::new();
    engine.set_misspellings(misspellings("かんし\tがっこう\n"));
    let has_school = |v: Vec<Suggestion>| v.iter().any(|s| s.surface == "学校");
    assert!(has_school(suggest(&engine, "かんし", &settings(), false)));
    let mut off = settings();
    off.misspellings = false;
    assert!(!has_school(suggest(&engine, "かんし", &off, false)));
    let mut disabled = settings();
    disabled.enabled = false;
    assert!(suggest(&engine, "かんし", &disabled, false).is_empty());
    assert!(suggest(&engine, "かんし", &settings(), true).is_empty());
    let mut none = settings();
    none.max_candidates = 0;
    assert!(suggest(&engine, "かんし", &none, false).is_empty());
}

#[test]
fn misspelling_candidates_come_before_the_normal_ones_and_respect_the_limit() {
    let engine = FuzzyEngine::new();
    // 「かんし」→ 通常の補正は濁点の付け忘れで「かんじ」(漢字) を出し得るので、別の語を誤り辞書に置く
    engine.set_misspellings(misspellings("かんし\tがっこう\n"));
    let mut s = settings();
    s.max_candidates = 5;
    let out = suggest(&engine, "かんし", &s, false);
    assert_eq!(out[0].surface, "学校", "{out:?}");
    let mut one = s;
    one.max_candidates = 1;
    assert_eq!(suggest(&engine, "かんし", &one, false).len(), 1);
    // 重複する表記は出ない
    let mut surfaces: Vec<&str> = out.iter().map(|s| s.surface.as_str()).collect();
    surfaces.sort_unstable();
    surfaces.dedup();
    assert_eq!(surfaces.len(), out.len(), "{out:?}");
}

#[test]
fn user_file_comes_before_the_bundled_list() {
    let dir = std::env::temp_dir().join(format!("nagi-misspell-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("misspellings.tsv");
    std::fs::write(&path, "あらたしい\tしんぶん\n").unwrap();
    let (m, issues) = Misspellings::load(Some(&path));
    assert!(issues.is_empty(), "{issues:?}");
    let engine = FuzzyEngine::new();
    engine.set_misspellings(Arc::new(m));
    let out = suggest(&engine, "あらたしい", &settings(), false);
    let readings: Vec<&str> = out.iter().map(|s| s.reading.as_str()).collect();
    assert_eq!(readings, ["しんぶん", "あたらしい"], "{out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pipeline_adds_fuzzy_source_and_honours_the_setting() {
    let build = |enabled: bool, misspell: bool| {
        let mut config = Config::default();
        config.input.fuzzy.enabled = enabled;
        config.input.fuzzy.misspellings = misspell;
        let runtime = Runtime::new(
            SystemInfo {
                total_memory_mb: 16384,
                cpu_cores: 8,
                has_gpu: false,
            },
            false,
            Tunables::from_config(&config),
            Learner::new(LearnConfig::default()),
            None,
        );
        let p = Pipeline::new(
            Some(dict_with_extra(EXTRA)),
            Some(Box::new(FakeModel::new(&[]))),
            runtime,
        );
        p.set_misspellings(misspellings("かんし\tしんぶん\n"));
        p
    };
    use nagi_engine::ConversionBackend;
    let fuzzy_surfaces = |p: &Pipeline| -> Vec<String> {
        p.segment_candidates("かんし", "", 20)
            .into_iter()
            .filter(|c| c.sources.contains(&CandidateSource::Fuzzy))
            .map(|c| c.surface)
            .collect()
    };
    assert_eq!(
        fuzzy_surfaces(&build(true, true))
            .first()
            .map(String::as_str),
        Some("新聞")
    );
    assert!(!fuzzy_surfaces(&build(true, false)).contains(&"新聞".to_string()));
    assert!(fuzzy_surfaces(&build(false, true)).is_empty());
}

/// 同梱のリストの誤った読みが、実辞書の語として完全一致しないこと（誤発動の元になるため）。
/// 辞書が無い環境では何もせず成功する
#[test]
fn bundled_wrong_readings_are_not_dictionary_words() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dict/nagi-dict.bin");
    if !path.exists() {
        eprintln!("skip: {} が無い（cargo xtask build-dict）", path.display());
        return;
    }
    let dict = nagi_dict::Dictionary::load(&path).expect("辞書の読み込み");
    let mut issues = Vec::new();
    let m = Misspellings::builtin(&mut issues);
    assert!(issues.is_empty(), "{issues:?}");
    let mut hits: Vec<&str> = m
        .iter()
        .map(|(wrong, _)| wrong)
        .filter(|wrong| !dict.lookup(wrong).is_empty())
        .collect();
    hits.sort_unstable();
    assert!(hits.is_empty(), "辞書語と完全一致する誤った読み: {hits:?}");
}
