//! ユーザー辞書・学習・抑制語の管理 API を、実 `imserver` と実 HTTP サーバーで通す統合テスト。
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
        std::env::temp_dir().join(format!("nagi-settings-ui-ud-{}-{tag}", std::process::id()));
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

/// 標準の base64（依存を増やさないための小さな実装）
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, &b)| a | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn post(r: &Running, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let reply = api(r, path, &body.to_string());
    let value = serde_json::from_str(&reply.body).unwrap_or(serde_json::Value::Null);
    (reply.status, value)
}

#[test]
fn userdict_learning_and_suppression_against_a_real_imserver() {
    use serde_json::json;
    let Some(imserver) = imserver_path() else {
        eprintln!("skip: imserver のバイナリが無い（cargo build -p nagi-server --bin imserver）");
        return;
    };
    let dir = temp_dir("userdict");
    let run = start(&imserver, &dir);

    // 登録 → 一覧（カタカナの読みはひらがなに直る）
    let (status, v) = post(
        &run,
        "/api/userdict/add",
        &json!({"entry": {"reading": "ヒナタ", "surface": "日向", "pos": "地名", "comment": "メモ"}}),
    );
    assert_eq!(
        (status, &v),
        (200, &json!({"added": true, "replaced": false}))
    );
    let (status, v) = post(&run, "/api/userdict/list", &json!({}));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["reading"], "ひなた");
    assert_eq!(v["items"][0]["file"], "registered.tsv");
    let file = std::fs::read_to_string(dir.join("user_dict/registered.tsv")).unwrap();
    assert_eq!(file, "ひなた\t日向\t地名\tメモ\n");

    // 検証エラーは 400 でメッセージが返る（入力内容は含まれない）
    let (status, v) = post(
        &run,
        "/api/userdict/add",
        &json!({"entry": {"reading": "漢字", "surface": "x"}}),
    );
    assert_eq!(status, 400, "{v}");
    assert!(v["error"].as_str().unwrap().contains("ひらがな"));
    let (status, _) = post(
        &run,
        "/api/userdict/add",
        &json!({"entry": {"reading": "ひなた", "surface": "日向"}}),
    );
    assert_eq!(status, 400, "重複");

    // 64KB を超える取り込み（3000 行）。行番号つきでスキップを返し、重複は上書きできる
    let mut text = String::new();
    for i in 0..3000 {
        text.push_str(&format!("a{i}\t語{i}\t名詞\t行{i}\n"));
    }
    text.push_str("ひなた\t日向\t人名\t新\n漢字\t不正\t名詞\n");
    assert!(text.len() > 64 * 1024, "{}", text.len());
    let b64 = base64(text.as_bytes());
    let (status, v) = post(
        &run,
        "/api/userdict/import",
        &json!({"data_base64": b64, "on_duplicate": "overwrite"}),
    );
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["replaced"], 1);
    assert_eq!(v["imported"], 3000);
    let lines: Vec<u64> = v["skipped_lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["line"].as_u64().unwrap())
        .collect();
    assert!(lines.contains(&3002), "{lines:?}");
    let file = std::fs::read_to_string(dir.join("user_dict/registered.tsv")).unwrap();
    assert_eq!(file, "ひなた\t日向\t人名\t新\n");

    // ページング・検索・書き出し
    let (_, v) = post(
        &run,
        "/api/userdict/list",
        &json!({"limit": 20, "offset": 10, "sort": "reading"}),
    );
    assert_eq!(v["items"].as_array().unwrap().len(), 20);
    let (_, v) = post(&run, "/api/userdict/list", &json!({"query": "日向"}));
    assert_eq!(v["matched"], 1);
    let (status, v) = post(&run, "/api/userdict/export", &json!({}));
    assert_eq!(status, 200);
    assert!(
        v["text"]
            .as_str()
            .unwrap()
            .contains("ひなた\t日向\t人名\t新\n")
    );

    // 削除
    let (_, v) = post(
        &run,
        "/api/userdict/remove",
        &json!({"items": [{"reading": "ひなた", "surface": "日向"}]}),
    );
    assert_eq!(v["removed"], 1);

    // 抑制語: 追加 → 一覧 → 統計 → 解除
    let (status, _) = post(
        &run,
        "/api/suppressed/add",
        &json!({"reading": "こうえん", "surface": "講演"}),
    );
    assert_eq!(status, 200);
    let (_, v) = post(&run, "/api/suppressed/list", &json!({}));
    assert_eq!(
        v["items"],
        json!([{"reading": "こうえん", "surface": "講演"}])
    );
    let (status, v) = post(&run, "/api/learning/stats", &json!({}));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["suppressed_entries"], 1);
    assert_eq!(v["enabled"], true);
    assert!(!v.to_string().contains("講演"));
    let (_, _) = post(
        &run,
        "/api/suppressed/remove",
        &json!({"reading": "こうえん", "surface": "講演"}),
    );
    let (_, v) = post(&run, "/api/suppressed/list", &json!({}));
    assert_eq!(v["items"], json!([]));

    // 学習データの全削除（既存の API）
    let (status, _) = post(&run, "/api/clear_learning", &json!({}));
    assert_eq!(status, 200);

    // 通常の API は 64KB まで
    let big = json!({"query": "a".repeat(70 * 1024)});
    let (status, _) = post(&run, "/api/userdict/list", &big);
    assert_eq!(status, 413);
    let _ = std::fs::remove_dir_all(&dir);
}
