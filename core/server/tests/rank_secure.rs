//! 通常の変換候補の順位付け（学習済みの語の優先・学習キャッシュ供給元の候補）も、学習を止める状況
//! （secure input・停止アプリ・学習の無効）では使わない（設計書 §4.11）。ユーザー辞書・システム辞書は影響しない。

mod common;

use common::dict;
use nagi_config::Config;
use nagi_engine::ir::CandidateSource;
use nagi_engine::{CommitContext, ConversionBackend};
use nagi_learn::{LearnConfig, Learner, context_hash};
use nagi_profile::SystemInfo;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::sync::Arc;

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

const READING: &str = "こうえん";
const LEARNED: &str = "公演会場";

fn pipeline(config: &Config, tag: &str) -> (Arc<Pipeline>, Arc<Runtime>) {
    let path =
        std::env::temp_dir().join(format!("nagi-rank-secure-{tag}-{}.tsv", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let (learner, _) = Learner::load(&path, LearnConfig::default()).unwrap();
    let runtime = Runtime::new(
        SYSTEM,
        true,
        Tunables::from_config(config),
        learner,
        Some(path),
    );
    let pipeline = Arc::new(Pipeline::new(Some(dict()), None, runtime.clone()));
    for _ in 0..5 {
        runtime.learn_record(READING, LEARNED, context_hash(""));
    }
    (pipeline, runtime)
}

fn enter(p: &Pipeline, secure_input: bool, app_id: Option<&str>) {
    p.set_session_context(&CommitContext {
        session_id: "s",
        app_id,
        secure_input,
    });
}

fn candidates(p: &Pipeline) -> Vec<String> {
    p.segment_candidates(READING, "", 20)
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

fn sentence(p: &Pipeline) -> String {
    p.convert_sentence(READING, "")
        .into_iter()
        .map(|s| s.surface)
        .collect()
}

#[test]
fn learned_ranking_applies_normally() {
    let (p, _rt) = pipeline(&Config::default(), "normal");
    enter(&p, false, Some("com.example.editor"));
    assert_eq!(candidates(&p).first().map(String::as_str), Some(LEARNED));
    assert_eq!(sentence(&p), LEARNED);
    let c = p.segment_candidates(READING, "", 20);
    assert!(c[0].sources.contains(&CandidateSource::Learned));
}

#[test]
fn learned_ranking_is_off_in_secure_input() {
    let (p, _rt) = pipeline(&Config::default(), "secure");
    enter(&p, true, Some("com.example.editor"));
    assert!(!candidates(&p).contains(&LEARNED.to_string()));
    assert_ne!(sentence(&p), LEARNED);
    // 通常の状況に戻れば再び効く
    enter(&p, false, Some("com.example.editor"));
    assert_eq!(candidates(&p).first().map(String::as_str), Some(LEARNED));
}

#[test]
fn learned_ranking_is_off_in_disabled_app() {
    let mut config = Config::default();
    config.learning.disabled_apps = vec!["com.example.vault".to_string()];
    let (p, _rt) = pipeline(&config, "vault");
    enter(&p, false, Some("com.example.vault"));
    assert!(!candidates(&p).contains(&LEARNED.to_string()));
    assert_ne!(sentence(&p), LEARNED);
    enter(&p, false, Some("com.example.editor"));
    assert_eq!(candidates(&p).first().map(String::as_str), Some(LEARNED));
}

#[test]
fn system_dictionary_candidates_remain_in_secure_input() {
    let (p, _rt) = pipeline(&Config::default(), "sysdict");
    enter(&p, false, None);
    let normal = candidates(&p);
    enter(&p, true, None);
    let secure = candidates(&p);
    assert!(!secure.is_empty());
    // 学習の候補を除いた辞書の候補は変わらない
    let expected: Vec<String> = normal.into_iter().filter(|s| s != LEARNED).collect();
    let got: Vec<String> = secure.into_iter().filter(|s| s != LEARNED).collect();
    assert_eq!(got, expected);
}
