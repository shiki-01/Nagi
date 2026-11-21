//! 前方一致の予測（Tab）の学習キャッシュ由来の候補は、学習を止める状況（secure input・停止アプリ・学習の無効）
//! では出さない（設計書 §4.11）。ユーザー辞書の前方一致は利用者が登録したものなので影響しない。

mod common;

use common::dict;
use nagi_config::Config;
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

fn pipeline(config: &Config) -> (Arc<Pipeline>, Arc<Runtime>) {
    let path = std::env::temp_dir().join(format!("nagi-predict-secure-{}.tsv", std::process::id()));
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
    (pipeline, runtime)
}

fn ctx<'a>(secure_input: bool, app_id: Option<&'a str>) -> CommitContext<'a> {
    CommitContext {
        session_id: "s",
        app_id,
        secure_input,
    }
}

fn learned(p: &Pipeline, ctx: &CommitContext<'_>) -> Vec<String> {
    p.predict(ctx, "こう", "", 10)
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

#[test]
fn learned_predictions_are_hidden_while_learning_is_stopped() {
    let mut config = Config::default();
    config.learning.disabled_apps = vec!["com.example.vault".to_string()];
    let (p, rt) = pipeline(&config);
    rt.learn_record("こうえん", "公演会場", context_hash(""));

    // 通常は学習した語が出る
    assert!(learned(&p, &ctx(false, Some("com.example.editor"))).contains(&"公演会場".to_string()));
    assert!(learned(&p, &ctx(false, None)).contains(&"公演会場".to_string()));
    // secure input 中・停止アプリでは出ない
    assert!(learned(&p, &ctx(true, Some("com.example.editor"))).is_empty());
    assert!(learned(&p, &ctx(false, Some("com.example.vault"))).is_empty());
}

#[test]
fn learned_predictions_are_hidden_when_learning_is_disabled() {
    let mut config = Config::default();
    config.learning.enabled = false;
    let (p, rt) = pipeline(&config);
    rt.learn_record("こうえん", "公演会場", context_hash(""));
    assert!(learned(&p, &ctx(false, None)).is_empty());
}
