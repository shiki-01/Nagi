//! ユーザー辞書の管理 RPC（`list_user_dict` ほか）と `get_learning_stats` の統合テスト（プロセス内）。
//! 変更が即座にサーバーの辞書へ反映されること、手書きの行を壊さないこと、入力内容を返信のエラーに含めないことを確かめる。

mod common;

use base64::Engine as _;
use common::dict;
use nagi_config::Config;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::setup::{Built, Reloadable};
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
    let dir = std::env::temp_dir().join(format!("nagi-userdict-rpc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn server_in(user_dir: &Path) -> Server {
    let config_path = user_dir.join("config.toml");
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
    runtime.set_config_state(Some(config_path.clone()), false);
    let dict = dict();
    let user_dict_dir = user_dir.join("user_dict");
    let (user, _) = nagi_dict::UserDictionary::load_dir(&user_dict_dir);
    runtime.set_overlay(dict.make_overlay(&user));
    let pipeline = Arc::new(Pipeline::new(Some(dict.clone()), None, runtime.clone()));
    let mut reload = Reloadable::detached();
    reload.config_path = Some(config_path);
    reload.user_dict_dir = Some(user_dict_dir);
    reload.suppressed_path = Some(user_dir.join("suppressed.tsv"));
    reload.pipeline = Some(std::panic::AssertUnwindSafe(pipeline.clone()));
    reload.dict = Some(dict);
    reload.mark_read();
    Server::from_built(Built {
        pipeline,
        runtime,
        config,
        reload,
        notes: Vec::new(),
    })
}

fn send(server: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let first = server.handle_line(&line).into_iter().next().expect("応答");
    serde_json::from_str(&first).unwrap()
}

fn ok(server: &mut Server, method: &str, params: Value) -> Value {
    let r = send(server, method, params);
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn init(server: &mut Server) {
    ok(
        server,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
    );
}

fn status_entries(server: &mut Server) -> u64 {
    ok(server, "get_status", json!({}))["user_dict_entries"]
        .as_u64()
        .unwrap()
}

#[test]
fn requires_initialize() {
    let dir = temp_dir("uninit");
    let mut s = server_in(&dir);
    for m in ["list_user_dict", "export_user_dict", "get_learning_stats"] {
        let r = send(&mut s, m, json!({}));
        assert_eq!(r["error"]["code"], -32002, "{m}: {r}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_list_update_remove_export_apply_immediately() {
    let dir = temp_dir("crud");
    let ud = dir.join("user_dict");
    std::fs::create_dir_all(&ud).unwrap();
    std::fs::write(ud.join("mine.txt"), "# 手書き\nこうえん\t公苑\t名詞\t旧\n").unwrap();
    let mut s = server_in(&dir);
    init(&mut s);
    assert_eq!(status_entries(&mut s), 1);

    // 登録（カタカナの読みはひらがなに直る）。サーバーの辞書へ即時反映
    let r = ok(
        &mut s,
        "add_user_dict_entry",
        json!({"entry": {"reading": "ヒナタ", "surface": "日向", "pos": "地名", "comment": "メモ"}}),
    );
    assert_eq!(r, json!({"added": true, "replaced": false}));
    assert_eq!(status_entries(&mut s), 2);
    assert_eq!(
        std::fs::read_to_string(ud.join("registered.tsv")).unwrap(),
        "ひなた\t日向\t地名\tメモ\n"
    );

    let list = ok(&mut s, "list_user_dict", json!({"sort": "reading"}));
    assert_eq!(list["total"], 2);
    assert_eq!(list["items"][0]["file"], "mine.txt");
    assert_eq!(list["items"][1]["reading"], "ひなた");
    assert!(list["max_entries"].as_u64().unwrap() >= 100);
    let found = ok(&mut s, "list_user_dict", json!({"query": "ヒナ"}));
    assert_eq!(
        (found["matched"].as_u64(), found["total"].as_u64()),
        (Some(1), Some(2))
    );

    // 重複は既定でエラー。メッセージに入力内容は含まれない
    let dup = send(
        &mut s,
        "add_user_dict_entry",
        json!({"entry": {"reading": "ひなた", "surface": "日向"}}),
    );
    assert_eq!(dup["error"]["code"], -32602);
    assert!(!dup.to_string().contains("日向"), "{dup}");
    let skip = ok(
        &mut s,
        "add_user_dict_entry",
        json!({"entry": {"reading": "ひなた", "surface": "日向"}, "on_duplicate": "skip"}),
    );
    assert_eq!(skip, json!({"added": false, "replaced": false}));

    // 編集: 手書きファイルのコメント行を残したまま、その行だけ差し替える
    ok(
        &mut s,
        "update_user_dict_entry",
        json!({"target": {"reading": "こうえん", "surface": "公苑", "file": "mine.txt"},
               "entry": {"reading": "こうえん", "surface": "公苑", "pos": "地名", "comment": "新"}}),
    );
    assert_eq!(
        std::fs::read_to_string(ud.join("mine.txt")).unwrap(),
        "# 手書き\nこうえん\t公苑\t地名\t新\n"
    );
    let missing = send(
        &mut s,
        "update_user_dict_entry",
        json!({"target": {"reading": "ない", "surface": "無い"}, "entry": {"reading": "ない", "surface": "無"}}),
    );
    assert_eq!(missing["error"]["code"], -32602);

    // 書き出し（Mozc TSV）
    let export = ok(&mut s, "export_user_dict", json!({}));
    assert_eq!(export["count"], 2);
    assert_eq!(
        export["text"],
        "こうえん\t公苑\t地名\t新\nひなた\t日向\t地名\tメモ\n"
    );

    // 削除
    let r = ok(
        &mut s,
        "remove_user_dict_entries",
        json!({"items": [{"reading": "ひなた", "surface": "日向"}, {"reading": "ない", "surface": "無い"}]}),
    );
    assert_eq!(r["removed"], 1);
    assert_eq!(status_entries(&mut s), 1);
    assert_eq!(ok(&mut s, "list_user_dict", json!({}))["total"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_reports_line_numbers_and_honours_the_duplicate_policy() {
    let dir = temp_dir("merge");
    let ud = dir.join("user_dict");
    std::fs::create_dir_all(&ud).unwrap();
    std::fs::write(ud.join("mine.txt"), "こうえん\t公苑\t名詞\t旧\n").unwrap();
    let mut s = server_in(&dir);
    init(&mut s);
    let text = "\u{feff}こうえん\t公苑\t地名\t新\nひなた\t日向\t地名\n漢字\t不正\t名詞\n";
    let data = base64::engine::general_purpose::STANDARD.encode(text);

    let r = ok(&mut s, "merge_user_dict", json!({"data_base64": data}));
    assert_eq!(
        (
            r["imported"].as_u64(),
            r["replaced"].as_u64(),
            r["skipped"].as_u64()
        ),
        (Some(1), Some(0), Some(2))
    );
    assert_eq!(r["format"], "mozc");
    let lines: Vec<u64> = r["skipped_lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["line"].as_u64().unwrap())
        .collect();
    assert_eq!(lines, [1, 3]);
    assert_eq!(status_entries(&mut s), 2);

    let r = ok(
        &mut s,
        "merge_user_dict",
        json!({"data_base64": data, "on_duplicate": "overwrite"}),
    );
    assert_eq!(
        (
            r["imported"].as_u64(),
            r["replaced"].as_u64(),
            r["skipped"].as_u64()
        ),
        (Some(0), Some(2), Some(1))
    );
    assert_eq!(
        std::fs::read_to_string(ud.join("mine.txt")).unwrap(),
        "こうえん\t公苑\t地名\t新\n"
    );
    assert_eq!(status_entries(&mut s), 2);

    for bad in ["!!!", ""] {
        let r = send(&mut s, "merge_user_dict", json!({"data_base64": bad}));
        assert_eq!(r["error"]["code"], -32602, "{r}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn learning_stats_counts_without_content() {
    let dir = temp_dir("stats");
    let mut s = server_in(&dir);
    init(&mut s);
    let r = ok(&mut s, "get_learning_stats", json!({}));
    assert_eq!(r["enabled"], true);
    assert_eq!(r["learned_entries"], 0);
    assert_eq!(r["suppressed_entries"], 0);
    assert!(r.get("last_saved_unix").is_none());
    ok(
        &mut s,
        "suppress_candidate",
        json!({"reading": "こうえん", "surface": "講演"}),
    );
    ok(
        &mut s,
        "add_user_dict_entry",
        json!({"entry": {"reading": "ひなた", "surface": "日向"}}),
    );
    let r = ok(&mut s, "get_learning_stats", json!({}));
    assert_eq!(r["suppressed_entries"], 1);
    assert_eq!(r["user_dict_entries"], 1);
    // 学習した語・表記はどこにも出ない
    assert!(!r.to_string().contains("講演"), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}
