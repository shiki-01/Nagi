//! バックアップ RPC（`export_backup` / `preview_backup` / `apply_backup` / `list_backups` / `restore_backup`）の
//! 統合テスト（プロセス内）。往復・プレビュー・全か無か・自動バックアップと世代管理・不正な入力・未知の版を確かめる。

mod common;

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
    let dir = std::env::temp_dir().join(format!("nagi-backup-rpc-{}-{name}", std::process::id()));
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
    let mut server = Server::from_built(Built {
        pipeline,
        runtime,
        config,
        reload,
        notes: Vec::new(),
    });
    init(&mut server);
    server
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

fn entries(server: &mut Server) -> u64 {
    ok(server, "get_status", json!({}))["user_dict_entries"]
        .as_u64()
        .unwrap()
}

const CONFIG_A: &str = "# 私の設定\n[conversion]\nsync_budget_ms = 200\nlive_conversion = false\n\n[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n";
const DICT_A: &str = "# 手書き\nこうえん\t公苑\t名詞\t旧\nひなた\t日向\t地名\t\n";
const SUPPRESSED_A: &str = "あい\t愛\nかお\t顔\n";

/// 設定・辞書・抑制語・学習がそろった利用者のフォルダを作る
fn populated(tag: &str) -> PathBuf {
    let dir = temp_dir(tag);
    std::fs::write(dir.join("config.toml"), CONFIG_A).unwrap();
    std::fs::create_dir_all(dir.join("user_dict")).unwrap();
    std::fs::write(dir.join("user_dict/mine.txt"), DICT_A).unwrap();
    std::fs::write(dir.join("suppressed.tsv"), SUPPRESSED_A).unwrap();
    let mut learner = Learner::new(LearnConfig::default());
    for (r, s) in [("きょう", "今日"), ("あした", "明日"), ("きょう", "今日")] {
        learner.record(r, s, 0, "conv", 1_700_000_000);
    }
    learner.flush(&dir.join("learning.tsv")).unwrap();
    dir
}

const ALL: [&str; 4] = ["settings", "user_dict", "suppressed", "learning"];

fn export_all(s: &mut Server) -> Value {
    ok(s, "export_backup", json!({"include": ALL}))["backup"].clone()
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn requires_initialize() {
    let dir = temp_dir("uninit");
    let config = Config::default();
    let runtime = Runtime::new(
        SYSTEM,
        true,
        Tunables::from_config(&config),
        Learner::new(LearnConfig::default()),
        None,
    );
    let pipeline = Arc::new(Pipeline::new(Some(dict()), None, runtime.clone()));
    let mut server = Server::from_built(Built {
        pipeline,
        runtime,
        config,
        reload: Reloadable::detached(),
        notes: Vec::new(),
    });
    for m in ["export_backup", "list_backups", "restore_backup"] {
        let r = send(&mut server, m, json!({"include": ["settings"]}));
        assert_eq!(r["error"]["code"], -32002, "{m}: {r}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_has_schema_and_selected_items_only() {
    let dir = populated("export");
    let mut s = server_in(&dir);
    let r = ok(
        &mut s,
        "export_backup",
        json!({"include": ["settings", "user_dict"]}),
    );
    let b = &r["backup"];
    assert_eq!(b["version"], 1);
    assert!(b["created_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(b["platform"], std::env::consts::OS);
    assert_eq!(b["included"], json!(["settings", "user_dict"]));
    let kinds: Vec<&str> = b["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["settings", "user_dict"], "学習・抑制語は含まれない");
    assert_eq!(b["entries"][0]["name"], "config.toml");
    assert_eq!(b["entries"][0]["text"], CONFIG_A);
    assert_eq!(b["entries"][1]["text"], DICT_A);
    assert!(r["file_name"].as_str().unwrap().starts_with("nagi-backup-"));
    // 空の選択は拒否
    let e = send(&mut s, "export_backup", json!({"include": []}));
    assert_eq!(e["error"]["code"], -32602);
    // 学習を含めると入力履歴への注意が付く
    let r = ok(&mut s, "export_backup", json!({"include": ["learning"]}));
    assert!(r["warnings"].to_string().contains("入力した内容の履歴"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_strips_machine_specific_model_path() {
    let dir = temp_dir("modelpath");
    std::fs::write(
        dir.join("config.toml"),
        "[conversion]\nmodel = \"C:\\\\Users\\\\someone\\\\z.gguf\"\nuse_llm = true\n",
    )
    .unwrap();
    let mut s = server_in(&dir);
    let r = ok(&mut s, "export_backup", json!({"include": ["settings"]}));
    let text = r["backup"]["entries"][0]["text"].as_str().unwrap();
    assert!(
        !text.contains("someone") && !text.contains("model"),
        "{text}"
    );
    assert!(text.contains("use_llm"));
    assert!(r["warnings"].to_string().contains("conversion.model"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn round_trip_into_a_fresh_profile() {
    let src = populated("rt-src");
    let mut a = server_in(&src);
    let backup = export_all(&mut a);

    let dst = temp_dir("rt-dst");
    let mut b = server_in(&dst);
    assert_eq!(entries(&mut b), 0);
    let r = ok(
        &mut b,
        "apply_backup",
        json!({"backup": backup, "apply": ALL}),
    );
    let applied = r["applied"].as_array().unwrap();
    assert_eq!(applied.len(), 4);
    assert!(
        r["auto_backup"]
            .as_str()
            .unwrap()
            .starts_with("nagi-backup-")
    );

    // 設定は一字一句そのまま（コメント・キー割当を含む）
    assert_eq!(
        std::fs::read_to_string(dst.join("config.toml")).unwrap(),
        CONFIG_A
    );
    // 辞書は統合: imported.tsv に新しい語が入り、サーバーに即時反映
    assert_eq!(entries(&mut b), 2);
    let list = ok(&mut b, "list_user_dict", json!({}));
    assert_eq!(list["total"], 2);
    // 抑制語
    let sup = ok(&mut b, "list_suppressed", json!({}));
    assert_eq!(sup["items"].as_array().unwrap().len(), 2);
    // 学習
    assert_eq!(ok(&mut b, "get_status", json!({}))["learned_entries"], 2);
    let (l, _) = Learner::load(&dst.join("learning.tsv"), LearnConfig::default()).unwrap();
    assert_eq!(l.len(), 2);
    // キー割当が反映される
    let km = ok(&mut b, "get_keymap", json!({}));
    assert!(
        km["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["key"] == "Ctrl+j" && x["action"] == "ToKatakana")
    );
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn preview_reports_differences_and_writes_nothing() {
    let src = populated("pv-src");
    let mut a = server_in(&src);
    let mut backup = export_all(&mut a);
    backup["platform"] = json!("someos");
    backup["app_version"] = json!("0.0.1");

    let dst = temp_dir("pv-dst");
    std::fs::write(
        dst.join("config.toml"),
        "[conversion]\nsync_budget_ms = 150\nuse_llm = false\n",
    )
    .unwrap();
    std::fs::create_dir_all(dst.join("user_dict")).unwrap();
    std::fs::write(
        dst.join("user_dict/x.tsv"),
        "こうえん\t公苑\t名詞\t\nあるく\t歩く\t名詞\t\n",
    )
    .unwrap();
    std::fs::write(dst.join("suppressed.tsv"), "あい\t愛\n").unwrap();
    let before = file_names(&dst);
    let mut b = server_in(&dst);
    let r = ok(&mut b, "preview_backup", json!({"backup": backup}));
    assert_eq!(r["same_platform"], false);
    assert!(r["warnings"].to_string().contains("別の OS"));
    assert!(r["warnings"].to_string().contains("0.0.1"));
    let items = r["items"].as_array().unwrap();
    let item = |k: &str| items.iter().find(|i| i["kind"] == k).unwrap().clone();

    let settings = item("settings");
    assert_eq!(settings["applicable"], true);
    assert_eq!(
        settings["changed"], 1,
        "sync_budget_ms の値が違う: {settings}"
    );
    assert_eq!(
        settings["added"], 2,
        "live_conversion と keymap の 1 キー: {settings}"
    );
    assert_eq!(
        settings["removed"], 1,
        "use_llm はバックアップに無い: {settings}"
    );
    let changes = settings["changes"].as_array().unwrap();
    let c = changes
        .iter()
        .find(|c| c["key"] == "conversion.sync_budget_ms")
        .unwrap();
    assert_eq!(
        (c["before"].as_str(), c["after"].as_str()),
        (Some("150"), Some("200"))
    );

    let dict = item("user_dict");
    assert_eq!(
        (
            dict["count"].as_u64(),
            dict["added"].as_u64(),
            dict["same"].as_u64()
        ),
        (Some(2), Some(1), Some(1)),
        "{dict}"
    );
    let sup = item("suppressed");
    assert_eq!(
        (
            sup["count"].as_u64(),
            sup["added"].as_u64(),
            sup["same"].as_u64()
        ),
        (Some(2), Some(1), Some(1))
    );
    let learning = item("learning");
    assert_eq!(learning["count"], 2);
    assert_eq!(learning["current"], 0);

    // プレビューは何も書かない（バックアップ・一時ファイルも作らない）
    assert_eq!(file_names(&dst), before);
    assert_eq!(
        std::fs::read_to_string(dst.join("suppressed.tsv")).unwrap(),
        "あい\t愛\n"
    );
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn apply_merges_dict_and_suppressed_and_replaces_settings() {
    let src = populated("merge-src");
    let mut a = server_in(&src);
    let backup = export_all(&mut a);

    let dst = temp_dir("merge-dst");
    std::fs::write(dst.join("config.toml"), "[conversion]\nuse_llm = false\n").unwrap();
    std::fs::create_dir_all(dst.join("user_dict")).unwrap();
    std::fs::write(
        dst.join("user_dict/x.tsv"),
        "こうえん\t公苑\t名詞\t\nあるく\t歩く\t名詞\t\n",
    )
    .unwrap();
    std::fs::write(dst.join("suppressed.tsv"), "あい\t愛\nさけ\t酒\n").unwrap();
    let mut b = server_in(&dst);
    // 設定だけ・辞書だけを選ぶこともできる
    ok(
        &mut b,
        "apply_backup",
        json!({"backup": backup, "apply": ["user_dict", "suppressed"]}),
    );
    assert_eq!(entries(&mut b), 3, "既存 2 + 新規 1（重複は既存を優先）");
    assert_eq!(
        std::fs::read_to_string(dst.join("config.toml")).unwrap(),
        "[conversion]\nuse_llm = false\n",
        "選ばなかった設定は変わらない"
    );
    let sup = ok(&mut b, "list_suppressed", json!({}));
    assert_eq!(sup["items"].as_array().unwrap().len(), 3, "和集合");
    // 同じものをもう一度適用しても増えない
    ok(
        &mut b,
        "apply_backup",
        json!({"backup": backup, "apply": ["user_dict", "suppressed"]}),
    );
    assert_eq!(entries(&mut b), 3);
    ok(
        &mut b,
        "apply_backup",
        json!({"backup": backup, "apply": ["settings"]}),
    );
    assert_eq!(
        std::fs::read_to_string(dst.join("config.toml")).unwrap(),
        CONFIG_A
    );
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn failure_midway_rolls_everything_back() {
    let src = populated("aon-src");
    let mut a = server_in(&src);
    let backup = export_all(&mut a);

    let dst = temp_dir("aon-dst");
    std::fs::write(dst.join("config.toml"), "[conversion]\nuse_llm = false\n").unwrap();
    std::fs::create_dir_all(dst.join("user_dict")).unwrap();
    std::fs::write(dst.join("user_dict/x.tsv"), "あるく\t歩く\t名詞\t\n").unwrap();
    // 抑制語の保存（一時ファイル `suppressed.tmp`）を失敗させる
    std::fs::create_dir_all(dst.join("suppressed.tmp")).unwrap();
    let mut b = server_in(&dst);
    let r = send(
        &mut b,
        "apply_backup",
        json!({"backup": backup, "apply": ALL}),
    );
    assert_eq!(r["error"]["code"], -32004, "{r}");
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("元に戻しました"),
        "{r}"
    );
    // 設定・辞書は元のまま（先に書いた分も戻っている）
    assert_eq!(
        std::fs::read_to_string(dst.join("config.toml")).unwrap(),
        "[conversion]\nuse_llm = false\n"
    );
    assert_eq!(entries(&mut b), 1);
    assert_eq!(
        file_names(&dst.join("user_dict")),
        ["x.tsv"],
        "imported.tsv も残らない"
    );
    assert_eq!(ok(&mut b, "get_status", json!({}))["learned_entries"], 0);
    assert!(!dst.join("suppressed.tsv").exists());
    // エラーの文言に入力内容を含めない
    assert!(!r.to_string().contains("歩く") && !r.to_string().contains("今日"));
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn auto_backup_is_saved_before_apply_and_undo_restores_exactly() {
    let src = populated("undo-src");
    let mut a = server_in(&src);
    let backup = export_all(&mut a);

    let dst = temp_dir("undo-dst");
    let original_config = "[conversion]\nuse_llm = false\n# 元のコメント\n";
    std::fs::write(dst.join("config.toml"), original_config).unwrap();
    std::fs::create_dir_all(dst.join("user_dict")).unwrap();
    std::fs::write(dst.join("user_dict/x.tsv"), "あるく\t歩く\t名詞\t\n").unwrap();
    std::fs::write(dst.join("suppressed.tsv"), "さけ\t酒\n").unwrap();
    let mut b = server_in(&dst);
    assert!(
        ok(&mut b, "list_backups", json!({}))["backups"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = ok(
        &mut b,
        "apply_backup",
        json!({"backup": backup, "apply": ALL}),
    );
    let auto = r["auto_backup"].as_str().unwrap().to_string();
    assert!(dst.join("backups").join(&auto).is_file());
    assert_eq!(entries(&mut b), 3);

    let list = ok(&mut b, "list_backups", json!({}));
    assert_eq!(list["backups"][0]["name"], auto.as_str());
    assert_eq!(list["backups"][0]["kinds"], json!(ALL));
    assert_eq!(list["keep"], 5);

    // 元に戻す: 設定は一字一句、辞書は適用前のファイル構成に戻る（imported.tsv は消える）
    let restored = ok(&mut b, "restore_backup", json!({}));
    assert_eq!(restored["applied"].as_array().unwrap().len(), 4);
    assert_eq!(
        std::fs::read_to_string(dst.join("config.toml")).unwrap(),
        original_config
    );
    assert_eq!(file_names(&dst.join("user_dict")), ["x.tsv"]);
    assert_eq!(entries(&mut b), 1);
    assert!(
        std::fs::read_to_string(dst.join("suppressed.tsv"))
            .unwrap()
            .contains("酒")
    );
    assert_eq!(
        ok(&mut b, "list_suppressed", json!({}))["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(ok(&mut b, "get_status", json!({}))["learned_entries"], 0);
    // 復元の前にも自動バックアップが作られる（元に戻すを取り消せる）
    assert_eq!(
        ok(&mut b, "list_backups", json!({}))["backups"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn keeps_only_the_latest_five_generations() {
    let dir = populated("gens");
    let mut s = server_in(&dir);
    let backup = ok(&mut s, "export_backup", json!({"include": ["suppressed"]}))["backup"].clone();
    for _ in 0..8 {
        ok(
            &mut s,
            "apply_backup",
            json!({"backup": backup, "apply": ["suppressed"]}),
        );
    }
    let list = ok(&mut s, "list_backups", json!({}));
    let names: Vec<&str> = list["backups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 5, "{names:?}");
    assert_eq!(std::fs::read_dir(dir.join("backups")).unwrap().count(), 5);
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(names, sorted, "新しい順");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_input_is_rejected_without_touching_anything() {
    let dir = populated("invalid");
    let mut s = server_in(&dir);
    let good = export_all(&mut s);
    let before_config = std::fs::read_to_string(dir.join("config.toml")).unwrap();

    // 設定の検証エラー（行番号つき）→ プレビューで適用不可、適用は拒否
    let mut broken = good.clone();
    broken["entries"][0]["text"] = json!("[conversion]\nsync_budget_ms = \"abc\"\n");
    let r = ok(&mut s, "preview_backup", json!({"backup": broken}));
    let st = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "settings")
        .unwrap()
        .clone();
    assert_eq!(st["applicable"], false);
    assert!(!st["errors"].as_array().unwrap().is_empty());
    let e = send(
        &mut s,
        "apply_backup",
        json!({"backup": broken, "apply": ["settings", "user_dict"]}),
    );
    assert_eq!(e["error"]["code"], -32602, "{e}");
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        before_config
    );
    assert!(
        !dir.join("backups").exists(),
        "検証で落ちたら自動バックアップも作らない"
    );

    // 構文エラー
    let mut syntax = good.clone();
    syntax["entries"][0]["text"] = json!("[conversion\n");
    let e = send(
        &mut s,
        "apply_backup",
        json!({"backup": syntax, "apply": ["settings"]}),
    );
    assert_eq!(e["error"]["code"], -32602);

    // 版が 0 / 項目が無い / 空の選択 / 含まれない項目
    let mut v0 = good.clone();
    v0["version"] = json!(0);
    assert_eq!(
        send(&mut s, "preview_backup", json!({"backup": v0}))["error"]["code"],
        -32602
    );
    let empty = json!({"version":1,"created_at":"","app_version":"","platform":"","entries":[]});
    assert_eq!(
        send(&mut s, "preview_backup", json!({"backup": empty}))["error"]["code"],
        -32602
    );
    assert_eq!(
        send(&mut s, "apply_backup", json!({"backup": good, "apply": []}))["error"]["code"],
        -32602
    );
    let only_dict =
        ok(&mut s, "export_backup", json!({"include": ["user_dict"]}))["backup"].clone();
    assert_eq!(
        send(
            &mut s,
            "apply_backup",
            json!({"backup": only_dict, "apply": ["settings"]})
        )["error"]["code"],
        -32602
    );
    // 型が違う本文
    assert_eq!(
        send(&mut s, "preview_backup", json!({"backup": {"foo": 1}}))["error"]["code"],
        -32602
    );
    // 復元の名前は検証される（パス区切り・任意のファイル名は拒否）
    for bad in ["../config.toml", "nagi-backup-x.json", "/etc/passwd"] {
        assert_eq!(
            send(&mut s, "restore_backup", json!({"name": bad}))["error"]["code"],
            -32602,
            "{bad}"
        );
    }
    assert_eq!(
        send(&mut s, "restore_backup", json!({}))["error"]["code"],
        -32602,
        "自動バックアップが無い"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_versions_and_kinds_are_tolerated_with_warnings() {
    let src = populated("future-src");
    let mut a = server_in(&src);
    let mut backup = export_all(&mut a);
    backup["version"] = json!(99);
    backup["future_field"] = json!({"x": 1});
    backup["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"themes","name":"t.json","text":"{}","extra":1}));

    let dst = temp_dir("future-dst");
    let mut b = server_in(&dst);
    let r = ok(&mut b, "preview_backup", json!({"backup": backup}));
    let w = r["warnings"].to_string();
    assert!(w.contains("新しい版"), "{w}");
    assert!(w.contains("扱えない項目が 1 件"), "{w}");
    ok(
        &mut b,
        "apply_backup",
        json!({"backup": backup, "apply": ["settings"]}),
    );
    assert_eq!(
        std::fs::read_to_string(dst.join("config.toml")).unwrap(),
        CONFIG_A
    );
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&dst);
}

#[test]
fn windows_only_section_is_flagged_on_other_platforms() {
    let dir = temp_dir("winonly");
    let mut s = server_in(&dir);
    let backup = json!({"version":1,"created_at":"2026-11-22T00:00:00Z","app_version":"0.1.29","platform":"windows",
        "included":["settings"],
        "entries":[{"kind":"settings","name":"config.toml","text":"[windows]\nindicator_icon = \"mono\"\n"}]});
    let r = ok(&mut s, "preview_backup", json!({"backup": backup}));
    let st = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "settings")
        .unwrap()
        .clone();
    if std::env::consts::OS != "windows" {
        assert!(
            st["warnings"].to_string().contains("Windows でだけ"),
            "{st}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dictionary_limit_is_checked_before_writing() {
    let dir = temp_dir("limit");
    let mut s = server_in(&dir);
    // 既定の上限（ユーザー辞書 10000 語）を超える語数
    let mut text = String::new();
    for i in 0..10_500 {
        let reading: String = format!("{i:05}")
            .chars()
            .map(|c| char::from_u32('あ' as u32 + c.to_digit(10).unwrap_or(0)).unwrap_or('あ'))
            .collect();
        text.push_str(&format!("{reading}\t語{i}\t名詞\t\n"));
    }
    let backup = json!({"version":1,"created_at":"x","app_version":"x","platform":"x","included":["user_dict"],
        "entries":[{"kind":"user_dict","name":"big.tsv","text":text}]});
    let r = ok(&mut s, "preview_backup", json!({"backup": backup}));
    let d = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "user_dict")
        .unwrap()
        .clone();
    assert_eq!(d["applicable"], false, "{d}");
    assert!(d["errors"].to_string().contains("上限"), "{d}");
    let e = send(
        &mut s,
        "apply_backup",
        json!({"backup": backup, "apply": ["user_dict"]}),
    );
    assert_eq!(e["error"]["code"], -32602, "{e}");
    assert!(!dir.join("user_dict").exists(), "何も書かれない");
    assert!(!e.to_string().contains("語1"), "内容を含めない");
    let _ = std::fs::remove_dir_all(&dir);
}
