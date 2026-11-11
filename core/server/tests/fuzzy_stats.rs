//! 曖昧入力の「もしかして」候補の採用状況（`fuzzy_stats.json`）の統合テスト: エンジン経由の計上、
//! 二重計上の防止、個人化オフ・secure input での停止、全削除、保存、プロトコル往復。
//! 合成辞書で決定的に動かす。

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_engine::{Engine, EngineConfig, parse_key};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::methods::SetContextParams;
use nagi_server::fuzzy_stats::FILE_NAME;
use nagi_server::learning::LearningObserver;
use nagi_server::setup::{Built, Reloadable};
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

const EXTRA: &str = "\
かんじ\t1\t1\t3000\t感じ
かんじ\t1\t1\t3500\t漢字
かじ\t1\t1\t3200\t火事
";

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("nagi-fuzzy-stats-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(personalize: bool) -> Config {
    let mut config = Config::default();
    config.input.fuzzy.enabled = true;
    config.input.fuzzy.personalize = personalize;
    config
}

fn parts(config: &Config) -> (Arc<Pipeline>, Arc<Runtime>) {
    let runtime = Runtime::new(
        SYSTEM,
        false,
        Tunables::from_config(config),
        Learner::new(LearnConfig::default()),
        None,
    );
    let pipeline = Arc::new(Pipeline::new(
        Some(dict_with_extra(EXTRA)),
        Some(Box::new(FakeModel::new(&[]))),
        runtime.clone(),
    ));
    (pipeline, runtime)
}

fn engine(config: &Config) -> (Engine, Arc<Runtime>) {
    let (pipeline, runtime) = parts(config);
    let mut e = Engine::new(
        pipeline,
        EngineConfig {
            fuzzy_input: true,
            ..EngineConfig::default()
        },
    );
    e.set_commit_observer(Arc::new(LearningObserver::new(runtime.clone())));
    (e, runtime)
}

fn type_romaji(e: &mut Engine, romaji: &str) {
    for c in romaji.chars() {
        e.process_key("s", &parse_key(&c.to_string()).unwrap());
    }
}

/// 種類 → (shown, adopted)
fn counts(runtime: &Runtime) -> Vec<(String, u32, u32)> {
    runtime
        .fuzzy_stats()
        .snapshot()
        .into_iter()
        .map(|s| (s.kind, s.shown, s.adopted))
        .collect()
}

fn get(runtime: &Runtime, kind: &str) -> (u32, u32) {
    counts(runtime)
        .into_iter()
        .find(|(k, _, _)| k == kind)
        .map_or((0, 0), |(_, s, a)| (s, a))
}

/// 入力中の候補を作り直して返す（バックグラウンドの再計算の代わり）
fn refresh(e: &mut Engine) -> Vec<nagi_protocol::view::CandidateView> {
    let reading = e.composing_candidate_reading("s").expect("読み");
    e.refresh_composing_candidates("s", &reading)
        .and_then(|r| r.candidates)
        .map(|c| c.items)
        .unwrap_or_default()
}

#[test]
fn shown_is_counted_once_per_episode_even_when_candidates_are_rebuilt() {
    let (mut e, runtime) = engine(&config(true));
    type_romaji(&mut e, "kamji");
    let reading = e.composing_candidate_reading("s").expect("読み");
    refresh(&mut e);
    let first = counts(&runtime);
    // 感じ = m→n の置換、火事 = m の削除（挿入）
    assert_eq!(get(&runtime, "substitute").0, 1, "{first:?}");
    assert_eq!(get(&runtime, "insert").0, 1, "{first:?}");
    // 作り直し・Space 変換を重ねても、同じ補正は数え直さない
    e.refresh_composing_candidates("s", &reading);
    e.process_key("s", &parse_key("Space").unwrap());
    e.process_key("s", &parse_key("Down").unwrap());
    assert_eq!(counts(&runtime), first);
    assert!(counts(&runtime).iter().all(|(_, _, adopted)| *adopted == 0));
}

#[test]
fn a_new_episode_counts_again() {
    let (mut e, runtime) = engine(&config(true));
    type_romaji(&mut e, "kamji");
    refresh(&mut e);
    // 入力を破棄（Esc）してから同じ入力をやり直す
    e.process_key("s", &parse_key("Escape").unwrap());
    e.process_key("s", &parse_key("Escape").unwrap());
    type_romaji(&mut e, "kamji");
    refresh(&mut e);
    assert_eq!(get(&runtime, "substitute").0, 2, "{:?}", counts(&runtime));
}

#[test]
fn adopting_a_candidate_in_space_conversion_increments_adopted() {
    let (mut e, runtime) = engine(&config(true));
    type_romaji(&mut e, "kamji");
    let r = e.process_key("s", &parse_key("Space").unwrap());
    let items = r.candidates.unwrap().items;
    let index = items
        .iter()
        .position(|i| i.surface == "火事")
        .expect("火事");
    e.select_candidate("s", index as u32);
    let r = e.process_key("s", &parse_key("Enter").unwrap());
    assert_eq!(r.commit.as_deref(), Some("火事"));
    assert_eq!(get(&runtime, "insert"), (1, 1), "{:?}", counts(&runtime));
    // 選ばれなかった種類は提示だけ
    assert_eq!(
        get(&runtime, "substitute"),
        (1, 0),
        "{:?}",
        counts(&runtime)
    );
}

#[test]
fn adopting_a_composing_candidate_increments_adopted() {
    let (mut e, runtime) = engine(&config(true));
    type_romaji(&mut e, "kamji");
    let items = refresh(&mut e);
    let index = items
        .iter()
        .position(|i| i.surface == "火事")
        .expect("火事");
    let r = e.select_candidate("s", index as u32);
    assert_eq!(r.commit.as_deref(), Some("火事"));
    assert_eq!(get(&runtime, "insert"), (1, 1), "{:?}", counts(&runtime));
    assert_eq!(get(&runtime, "substitute"), (1, 0));
}

#[test]
fn personalize_off_records_nothing() {
    let (mut e, runtime) = engine(&config(false));
    type_romaji(&mut e, "kamji");
    let items = refresh(&mut e);
    let index = items
        .iter()
        .position(|i| i.surface == "火事")
        .expect("火事");
    e.select_candidate("s", index as u32);
    assert!(counts(&runtime).is_empty(), "{:?}", counts(&runtime));
}

#[test]
fn secure_input_is_not_counted() {
    let (mut e, runtime) = engine(&config(true));
    e.set_context(
        "s",
        &SetContextParams {
            session_id: "s".into(),
            surrounding_text: None,
            app_id: None,
            secure_input: Some(true),
        },
    );
    type_romaji(&mut e, "kamji");
    let items = refresh(&mut e);
    let index = items
        .iter()
        .position(|i| i.surface == "火事")
        .expect("火事");
    e.select_candidate("s", index as u32);
    assert!(counts(&runtime).is_empty(), "{:?}", counts(&runtime));
}

#[test]
fn learning_disabled_is_not_counted() {
    let mut c = config(true);
    c.learning.enabled = false;
    let (mut e, runtime) = engine(&c);
    type_romaji(&mut e, "kamji");
    refresh(&mut e);
    assert!(counts(&runtime).is_empty(), "{:?}", counts(&runtime));
}

#[test]
fn flush_writes_counts_only_and_learn_clear_zeroes_and_removes_the_file() {
    let dir = temp_dir("flush");
    let (mut e, runtime) = engine(&config(true));
    runtime.set_fuzzy_stats_path(Some(dir.join(FILE_NAME)));
    type_romaji(&mut e, "kamji");
    refresh(&mut e);
    runtime.flush_learning(true);
    let text = std::fs::read_to_string(dir.join(FILE_NAME)).expect("保存される");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["version"], 1);
    assert_eq!(v["kinds"]["substitute"]["shown"], 1);
    assert_eq!(v["kinds"]["substitute"]["adopted"], 0);
    // 入力した読み・表記は含まれない
    for s in ["かmじ", "かんじ", "感じ", "火事"] {
        assert!(!text.contains(s), "{text}");
    }

    // 新しい Runtime が読み込める
    let (_, runtime2) = engine(&config(true));
    runtime2.set_fuzzy_stats_path(Some(dir.join(FILE_NAME)));
    assert_eq!(get(&runtime2, "substitute"), (1, 0));

    runtime.learn_clear();
    assert!(counts(&runtime).is_empty());
    assert!(!dir.join(FILE_NAME).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- プロトコル往復 ----

fn send(server: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let mut lines = server.handle_line(&line).into_iter();
    serde_json::from_str(&lines.next().expect("応答")).unwrap()
}

fn press(server: &mut Server, key: &str) -> Value {
    let k = parse_key(key).unwrap();
    send(
        server,
        "process_key",
        json!({"session_id":"s","keysym":k.keysym,"keycode":0,"text":k.text,
               "modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},
               "kind":"down"}),
    )["result"]
        .clone()
}

#[test]
fn rpc_get_fuzzy_stats_round_trip() {
    let config = config(true);
    let (pipeline, runtime) = parts(&config);
    let mut reload = Reloadable::detached();
    reload.pipeline = Some(std::panic::AssertUnwindSafe(pipeline.clone()));
    reload.mark_read();
    let mut s = Server::from_built(Built {
        pipeline,
        runtime: runtime.clone(),
        config,
        reload,
        notes: Vec::new(),
    });
    let init = send(
        &mut s,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
    );
    assert_eq!(init["result"]["capabilities"]["fuzzy_stats"], true);

    // 記録が無ければ空
    let r = send(&mut s, "get_fuzzy_stats", json!({}));
    assert_eq!(r["result"]["items"], json!([]), "{r}");

    for c in "kamji".chars() {
        press(&mut s, &c.to_string());
    }
    let r = press(&mut s, "Space");
    let items = r["candidates"]["items"].as_array().cloned().unwrap();
    let index = items.iter().position(|i| i["surface"] == "火事").unwrap();
    send(
        &mut s,
        "select_candidate",
        json!({"session_id":"s","index":index}),
    );
    press(&mut s, "Enter");

    let r = send(&mut s, "get_fuzzy_stats", json!({}));
    let items = r["result"]["items"].as_array().cloned().unwrap();
    let insert = items
        .iter()
        .find(|i| i["kind"] == "insert")
        .expect("insert");
    assert_eq!(insert["shown"], 1);
    assert_eq!(insert["adopted"], 1);
    // データ不足なので倍率は 1.0
    assert_eq!(insert["margin_scale"], 1.0);

    // 全削除でゼロ
    send(&mut s, "clear_learning", json!({}));
    let r = send(&mut s, "get_fuzzy_stats", json!({}));
    assert_eq!(r["result"]["items"], json!([]), "{r}");
}

// ---- マージンの倍率 ----

mod margin {
    use super::*;
    use nagi_dict::Overlay;
    use nagi_engine::TypingHint;
    use nagi_server::fuzzy::{FuzzyEngine, FuzzySettings};
    use nagi_server::fuzzy_stats::FuzzyStats;
    use std::time::Duration;

    const EXTRA: &str = "\
かん\t1\t1\t3000\t感
し\t1\t1\t3000\t氏
かんじ\t1\t1\t3000\t漢字
";

    fn found(engine: &FuzzyEngine, settings: &FuzzySettings) -> bool {
        let dict = dict_with_extra(EXTRA);
        engine
            .suggest(
                &dict,
                &Overlay::default(),
                "かんし",
                &TypingHint::default(),
                settings,
                false,
                Duration::from_millis(150),
            )
            .iter()
            .any(|s| s.reading == "かんじ")
    }

    fn stats_with(kind: &str, shown: u32, adopted: u32) -> Arc<FuzzyStats> {
        let stats = Arc::new(FuzzyStats::new());
        for _ in 0..shown {
            stats.record_shown(kind);
        }
        for _ in 0..adopted {
            stats.record_adopted(kind);
        }
        stats
    }

    fn settings(personalize: bool, margin_scale: f32) -> FuzzySettings {
        FuzzySettings {
            enabled: true,
            misspellings: false,
            no_time_limit: true,
            personalize,
            margin_scale,
            ..FuzzySettings::default()
        }
    }

    #[test]
    fn per_kind_scale_multiplies_the_covered_margin() {
        let engine = FuzzyEngine::new();
        // 統計が空なら倍率 1.0。濁点の補正が通る最大のマージン倍率を探して、そこを基準にする
        let mut edge = None;
        for i in 1..400 {
            let m = i as f32 * 0.05;
            if found(&engine, &settings(true, m)) {
                edge = Some(m);
            } else if edge.is_some() {
                break;
            }
        }
        let edge = edge.expect("通る倍率がある");
        // `edge` は通る最大の刻み、その次の刻みは通らない
        let inside = edge;
        let outside = edge + 0.05;
        assert!(found(&engine, &settings(true, inside)));
        assert!(!found(&engine, &settings(true, outside)));

        // 濁点がよく採用される → 倍率 0.6 → 外側でも通る
        engine.set_stats(stats_with("voicing", 100, 100));
        assert!(found(&engine, &settings(true, outside)));
        // 個人化オフなら倍率は使われない
        assert!(!found(&engine, &settings(false, outside)));
        // ほとんど採用されない → 倍率 1.6 → 内側でも通らない（境界が 1.6 倍に縮む）
        engine.set_stats(stats_with("voicing", 1000, 0));
        assert!(!found(&engine, &settings(true, inside)));
        // 別の種類の統計は影響しない
        engine.set_stats(stats_with("insert", 100, 100));
        assert!(!found(&engine, &settings(true, outside)));
    }
}
