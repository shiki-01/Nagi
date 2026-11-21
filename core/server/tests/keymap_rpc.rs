//! キー割当の RPC（`get_keymap` / `validate_keymap`）と、拡張入力モードの読み替え（宣言の有無）の統合テスト。

use nagi_engine::{EngineConfig, KanaBackend, KeyContext, KeyMap};
use nagi_server::Server;
use serde_json::{Value, json};
use std::sync::Arc;

fn server_with(config: EngineConfig) -> Server {
    Server::new(Arc::new(KanaBackend), "minimal", config)
}

fn call(server: &mut Server, request: Value) -> Value {
    let lines = server.handle_line(&request.to_string());
    assert_eq!(lines.len(), 1, "応答は 1 行のはず: {lines:?}");
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn init(server: &mut Server, capabilities: Value) -> Value {
    call(
        server,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"test","client_version":"0","os":"test",
            "capabilities": capabilities}}),
    )
}

fn press(server: &mut Server, id: i64, keysym: u32, text: &str) -> Value {
    call(
        server,
        json!({"jsonrpc":"2.0","id":id,"method":"process_key","params":{
            "session_id":"s","keysym":keysym,"keycode":0,"text":text,
            "modifiers":{},"kind":"down"}}),
    )
}

const KATAKANA_KEY: u32 = 0xff26;

#[test]
fn initialize_advertises_keymap_capability() {
    let mut s = server_with(EngineConfig::default());
    let r = init(&mut s, json!({}));
    assert_eq!(r["result"]["capabilities"]["keymap"], true);
}

#[test]
fn extended_modes_are_downgraded_unless_the_client_declares_support() {
    // 宣言なし: カタカナモードに入っても応答の mode は出ない（ひらがなのまま）
    let mut s = server_with(EngineConfig::default());
    init(&mut s, json!({}));
    let r = press(&mut s, 2, KATAKANA_KEY, "");
    assert_eq!(r["result"]["consumed"], true);
    assert!(r["result"].get("mode").is_none(), "{r}");
    // 宣言あり: そのまま返る
    let mut s = server_with(EngineConfig::default());
    init(&mut s, json!({"extended_input_modes": true}));
    let r = press(&mut s, 2, KATAKANA_KEY, "");
    assert_eq!(r["result"]["mode"], "katakana");
}

#[test]
fn perform_action_accepts_new_modes_in_set_mode() {
    let mut s = server_with(EngineConfig::default());
    init(&mut s, json!({"extended_input_modes": true}));
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"perform_action","params":{
            "session_id":"s","action":{"type":"set_mode","content":"zenkaku_alnum"}}}),
    );
    assert_eq!(r["result"]["mode"], "zenkaku_alnum", "{r}");
    // 旧クライアントへは英数に読み替える
    let mut s = server_with(EngineConfig::default());
    init(&mut s, json!({}));
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"perform_action","params":{
            "session_id":"s","action":{"type":"set_mode","content":"zenkaku_alnum"}}}),
    );
    assert_eq!(r["result"]["mode"], "alphanumeric", "{r}");
}

#[test]
fn get_keymap_lists_defaults_and_catalogs() {
    let mut s = server_with(EngineConfig::default());
    init(&mut s, json!({}));
    // params 省略でも呼べる
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"get_keymap"}),
    );
    let result = &r["result"];
    assert_eq!(result["preset"], "standard");
    let presets: Vec<&str> = result["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        presets,
        ["standard", "function_keys", "ctrl_letters", "henkan_keys"]
    );
    assert!(result["actions"].as_array().unwrap().len() > 30);
    let f9 = result["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["context"] == "composing" && b["key"] == "F9")
        .expect("F9");
    assert_eq!(f9["action"], "ToZenkakuAlnum");
    assert_eq!(f9["source"], "default");
}

#[test]
fn get_keymap_reflects_user_overrides_and_previews_presets() {
    let (keymap, warnings) = KeyMap::with_preset(
        "function_keys",
        &[(KeyContext::Composing, "Ctrl+j".into(), "ToKatakana".into())],
    );
    assert!(warnings.is_empty());
    let mut s = server_with(EngineConfig {
        keymap,
        ..EngineConfig::default()
    });
    init(&mut s, json!({}));
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"get_keymap"}),
    );
    assert_eq!(r["result"]["preset"], "function_keys");
    let find = |r: &Value, ctx: &str, key: &str| -> Option<(String, String)> {
        r["result"]["bindings"]
            .as_array()?
            .iter()
            .find(|b| b["context"] == ctx && b["key"] == key)
            .map(|b| {
                (
                    b["action"].as_str().unwrap().to_string(),
                    b["source"].as_str().unwrap().to_string(),
                )
            })
    };
    assert_eq!(
        find(&r, "composing", "Ctrl+j"),
        Some(("ToKatakana".into(), "user".into()))
    );
    assert_eq!(
        find(&r, "composing", "Ctrl+k"),
        Some(("CommitAndPassThrough".into(), "preset".into()))
    );
    // 別のプリセットに切り替えたときの一覧
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":3,"method":"get_keymap","params":{"preset":"henkan_keys"}}),
    );
    assert_eq!(r["result"]["preset"], "henkan_keys");
    assert_eq!(
        find(&r, "common", "Muhenkan"),
        Some(("SetMode:Alphanumeric".into(), "preset".into()))
    );
    assert_eq!(
        find(&r, "composing", "Ctrl+j"),
        Some(("ToKatakana".into(), "user".into())),
        "利用者の上書きはプリセットを替えても残る"
    );
}

#[test]
fn validate_keymap_returns_warnings_not_rejections() {
    let mut s = server_with(EngineConfig::default());
    init(&mut s, json!({}));
    let toml = "[keymap.composing]\n\"Enter\" = \"NoOp\"\n\"Nope\" = \"NoOp\"\n[keymap.converting]\n\"Alt+F4\" = \"Cancel\"\n";
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"validate_keymap","params":{"toml": toml}}),
    );
    assert!(r.get("error").is_none(), "{r}");
    let errors = r["result"]["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["kind"], "invalid_key");
    assert_eq!(errors[0]["line"], 3);
    let warnings = r["result"]["warnings"].as_array().unwrap();
    let kinds: Vec<(&str, u64)> = warnings
        .iter()
        .map(|w| (w["kind"].as_str().unwrap(), w["line"].as_u64().unwrap()))
        .collect();
    assert_eq!(kinds, [("overrides_essential_key", 2), ("os_reserved", 5)]);
}

#[test]
fn validate_keymap_requires_params_and_initialize() {
    let mut s = server_with(EngineConfig::default());
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"validate_keymap","params":{"toml":""}}),
    );
    assert_eq!(r["error"]["code"], -32002, "{r}");
    init(&mut s, json!({}));
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"validate_keymap"}),
    );
    assert_eq!(r["error"]["code"], -32602, "{r}");
}
