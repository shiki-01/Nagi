//! キー割当の管理 API（`/api/keymap/*`）を、実 `imserver` と実 HTTP サーバーで通す統合テスト。
//! `imserver` のバイナリが無い環境では skip する（`cargo build -p nagi-server --bin imserver`）。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn imserver_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // target/<profile>/deps/<test> → target/<profile>/imserver
    let dir = exe.parent()?.parent()?;
    let name = if cfg!(windows) {
        "imserver.exe"
    } else {
        "imserver"
    };
    let path = dir.join(name);
    path.is_file().then_some(path)
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("nagi-settings-ui-km-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 起動した設定画面。テストが終わったら（失敗しても）必ず終了させる
struct Running {
    child: Child,
    port: u16,
    token: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(imserver: &std::path::Path, dir: &std::path::Path) -> Running {
    let url_file = dir.join("url.txt");
    let child = Command::new(env!("CARGO_BIN_EXE_nagi-settings"))
        .arg("--imserver")
        .arg(imserver)
        .arg("--user-dir")
        .arg(dir)
        .arg("--data-dir")
        .arg(dir.join("data"))
        // Windows の既定はネイティブのウィンドウなので、HTTP の経路を試すときは --web を付ける
        .arg("--web")
        .arg("--no-browser")
        .arg("--url-file")
        .arg(&url_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running {
        child,
        port: 0,
        token: String::new(),
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let url = loop {
        if let Ok(s) = std::fs::read_to_string(&url_file)
            && s.ends_with(|c: char| c.is_ascii_hexdigit())
        {
            break s;
        }
        assert!(Instant::now() < deadline, "URL ファイルができない");
        std::thread::sleep(Duration::from_millis(50));
    };
    // http://127.0.0.1:<port>/?t=<token>
    let rest = url.strip_prefix("http://127.0.0.1:").expect("URL の形式");
    let (port, rest) = rest.split_once("/?t=").expect("URL の形式");
    running.port = port.parse().unwrap();
    running.token = rest.to_owned();
    running
}

struct Reply {
    status: u16,
    body: String,
}

fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or((raw.len(), raw.len()), |i| (i, i + 4));
    let head = String::from_utf8_lossy(&raw[..split.0]).into_owned();
    let mut body = raw[split.1..].to_vec();
    // 本文が大きい（約 32KB 超）と tiny-http は chunked で返す
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Reply {
        status,
        body: String::from_utf8(body).unwrap(),
    }
}

/// `Transfer-Encoding: chunked` の本文を戻す
fn dechunk(mut rest: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = rest.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size =
            usize::from_str_radix(std::str::from_utf8(&rest[..eol]).unwrap().trim(), 16).unwrap();
        rest = &rest[eol + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

fn api(r: &Running, path: &str, body: &str) -> Reply {
    let host = format!("127.0.0.1:{}", r.port);
    http(
        r.port,
        "POST",
        path,
        &[
            ("Host", &host),
            ("Content-Type", "application/json"),
            ("X-Nagi-Token", &r.token),
        ],
        body,
    )
}

fn json(r: &Reply) -> serde_json::Value {
    serde_json::from_str(&r.body).unwrap_or_else(|_| panic!("JSON ではない: {}", r.body))
}

fn binding(keymap: &serde_json::Value, ctx: &str, key: &str) -> Option<(String, String)> {
    keymap["bindings"]
        .as_array()?
        .iter()
        .find(|b| b["context"] == ctx && b["key"] == key)
        .map(|b| {
            (
                b["action"].as_str().unwrap().to_owned(),
                b["source"].as_str().unwrap().to_owned(),
            )
        })
}

#[test]
fn keymap_edit_round_trip_against_a_real_imserver() {
    let Some(imserver) = imserver_path() else {
        eprintln!("skip: imserver のバイナリが無い（cargo build -p nagi-server --bin imserver）");
        return;
    };
    let dir = temp_dir("rt");
    std::fs::write(
        dir.join("config.toml"),
        "# 私の設定\n[update]\nauto_check = false # 確認しない\n",
    )
    .unwrap();
    let run = start(&imserver, &dir);

    // 一覧
    let r = api(&run, "/api/keymap/get", "{}");
    assert_eq!(r.status, 200, "{}", r.body);
    let v = json(&r);
    assert_eq!(v["preset"], "standard");
    assert!(v["bindings"].as_array().unwrap().len() > 30);
    assert!(
        v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "ToKatakana")
    );
    // 別のプリセットを見る（保存はしない）
    let r = api(&run, "/api/keymap/get", r#"{"preset":"ctrl_letters"}"#);
    assert_eq!(json(&r)["preset"], "ctrl_letters");
    assert!(
        !std::fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .contains("keymap")
    );

    // 検証（警告つきでも 200）
    let r = api(
        &run,
        "/api/keymap/validate",
        r#"{"toml":"[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n\"Enter\" = \"NoOp\"\n"}"#,
    );
    let v = json(&r);
    assert_eq!(v["errors"].as_array().unwrap().len(), 0);
    assert_eq!(v["warnings"][0]["kind"], "overrides_essential_key");
    assert_eq!(v["warnings"][0]["line"], 3);

    // 保存 → 即反映 → config.toml のコメント・他の節は残る
    let r = api(
        &run,
        "/api/keymap/set",
        r#"{"context":"composing","key":"Ctrl+Shift+k","action":"ToHankakuKana"}"#,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let v = json(&r);
    assert_eq!(
        binding(&v["keymap"], "composing", "Ctrl+Shift+k"),
        Some(("ToHankakuKana".to_owned(), "user".to_owned()))
    );
    let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(
        text.contains("# 私の設定") && text.contains("auto_check = false # 確認しない"),
        "{text}"
    );
    assert!(text.contains("[keymap.composing]"), "{text}");

    // 上書きできない文脈・解釈できないキーは 400 で、ファイルは変わらない
    let before = text.clone();
    for body in [
        r#"{"context":"direct","key":"F2","action":"Commit"}"#,
        r#"{"context":"common","key":"F2","action":"Commit"}"#,
        r#"{"context":"composing","key":"NotAKey","action":"Commit"}"#,
    ] {
        let r = api(&run, "/api/keymap/set", body);
        assert_eq!(r.status, 400, "{body} {}", r.body);
        assert!(json(&r)["error"].as_str().unwrap().len() > 3);
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        before
    );

    // 書き出し → リセット → 取り込みで元に戻る
    let exported = json(&api(&run, "/api/keymap/export", "{}"))["toml"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(exported.contains("Ctrl+Shift+k"), "{exported}");
    let r = api(&run, "/api/keymap/reset", r#"{"scope":"all"}"#);
    assert_eq!(r.status, 200);
    assert!(binding(&json(&r)["keymap"], "composing", "Ctrl+Shift+k").is_none());
    assert!(
        !std::fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .contains("keymap")
    );
    let body = serde_json::json!({"toml": exported}).to_string();
    let r = api(&run, "/api/keymap/import", &body);
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(binding(&json(&r)["keymap"], "composing", "Ctrl+Shift+k").is_some());

    // 行番号つきの取り込みエラー（何も変わらない）
    let r = api(
        &run,
        "/api/keymap/import",
        r#"{"toml":"[keymap.composing]\nF2 = \"Commit\"\n\"NotAKey\" = \"NoOp\"\n"}"#,
    );
    assert_eq!(r.status, 400);
    assert!(
        json(&r)["error"].as_str().unwrap().contains("3 行目"),
        "{}",
        r.body
    );

    // トークンなしは 403
    let host = format!("127.0.0.1:{}", run.port);
    let r = http(
        run.port,
        "POST",
        "/api/keymap/export",
        &[("Host", &host), ("Content-Type", "application/json")],
        "{}",
    );
    assert_eq!(r.status, 403);
}
