//! 抑制語（`suppressed.tsv`）の統合テスト: 候補・N-best・ライブ変換の best・予測変換からの除外と、
//! プロトコル往復（suppress → 消える → list → unsuppress → 戻る）・再読込。合成辞書で決定的に動かす。

mod common;

use common::{dict, dict_with_extra};
use nagi_config::Config;
use nagi_engine::{ConversionBackend, parse_key};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::setup::{Built, Reloadable};
use nagi_server::suppress::Suppressions;
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nagi-suppress-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn suppressions(tsv: &str) -> Arc<Suppressions> {
    let mut issues = Vec::new();
    let s = Suppressions::parse("t.tsv", tsv, &mut issues);
    assert!(issues.is_empty(), "{issues:?}");
    Arc::new(s)
}

/// 辞書のみ（LLM なし）の Pipeline と Runtime
fn pipeline_with(
    dict: Arc<nagi_dict::Dictionary>,
    user_dir: &Path,
) -> (Arc<Pipeline>, Arc<Runtime>) {
    let config = Config::default();
    let learn_path = user_dir.join("learning.tsv");
    let (learner, _) = Learner::load(&learn_path, LearnConfig::default()).unwrap();
    let runtime = Runtime::new(
        SYSTEM,
        true,
        Tunables::from_config(&config),
        learner,
        Some(learn_path),
    );
    let pipeline = Arc::new(Pipeline::new(Some(dict), None, runtime.clone()));
    (pipeline, runtime)
}

fn surfaces(cands: &[nagi_engine::BackendCandidate]) -> Vec<String> {
    cands.iter().map(|c| c.surface.clone()).collect()
}

#[test]
fn candidates_drop_only_the_registered_reading_and_surface() {
    let dir = temp_dir("filter");
    let extra = "こうえん\t1\t1\t3000\t公園\nこうえい\t1\t1\t3000\t講演\n";
    let (p, _) = pipeline_with(dict_with_extra(extra), &dir);
    let before = surfaces(&p.segment_candidates("こうえん", "", 20));
    assert!(before.contains(&"講演".to_string()), "{before:?}");

    p.set_suppressions(suppressions("こうえん\t講演\n"));
    let after = surfaces(&p.segment_candidates("こうえん", "", 20));
    assert!(!after.contains(&"講演".to_string()), "{after:?}");
    assert!(after.contains(&"公園".to_string()));
    // 同じ表記でも別の読みでは除かれない
    let other = surfaces(&p.segment_candidates("こうえい", "", 20));
    assert!(other.contains(&"講演".to_string()), "{other:?}");
    // 入力中の候補（接頭辞用）も同じ
    let prefix = surfaces(&p.prefix_candidates("こうえん", "", 20));
    assert!(!prefix.contains(&"講演".to_string()), "{prefix:?}");
}

#[test]
fn reading_itself_is_never_suppressed() {
    let dir = temp_dir("kana");
    let (p, _) = pipeline_with(dict(), &dir);
    // 読みそのままの表記は登録しても、候補が空にならないよう残す（登録行自体が不正になる）
    let mut issues = Vec::new();
    let s = Suppressions::parse("t.tsv", "こうえん\tこうえん\n", &mut issues);
    assert_eq!(issues.len(), 1);
    p.set_suppressions(Arc::new(s));
    // 全部の表記を抑制しても、読みそのままは残る
    p.set_suppressions(suppressions(
        "こうえん\t公園\nこうえん\t講演\nこうえん\t後援\n",
    ));
    let after = surfaces(&p.segment_candidates("こうえん", "", 20));
    assert!(after.contains(&"こうえん".to_string()), "{after:?}");
    assert!(!after.contains(&"公園".to_string()));
}

#[test]
fn sentence_n_best_drops_sentences_with_a_suppressed_segment() {
    let dir = temp_dir("nbest");
    let (p, _) = pipeline_with(dict(), &dir);
    let all: Vec<String> = p
        .sentence_candidates("こうえん", "", 10)
        .iter()
        .map(|s| s.segments.iter().map(|g| g.surface.as_str()).collect())
        .collect();
    assert!(all.contains(&"講演".to_string()), "{all:?}");

    p.set_suppressions(suppressions("こうえん\t講演\n"));
    let filtered: Vec<String> = p
        .sentence_candidates("こうえん", "", 10)
        .iter()
        .map(|s| s.segments.iter().map(|g| g.surface.as_str()).collect())
        .collect();
    assert!(!filtered.contains(&"講演".to_string()), "{filtered:?}");
    assert!(filtered.contains(&"公園".to_string()), "{filtered:?}");
}

#[test]
fn live_best_falls_back_to_the_next_candidate() {
    let dir = temp_dir("best");
    let (p, _) = pipeline_with(dict(), &dir);
    let best = |p: &Pipeline| -> String {
        p.convert_sentence("こうえん", "")
            .iter()
            .map(|s| s.surface.as_str())
            .collect()
    };
    assert_eq!(best(&p), "公園");
    p.set_suppressions(suppressions("こうえん\t公園\n"));
    let next = best(&p);
    assert_ne!(next, "公園");
    assert!(!next.is_empty());
}

#[test]
fn live_best_falls_back_to_the_reading_when_everything_is_suppressed() {
    let dir = temp_dir("best-all");
    let (p, _) = pipeline_with(dict(), &dir);
    p.set_suppressions(suppressions(
        "こうえん\t公園\nこうえん\t講演\nこうえん\t後援\n",
    ));
    let best: String = p
        .convert_sentence("こうえん", "")
        .iter()
        .map(|s| s.surface.as_str())
        .collect();
    assert_eq!(best, "こうえん");
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

fn server_in(user_dir: &Path) -> Server {
    let mut config = Config::default();
    config.conversion.initial_view = nagi_config::InitialView::Segment;
    let (pipeline, runtime) = pipeline_with(dict(), user_dir);
    let mut reload = Reloadable::detached();
    reload.suppressed_path = Some(user_dir.join("suppressed.tsv"));
    reload.pipeline = Some(std::panic::AssertUnwindSafe(pipeline.clone()));
    reload.mark_read();
    Server::from_built(Built {
        pipeline,
        runtime,
        config,
        reload,
        notes: Vec::new(),
    })
}

/// 「こうえん」を入力して Space を押したときの候補の表記
fn space_candidates(server: &mut Server) -> Vec<String> {
    send(server, "reset", json!({"session_id":"s"}));
    for c in "kouen".chars() {
        press(server, &c.to_string());
    }
    press(server, "Space");
    let r = press(server, "Space");
    let items = r["candidates"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    items
        .iter()
        .map(|i| i["surface"].as_str().unwrap().to_string())
        .collect()
}

fn init(server: &mut Server) -> Value {
    send(
        server,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
    )
}

#[test]
fn rpc_round_trip_suppress_list_unsuppress() {
    let dir = temp_dir("rpc");
    let mut s = server_in(&dir);
    let init = init(&mut s);
    assert_eq!(init["result"]["capabilities"]["suppression"], true);

    assert!(space_candidates(&mut s).contains(&"講演".to_string()));

    // 登録 → 候補から消える・ファイルに保存される（読みのカタカナはひらがなに直る）
    let r = send(
        &mut s,
        "suppress_candidate",
        json!({"reading":"コウエン","surface":"講演"}),
    );
    assert!(r.get("error").is_none(), "{r}");
    assert!(!space_candidates(&mut s).contains(&"講演".to_string()));
    assert!(space_candidates(&mut s).contains(&"公園".to_string()));
    let saved = std::fs::read_to_string(dir.join("suppressed.tsv")).unwrap();
    assert!(saved.contains("こうえん\t講演"), "{saved}");

    let r = send(&mut s, "list_suppressed", json!({}));
    assert_eq!(
        r["result"]["items"],
        json!([{"reading":"こうえん","surface":"講演"}])
    );

    // 不正な読みはエラー
    let r = send(
        &mut s,
        "suppress_candidate",
        json!({"reading":"abc","surface":"講演"}),
    );
    assert!(r.get("error").is_some());

    // 解除 → 戻る
    let r = send(
        &mut s,
        "unsuppress_candidate",
        json!({"reading":"こうえん","surface":"講演"}),
    );
    assert!(r.get("error").is_none(), "{r}");
    assert!(space_candidates(&mut s).contains(&"講演".to_string()));
    assert_eq!(
        send(&mut s, "list_suppressed", json!({}))["result"]["items"],
        json!([])
    );
}

#[test]
fn file_changes_are_reloaded_with_line_numbered_warnings() {
    let dir = temp_dir("reload");
    let mut s = server_in(&dir);
    init(&mut s);
    assert!(space_candidates(&mut s).contains(&"講演".to_string()));

    std::fs::write(
        dir.join("suppressed.tsv"),
        "# コメント\nこうえん\t講演\nだめな行\n",
    )
    .unwrap();
    let line = json!({"jsonrpc":"2.0","id":1,"method":"reload_config","params":{}}).to_string();
    let lines = s.handle_line(&line);
    let warnings: Vec<Value> = lines
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["params"]["code"] == "suppressed_warning")
        .collect();
    assert_eq!(warnings.len(), 1, "{lines:?}");
    let message = warnings[0]["params"]["message"].as_str().unwrap();
    assert!(
        message.contains("4 行目") || message.contains("3 行目"),
        "{message}"
    );
    assert!(!message.contains("だめな行"), "登録内容は通知に出さない");
    assert!(!space_candidates(&mut s).contains(&"講演".to_string()));

    // ファイルを消すと全部戻る
    std::fs::remove_file(dir.join("suppressed.tsv")).unwrap();
    send(&mut s, "reload_config", json!({}));
    assert!(space_candidates(&mut s).contains(&"講演".to_string()));
}
