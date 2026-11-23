//! バックアップ API（`/api/backup/*`）を、実 `imserver` と実 HTTP サーバーで通す統合テスト。
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
        std::env::temp_dir().join(format!("nagi-settings-ui-bk-{}-{tag}", std::process::id()));
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
        // Windows の既定は WebView2 のウィンドウなので、HTTP の経路を試すときは --web を付ける
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
    #[allow(dead_code)]
    headers: String,
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
        headers: head.to_ascii_lowercase(),
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

fn post(r: &Running, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let reply = api(r, path, &body.to_string());
    let value = serde_json::from_str(&reply.body).unwrap_or(serde_json::Value::Null);
    (reply.status, value)
}

#[test]
fn export_preview_apply_and_undo_against_a_real_imserver() {
    use serde_json::json;
    let Some(imserver) = imserver_path() else {
        eprintln!("skip: imserver のバイナリが無い（cargo build -p nagi-server --bin imserver）");
        return;
    };
    let dir = temp_dir("flow");
    let config_a = "# 私の設定\n[conversion]\nsync_budget_ms = 200\n";
    std::fs::write(dir.join("config.toml"), config_a).unwrap();
    std::fs::create_dir_all(dir.join("user_dict")).unwrap();
    std::fs::write(dir.join("user_dict/mine.tsv"), "こうえん\t公苑\t名詞\t\n").unwrap();
    std::fs::write(dir.join("suppressed.tsv"), "あい\t愛\n").unwrap();
    let run = start(&imserver, &dir);

    // 書き出し（学習データは選ばない）
    let (status, exported) = post(
        &run,
        "/api/backup/export",
        &json!({"include": ["settings", "user_dict", "suppressed"]}),
    );
    assert_eq!(status, 200, "{exported}");
    let backup = exported["backup"].clone();
    assert_eq!(backup["version"], 1);
    assert_eq!(backup["entries"].as_array().unwrap().len(), 3);
    assert!(
        exported["file_name"]
            .as_str()
            .unwrap()
            .starts_with("nagi-backup-")
    );

    // 変更してから、プレビュー → 適用
    std::fs::write(
        dir.join("config.toml"),
        "[conversion]\nsync_budget_ms = 120\nuse_llm = false\n",
    )
    .unwrap();
    let (status, preview) = post(&run, "/api/backup/preview", &json!({"backup": backup}));
    assert_eq!(status, 200, "{preview}");
    let settings = preview["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "settings")
        .unwrap();
    assert_eq!(settings["changed"], 1);
    assert_eq!(settings["removed"], 1);
    // プレビューは何も書かない
    assert!(!dir.join("backups").exists());

    let (status, applied) = post(
        &run,
        "/api/backup/apply",
        &json!({"backup": backup, "apply": ["settings", "suppressed"]}),
    );
    assert_eq!(status, 200, "{applied}");
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        config_a
    );
    let auto = applied["auto_backup"].as_str().unwrap();
    assert!(dir.join("backups").join(auto).is_file());

    // 自動バックアップの一覧と、元に戻す
    let (status, list) = post(&run, "/api/backup/list", &json!({}));
    assert_eq!(status, 200);
    assert_eq!(list["backups"].as_array().unwrap().len(), 1);
    let (status, restored) = post(&run, "/api/backup/restore", &json!({}));
    assert_eq!(status, 200, "{restored}");
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        "[conversion]\nsync_budget_ms = 120\nuse_llm = false\n"
    );

    // 不正な入力は 400（何も変わらない）。復元の名前はパスを受け付けない
    let (status, _) = post(
        &run,
        "/api/backup/apply",
        &json!({"backup": {"version": 1, "entries": [{"kind":"settings","name":"config.toml","text":"[conversion\n"}]}, "apply": ["settings"]}),
    );
    assert_eq!(status, 400);
    let (status, _) = post(
        &run,
        "/api/backup/restore",
        &json!({"name": "../config.toml"}),
    );
    assert_eq!(status, 400);
    // 新しい画面の静的ファイルが配られる
    let host = format!("127.0.0.1:{}", run.port);
    let page = http(
        run.port,
        "GET",
        &format!("/backup.js?t={}", run.token),
        &[("Host", &host)],
        "",
    );
    assert_eq!(page.status, 200);
    let _ = std::fs::remove_dir_all(&dir);
}
