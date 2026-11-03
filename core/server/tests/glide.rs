//! グライド入力（`glide_input`、`input.glide`）の JSON-RPC 統合テスト。
//!
//! フロントが送る JSON の形で `set_layout_geometry` → `glide_input` を呼び、合成軌跡（`nagi_glide::synth`）が
//! 読みの候補（出所 `glide`）になり、選んで確定できることを確認する。LLM 無し（辞書のみ）と、偽のモデル付きの両方。

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_glide::synth::{Rng, SynthParams, generate};
use nagi_glide::{GlideIndex, GlideLayout, KeyRect};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::setup::Reloadable;
use nagi_server::{Built, Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::sync::Arc;

const EXTRA: &str = "\
かんじ\t1\t1\t3000\t感じ
かんじ\t1\t1\t3500\t漢字
がっこう\t1\t1\t3000\t学校
しんぶん\t1\t1\t3000\t新聞
ありがとう\t1\t1\t3000\tありがとう
たいへん\t1\t1\t3200\t大変
";

/// フロントのキー矩形（ピクセル相当。`GlideLayout::qwerty_phone` を 40 倍したもの）
fn key_rects() -> Vec<(char, f64, f64, f64, f64)> {
    const ROWS: [(&str, f64); 3] = [("qwertyuiop", 0.0), ("asdfghjkl", 0.5), ("zxcvbnm", 1.5)];
    let mut out = Vec::new();
    for (row, (letters, offset)) in ROWS.iter().enumerate() {
        for (col, c) in letters.chars().enumerate() {
            out.push((
                c,
                (offset + col as f64 + 0.05) * 40.0,
                (row as f64 * 1.45 + 0.075) * 40.0,
                0.9 * 40.0,
                1.3 * 40.0,
            ));
        }
    }
    out
}

fn layout() -> GlideLayout {
    let rects: Vec<KeyRect> = key_rects()
        .into_iter()
        .map(|(label, x, y, w, h)| KeyRect {
            label,
            x: x as f32,
            y: y as f32,
            w: w as f32,
            h: h as f32,
        })
        .collect();
    GlideLayout::new(&rects).expect("26 キー")
}

fn geometry_params(layout_id: &str) -> Value {
    let keys: Vec<Value> = key_rects()
        .into_iter()
        .map(|(c, x, y, w, h)| json!({"id": c.to_string(), "label": c.to_string(), "x": x, "y": y, "w": w, "h": h}))
        .collect();
    json!({"layout_id": layout_id, "mode": "qwerty", "width": 440.0, "height": 160.0, "keys": keys})
}

fn server(glide: bool, with_llm: bool) -> Server {
    server_with(glide, with_llm, None)
}

/// `file_only` は `Pipeline::set_glide_file_only` の引数（`Some` ならファイル専用）
fn server_with(
    glide: bool,
    with_llm: bool,
    file_only: Option<Option<std::path::PathBuf>>,
) -> Server {
    let mut config = Config::default();
    config.input.glide = glide;
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        with_llm,
        Tunables {
            sync_budget: std::time::Duration::from_millis(1000),
            ..Tunables::from_config(&config)
        },
        Learner::new(LearnConfig::default()),
        None,
    );
    let dict = dict_with_extra(EXTRA);
    let model: Option<Box<dyn nagi_infer::ConversionModel>> = with_llm.then(|| {
        Box::new(FakeModel::new(&[("がっこう", "学校"), ("かんじ", "漢字")]))
            as Box<dyn nagi_infer::ConversionModel>
    });
    let mut pipeline = Pipeline::new(Some(dict.clone()), model, runtime.clone());
    if let Some(path) = file_only {
        pipeline.set_glide_file_only(path);
    }
    let mut reload = Reloadable::detached();
    reload.dict = Some(dict);
    let mut s = Server::from_built(Built {
        pipeline: Arc::new(pipeline),
        runtime,
        config,
        reload,
        notes: Vec::new(),
    });
    send(
        &mut s,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"ios"}),
    );
    send(&mut s, "set_layout_geometry", geometry_params("qwerty-1"));
    s
}

fn send(s: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let out = s.handle_line(&line);
    let v: Value = serde_json::from_str(&out[0]).expect("応答は JSON");
    assert!(v.get("error").is_none(), "{method}: {v}");
    v["result"].clone()
}

/// 読みをなぞった JSON の `points`（`{x, y, t}` の配列）
fn trace(reading: &str, seed: u64) -> Value {
    let pts = generate(
        &layout(),
        reading,
        &SynthParams::normal(),
        &mut Rng::new(seed),
    )
    .expect("なぞれる読み");
    Value::Array(
        pts.iter()
            .map(|p| json!({"x": p.x, "y": p.y, "t": p.t}))
            .collect(),
    )
}

fn glide(s: &mut Server, reading: &str, seed: u64) -> Value {
    send(
        s,
        "glide_input",
        json!({"session_id":"s","points":trace(reading, seed),"layout_id":"qwerty-1"}),
    )
}

fn items(r: &Value) -> Vec<(String, Vec<String>, Option<String>)> {
    r["candidates"]["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|i| {
                    (
                        i["surface"].as_str().unwrap().to_string(),
                        i["sources"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|s| s.as_str().unwrap().to_string())
                            .collect(),
                        i["reading"].as_str().map(str::to_string),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn disabled_by_default() {
    let mut s = server(false, false);
    let r = glide(&mut s, "がっこう", 1);
    assert_eq!(r["consumed"], false, "{r}");
    assert!(r.get("preedit").is_none() && r.get("candidates").is_none());
}

#[test]
fn recognized_word_becomes_preedit_and_glide_candidates_without_llm() {
    let mut s = server(true, false);
    let r = glide(&mut s, "がっこう", 1);
    assert_eq!(r["consumed"], true, "{r}");
    assert_eq!(r["preedit"]["text"], "がっこう", "{r}");
    let c = items(&r);
    assert!(!c.is_empty(), "{r}");
    // 先頭は認識した読みの変換結果。出所は glide（に辞書）、読みが付く
    assert_eq!(c[0].0, "学校", "{c:?}");
    assert_eq!(c[0].1.first().map(String::as_str), Some("glide"), "{c:?}");
    assert!(c[0].1.contains(&"system_dict".to_string()), "{c:?}");
    assert_eq!(c[0].2.as_deref(), Some("がっこう"));
}

#[test]
fn works_with_the_llm_and_selection_commits() {
    let mut s = server(true, true);
    let r = glide(&mut s, "かんじ", 3);
    assert_eq!(r["consumed"], true, "{r}");
    assert_eq!(r["preedit"]["text"], "かんじ", "{r}");
    let c = items(&r);
    // LLM の変換結果（漢字）が先頭で、出所に llm と glide の両方が付く
    assert_eq!(c[0].0, "漢字", "{c:?}");
    assert!(c[0].1.contains(&"glide".to_string()) && c[0].1.contains(&"llm".to_string()));
    // 候補を選ぶと確定する
    let r = send(
        &mut s,
        "select_candidate",
        json!({"session_id":"s","index":0}),
    );
    assert_eq!(r["commit"], "漢字", "{r}");
}

#[test]
fn consecutive_strokes_commit_the_previous_word() {
    let mut s = server(true, false);
    glide(&mut s, "がっこう", 1);
    let r = glide(&mut s, "しんぶん", 2);
    assert_eq!(r["commit"], "学校", "{r}");
    assert_eq!(r["preedit"]["text"], "しんぶん", "{r}");
}

#[test]
fn stale_layout_and_garbage_are_not_consumed() {
    let mut s = server(true, false);
    // 古い配置の軌跡
    let r = send(
        &mut s,
        "glide_input",
        json!({"session_id":"s","points":trace("がっこう", 1),"layout_id":"old"}),
    );
    assert_eq!(r["consumed"], false, "{r}");
    // layout_id は省略できる。点が少なすぎる（タップ）・空は認識しない
    let r = send(
        &mut s,
        "glide_input",
        json!({"session_id":"s","points":[{"x":100.0,"y":40.0,"t":0.0}]}),
    );
    assert_eq!(r["consumed"], false, "{r}");
    let r = send(&mut s, "glide_input", json!({"session_id":"s","points":[]}));
    assert_eq!(r["consumed"], false, "{r}");
    // 不正な params はエラー
    let line = json!({"jsonrpc":"2.0","id":2,"method":"glide_input","params":{"session_id":"s"}});
    let out = s.handle_line(&line.to_string());
    assert!(out[0].contains("\"error\""), "{}", out[0]);
}

#[test]
fn without_geometry_nothing_is_recognized() {
    let mut config = Config::default();
    config.input.glide = true;
    let runtime = Runtime::detached(false);
    let pipeline = Pipeline::new(Some(dict_with_extra(EXTRA)), None, runtime.clone());
    let mut s = Server::from_built(Built {
        pipeline: Arc::new(pipeline),
        runtime,
        config,
        reload: Reloadable::detached(),
        notes: Vec::new(),
    });
    send(
        &mut s,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"ios"}),
    );
    let r = glide(&mut s, "がっこう", 1);
    assert_eq!(r["consumed"], false, "{r}");
}

#[test]
fn preloaded_index_file_is_used() {
    // 辞書から索引ファイルを作って読み直す（`cargo xtask build-glide-index` と同じ経路）
    let dict = dict_with_extra(EXTRA);
    let bytes = GlideIndex::dictionary_bytes(&dict);
    let dir = std::env::temp_dir().join(format!("nagi-glide-server-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("dict")).unwrap();
    GlideIndex::write_file(&dir.join("dict").join("nagi-glide.bin"), &bytes).unwrap();
    let loaded = nagi_server::setup::load_glide_index(Some(&dir))
        .expect("ファイルがある")
        .expect("読める");
    assert!(!loaded.is_empty());
    assert!(nagi_server::setup::load_glide_index(Some(&dir.join("none"))).is_none());
    // 壊れたファイルはエラー（呼び出し側が辞書から作る縮退に落ちる）
    std::fs::write(dir.join("dict").join("nagi-glide.bin"), b"broken").unwrap();
    assert!(
        nagi_server::setup::load_glide_index(Some(&dir))
            .unwrap()
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_only_mode_does_not_build_an_index_in_memory() {
    let dir = std::env::temp_dir().join(format!("nagi-glide-file-only-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("dict").join("nagi-glide.bin");

    // 索引ファイルが無い間は利用不可（consumed=false。パニックせず、入力も動かない）
    let mut s = server_with(true, false, Some(Some(path.clone())));
    let r = glide(&mut s, "がっこう", 1);
    assert_eq!(r["consumed"], false, "{r}");
    assert!(r.get("preedit").is_none() && r.get("candidates").is_none());
    // 通常のキー入力は動く
    let k = send(
        &mut s,
        "insert_reading",
        json!({"session_id":"s","input":{"type":"text","content":"k"}}),
    );
    assert_eq!(k["consumed"], true, "{k}");
    // （ここまでのキー入力は入力中のまま。確定されてもされなくても、次のなぞりの可否には関係しない）

    // 壊れたファイルも利用不可のまま
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"broken").unwrap();
    let r = glide(&mut s, "がっこう", 1);
    assert_eq!(r["consumed"], false, "{r}");

    // 後から（本体アプリが）索引を置くと、サーバーを作り直さなくても次のなぞりから使える
    let bytes = GlideIndex::dictionary_bytes(&dict_with_extra(EXTRA));
    GlideIndex::write_file(&path, &bytes).unwrap();
    let r = glide(&mut s, "がっこう", 1);
    assert_eq!(r["consumed"], true, "{r}");
    assert_eq!(r["preedit"]["segments"][0]["reading"], "がっこう", "{r}");

    // データディレクトリ無し（パスなし）は常に利用不可
    let mut none = server_with(true, false, Some(None));
    assert_eq!(glide(&mut none, "がっこう", 1)["consumed"], false);
    let _ = std::fs::remove_dir_all(&dir);
}
