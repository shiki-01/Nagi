use super::*;
use crate::backend::{UpdateCheck, UpdateStatus};
use nagi_protocol::jsonrpc::error_code;
use nagi_protocol::settings::{
    IntRange, SettingError, SettingItem, SettingKind, SettingLevel, SettingPlatforms, SettingValue,
    SettingsSnapshot, UpdateSettingsResult,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PORT: u16 = 40123;

#[derive(Default)]
struct Calls {
    updates: Vec<Vec<SettingChange>>,
    cleared: u32,
    opened_files: Vec<PathBuf>,
    opened_folders: Vec<PathBuf>,
    checks: u32,
    applies: u32,
}

type Shared = Rc<RefCell<Calls>>;

fn item(key: &str, group: &str, platforms: SettingPlatforms, value: i32) -> SettingItem {
    SettingItem {
        key: key.into(),
        group: group.into(),
        label: key.into(),
        description: "説明".into(),
        kind: SettingKind::Int(IntRange {
            min: 0,
            max: 100,
            step: 1,
        }),
        value: Some(SettingValue::Int(value)),
        default: Some(SettingValue::Int(1)),
        explicit: false,
        requires_restart: false,
        level: SettingLevel::Simple,
        platforms,
    }
}

struct Fake {
    value: i32,
    calls: Shared,
    fail: Option<BackendError>,
    notes_url: Option<String>,
}

impl Fake {
    fn snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            items: vec![
                item("a.x", "グループ", SettingPlatforms::Both, self.value),
                item("a.ios", "グループ", SettingPlatforms::Ios, 0),
            ],
            error: None,
        }
    }
}

impl SettingsBackend for Fake {
    fn get_settings(&mut self) -> Result<SettingsSnapshot, BackendError> {
        match &self.fail {
            Some(e) => Err(e.clone()),
            None => Ok(self.snapshot()),
        }
    }

    fn update_settings(
        &mut self,
        changes: Vec<SettingChange>,
    ) -> Result<UpdateSettingsResult, BackendError> {
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        self.calls.borrow_mut().updates.push(changes.clone());
        let mut applied = Vec::new();
        let mut errors = Vec::new();
        for c in changes {
            match c.value {
                Some(SettingValue::Int(n)) if (0..=100).contains(&n) => {
                    self.value = n;
                    applied.push(c.key);
                }
                _ => errors.push(SettingError {
                    key: c.key,
                    message: "0 から 100 の整数にしてください".into(),
                }),
            }
        }
        Ok(UpdateSettingsResult {
            applied,
            errors,
            snapshot: self.snapshot(),
        })
    }

    fn clear_learning(&mut self) -> Result<(), BackendError> {
        self.calls.borrow_mut().cleared += 1;
        Ok(())
    }

    fn config_path(&mut self) -> Result<Option<String>, BackendError> {
        Ok(Some("/u/Nagi/config.toml".into()))
    }

    fn check_update(&mut self) -> Result<UpdateCheck, BackendError> {
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        self.calls.borrow_mut().checks += 1;
        Ok(UpdateCheck {
            current: "0.1.14".into(),
            latest: Some("0.1.15".into()),
            status: UpdateStatus::Staged,
            notes_url: self.notes_url.clone(),
            error: None,
        })
    }

    fn apply_update(&mut self) -> Result<(), BackendError> {
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        self.calls.borrow_mut().applies += 1;
        Ok(())
    }
}

struct FakeOpener(Shared);

impl Opener for FakeOpener {
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }
    fn open_file_in_editor(&self, path: &Path) -> Result<(), String> {
        self.0.borrow_mut().opened_files.push(path.to_path_buf());
        Ok(())
    }
    fn open_folder(&self, path: &Path) -> Result<(), String> {
        self.0.borrow_mut().opened_folders.push(path.to_path_buf());
        Ok(())
    }
}

fn app_with(fail: Option<BackendError>) -> (App<Fake>, Shared, Instant) {
    let calls = Shared::default();
    let now = Instant::now();
    let backend = Fake {
        value: 5,
        calls: calls.clone(),
        fail,
        notes_url: Some("https://example.com/notes".into()),
    };
    let mut app = App::new(
        backend,
        Box::new(FakeOpener(calls.clone())),
        TOKEN.to_owned(),
        PORT,
        now,
    );
    app.load_config_path();
    (app, calls, now)
}

fn app() -> (App<Fake>, Shared, Instant) {
    app_with(None)
}

fn host() -> Option<String> {
    Some(format!("127.0.0.1:{PORT}"))
}

fn page_request() -> Request {
    Request {
        method: "GET".into(),
        path: "/".into(),
        query: Some(format!("t={TOKEN}")),
        host: host(),
        ..Request::default()
    }
}

fn api_request(path: &str, body: &str) -> Request {
    Request {
        method: "POST".into(),
        path: path.into(),
        host: host(),
        origin: Some(format!("http://127.0.0.1:{PORT}")),
        content_type: Some("application/json".into()),
        token_header: Some(TOKEN.into()),
        body: body.as_bytes().to_vec(),
        ..Request::default()
    }
}

fn json_of(r: &Response) -> Value {
    serde_json::from_slice(&r.body).unwrap()
}

#[test]
fn page_is_served_with_a_valid_token_and_a_strict_csp() {
    let (mut app, _, now) = app();
    let r = app.handle(&page_request(), now);
    assert_eq!(r.status, 200);
    assert!(r.content_type.starts_with("text/html"));
    let html = String::from_utf8(r.body.clone()).unwrap();
    assert!(html.contains(TOKEN), "meta にトークンが入る");
    assert!(html.contains(&format!("src=\"/app.js?t={TOKEN}\"")));
    assert!(!html.contains("{{"), "プレースホルダーが残っていない");
    // 同一オリジンの外部ファイルだけ。インライン・nonce・unsafe-* は使わない
    for directive in [
        "default-src 'none'",
        "script-src 'self'",
        "style-src 'self'",
        "img-src 'self' data:",
        "connect-src 'self'",
        "frame-ancestors 'none'",
    ] {
        assert!(r.csp.contains(directive), "{directive}: {}", r.csp);
    }
    assert!(!r.csp.contains("unsafe-"));
    assert!(!r.csp.contains("nonce"));
}

#[test]
fn static_files_are_served_with_the_token_and_the_right_types() {
    let (mut app, _, now) = app();
    for (path, content_type) in [
        ("/app.css", "text/css"),
        ("/app.js", "text/javascript"),
        ("/motion.css", "text/css"),
        ("/icon.svg", "image/svg+xml"),
    ] {
        let mut req = page_request();
        req.path = path.into();
        let r = app.handle(&req, now);
        assert_eq!(r.status, 200, "{path}");
        assert!(r.content_type.starts_with(content_type), "{path}");
        assert!(!r.body.is_empty());
        let headers = r.security_headers();
        assert!(headers.iter().any(|(k, _)| *k == "X-Content-Type-Options"));
        // トークンなし・違う・ヘッダーだけ・Host 違い・POST は通らない
        let mut no_token = req.clone();
        no_token.query = None;
        assert_eq!(app.handle(&no_token, now).status, 403, "{path}");
        let mut wrong = req.clone();
        wrong.query = Some("t=wrong".into());
        assert_eq!(app.handle(&wrong, now).status, 403, "{path}");
        let mut header_only = no_token.clone();
        header_only.token_header = Some(TOKEN.into());
        assert_eq!(app.handle(&header_only, now).status, 403, "{path}");
        let mut bad_host = req.clone();
        bad_host.host = Some("evil.example".into());
        assert_eq!(app.handle(&bad_host, now).status, 403, "{path}");
        let mut post = req.clone();
        post.method = "POST".into();
        post.token_header = Some(TOKEN.into());
        assert_eq!(app.handle(&post, now).status, 405, "{path}");
    }
    // 配らないパスは（トークンがあっても）404
    let mut other = page_request();
    other.path = "/index.html".into();
    assert_eq!(
        app.handle(&other, now).status,
        403,
        "既知でないパスはヘッダーのトークンが要る"
    );
    other.query = None;
    other.token_header = Some(TOKEN.into());
    assert_eq!(app.handle(&other, now).status, 404);
}

#[test]
fn page_url_carries_the_token_only_there() {
    let (app, _, _) = app();
    assert_eq!(
        app.page_url(),
        format!("http://127.0.0.1:{PORT}/?t={TOKEN}")
    );
}

#[test]
fn every_response_has_security_headers_and_no_cors() {
    let (mut app, _, now) = app();
    for r in [
        app.handle(&page_request(), now),
        app.handle(&Request::default(), now),
        app.handle(&api_request("/api/settings", "{}"), now),
    ] {
        let headers = r.security_headers();
        let get = |n: &str| {
            headers
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("Cache-Control"), Some("no-store"));
        assert_eq!(get("X-Content-Type-Options"), Some("nosniff"));
        assert!(get("Content-Security-Policy").is_some_and(|c| c.contains("default-src 'none'")));
        assert!(
            headers
                .iter()
                .all(|(k, _)| !k.to_ascii_lowercase().starts_with("access-control-"))
        );
    }
}

#[test]
fn missing_or_wrong_token_is_forbidden_everywhere() {
    let (mut app, calls, now) = app();
    // 画面: トークンなし・違う・長さ違い
    for query in [None, Some("t=wrong".to_owned()), Some("t=".to_owned())] {
        let mut req = page_request();
        req.query = query;
        assert_eq!(app.handle(&req, now).status, 403);
    }
    // 画面の取得にヘッダーのトークンは使えない（URL のトークンのみ）
    let mut req = page_request();
    req.query = None;
    req.token_header = Some(TOKEN.into());
    assert_eq!(app.handle(&req, now).status, 403);
    // API: ヘッダーなし・違う。URL のトークンは使えない
    for path in [
        "/api/settings",
        "/api/update",
        "/api/open",
        "/api/clear_learning",
        "/nope",
    ] {
        let mut req = api_request(path, "{}");
        req.token_header = None;
        assert_eq!(app.handle(&req, now).status, 403, "{path}");
        req.token_header = Some("x".repeat(TOKEN.len()));
        assert_eq!(app.handle(&req, now).status, 403, "{path}");
        req.token_header = None;
        req.query = Some(format!("t={TOKEN}"));
        assert_eq!(app.handle(&req, now).status, 403, "{path}");
    }
    assert!(calls.borrow().updates.is_empty());
    assert_eq!(calls.borrow().cleared, 0);
}

#[test]
fn host_must_be_loopback_with_the_bound_port() {
    let (mut app, _, now) = app();
    for bad in [
        None,
        Some("evil.example".to_owned()),
        Some("evil.example:40123".to_owned()),
        Some("127.0.0.1".to_owned()),
        Some("127.0.0.1:1".to_owned()),
        Some("localhost:40123".to_owned()),
        Some("127.0.0.1:40123.evil.example".to_owned()),
    ] {
        let mut req = page_request();
        req.host = bad.clone();
        assert_eq!(app.handle(&req, now).status, 403, "{bad:?}");
        let mut req = api_request("/api/settings", "{}");
        req.host = bad.clone();
        assert_eq!(app.handle(&req, now).status, 403, "{bad:?}");
    }
}

#[test]
fn foreign_origin_is_rejected_but_missing_origin_is_allowed() {
    let (mut app, _, now) = app();
    for bad in [
        "http://evil.example",
        "null",
        "http://127.0.0.1:1",
        "https://127.0.0.1:40123",
        "http://localhost:40123",
    ] {
        let mut req = api_request("/api/settings", "{}");
        req.origin = Some(bad.into());
        assert_eq!(app.handle(&req, now).status, 403, "{bad}");
    }
    let mut req = api_request("/api/settings", "{}");
    req.origin = None;
    assert_eq!(app.handle(&req, now).status, 200);
}

#[test]
fn api_requires_post_and_json_content_type() {
    let (mut app, calls, now) = app();
    let mut req = api_request("/api/settings", "{}");
    req.method = "GET".into();
    assert_eq!(app.handle(&req, now).status, 405);
    req.method = "PUT".into();
    assert_eq!(app.handle(&req, now).status, 405);

    for ct in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
        Some("multipart/form-data"),
    ] {
        let mut req = api_request("/api/update", r#"{"changes":[]}"#);
        req.content_type = ct.map(str::to_owned);
        assert_eq!(app.handle(&req, now).status, 415, "{ct:?}");
    }
    let mut req = api_request("/api/settings", "{}");
    req.content_type = Some("Application/JSON; charset=utf-8".into());
    assert_eq!(app.handle(&req, now).status, 200);
    assert!(calls.borrow().updates.is_empty());

    // POST / は不可、未知のパスは 404
    let mut req = page_request();
    req.method = "POST".into();
    req.token_header = Some(TOKEN.into());
    assert_eq!(app.handle(&req, now).status, 405);
    let mut req = api_request("/nope", "{}");
    req.method = "GET".into();
    assert_eq!(app.handle(&req, now).status, 404);
    assert_eq!(app.handle(&api_request("/api/nope", "{}"), now).status, 404);
}

#[test]
fn oversized_body_is_rejected() {
    let (mut app, _, now) = app();
    let big = " ".repeat(MAX_BODY + 1);
    assert_eq!(
        app.handle(&api_request("/api/update", &big), now).status,
        413
    );
}

#[test]
fn get_settings_returns_screen_json_without_ios_items() {
    let (mut app, _, now) = app();
    let r = app.handle(&api_request("/api/settings", "{}"), now);
    assert_eq!(r.status, 200);
    let v = json_of(&r);
    assert_eq!(v["config_path"], "/u/Nagi/config.toml");
    let items = v["groups"][0]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["key"], "a.x");
    assert_eq!(items[0]["value"], 5);
}

#[test]
fn update_round_trip_returns_fresh_screen() {
    let (mut app, calls, now) = app();
    let body = r#"{"changes":[{"key":"a.x","value":{"type":"int","value":42}}]}"#;
    let r = app.handle(&api_request("/api/update", body), now);
    assert_eq!(r.status, 200);
    let v = json_of(&r);
    assert_eq!(v["applied"], json!(["a.x"]));
    assert_eq!(v["errors"], json!([]));
    assert_eq!(v["screen"]["groups"][0]["items"][0]["value"], 42);
    assert_eq!(
        calls.borrow().updates[0],
        vec![SettingChange {
            key: "a.x".into(),
            value: Some(SettingValue::Int(42))
        }]
    );
    // value 省略・null は「既定に戻す」としてそのまま渡る
    let body = r#"{"changes":[{"key":"a.x","value":null}]}"#;
    app.handle(&api_request("/api/update", body), now);
    assert_eq!(calls.borrow().updates[1][0].value, None);
}

#[test]
fn validation_errors_pass_through_to_the_page() {
    let (mut app, _, now) = app();
    let body = r#"{"changes":[{"key":"a.x","value":{"type":"int","value":999}}]}"#;
    let v = json_of(&app.handle(&api_request("/api/update", body), now));
    assert_eq!(v["applied"], json!([]));
    assert_eq!(
        v["errors"],
        json!([{"key":"a.x","message":"0 から 100 の整数にしてください"}])
    );
    // 値は変わっていない
    assert_eq!(v["screen"]["groups"][0]["items"][0]["value"], 5);
}

#[test]
fn malformed_update_bodies_are_bad_requests() {
    let (mut app, calls, now) = app();
    for body in [
        "",
        "{",
        r#"{"changes":"x"}"#,
        r#"{"changes":[{"value":1}]}"#,
        r#"{"changes":[{"key":"a","value":{"type":"nope","value":1}}]}"#,
    ] {
        assert_eq!(
            app.handle(&api_request("/api/update", body), now).status,
            400,
            "{body}"
        );
    }
    assert!(calls.borrow().updates.is_empty());
}

#[test]
fn backend_errors_map_to_status_and_keep_the_message() {
    let (mut app, _, now) = app_with(Some(BackendError::rpc(
        error_code::INVALID_PARAMS,
        "invalid params: 範囲外",
    )));
    let r = app.handle(&api_request("/api/update", r#"{"changes":[]}"#), now);
    assert_eq!(r.status, 400);
    assert_eq!(json_of(&r)["error"], "invalid params: 範囲外");

    let (mut app, _, now) = app_with(Some(BackendError::transport("設定エンジンが終了しました")));
    let r = app.handle(&api_request("/api/settings", "{}"), now);
    assert_eq!(r.status, 503);
    assert_eq!(json_of(&r)["error"], "設定エンジンが終了しました");

    let (mut app, _, now) = app_with(Some(BackendError::rpc(error_code::INTERNAL_ERROR, "x")));
    assert_eq!(
        app.handle(&api_request("/api/settings", "{}"), now).status,
        502
    );
}

#[test]
fn open_actions_use_fixed_targets_only() {
    let (mut app, calls, now) = app();
    let r = app.handle(&api_request("/api/open", r#"{"target":"config"}"#), now);
    assert_eq!(r.status, 200);
    assert_eq!(
        calls.borrow().opened_files,
        vec![PathBuf::from("/u/Nagi/config.toml")]
    );
    // 任意のパスは受け付けない
    for body in [r#"{"target":"/etc/passwd"}"#, r#"{"target":".."}"#, "{}"] {
        let r = app.handle(&api_request("/api/open", body), now);
        assert_eq!(r.status, 400, "{body}");
    }
    assert_eq!(calls.borrow().opened_files.len(), 1);
    assert!(calls.borrow().opened_folders.is_empty());
    // /u/Nagi は存在しないので、ユーザー辞書フォルダは開けない（500 とメッセージ）
    let r = app.handle(&api_request("/api/open", r#"{"target":"user_dict"}"#), now);
    assert_eq!(r.status, 500);
}

#[test]
fn user_dict_dir_prefers_subfolder_then_user_dir() {
    let cfg = Path::new("/u/Nagi/config.toml");
    let only =
        |dirs: &'static [&'static str]| move |p: &Path| dirs.iter().any(|d| Path::new(d) == p);
    assert_eq!(
        user_dict_dir(cfg, only(&["/u/Nagi", "/u/Nagi/user_dict"])),
        Some(PathBuf::from("/u/Nagi/user_dict"))
    );
    assert_eq!(
        user_dict_dir(cfg, only(&["/u/Nagi"])),
        Some(PathBuf::from("/u/Nagi"))
    );
    assert_eq!(user_dict_dir(cfg, only(&[])), None);
    assert_eq!(user_dict_dir(Path::new("config.toml"), only(&[""])), None);
}

#[test]
fn clear_learning_calls_the_backend() {
    let (mut app, calls, now) = app();
    let r = app.handle(&api_request("/api/clear_learning", "{}"), now);
    assert_eq!(r.status, 200);
    assert_eq!(calls.borrow().cleared, 1);
}

#[test]
fn bye_starts_the_exit_timer_and_a_following_request_cancels_it() {
    let (mut app, _, now) = app();
    // sendBeacon: ヘッダーなし・URL のトークン
    let beacon = Request {
        method: "POST".into(),
        path: "/bye".into(),
        query: Some(format!("t={TOKEN}")),
        host: host(),
        origin: Some(format!("http://127.0.0.1:{PORT}")),
        content_type: Some("application/json".into()),
        body: b"{}".to_vec(),
        ..Request::default()
    };
    let t1 = now + Duration::from_secs(10);
    assert_eq!(app.handle(&beacon, t1).status, 204);
    assert!(!app.lifecycle.should_exit(t1 + Duration::from_secs(4)));
    assert!(app.lifecycle.should_exit(t1 + Duration::from_secs(5)));
    // 再読み込み: 猶予内に API が来れば取り消される
    app.handle(
        &api_request("/api/settings", "{}"),
        t1 + Duration::from_secs(2),
    );
    assert!(!app.lifecycle.should_exit(t1 + Duration::from_secs(60)));

    // トークンなしの /bye は無視（403）され、終了もしない
    let mut bad = beacon.clone();
    bad.query = None;
    assert_eq!(app.handle(&bad, t1).status, 403);
    let mut bad_host = beacon;
    bad_host.host = Some("evil.example".into());
    assert_eq!(app.handle(&bad_host, t1).status, 403);
}

#[test]
fn only_authenticated_requests_reset_the_idle_timer() {
    let (mut app, _, now) = app();
    let mut req = page_request();
    req.query = None;
    app.handle(&req, now + Duration::from_secs(14 * 60));
    assert!(
        app.lifecycle
            .should_exit(now + Duration::from_secs(15 * 60))
    );
    app.handle(&page_request(), now + Duration::from_secs(14 * 60));
    assert!(
        !app.lifecycle
            .should_exit(now + Duration::from_secs(15 * 60))
    );
}

#[test]
fn update_check_round_trips_json_and_filters_the_notes_url() {
    let (mut app, calls, now) = app();
    let r = app.handle(&api_request("/api/update/check", "{}"), now);
    assert_eq!(r.status, 200);
    let v = json_of(&r);
    assert_eq!(v["current"], "0.1.14");
    assert_eq!(v["latest"], "0.1.15");
    assert_eq!(v["status"], "staged");
    assert_eq!(v["notes_url"], "https://example.com/notes");
    assert!(v["error"].is_null());
    assert_eq!(calls.borrow().checks, 1);
    assert_eq!(calls.borrow().applies, 0);

    // https 以外・引用符や空白を含む URL は画面に渡さない
    for bad in [
        "javascript:alert(1)",
        "http://example.com/",
        "https://",
        "https://e.com/\"x",
        "https://e.com/a b",
    ] {
        let (mut other, _, now) = app_with(None);
        other.backend.notes_url = Some(bad.into());
        let v = json_of(&other.handle(&api_request("/api/update/check", "{}"), now));
        assert!(v["notes_url"].is_null(), "{bad}");
    }
}

#[test]
fn update_apis_require_token_origin_post_and_json() {
    let (mut app, calls, now) = app();
    for path in ["/api/update/check", "/api/update/apply"] {
        let mut no_token = api_request(path, "{}");
        no_token.token_header = None;
        assert_eq!(app.handle(&no_token, now).status, 403, "{path}");
        let mut wrong = api_request(path, "{}");
        wrong.token_header = Some("x".repeat(64));
        assert_eq!(app.handle(&wrong, now).status, 403, "{path}");
        let mut foreign = api_request(path, "{}");
        foreign.origin = Some("http://evil.example".into());
        assert_eq!(app.handle(&foreign, now).status, 403, "{path}");
        let mut bad_host = api_request(path, "{}");
        bad_host.host = Some("evil.example".into());
        assert_eq!(app.handle(&bad_host, now).status, 403, "{path}");
        let mut get = api_request(path, "{}");
        get.method = "GET".into();
        assert_eq!(app.handle(&get, now).status, 405, "{path}");
        let mut text = api_request(path, "{}");
        text.content_type = Some("text/plain".into());
        assert_eq!(app.handle(&text, now).status, 415, "{path}");
    }
    assert_eq!(calls.borrow().checks, 0);
    assert_eq!(calls.borrow().applies, 0);
}

#[test]
fn apply_is_a_fixed_action_that_ignores_the_body() {
    let (mut app, calls, now) = app();
    // 本文に何を入れても、パス・引数としては使われない（バックエンドは引数を取らない）
    for body in [
        "{}",
        r#"{"script":"C:/evil.ps1","args":["-x"]}"#,
        "not json",
    ] {
        let r = app.handle(&api_request("/api/update/apply", body), now);
        assert_eq!(r.status, 200, "{body}");
    }
    assert_eq!(calls.borrow().applies, 3);
}

#[test]
fn update_backend_errors_map_to_status_and_keep_the_message() {
    let (mut app, _, now) = app_with(Some(BackendError::transport("起動できません")));
    for path in ["/api/update/check", "/api/update/apply"] {
        let r = app.handle(&api_request(path, "{}"), now);
        assert_eq!(r.status, 503, "{path}");
        assert_eq!(json_of(&r)["error"], "起動できません");
    }
}

#[test]
fn start_fragment_is_appended_to_the_page_url_and_version_is_in_the_screen() {
    let (mut app, _, now) = app();
    assert!(!app.page_url().contains('#'));
    app.set_start_fragment(Some("update-check"));
    assert_eq!(
        app.page_url(),
        format!("http://127.0.0.1:{PORT}/?t={TOKEN}#update-check")
    );
    let v = json_of(&app.handle(&api_request("/api/settings", "{}"), now));
    assert_eq!(v["app_version"], env!("CARGO_PKG_VERSION"));
}

// ---- 動作確認（診断）の API ----

use crate::diag::{ItemId, ItemResult, Phase, Progress, Report, Status};
use std::sync::atomic::{AtomicBool, Ordering};

/// 診断サービスのフェイク。`gate` が開くまで全項目の実行を止める（実行中の状態を観察するため）
struct FakeDiag {
    gate: Arc<AtomicBool>,
    saves: std::sync::Mutex<Vec<usize>>,
    panic_in_run: bool,
}

impl FakeDiag {
    fn new(open: bool) -> Arc<Self> {
        Arc::new(Self {
            gate: Arc::new(AtomicBool::new(open)),
            saves: std::sync::Mutex::new(Vec::new()),
            panic_in_run: false,
        })
    }
}

impl DiagService for FakeDiag {
    fn run_all(
        &self,
        cancel: Arc<AtomicBool>,
        progress: &mut dyn FnMut(&Progress),
    ) -> Result<Report, String> {
        assert!(!self.panic_in_run, "診断の中の panic");
        let total = ItemId::ALL.len();
        let mut items = Vec::new();
        for (index, &id) in ItemId::ALL.iter().enumerate() {
            progress(&Progress {
                index,
                total,
                id,
                phase: Phase::Started,
                result: None,
            });
            while !self.gate.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(2));
            }
            let result = if cancel.load(Ordering::Relaxed) {
                ItemResult::skipped(id, "中断されました")
            } else {
                ItemResult::new(id, Status::Ok, "良好")
            };
            progress(&Progress {
                index,
                total,
                id,
                phase: Phase::Finished,
                result: Some(&result),
            });
            items.push(result);
        }
        Ok(Report {
            tool_version: "0".into(),
            generated_at: "t".into(),
            environment: "e".into(),
            items,
        })
    }

    fn run_one(&self, id: ItemId, _cancel: Arc<AtomicBool>) -> Result<ItemResult, String> {
        Ok(ItemResult::manual(id, "目視", "1. 開く"))
    }

    fn copy(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }

    fn save(&self, report: &Report) -> Result<String, String> {
        self.saves.lock().unwrap().push(report.items.len());
        Ok("%TEMP%/nagi-diag.txt".into())
    }
}

fn diag_app(service: Arc<FakeDiag>) -> (App<Fake>, Instant) {
    let (mut app, _, now) = app();
    app.set_diag_service(service);
    (app, now)
}

fn diag_status(app: &mut App<Fake>, now: Instant) -> Value {
    let r = app.handle(&api_request("/api/diag/status", "{}"), now);
    assert_eq!(r.status, 200);
    json_of(&r)
}

fn wait_idle(app: &mut App<Fake>, now: Instant) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let v = diag_status(app, now);
        if v["running"] == false {
            return v;
        }
        assert!(Instant::now() < deadline, "診断が終わらない");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn diag_apis_require_token_origin_post_and_json() {
    let (mut app, now) = diag_app(FakeDiag::new(true));
    for path in [
        "/api/diag/status",
        "/api/diag/start",
        "/api/diag/cancel",
        "/api/diag/report",
        "/api/diag/save",
    ] {
        let mut no_token = api_request(path, "{}");
        no_token.token_header = None;
        assert_eq!(app.handle(&no_token, now).status, 403, "{path}");
        let mut query_token = no_token.clone();
        query_token.query = Some(format!("t={TOKEN}"));
        assert_eq!(app.handle(&query_token, now).status, 403, "{path}");
        let mut foreign = api_request(path, "{}");
        foreign.origin = Some("http://evil.example".into());
        assert_eq!(app.handle(&foreign, now).status, 403, "{path}");
        let mut bad_host = api_request(path, "{}");
        bad_host.host = Some("evil.example".into());
        assert_eq!(app.handle(&bad_host, now).status, 403, "{path}");
        let mut get = api_request(path, "{}");
        get.method = "GET".into();
        assert_eq!(app.handle(&get, now).status, 405, "{path}");
        let mut text = api_request(path, "{}");
        text.content_type = Some("text/plain".into());
        assert_eq!(app.handle(&text, now).status, 415, "{path}");
    }
    // 何も始まっていない
    assert_eq!(diag_status(&mut app, now)["running"], false);
}

#[test]
fn diag_idle_status_lists_all_items_and_nothing_is_exportable() {
    let (mut app, now) = diag_app(FakeDiag::new(true));
    let v = diag_status(&mut app, now);
    assert_eq!(v["items"].as_array().unwrap().len(), ItemId::ALL.len());
    assert_eq!(v["can_export"], false);
    assert_eq!(v["items"][0]["label"], "未確認");
    // 結果が無い間はコピー・保存できない
    assert_eq!(
        app.handle(&api_request("/api/diag/report", "{}"), now)
            .status,
        409
    );
    assert_eq!(
        app.handle(&api_request("/api/diag/save", "{}"), now).status,
        409
    );
}

#[test]
fn diag_run_all_reports_progress_then_results_then_report_and_save() {
    let service = FakeDiag::new(false);
    let (mut app, now) = diag_app(service.clone());
    let r = app.handle(&api_request("/api/diag/start", ""), now);
    assert_eq!(r.status, 200, "本文なしは全項目");
    // 実行中: 進捗がある。重ねて始められない・出力できない
    let v = diag_status(&mut app, now);
    assert_eq!(v["running"], true);
    assert_eq!(v["mode"], "all");
    assert_eq!(v["can_export"], false);
    assert!(
        v["progress_line"]
            .as_str()
            .unwrap()
            .contains("確認しています")
    );
    assert_eq!(
        app.handle(&api_request("/api/diag/start", "{}"), now)
            .status,
        409
    );
    assert_eq!(
        app.handle(&api_request("/api/diag/report", "{}"), now)
            .status,
        409
    );
    // 中断せずに終わらせる
    service.gate.store(true, Ordering::Relaxed);
    let v = wait_idle(&mut app, now);
    assert_eq!(v["can_export"], true);
    assert_eq!(v["summary"]["ok"], ItemId::ALL.len());
    assert_eq!(v["items"][0]["label"], "OK");
    assert_eq!(v["items"][0]["badge"], "ok");
    // レポート本文（コピー用）と保存
    let r = app.handle(&api_request("/api/diag/report", "{}"), now);
    assert_eq!(r.status, 200);
    assert!(json_of(&r)["text"].as_str().unwrap().contains("env.arch"));
    let r = app.handle(&api_request("/api/diag/save", "{}"), now);
    assert_eq!(r.status, 200);
    assert_eq!(json_of(&r)["path"], "%TEMP%/nagi-diag.txt");
    assert_eq!(*service.saves.lock().unwrap(), vec![ItemId::ALL.len()]);
}

#[test]
fn diag_cancel_skips_the_remaining_items_but_keeps_the_report() {
    let service = FakeDiag::new(false);
    let (mut app, now) = diag_app(service);
    assert_eq!(
        app.handle(&api_request("/api/diag/start", "{}"), now)
            .status,
        200
    );
    let r = app.handle(&api_request("/api/diag/cancel", "{}"), now);
    assert_eq!(r.status, 200);
    assert_eq!(diag_status(&mut app, now)["cancelling"], true);
    let v = wait_idle(&mut app, now);
    assert_eq!(v["summary"]["skipped"], ItemId::ALL.len());
    assert!(
        v["note"]["text"]
            .as_str()
            .unwrap()
            .starts_with("中止しました")
    );
    assert_eq!(v["cancelling"], false);
}

#[test]
fn diag_single_item_runs_and_shows_the_steps_for_manual_items() {
    let (mut app, now) = diag_app(FakeDiag::new(true));
    let r = app.handle(
        &api_request("/api/diag/start", r#"{"item":"manual.multi_dpi"}"#),
        now,
    );
    assert_eq!(r.status, 200);
    let v = wait_idle(&mut app, now);
    let item = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "manual.multi_dpi")
        .unwrap();
    assert_eq!(item["label"], "手動確認");
    assert_eq!(item["advice"], "1. 開く");
    assert_eq!(item["kind"], "guide");
    // 1 項目だけでは全体のレポートは無い（コピー・保存は全項目を確認してから）
    assert_eq!(v["can_export"], false);
}

#[test]
fn diag_start_rejects_unknown_items_and_bad_bodies() {
    let (mut app, now) = diag_app(FakeDiag::new(true));
    for body in [
        r#"{"item":"no.such"}"#,
        r#"{"item":"../../etc/passwd"}"#,
        "not json",
        r#"{"item":1}"#,
    ] {
        let r = app.handle(&api_request("/api/diag/start", body), now);
        assert_eq!(r.status, 400, "{body}");
    }
    assert_eq!(diag_status(&mut app, now)["running"], false);
}

#[test]
fn diag_worker_panic_is_reported_and_unlocks() {
    let service = Arc::new(FakeDiag {
        gate: Arc::new(AtomicBool::new(true)),
        saves: std::sync::Mutex::new(Vec::new()),
        panic_in_run: true,
    });
    let (mut app, now) = diag_app(service);
    assert_eq!(
        app.handle(&api_request("/api/diag/start", "{}"), now)
            .status,
        200
    );
    let v = wait_idle(&mut app, now);
    assert_eq!(v["note"]["is_error"], true);
    assert!(v["note"]["text"].as_str().unwrap().contains("内部エラー"));
    // 次の実行を始められる
    assert_eq!(
        app.handle(&api_request("/api/diag/start", "{}"), now)
            .status,
        200
    );
}

#[test]
fn update_check_carries_the_described_wording() {
    let (mut app, _, now) = app();
    let v = json_of(&app.handle(&api_request("/api/update/check", "{}"), now));
    // 既存の項目はそのまま。`view` が足される
    assert_eq!(v["status"], "staged");
    assert_eq!(v["view"]["badge"], "warn");
    assert_eq!(v["view"]["label"], "適用待ち");
    assert_eq!(v["view"]["can_apply"], true);
    assert_eq!(v["view"]["notes_url"], "https://example.com/notes");
    assert!(v["view"]["text"].as_str().unwrap().contains("0.1.15"));
}

#[test]
fn open_config_dir_opens_the_parent_folder_of_config_toml() {
    let (mut app, calls, now) = app();
    let r = app.handle(&api_request("/api/open", r#"{"target":"config_dir"}"#), now);
    assert_eq!(r.status, 200);
    assert_eq!(
        calls.borrow().opened_folders,
        vec![PathBuf::from("/u/Nagi")]
    );
    let r = app.handle(&api_request("/api/open", r#"{"target":"anything"}"#), now);
    assert_eq!(r.status, 400);
}
