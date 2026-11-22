//! キー割当の編集 RPC（`set_keymap` / `reset_keymap` / `export_keymap` / `import_keymap`）の統合テスト（プロセス内）。
//! `config.toml` に書き、既存のホットリロードで `get_keymap` に反映されることを確かめる。

use nagi_server::{Server, ServerOptions, build};
use serde_json::{Value, json};
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nagi-keymap-edit-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn server_with_config(tag: &str, initial: Option<&str>) -> (Server, PathBuf) {
    let dir = temp_dir(tag);
    let path = dir.join("config.toml");
    if let Some(s) = initial {
        std::fs::write(&path, s).unwrap();
    }
    let built = build(&ServerOptions {
        user_dir: Some(dir),
        use_llm: false,
        ..ServerOptions::default()
    });
    let mut server = Server::from_built(built);
    call(
        &mut server,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"test"}}),
    );
    (server, path)
}

fn call(server: &mut Server, request: Value) -> Value {
    let lines = server.handle_line(&request.to_string());
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    call(
        server,
        json!({"jsonrpc":"2.0","id":2,"method":method,"params":params}),
    )
}

/// 一覧から (文脈, キー) の割当を引く: (Action, 出所)
fn binding(keymap: &Value, context: &str, key: &str) -> Option<(String, String)> {
    keymap["bindings"]
        .as_array()?
        .iter()
        .find(|b| b["context"] == context && b["key"] == key)
        .map(|b| {
            (
                b["action"].as_str().unwrap().to_string(),
                b["source"].as_str().unwrap().to_string(),
            )
        })
}

#[test]
fn set_saves_to_config_and_reflects_immediately() {
    let src = "# 私の設定\n[conversion]\nsync_budget_ms = 150 # 待ち時間\n";
    let (mut s, path) = server_with_config("set", Some(src));
    let r = rpc(
        &mut s,
        "set_keymap",
        json!({"context":"composing","key":"Ctrl+j","action":"ToKatakana"}),
    );
    assert!(r.get("error").is_none(), "{r}");
    // 保存後の一覧が応答に入る（再起動・再読み込みの呼び出しは不要）
    let keymap = &r["result"]["keymap"];
    assert_eq!(
        binding(keymap, "composing", "Ctrl+j").map(|b| b.1),
        Some("user".into())
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# 私の設定"), "{text}");
    assert!(text.contains("sync_budget_ms = 150 # 待ち時間"), "{text}");
    assert!(text.contains("[keymap.composing]"), "{text}");
    // get_keymap でも同じ
    let g = rpc(&mut s, "get_keymap", json!({}));
    assert_eq!(
        binding(&g["result"], "composing", "Ctrl+j"),
        Some(("ToKatakana".into(), "user".into()))
    );
    // 別の書き方で同じキーを解除できる
    let r = rpc(
        &mut s,
        "set_keymap",
        json!({"context":"composing","key":"Control+j"}),
    );
    assert!(r.get("error").is_none(), "{r}");
    assert!(
        binding(&r["result"]["keymap"], "composing", "Ctrl+j").is_none(),
        "{r}"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("keymap"), "{text}");
    assert!(text.contains("sync_budget_ms = 150"), "{text}");
}

#[test]
fn set_returns_warnings_but_saves() {
    let (mut s, path) = server_with_config("warn", None);
    let r = rpc(
        &mut s,
        "set_keymap",
        json!({"context":"converting","key":"Enter","action":"NoOp"}),
    );
    assert!(r.get("error").is_none(), "{r}");
    let kinds: Vec<&str> = r["result"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["overrides_essential_key"], "{r}");
    assert!(std::fs::read_to_string(&path).unwrap().contains("Enter"));
}

#[test]
fn set_rejects_bad_input_without_touching_the_file() {
    let (mut s, path) = server_with_config("bad", Some("x = 1\n"));
    for params in [
        json!({"context":"composing","key":"NotAKey","action":"Commit"}),
        json!({"context":"composing","key":"F2","action":"NotAnAction"}),
        json!({"context":"direct","key":"F2","action":"Commit"}),
        json!({"context":"common","key":"F2","action":"Commit"}),
        json!({"context":"composing","key":"","action":"Commit"}),
    ] {
        let r = rpc(&mut s, "set_keymap", params.clone());
        assert_eq!(r["error"]["code"], -32602, "{params} {r}");
        assert!(r["error"]["message"].as_str().unwrap().len() > 3);
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "x = 1\n");
}

#[test]
fn reset_context_and_all_keep_preset_and_other_settings() {
    let src = "[keymap]\npreset = \"ctrl_letters\"\n[keymap.composing]\nF2 = \"Commit\"\n[keymap.converting]\nF3 = \"Cancel\"\n[update]\nauto_check = false\n";
    let (mut s, path) = server_with_config("reset", Some(src));
    let r = rpc(
        &mut s,
        "reset_keymap",
        json!({"scope":"context","context":"composing"}),
    );
    assert!(r.get("error").is_none(), "{r}");
    let keymap = &r["result"]["keymap"];
    assert!(binding(keymap, "composing", "F2").is_none_or(|b| b.1 != "user"));
    assert_eq!(
        binding(keymap, "converting", "F3"),
        Some(("Cancel".into(), "user".into()))
    );
    // context の指定が無い・共通は断る
    for params in [
        json!({"scope":"context"}),
        json!({"scope":"context","context":"common"}),
    ] {
        let r = rpc(&mut s, "reset_keymap", params);
        assert_eq!(r["error"]["code"], -32602, "{r}");
    }
    let r = rpc(&mut s, "reset_keymap", json!({"scope":"all"}));
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(r["result"]["keymap"]["preset"], "ctrl_letters");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("preset = \"ctrl_letters\""), "{text}");
    assert!(text.contains("auto_check = false"), "{text}");
    assert!(
        !text.contains("F3") && !text.contains("composing"),
        "{text}"
    );
}

#[test]
fn export_then_import_round_trips_and_reports_line_numbers() {
    let src = "[keymap]\npreset = \"henkan_keys\"\n[keymap.converting]\nTab = \"NextCand\"\n[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\nF2 = \"Commit\"\n";
    let (mut s, path) = server_with_config("export", Some(src));
    let r = rpc(&mut s, "export_keymap", json!({}));
    let exported = r["result"]["toml"].as_str().unwrap().to_string();
    assert!(exported.contains("preset = \"henkan_keys\""), "{exported}");
    assert!(
        exported.contains("\"Ctrl+j\" = \"ToKatakana\""),
        "{exported}"
    );
    // 上書きの無いファイルへ取り込む
    let (mut t, path2) = server_with_config("import", Some("[update]\nauto_check = false\n"));
    let r = rpc(&mut t, "import_keymap", json!({"toml": exported}));
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(r["result"]["keymap"]["preset"], "henkan_keys");
    assert_eq!(
        binding(&r["result"]["keymap"], "composing", "Ctrl+j"),
        Some(("ToKatakana".into(), "user".into()))
    );
    let again = rpc(&mut t, "export_keymap", json!({}));
    assert_eq!(again["result"]["toml"], r2_toml(&exported));
    assert!(
        std::fs::read_to_string(&path2)
            .unwrap()
            .contains("auto_check = false")
    );
    let _ = path;
    // 解釈できない行があれば何も反映せず、行番号つきで断る
    let before = std::fs::read_to_string(&path2).unwrap();
    let r = rpc(
        &mut t,
        "import_keymap",
        json!({"toml": "[keymap.composing]\nF2 = \"Commit\"\n\"NotAKey\" = \"NoOp\"\nbroken\n"}),
    );
    assert_eq!(r["error"]["code"], -32602, "{r}");
    let message = r["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("3 行目") && message.contains("4 行目"),
        "{message}"
    );
    assert_eq!(std::fs::read_to_string(&path2).unwrap(), before);
}

/// 書き出しは読み込んだものと同じ（キーの昇順・文脈順に整列済みの入力なので、そのまま返る）
fn r2_toml(exported: &str) -> Value {
    json!(exported)
}

#[test]
fn import_merge_keeps_existing_overrides() {
    let (mut s, _) = server_with_config("merge", Some("[keymap.composing]\nF2 = \"Commit\"\n"));
    let r = rpc(
        &mut s,
        "import_keymap",
        json!({"toml":"[keymap.composing]\nF3 = \"Cancel\"\n","merge":true}),
    );
    assert!(r.get("error").is_none(), "{r}");
    let keymap = &r["result"]["keymap"];
    assert_eq!(
        binding(keymap, "composing", "F2").map(|b| b.1),
        Some("user".into())
    );
    assert_eq!(
        binding(keymap, "composing", "F3").map(|b| b.1),
        Some("user".into())
    );
    // 置き換えなら F2 は消える
    let r = rpc(
        &mut s,
        "import_keymap",
        json!({"toml":"[keymap.composing]\nF3 = \"Cancel\"\n"}),
    );
    assert_ne!(
        binding(&r["result"]["keymap"], "composing", "F2").map(|b| b.1),
        Some("user".into())
    );
}

#[test]
fn edits_require_initialize_and_a_config_path() {
    let mut s = Server::new(
        std::sync::Arc::new(nagi_engine::KanaBackend),
        "minimal",
        nagi_engine::EngineConfig::default(),
    );
    let r = rpc(&mut s, "export_keymap", json!({}));
    assert!(r.get("error").is_some(), "初期化前は断る: {r}");
    call(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"test"}}),
    );
    // 設定ファイルの場所が無いサーバーは保存できない（書き出しは空）
    let r = rpc(
        &mut s,
        "set_keymap",
        json!({"context":"composing","key":"F2","action":"Commit"}),
    );
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("保存できません"),
        "{r}"
    );
    let r = rpc(&mut s, "export_keymap", json!({}));
    assert_eq!(r["result"]["toml"], "");
}
