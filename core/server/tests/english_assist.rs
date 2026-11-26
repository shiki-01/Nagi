//! 英字入力の予測候補・スペル訂正（H-146）: `english_suggest` / `english_commit` の JSON-RPC と、
//! デスクトップの英字の訂正候補（`input.english.literal_correction`）

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_english::{Lexicon, LexiconBuilder};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::setup::{Built, Reloadable};
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::sync::Arc;

/// `core/english/tests/fixtures/words.txt`（自作の小さな語彙）
fn fixture_lexicon() -> Arc<Lexicon> {
    let text = include_str!("../../english/tests/fixtures/words.txt");
    let mut b = LexiconBuilder::new();
    for l in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let (w, t) = l.split_once('\t').unwrap();
        b.add(w, t.parse().unwrap());
    }
    Arc::new(Lexicon::from_vec(b.build().unwrap()).unwrap())
}

fn server_with(config: Config) -> (Server, Arc<Runtime>) {
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
    let lexicon = fixture_lexicon();
    runtime.set_english_lexicon(Some(lexicon.clone()));
    let mut pipeline = Pipeline::new(
        Some(dict_with_extra("")),
        Some(Box::new(FakeModel::new(&[]))),
        runtime.clone(),
    );
    pipeline.set_english_lexicon(Some(lexicon));
    let mut s = Server::from_built(Built {
        pipeline: Arc::new(pipeline),
        runtime: runtime.clone(),
        config,
        reload: Reloadable::detached(),
        notes: Vec::new(),
    });
    s.handle_line(
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"ios"}})
        .to_string(),
    );
    (s, runtime)
}

fn call(s: &mut Server, id: i64, method: &str, params: Value) -> Value {
    let lines = s
        .handle_line(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string());
    let v: Value = serde_json::from_str(&lines[0]).unwrap();
    assert!(v.get("error").is_none(), "{v}");
    v["result"].clone()
}

fn texts(result: &Value) -> Vec<String> {
    result["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["text"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn suggest_returns_typed_corrections_and_autocorrect() {
    let (mut s, _) = server_with(Config::default());
    let r = call(&mut s, 2, "english_suggest", json!({"word":"recieve"}));
    let t = texts(&r);
    assert_eq!(t[0], "recieve");
    assert_eq!(t[1], "receive");
    assert_eq!(r["suggestions"][0]["kind"], "typed");
    assert_eq!(r["suggestions"][1]["kind"], "correction");
    assert_eq!(r["autocorrect"], "receive");

    // 正しい語は直さず、補完を出す
    let r = call(
        &mut s,
        3,
        "english_suggest",
        json!({"word":"Comp","limit":3}),
    );
    assert!(r.get("autocorrect").is_none(), "{r}");
    let t = texts(&r);
    assert_eq!(t[0], "Comp");
    assert!(t.len() <= 3);
    assert!(t.contains(&"Computer".to_string()), "{t:?}");

    // 修正表（I・短縮形）
    let r = call(&mut s, 4, "english_suggest", json!({"word":"i"}));
    assert_eq!(r["autocorrect"], "I");
    // 空の語は空
    let r = call(&mut s, 5, "english_suggest", json!({"word":""}));
    assert!(texts(&r).is_empty());
}

#[test]
fn commit_learns_words_and_stops_autocorrect() {
    let (mut s, runtime) = server_with(Config::default());
    // 1 回だけでは打ち間違いかもしれないので、まだ直す対象
    call(&mut s, 2, "english_commit", json!({"word":"recieve"}));
    let r = call(&mut s, 3, "english_suggest", json!({"word":"recieve"}));
    assert_eq!(r["autocorrect"], "receive");
    // 自動修正を取り消したら、正しい語として扱う
    call(
        &mut s,
        4,
        "english_commit",
        json!({"word":"recieve","source":"reverted"}),
    );
    let r = call(&mut s, 5, "english_suggest", json!({"word":"recieve"}));
    assert!(r.get("autocorrect").is_none(), "{r}");
    // 学習した語は補完に出る
    call(
        &mut s,
        6,
        "english_commit",
        json!({"word":"kubernetes","source":"suggestion"}),
    );
    let r = call(&mut s, 7, "english_suggest", json!({"word":"kube"}));
    assert_eq!(texts(&r)[1], "kubernetes");
    assert_eq!(runtime.english_words_len(), 2);

    // 学習データの全削除で消える
    call(&mut s, 8, "clear_learning", json!({}));
    assert_eq!(runtime.english_words_len(), 0);
}

#[test]
fn commit_respects_learning_switches() {
    let mut config = Config::default();
    config.input.english.learn = false;
    let (mut s, runtime) = server_with(config);
    call(&mut s, 2, "english_commit", json!({"word":"kubernetes"}));
    assert_eq!(runtime.english_words_len(), 0);

    let mut config = Config::default();
    config.learning.enabled = false;
    let (mut s, runtime) = server_with(config);
    call(&mut s, 2, "english_commit", json!({"word":"kubernetes"}));
    assert_eq!(runtime.english_words_len(), 0);

    // secure input のセッションでは学習しない
    let (mut s, runtime) = server_with(Config::default());
    call(
        &mut s,
        2,
        "set_context",
        json!({"session_id":"p","secure_input":true}),
    );
    call(
        &mut s,
        3,
        "english_commit",
        json!({"session_id":"p","word":"hunter"}),
    );
    assert_eq!(runtime.english_words_len(), 0);
}

/// `process_key` で Shift+英字（Literal）を打って Space を押し、候補の表記を返す
fn literal_space_candidates(s: &mut Server, word: &str) -> Vec<String> {
    let mut id = 100;
    for ch in word.chars() {
        let upper = ch.is_ascii_uppercase();
        call(
            s,
            id,
            "process_key",
            json!({"session_id":"d",
                "keysym": ch.to_ascii_lowercase() as u32, "keycode": 0, "text": ch.to_string(),
                "modifiers": {"shift": upper}, "kind":"down"}),
        );
        id += 1;
    }
    let r = call(
        s,
        id,
        "process_key",
        json!({"session_id":"d",
            "keysym": 0x20, "keycode": 0, "text": " ", "modifiers": {}, "kind":"down"}),
    );
    r["candidates"]["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|i| i["surface"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn desktop_literal_correction_is_opt_in() {
    let (mut s, _) = server_with(Config::default());
    let off = literal_space_candidates(&mut s, "Recieve");
    assert!(!off.contains(&"Receive".to_string()), "{off:?}");

    let mut config = Config::default();
    config.input.english.literal_correction = true;
    let (mut s, _) = server_with(config);
    let on = literal_space_candidates(&mut s, "Recieve");
    assert_eq!(on.first().map(String::as_str), Some("Recieve"), "{on:?}");
    assert!(on.contains(&"Receive".to_string()), "{on:?}");
}
