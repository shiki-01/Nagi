//! HTTP サーバーのループ（`tiny_http`）。127.0.0.1 のランダムポートにだけバインドする。

use crate::backend::SettingsBackend;
use crate::http::{App, MAX_BODY, Request, Response, TOKEN_HEADER};
use std::io::Read;
use std::time::{Duration, Instant};
use tiny_http::{Header, Server};

/// 127.0.0.1 のランダムポートで待ち受ける。戻り値はサーバーとポート
pub fn bind() -> Result<(Server, u16), String> {
    let server =
        Server::http("127.0.0.1:0").map_err(|e| format!("待ち受けを開始できません: {e}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| "待ち受けのポートを取得できません".to_owned())?;
    Ok((server, port))
}

fn header_value(request: &tiny_http::Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_owned())
}

fn to_request(request: &mut tiny_http::Request) -> Request {
    let url = request.url().to_owned();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p.to_owned(), Some(q.to_owned())),
        None => (url, None),
    };
    let mut body = Vec::new();
    // 上限 + 1 バイトまで読み、超えていれば検査側が 413 にする
    let _ = request
        .as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut body);
    Request {
        method: request.method().as_str().to_ascii_uppercase(),
        path,
        query,
        host: header_value(request, "Host"),
        origin: header_value(request, "Origin"),
        content_type: header_value(request, "Content-Type"),
        token_header: header_value(request, TOKEN_HEADER),
        body,
    }
}

fn respond(request: tiny_http::Request, response: Response) {
    let security = response.security_headers();
    let header = |name: &str, value: &str| Header::from_bytes(name.as_bytes(), value.as_bytes());
    let mut out = tiny_http::Response::from_data(response.body).with_status_code(response.status);
    if let Ok(h) = header("Content-Type", response.content_type) {
        out.add_header(h);
    }
    for (name, value) in security {
        if let Ok(h) = header(name, &value) {
            out.add_header(h);
        }
    }
    let _ = request.respond(out);
}

/// リクエストを処理し続ける。操作が無い・ページが閉じられたら戻る
pub fn serve<B: SettingsBackend>(server: &Server, app: &mut App<B>) {
    loop {
        match server.recv_timeout(Duration::from_millis(500)) {
            Ok(Some(mut request)) => {
                let parsed = to_request(&mut request);
                let response = app.handle(&parsed, Instant::now());
                respond(request, response);
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if app.lifecycle.should_exit(Instant::now()) {
            break;
        }
    }
}
