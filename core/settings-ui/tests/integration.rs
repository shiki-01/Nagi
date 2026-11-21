//! 実 `imserver` と実 HTTP サーバーを使った統合テスト。`imserver` のバイナリが無い環境では skip する
//! （`cargo build -p nagi-server --bin imserver` または `cargo test --workspace` で作られる）。

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
    let dir = std::env::temp_dir().join(format!("nagi-settings-ui-{}-{tag}", std::process::id()));
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
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Reply {
        status,
        headers: head.to_ascii_lowercase(),
        body: body.to_owned(),
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

#[test]
fn full_stack_against_a_real_imserver() {
    let Some(imserver) = imserver_path() else {
        eprintln!("skip: imserver のバイナリが無い（cargo build -p nagi-server --bin imserver）");
        return;
    };
    let dir = temp_dir("full");
    let mut run = start(&imserver, &dir);
    let host = format!("127.0.0.1:{}", run.port);

    // 画面: トークンつきは 200 + セキュリティヘッダー、トークンなし・Host 違いは 403
    let page = http(
        run.port,
        "GET",
        &format!("/?t={}", run.token),
        &[("Host", &host)],
        "",
    );
    assert_eq!(page.status, 200);
    assert!(page.headers.contains("content-security-policy:"));
    assert!(page.headers.contains("cache-control: no-store"));
    assert!(page.headers.contains("x-content-type-options: nosniff"));
    assert!(!page.headers.contains("access-control-"));
    assert!(page.body.contains("Nagi 設定"));
    assert_eq!(
        http(run.port, "GET", "/", &[("Host", &host)], "").status,
        403
    );
    let bad_host = [("Host", "evil.example")];
    assert_eq!(
        http(
            run.port,
            "GET",
            &format!("/?t={}", run.token),
            &bad_host,
            ""
        )
        .status,
        403
    );

    // 項目の取得（imserver 起動 + initialize + get_settings が通っている）
    let reply = api(&run, "/api/settings", "{}");
    assert_eq!(reply.status, 200, "{}", reply.body);
    let screen: serde_json::Value = serde_json::from_str(&reply.body).unwrap();
    let config_path = screen["config_path"].as_str().expect("config_path");
    assert!(PathBuf::from(config_path).starts_with(&dir));
    let keys: Vec<&str> = screen["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["items"].as_array().unwrap())
        .map(|i| i["key"].as_str().unwrap())
        .collect();
    assert!(keys.contains(&"conversion.live_conversion_delay_ms"));
    assert!(!keys.contains(&"input.glide"), "iOS 専用の項目は出ない");

    // 更新 → config.toml に反映され、最新値が返る
    let body = r#"{"changes":[{"key":"conversion.live_conversion_delay_ms","value":{"type":"int","value":300}}]}"#;
    let reply = api(&run, "/api/update", body);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let v: serde_json::Value = serde_json::from_str(&reply.body).unwrap();
    assert_eq!(v["errors"], serde_json::json!([]));
    let written = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(
        written.contains("live_conversion_delay_ms = 300"),
        "{written}"
    );

    // 検証エラーはコアのメッセージがそのまま返り、ファイルは変わらない
    let body = r#"{"changes":[{"key":"conversion.live_conversion_delay_ms","value":{"type":"int","value":999999}}]}"#;
    let reply = api(&run, "/api/update", body);
    let v: serde_json::Value = serde_json::from_str(&reply.body).unwrap();
    assert_eq!(v["applied"], serde_json::json!([]));
    assert!(!v["errors"][0]["message"].as_str().unwrap().is_empty());
    assert!(
        std::fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .contains("live_conversion_delay_ms = 300")
    );

    // 学習データの全削除は通る
    assert_eq!(api(&run, "/api/clear_learning", "{}").status, 200);

    // ページを閉じる（/bye）と、猶予（5 秒）の後にプロセスが終了する
    let bye = http(
        run.port,
        "POST",
        &format!("/bye?t={}", run.token),
        &[("Host", &host), ("Content-Type", "application/json")],
        "{}",
    );
    assert_eq!(bye.status, 204);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = run.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "/bye 後に終了しない");
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_imserver_still_serves_the_page_with_an_error() {
    let dir = temp_dir("missing");
    let run = start(&dir.join("no-such-imserver"), &dir);
    let reply = api(&run, "/api/settings", "{}");
    assert_eq!(reply.status, 503);
    assert!(reply.body.contains("imserver"), "{}", reply.body);
    let _ = std::fs::remove_dir_all(&dir);
}
