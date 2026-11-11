//! 確定後の遡及再評価（`input.fuzzy.retro_review`）の統合テスト: 設定オフでは働かない、
//! 直前の確定 + 続きの読みで候補が出る、同じ確定に 1 回だけ、各種の破棄、secure input、
//! `accept_retro_review` による置換テキストと学習、プロトコルの往復。合成辞書で決定的に動かす。

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_engine::{Engine, EngineConfig, parse_key};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::methods::{SetContextParams, SurroundingText, TouchAction};
use nagi_server::learning::LearningObserver;
use nagi_server::setup::{Built, Reloadable};
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

// かくせい（覚醒）の濁点の付け忘れとして、がくせい（学生）が「する」との結合で良くなる
const EXTRA: &str = "\
かくせい\t1\t1\t7000\t覚醒
がくせい\t1\t1\t2500\t学生
する\t3\t3\t2000\tする
";

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

fn config(retro: bool) -> Config {
    let mut config = Config::default();
    config.input.fuzzy.enabled = true;
    config.input.fuzzy.retro_review = retro;
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
            retro_review: config.input.fuzzy.enabled && config.input.fuzzy.retro_review,
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

/// 「かくせい」を変換して確定する（表記は 覚醒）
fn commit_kakusei(e: &mut Engine) {
    type_romaji(e, "kakusei");
    e.process_key("s", &parse_key("Space").unwrap());
    let r = e.process_key("s", &parse_key("Enter").unwrap());
    assert_eq!(r.commit.as_deref(), Some("覚醒"));
}

fn context(secure: Option<bool>, text: Option<(&str, u32)>) -> SetContextParams {
    SetContextParams {
        session_id: "s".into(),
        surrounding_text: text.map(|(text, cursor)| SurroundingText {
            text: text.into(),
            cursor,
        }),
        app_id: None,
        secure_input: secure,
    }
}

#[test]
fn off_by_setting_nothing_is_recorded_or_asked() {
    let (mut e, _) = engine(&config(false));
    commit_kakusei(&mut e);
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);
    assert!(e.retro_review("s", "する").is_none());
}

#[test]
fn off_by_setting_no_notification_over_rpc() {
    let config = config(false);
    let (pipeline, runtime) = parts(&config);
    let mut s = server(pipeline, runtime, config);
    commit_kakusei_rpc(&mut s);
    for c in "suru".chars() {
        press(&mut s, &c.to_string());
    }
    std::thread::sleep(Duration::from_millis(400));
    assert!(retro_notification(&mut s, Duration::from_millis(100)).is_none());
}

#[test]
fn candidates_appear_after_a_commit_and_a_following_reading() {
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    // 1 文字ではまだ始めない
    type_romaji(&mut e, "s");
    type_romaji(&mut e, "u");
    assert_eq!(e.retro_review_reading("s"), None);
    type_romaji(&mut e, "ru");
    assert_eq!(e.retro_review_reading("s").as_deref(), Some("する"));
    let p = e.retro_review("s", "する").expect("候補");
    assert_eq!(p.original_surface, "覚醒");
    assert_eq!(p.replace_len_utf16, 2);
    assert_eq!(p.candidates.len(), 1);
    assert_eq!(p.candidates[0].reading, "がくせい");
    assert_eq!(p.candidates[0].surface, "学生");
}

#[test]
fn asked_only_once_per_commit() {
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    type_romaji(&mut e, "suru");
    assert!(e.retro_review("s", "する").is_some());
    // 同じ確定に対しては、続きを打っても再び問い合わせない
    assert_eq!(e.retro_review_reading("s"), None);
    type_romaji(&mut e, "ru");
    assert_eq!(e.retro_review_reading("s"), None);
    assert!(e.retro_review("s", "するる").is_none());
}

#[test]
fn an_empty_result_is_not_recomputed() {
    let (mut e, _) = engine(&config(true));
    // 正しい語の確定の後（補正の根拠が無い）
    type_romaji(&mut e, "gakusei");
    e.process_key("s", &parse_key("Space").unwrap());
    e.process_key("s", &parse_key("Enter").unwrap());
    type_romaji(&mut e, "nuno");
    assert_eq!(e.retro_review_reading("s").as_deref(), Some("ぬの"));
    assert!(e.retro_review("s", "ぬの").is_none());
    assert_eq!(e.retro_review_reading("s"), None);
}

#[test]
fn stale_reading_is_ignored() {
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    type_romaji(&mut e, "suru");
    // 実行時点の入力と読みが違えば何もしない（問い合わせも消費しない）
    assert!(e.retro_review("s", "す").is_none());
    assert_eq!(e.retro_review_reading("s").as_deref(), Some("する"));
}

#[test]
fn discarded_by_undo_commit() {
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    let r = e.perform_action("s", &TouchAction::UndoCommit);
    assert!(r.delete_before.is_some());
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);
}

#[test]
fn discarded_by_cursor_move() {
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    // カーソルの手前が確定した表記で終わらない（移動した）
    e.set_context("s", &context(None, Some(("覚醒あ", 3))));
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);

    // 手前が確定した表記で終わっていれば保つ
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    e.set_context("s", &context(None, Some(("こんにちは覚醒", 7))));
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s").as_deref(), Some("する"));
}

#[test]
fn discarded_by_a_key_passed_to_the_app() {
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    // 未入力での矢印キーはアプリに渡る（カーソル移動）
    let r = e.process_key("s", &parse_key("Left").unwrap());
    assert!(!r.consumed);
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);
}

#[test]
fn discarded_by_focus_out_reset_and_next_commit() {
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    e.focus_out("s");
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);

    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    e.reset("s");
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);

    // 次の確定（ひらがなのまま確定）で古い記録は捨てられる
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    type_romaji(&mut e, "nuno");
    e.process_key("s", &parse_key("Enter").unwrap());
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);
}

#[test]
fn not_active_under_secure_input() {
    // 確定の前から secure input
    let (mut e, _) = engine(&config(true));
    e.set_context("s", &context(Some(true), None));
    commit_kakusei(&mut e);
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);

    // 確定の後に secure input になれば破棄する
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    e.set_context("s", &context(Some(true), None));
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);
}

#[test]
fn not_active_when_learning_is_off() {
    // 学習を止める状況（学習無効・停止アプリ・secure input）では確定を保持しない
    let mut c = config(true);
    c.learning.enabled = false;
    let (mut e, _) = engine(&c);
    commit_kakusei(&mut e);
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s"), None);
}

#[test]
fn clean_pair_has_no_candidate() {
    // 正しく確定した語（学生）の後に「する」: 補正の根拠が無い
    let (mut e, _) = engine(&config(true));
    type_romaji(&mut e, "gakusei");
    e.process_key("s", &parse_key("Space").unwrap());
    let r = e.process_key("s", &parse_key("Enter").unwrap());
    assert_eq!(r.commit.as_deref(), Some("学生"));
    type_romaji(&mut e, "suru");
    assert_eq!(e.retro_review_reading("s").as_deref(), Some("する"));
    assert!(e.retro_review("s", "する").is_none());
}

#[test]
fn known_reading_is_left_alone() {
    // 覚醒を過去にも確定している（利用者にとって意味のある語）なら提案しない
    let (mut e, _) = engine(&config(true));
    commit_kakusei(&mut e);
    commit_kakusei(&mut e);
    type_romaji(&mut e, "suru");
    assert!(e.retro_review("s", "する").is_none());
}

#[test]
fn accept_returns_replacement_and_feeds_learning() {
    let (mut e, runtime) = engine(&config(true));
    commit_kakusei(&mut e);
    type_romaji(&mut e, "suru");
    e.retro_review("s", "する").expect("候補");

    // 範囲外は何もしない（候補は残る）
    let none = e.accept_retro_review("s", 5);
    assert!(none.text.is_empty());
    assert_eq!(none.replace_len_utf16, 0);

    let r = e.accept_retro_review("s", 0);
    assert_eq!(r.text, "学生");
    assert_eq!(r.replace_len_utf16, 2);
    // 補正後の (読み, 表記) が学習に載る
    let learned = runtime.learn_lookup("がくせい", 0);
    assert!(learned.iter().any(|(s, _)| s == "学生"), "{learned:?}");
    // 採用状況（種類ごとの件数）にも計上される
    let stats = runtime.fuzzy_stats().snapshot();
    let voicing = stats.iter().find(|s| s.kind == "voicing").expect("voicing");
    assert_eq!((voicing.shown, voicing.adopted), (1, 1), "{stats:?}");
    // 二度目は無効
    assert!(e.accept_retro_review("s", 0).text.is_empty());
}

#[test]
fn accept_without_an_offer_is_empty() {
    let (mut e, _) = engine(&config(true));
    let r = e.accept_retro_review("s", 0);
    assert!(r.text.is_empty());
}

// ---- プロトコル往復 ----

fn server(pipeline: Arc<Pipeline>, runtime: Arc<Runtime>, config: Config) -> Server {
    let mut reload = Reloadable::detached();
    reload.pipeline = Some(std::panic::AssertUnwindSafe(pipeline.clone()));
    reload.mark_read();
    let mut s = Server::from_built(Built {
        pipeline,
        runtime,
        config,
        reload,
        notes: Vec::new(),
    });
    let init = send(
        &mut s,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
    );
    assert_eq!(init["result"]["capabilities"]["retro_review"], true);
    s
}

fn send(server: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let lines = server.handle_line(&line);
    // 応答は `id` を持つ行（通知は `method` を持つ）
    lines
        .into_iter()
        .map(|l| serde_json::from_str::<Value>(&l).unwrap())
        .find(|v| v.get("id").is_some_and(|id| !id.is_null()))
        .expect("応答")
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

fn commit_kakusei_rpc(s: &mut Server) {
    for c in "kakusei".chars() {
        press(s, &c.to_string());
    }
    press(s, "Space");
    let r = press(s, "Enter");
    assert_eq!(r["commit"], "覚醒");
}

/// `retro_review` 通知を待つ（バックグラウンドのスレッドが積む）
fn retro_notification(server: &mut Server, timeout: Duration) -> Option<Value> {
    let until = Instant::now() + timeout;
    loop {
        for line in server.poll_notifications() {
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["method"] == "retro_review" {
                return Some(v["params"].clone());
            }
        }
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn rpc_notification_and_accept_round_trip() {
    let config = config(true);
    let (pipeline, runtime) = parts(&config);
    let mut s = server(pipeline, runtime.clone(), config);
    commit_kakusei_rpc(&mut s);
    for c in "suru".chars() {
        press(&mut s, &c.to_string());
    }
    let p = retro_notification(&mut s, Duration::from_secs(5)).expect("retro_review 通知");
    assert_eq!(p["session_id"], "s");
    assert_eq!(p["original_surface"], "覚醒");
    assert_eq!(p["replace_len_utf16"], 2);
    assert_eq!(p["candidates"][0]["reading"], "がくせい");
    assert_eq!(p["candidates"][0]["surface"], "学生");
    assert!(p["revision"].is_u64());
    // 同じ確定に対して 2 回目は来ない
    press(&mut s, "a");
    assert!(retro_notification(&mut s, Duration::from_millis(300)).is_none());

    let r = send(
        &mut s,
        "accept_retro_review",
        json!({"session_id":"s","index":0}),
    );
    assert_eq!(r["result"]["text"], "学生", "{r}");
    assert_eq!(r["result"]["replace_len_utf16"], 2);
    let r = send(
        &mut s,
        "accept_retro_review",
        json!({"session_id":"s","index":0}),
    );
    assert_eq!(r["result"]["text"], "");
    assert!(
        runtime
            .learn_lookup("がくせい", 0)
            .iter()
            .any(|(s, _)| s == "学生")
    );
}
