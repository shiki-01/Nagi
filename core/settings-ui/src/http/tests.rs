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
fn page_is_served_with_a_valid_token_and_csp_nonce() {
    let (mut app, _, now) = app();
    let r = app.handle(&page_request(), now);
    assert_eq!(r.status, 200);
    assert!(r.content_type.starts_with("text/html"));
    let html = String::from_utf8(r.body.clone()).unwrap();
    assert!(html.contains(TOKEN), "meta にトークンが入る");
    let nonce = r
        .csp
        .split("'nonce-")
        .nth(1)
        .and_then(|s| s.split('\'').next())
        .unwrap();
    assert!(html.contains(&format!("<script nonce=\"{nonce}\">")));
    assert!(html.contains(&format!("<style nonce=\"{nonce}\">")));
    assert!(!html.contains("{{"), "プレースホルダーが残っていない");
    assert!(r.csp.contains("default-src 'none'"));
    assert!(r.csp.contains("connect-src 'self'"));
    assert!(!r.csp.contains("unsafe-inline"));
    // nonce はリクエストごとに変わる
    let r2 = app.handle(&page_request(), now);
    assert_ne!(r.csp, r2.csp);
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
