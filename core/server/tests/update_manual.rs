//! `imserver --check-update` の本体（`update_manual`）の統合テスト。
//! ローカルの小さな HTTP サーバーが `latest.json` と配布物を返し、`update.check_url`（設定）を差し替えて検証する。
//! 署名検証は、`update_check` のテストと同じ既知の鍵・メッセージ・署名（秘密鍵は破棄済み）を使う。
#![cfg(feature = "update-check")]

use nagi_server::ServerOptions;
use nagi_server::update_manual::{CheckStatus, run_check_update_with_key};
use sha2::Digest;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

const PUBLIC_KEY_HEX: &str = "48880411a8c51a1e707022d84b711e9ed883a20143eb1734da57d11877fa3abc";
const PAYLOAD: &[u8] = b"nagi-update-test";
const SIGNATURE_HEX: &str = "d28314b502a250ea0471de96a5ec949bf254473f0ce3cc4c1078099e1434ef478a56a49bb49a7b60a25409be2342914b7d36782288e088885d1b83f5e824bc02";

/// パスごとに (ステータス, 本文) を返す。`routes` はベース URL から作る。接続を `max_requests` 回さばいて終わる。
/// ベース URL を返す
fn serve(
    routes: impl FnOnce(&str) -> Vec<(&'static str, u16, Vec<u8>)>,
    max_requests: usize,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = routes(&base);
    std::thread::spawn(move || {
        for _ in 0..max_requests {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, body) = routes
                .iter()
                .find(|(p, _, _)| *p == path)
                .map_or((404, b"not found".to_vec()), |(_, s, b)| (*s, b.clone()));
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    base
}

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("nagi-manual-it-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `update.auto_check = false`（手動は無視して実行できること）と `check_url` を書いた user-dir
fn user_dir(tmp: &Temp, check_url: &str) -> PathBuf {
    let dir = tmp.0.join("user");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        format!("[update]\nauto_check = false\ncheck_url = \"{check_url}\"\n"),
    )
    .unwrap();
    dir
}

fn manifest_json(base: &str, version: &str) -> Vec<u8> {
    let sha = format!("{:x}", sha2::Sha256::digest(PAYLOAD));
    format!(
        r#"{{"version":"{version}","notes_url":"https://example.com/notes","download_url":"{base}/pkg.zip","sha256":"{sha}","signature":"{SIGNATURE_HEX}"}}"#
    )
    .into_bytes()
}

#[test]
fn up_to_date_available_and_error() {
    // up_to_date
    let base = serve(
        |_| vec![("/latest.json", 200, br#"{"version":"0.1.14"}"#.to_vec())],
        1,
    );
    let tmp = Temp::new("a");
    let options = ServerOptions {
        user_dir: Some(user_dir(&tmp, &format!("{base}/latest.json"))),
        ..ServerOptions::default()
    };
    let r = run_check_update_with_key(&options, "0.1.14", PUBLIC_KEY_HEX);
    assert_eq!(r.status, CheckStatus::UpToDate, "{r:?}");
    assert_eq!(r.latest.as_deref(), Some("0.1.14"));
    // 確認の時刻が記録される（自動確認の間引きに反映）
    assert!(
        options
            .user_dir
            .as_ref()
            .unwrap()
            .join("update_check.json")
            .exists()
    );

    // available（--stage-updates なし → 取得しない）
    let base = serve(
        |_| {
            vec![(
                "/latest.json",
                200,
                manifest_json("http://unused", "0.1.15"),
            )]
        },
        1,
    );
    let options = ServerOptions {
        user_dir: Some(user_dir(&tmp, &format!("{base}/latest.json"))),
        ..ServerOptions::default()
    };
    let r = run_check_update_with_key(&options, "0.1.14", PUBLIC_KEY_HEX);
    assert_eq!(r.status, CheckStatus::Available, "{r:?}");
    assert_eq!(r.latest.as_deref(), Some("0.1.15"));
    assert_eq!(r.notes_url.as_deref(), Some("https://example.com/notes"));

    // error（HTTP 404）
    let base = serve(|_| vec![], 1);
    let options = ServerOptions {
        user_dir: Some(user_dir(&tmp, &format!("{base}/latest.json"))),
        ..ServerOptions::default()
    };
    let r = run_check_update_with_key(&options, "0.1.14", PUBLIC_KEY_HEX);
    assert_eq!(r.status, CheckStatus::Error, "{r:?}");
    assert!(r.error.is_some());
}

#[test]
fn stages_verified_update_and_reuses_it() {
    let tmp = Temp::new("b");
    // 要求は 5 回: 1 回目 = latest + 配布物、2 回目 = latest のみ（再取得しない）、3 回目 = latest + 配布物
    let base = serve(
        |base| {
            vec![
                ("/latest.json", 200, manifest_json(base, "0.1.15")),
                ("/pkg.zip", 200, PAYLOAD.to_vec()),
            ]
        },
        5,
    );
    let update_dir = tmp.0.join("update");
    let options = ServerOptions {
        user_dir: Some(user_dir(&tmp, &format!("{base}/latest.json"))),
        stage_updates: true,
        update_dir: Some(update_dir.clone()),
        ..ServerOptions::default()
    };
    let r = run_check_update_with_key(&options, "0.1.14", PUBLIC_KEY_HEX);
    assert_eq!(r.status, CheckStatus::Staged, "{r:?}");
    assert!(update_dir.join("ready.json").exists());
    assert!(update_dir.join("Nagi-windows-0.1.15.zip").exists());

    // 2 回目: 同じ版が保存済みなので配布物を取得せず staged
    let r = run_check_update_with_key(&options, "0.1.14", PUBLIC_KEY_HEX);
    assert_eq!(r.status, CheckStatus::Staged, "{r:?}");

    // 署名が違う鍵では検証に失敗して error（保存済みでない版を取得するため、別の update_dir）
    let options = ServerOptions {
        update_dir: Some(tmp.0.join("update2")),
        ..options
    };
    let r = run_check_update_with_key(&options, "0.1.14", &"0".repeat(64));
    assert_eq!(r.status, CheckStatus::Error, "{r:?}");
    assert_eq!(r.latest.as_deref(), Some("0.1.15"));
}
