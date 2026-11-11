//! `cargo xtask e2e-headless` — UI E2E の共通シナリオ（`tests/e2e/`）を、OS の入力経路を介さずに
//! `imserver` へ JSON-RPC で直接再生して検証する。
//!
//! 目的は「シナリオの期待値がエンジンの実際の挙動と一致している」ことを OS 非依存で保証すること。
//! macOS / Windows の実ドライバーは同じシナリオを実キー送出で再生するので、ここで通らない期待値は
//! 実機ドライバー側でも通らない。`ios` 専用のシナリオ（`tap` / `flick`）はスキップする。
//!
//! 環境は `tests/e2e/README.md` の前提と同じにする: `--no-llm`、システム辞書は空の最小辞書
//! （ユーザー辞書の重ね合わせに辞書本体が要るため。`e2e-fixture-data` で作れる）、
//! `fixtures/user-dir/` の一時コピーを `--user-dir` と `NAGI_CONFIG_DIR` に渡す。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("リポジトリルート")
        .to_path_buf()
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("core").join("target"))
}

/// 空のシステム辞書（品詞 2 つ・連接コスト 0）を `<out>/dict/nagi-dict.bin` に書く。
/// ユーザー辞書は辞書本体が無いと重ね合わせされないため、決定的な変換にはこれが要る。
pub fn write_fixture_data(out: &Path) -> Result<(), String> {
    let mut b = nagi_dict::DictionaryBuilder::new();
    b.add_dictionary_text("e2e", "")
        .map_err(|e| format!("辞書の組み立て: {e}"))?;
    b.set_pos_defs_text("0 BOS/EOS,*,*,*,*,*,*\n1 名詞,一般,*,*,*,*,*\n")
        .map_err(|e| format!("品詞表: {e}"))?;
    b.set_connection_text("2\n0\n0\n0\n0", nagi_dict::ConnectionLayout::RidMajor)
        .map_err(|e| format!("連接コスト表: {e}"))?;
    let dict = b.build().map_err(|e| format!("辞書のビルド: {e}"))?;
    let dir = out.join("dict");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("nagi-dict.bin");
    fs::write(&path, dict.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

pub fn run_fixture_data(out: PathBuf) -> Result<(), String> {
    write_fixture_data(&out)?;
    eprintln!("最小辞書を書きました: {}", out.join("dict").display());
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
    }
    Ok(())
}

/// 起動した imserver と、再生中のテキスト欄の模擬（確定済み + 未確定）。
struct Engine {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
    committed: String,
    marked: String,
    candidates: Vec<String>,
}

impl Engine {
    fn spawn(imserver: &Path, user_dir: &Path, data_dir: &Path) -> Result<Engine, String> {
        let mut child = Command::new(imserver)
            .arg("--no-llm")
            .arg("--user-dir")
            .arg(user_dir)
            .arg("--data-dir")
            .arg(data_dir)
            .env("NAGI_CONFIG_DIR", user_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("imserver を起動できません ({}): {e}", imserver.display()))?;
        let stdin = child.stdin.take().ok_or("stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("stdout")?);
        let mut engine = Engine {
            child,
            stdin,
            stdout,
            next_id: 1,
            committed: String::new(),
            marked: String::new(),
            candidates: Vec::new(),
        };
        let r = engine.rpc(
            "initialize",
            json!({"protocol_version":1,"client_name":"e2e-headless","client_version":"0","os":"linux"}),
        )?;
        if r.get("error").is_some() {
            return Err(format!("initialize が失敗: {r}"));
        }
        Ok(engine)
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        writeln!(self.stdin, "{line}").map_err(|e| format!("送信: {e}"))?;
        self.stdin.flush().map_err(|e| format!("送信: {e}"))?;
        loop {
            let mut out = String::new();
            let n = self
                .stdout
                .read_line(&mut out)
                .map_err(|e| format!("受信: {e}"))?;
            if n == 0 {
                return Err("imserver が応答の途中で終了しました".into());
            }
            let v: Value = serde_json::from_str(&out).map_err(|e| format!("{e}: {out:?}"))?;
            // 非同期通知（id なし）は読み飛ばす
            if v.get("id").is_none() && v.get("method").is_some() {
                continue;
            }
            return Ok(v);
        }
    }

    fn send_key(&mut self, key: Value) -> Result<(), String> {
        let mut params = key;
        params["session_id"] = json!("e2e");
        params["keycode"] = json!(0);
        params["kind"] = json!("down");
        self.call_and_apply("process_key", params)
    }

    /// 要求を送り、応答の差分（commit / preedit / candidates）をテキスト欄の模擬へ反映する。
    fn call_and_apply(&mut self, method: &str, params: Value) -> Result<(), String> {
        let r = self.rpc(method, params)?;
        let Some(result) = r.get("result") else {
            return Err(format!("{method} が失敗: {r}"));
        };
        if let Some(c) = result.get("commit").and_then(Value::as_str) {
            self.committed.push_str(c);
        }
        if let Some(n) = result.get("delete_before").and_then(Value::as_u64) {
            let units: Vec<u16> = self.committed.encode_utf16().collect();
            let keep = units.len().saturating_sub(n as usize);
            self.committed = String::from_utf16_lossy(&units[..keep]);
        }
        if let Some(p) = result.get("preedit") {
            self.marked = p
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
        if let Some(c) = result.get("candidates") {
            self.candidates = c
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.get("surface").and_then(Value::as_str))
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
        }
        Ok(())
    }
}

/// 12 キーの表示ラベル → コアのキー名
fn kana_key(label: &str) -> Option<&'static str> {
    Some(match label {
        "あ" => "a",
        "か" => "ka",
        "さ" => "sa",
        "た" => "ta",
        "な" => "na",
        "は" => "ha",
        "ま" => "ma",
        "や" => "ya",
        "ら" => "ra",
        "わ" => "wa",
        "、。？！" => "punct",
        _ => return None,
    })
}

impl Engine {
    /// iOS の `tap`: 12 キーは常に `Tap`（トグル入力は設定 `input.toggle_input` で既定 OFF のため扱わない）、それ以外の機能キーは `perform_action` / `Variant`
    fn tap(&mut self, label: &str) -> Result<(), String> {
        let sid = "e2e";
        if let Some(key) = kana_key(label) {
            self.call_and_apply(
                "insert_reading",
                json!({"session_id":sid,"input":{"type":"key12","content":{"key":key,"gesture":"tap"}}}),
            )?;
            return Ok(());
        }
        let action = match label {
            "゛゜小" => {
                return self.call_and_apply(
                    "insert_reading",
                    json!({"session_id":sid,"input":{"type":"variant"}}),
                );
            }
            "空白" => json!({"type":"space"}),
            "改行" => json!({"type":"enter"}),
            "⌫" => json!({"type":"backspace"}),
            "←" => json!({"type":"move_cursor","content":"left"}),
            "→" => json!({"type":"move_cursor","content":"right"}),
            other => return Err(format!("未知のキー: {other:?}")),
        };
        self.call_and_apply("perform_action", json!({"session_id":sid,"action":action}))
    }

    /// iOS の `flick`: 12 キーのフリック（方向の文字はコアの表で決まる）
    fn flick(&mut self, label: &str, dir: &str) -> Result<(), String> {
        let key = kana_key(label).ok_or_else(|| format!("フリックできないキー: {label:?}"))?;
        let gesture = match dir {
            "left" => "flick_left",
            "up" => "flick_up",
            "right" => "flick_right",
            "down" => "flick_down",
            other => return Err(format!("未知の方向: {other}")),
        };
        self.call_and_apply(
            "insert_reading",
            json!({"session_id":"e2e","input":{"type":"key12","content":{"key":key,"gesture":gesture}}}),
        )
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `type` の 1 文字を key イベント（keysym・text・modifiers）にする。
fn char_key(c: char) -> Value {
    json!({
        "keysym": c as u32,
        "text": c.to_string(),
        "modifiers": { "shift": c.is_ascii_uppercase() },
    })
}

/// `key` の名前（`Shift+Left` 等）を key イベントにする。
fn named_key(spec: &str) -> Result<Value, String> {
    let parts: Vec<&str> = spec.split('+').collect();
    let (name, mods) = parts.split_last().ok_or("空のキー名")?;
    let mut modifiers = json!({});
    for m in mods {
        match *m {
            "Shift" => modifiers["shift"] = json!(true),
            "Ctrl" | "Control" => modifiers["ctrl"] = json!(true),
            "Alt" | "Option" => modifiers["alt"] = json!(true),
            "Meta" | "Cmd" => modifiers["meta"] = json!(true),
            other => return Err(format!("未知の修飾キー: {other}")),
        }
    }
    let (keysym, text) = match *name {
        "Space" => (0x0020u32, " "),
        "Return" => (0xff0d, ""),
        "Backspace" => (0xff08, ""),
        "Escape" => (0xff1b, ""),
        "Tab" => (0xff09, ""),
        "Delete" => (0xffff, ""),
        "Left" => (0xff51, ""),
        "Up" => (0xff52, ""),
        "Right" => (0xff53, ""),
        "Down" => (0xff54, ""),
        other => return Err(format!("未知のキー: {other}")),
    };
    Ok(json!({"keysym": keysym, "text": text, "modifiers": modifiers}))
}

/// `expect` を検証する。失敗の説明を返す。
fn check_expect(engine: &Engine, expect: &Value) -> Result<(), String> {
    let obj = expect.as_object().ok_or("expect はオブジェクトのはず")?;
    for (k, v) in obj {
        match k.as_str() {
            "marked" => cmp("marked", v.as_str().unwrap_or_default(), &engine.marked)?,
            "committed" => cmp(
                "committed",
                v.as_str().unwrap_or_default(),
                &engine.committed,
            )?,
            "text" => cmp(
                "text",
                v.as_str().unwrap_or_default(),
                &format!("{}{}", engine.committed, engine.marked),
            )?,
            "candidates_contain" => {
                for want in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    if !engine.candidates.iter().any(|c| c == want) {
                        return Err(format!(
                            "candidates_contain: {want:?} が候補にない（実測 {:?}）",
                            engine.candidates
                        ));
                    }
                }
            }
            "timeout_ms" => {}
            other => return Err(format!("未知の expect キー: {other}")),
        }
    }
    Ok(())
}

fn cmp(name: &str, want: &str, got: &str) -> Result<(), String> {
    if want == got {
        Ok(())
    } else {
        Err(format!("{name}: 期待 {want:?} / 実測 {got:?}"))
    }
}

/// 1 シナリオのステップを再生する。
fn play(engine: &mut Engine, steps: &[Value]) -> Result<(), String> {
    for (i, step) in steps.iter().enumerate() {
        let at = |m: String| format!("step {}: {m}", i + 1);
        if let Some(t) = step.get("type").and_then(Value::as_str) {
            for c in t.chars() {
                if !c.is_ascii() {
                    return Err(at(format!("type は ASCII のみ: {c:?}")));
                }
                engine.send_key(char_key(c)).map_err(at)?;
            }
        } else if let Some(k) = step.get("key").and_then(Value::as_str) {
            let key = named_key(k).map_err(at)?;
            engine.send_key(key).map_err(at)?;
        } else if let Some(keys) = step.get("tap").and_then(Value::as_array) {
            for k in keys.iter().filter_map(Value::as_str) {
                engine.tap(k).map_err(at)?;
            }
        } else if let Some(f) = step.get("flick") {
            let key = f["key"].as_str().unwrap_or_default();
            let dir = f["dir"].as_str().unwrap_or_default();
            engine.flick(key, dir).map_err(at)?;
        } else if let Some(e) = step.get("expect") {
            check_expect(engine, e).map_err(at)?;
        } else if step.get("screenshot").is_some() {
            // ヘッドレスでは画面が無いので何もしない
        } else {
            return Err(at(format!("未対応のステップ: {step}")));
        }
    }
    Ok(())
}

pub struct Options {
    pub imserver: Option<PathBuf>,
    pub out_dir: Option<PathBuf>,
    pub suite: Option<String>,
    /// ios 向けスイート（tap / flick）もコアの touch API で再生する
    pub ios: bool,
}

pub fn run(opts: Options) -> Result<(), String> {
    let e2e = repo_root().join("tests").join("e2e");
    let imserver = opts.imserver.unwrap_or_else(|| {
        target_dir()
            .join("debug")
            .join(format!("imserver{}", std::env::consts::EXE_SUFFIX))
    });
    if !imserver.exists() {
        return Err(format!(
            "{} がありません。先に `cargo build -p nagi-server --bin imserver` を実行してください",
            imserver.display()
        ));
    }
    let out_dir = opts
        .out_dir
        .unwrap_or_else(|| target_dir().join("e2e-headless"));
    fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;

    let work = std::env::temp_dir().join(format!("nagi-e2e-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    let data_dir = work.join("data");
    write_fixture_data(&data_dir)?;

    let mut files: Vec<PathBuf> = fs::read_dir(e2e.join("scenarios"))
        .map_err(|e| format!("scenarios を読めません: {e}"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();

    let mut results = Vec::new();
    let mut failed = 0usize;
    for file in files {
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let suite: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let suite_name = suite["suite"].as_str().unwrap_or("?").to_string();
        if opts.suite.as_deref().is_some_and(|s| s != suite_name) {
            continue;
        }
        let platforms = suite.get("platforms").and_then(Value::as_array);
        let runs_here = |x: &Value| x == "macos" || x == "windows" || (opts.ios && x == "ios");
        if platforms.is_some_and(|p| !p.iter().any(runs_here)) {
            eprintln!("SKIP {suite_name}（このランナーの対象外のプラットフォーム）");
            continue;
        }
        for sc in suite["scenarios"].as_array().into_iter().flatten() {
            let name = sc["name"].as_str().unwrap_or("?").to_string();
            let steps = sc["steps"].as_array().cloned().unwrap_or_default();
            // シナリオごとに新しいプロセスと一時 user-dir（状態を持ち越さない）
            let user_dir = work.join(format!("user-{suite_name}-{name}"));
            copy_dir(&e2e.join("fixtures").join("user-dir"), &user_dir)?;
            let outcome = Engine::spawn(&imserver, &user_dir, &data_dir)
                .and_then(|mut engine| play(&mut engine, &steps));
            let (ok, message) = match outcome {
                Ok(()) => (true, String::new()),
                Err(m) => (false, m),
            };
            if ok {
                println!("PASS {suite_name}/{name}");
            } else {
                failed += 1;
                println!("FAIL {suite_name}/{name}: {message}");
            }
            results.push(json!({"suite": suite_name, "name": name, "ok": ok, "message": message}));
        }
    }
    let _ = fs::remove_dir_all(&work);

    let results_path = out_dir.join("results.json");
    let body = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
    fs::write(&results_path, body).map_err(|e| format!("{}: {e}", results_path.display()))?;
    println!(
        "{} 件中 {} 件成功 ({})",
        results.len(),
        results.len() - failed,
        results_path.display()
    );
    if results.is_empty() {
        return Err("実行したシナリオがありません".into());
    }
    if failed > 0 {
        return Err(format!("{failed} 件失敗"));
    }
    Ok(())
}
