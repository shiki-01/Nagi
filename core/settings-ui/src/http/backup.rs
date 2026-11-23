//! バックアップと移行の API（`/api/backup/*`。すべて POST・JSON）。
//!
//! 検査（Host / Origin / トークン / メソッド / Content-Type / 本文の大きさ）は `http::App::handle` と
//! `App::api` が済ませている。ここでは本文を型に読み、**固定の 5 メソッドだけ**をコアへ中継する
//! （クライアントが任意の JSON-RPC メソッドを呼べないようにする）。バックアップの検証・差分・適用・
//! 自動バックアップ・復元はすべてコア（`imserver`）が行い、ここには持たない。
//! バックアップには入力内容（辞書・学習）が含まれるので、本文・応答をログに出さない。
//! 保存先のパスはクライアントから受け取らない（自動バックアップはコアが `<user-dir>/backups/` に作る）。

use super::{Response, backend_error};
use crate::backend::SettingsBackend;
use nagi_protocol::backup::{
    ApplyBackupParams, ExportBackupParams, PreviewBackupParams, RestoreBackupParams, names,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

/// 読み込み系（プレビュー・適用）の本文の上限（バックアップのファイルの上限 20MB に JSON の枠を足す）
pub const MAX_IMPORT_BODY: usize = 21 * 1024 * 1024;
/// 画面が受け付けるバックアップのファイルの大きさの上限（バイト）
pub const MAX_BACKUP_FILE_BYTES: usize = 20 * 1024 * 1024;

/// `/api/backup/*` の経路か
pub fn is_backup_path(path: &str) -> bool {
    path.starts_with("/api/backup/")
}

/// この経路で受ける本文の上限（バイト。大きいのは読み込み系だけ）
pub fn max_body_for(path: &str) -> usize {
    if matches!(path, "/api/backup/preview" | "/api/backup/apply") {
        MAX_IMPORT_BODY
    } else {
        super::MAX_BODY
    }
}

/// 本文を型に読む（空の本文は `{}` として読む。読めなければ `None`）
fn read<T: DeserializeOwned>(body: &[u8]) -> Option<T> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return serde_json::from_value(json!({})).ok();
    }
    serde_json::from_slice(body).ok()
}

fn relay<B: SettingsBackend, P: Serialize>(backend: &mut B, rpc: &str, params: &P) -> Response {
    let Ok(params) = serde_json::to_value(params) else {
        return bad_request();
    };
    match backend.call_rpc(rpc, params) {
        Ok(result) => Response::json(200, &result),
        Err(e) => backend_error(&e),
    }
}

fn bad_request() -> Response {
    Response::error(400, "バックアップの形式が正しくありません")
}

fn typed<B: SettingsBackend, P: Serialize + DeserializeOwned>(
    backend: &mut B,
    rpc: &str,
    body: &[u8],
) -> Response {
    match read::<P>(body) {
        Some(p) => relay(backend, rpc, &p),
        None => bad_request(),
    }
}

/// `/api/backup/<name>` を処理する
pub fn handle<B: SettingsBackend>(backend: &mut B, path: &str, body: &[u8]) -> Response {
    match path {
        "/api/backup/export" => typed::<B, ExportBackupParams>(backend, names::EXPORT_BACKUP, body),
        "/api/backup/preview" => {
            typed::<B, PreviewBackupParams>(backend, names::PREVIEW_BACKUP, body)
        }
        "/api/backup/apply" => typed::<B, ApplyBackupParams>(backend, names::APPLY_BACKUP, body),
        "/api/backup/list" => relay(backend, names::LIST_BACKUPS, &json!({})),
        "/api/backup/restore" => {
            typed::<B, RestoreBackupParams>(backend, names::RESTORE_BACKUP, body)
        }
        _ => Response::error(404, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{App, Request};
    use crate::backend::{BackendError, SettingsBackend, UpdateCheck};
    use crate::opener::Opener;
    use nagi_protocol::settings::{SettingChange, SettingsSnapshot, UpdateSettingsResult};
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::time::Instant;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const PORT: u16 = 40126;

    type Log = Rc<RefCell<Vec<(String, Value)>>>;

    struct Fake {
        log: Log,
        result: Result<Value, BackendError>,
    }

    impl SettingsBackend for Fake {
        fn get_settings(&mut self) -> Result<SettingsSnapshot, BackendError> {
            Ok(SettingsSnapshot {
                items: vec![],
                error: None,
            })
        }
        fn update_settings(
            &mut self,
            _changes: Vec<SettingChange>,
        ) -> Result<UpdateSettingsResult, BackendError> {
            Err(BackendError::transport("使わない"))
        }
        fn clear_learning(&mut self) -> Result<(), BackendError> {
            Ok(())
        }
        fn config_path(&mut self) -> Result<Option<String>, BackendError> {
            Ok(None)
        }
        fn check_update(&mut self) -> Result<UpdateCheck, BackendError> {
            Err(BackendError::transport("使わない"))
        }
        fn apply_update(&mut self) -> Result<(), BackendError> {
            Ok(())
        }
        fn call_rpc(&mut self, method: &str, params: Value) -> Result<Value, BackendError> {
            self.log.borrow_mut().push((method.to_owned(), params));
            self.result.clone()
        }
    }

    struct NoOpener;
    impl Opener for NoOpener {
        fn open_url(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }
        fn open_file_in_editor(&self, _path: &Path) -> Result<(), String> {
            Ok(())
        }
        fn open_folder(&self, _path: &Path) -> Result<(), String> {
            Ok(())
        }
    }

    fn make_app(result: Result<Value, BackendError>) -> (App<Fake>, Log) {
        let log = Log::default();
        let app = App::new(
            Fake {
                log: log.clone(),
                result,
            },
            Box::new(NoOpener),
            TOKEN.to_owned(),
            PORT,
            Instant::now(),
        );
        (app, log)
    }

    fn request(path: &str, body: &str) -> Request {
        Request {
            method: "POST".into(),
            path: path.into(),
            host: Some(format!("127.0.0.1:{PORT}")),
            origin: Some(format!("http://127.0.0.1:{PORT}")),
            content_type: Some("application/json".into()),
            token_header: Some(TOKEN.into()),
            body: body.as_bytes().to_vec(),
            ..Request::default()
        }
    }

    const BACKUP: &str = r#"{"version":1,"created_at":"2026-11-22T00:00:00Z","app_version":"0.1.29","platform":"windows","included":["settings"],"entries":[{"kind":"settings","name":"config.toml","text":"[conversion]\nuse_llm = false\n"}]}"#;

    #[test]
    fn each_route_relays_to_exactly_one_fixed_core_method() {
        let apply = format!(r#"{{"backup":{BACKUP},"apply":["settings"]}}"#);
        let preview = format!(r#"{{"backup":{BACKUP}}}"#);
        let cases = [
            (
                "/api/backup/export",
                r#"{"include":["settings","learning"]}"#.to_string(),
                "export_backup",
            ),
            ("/api/backup/preview", preview, "preview_backup"),
            ("/api/backup/apply", apply, "apply_backup"),
            ("/api/backup/list", String::new(), "list_backups"),
            ("/api/backup/restore", "{}".to_string(), "restore_backup"),
            (
                "/api/backup/restore",
                r#"{"name":"nagi-backup-20261122-102030.json"}"#.to_string(),
                "restore_backup",
            ),
        ];
        for (path, body, rpc) in cases {
            let (mut app, log) = make_app(Ok(json!({"ok": true})));
            let r = app.handle(&request(path, &body), Instant::now());
            assert_eq!(r.status, 200, "{path}");
            let log = log.borrow();
            assert_eq!(log.len(), 1, "{path}");
            assert_eq!(log[0].0, rpc, "{path}");
        }
    }

    #[test]
    fn params_are_retyped_so_unknown_fields_never_reach_the_core() {
        let (mut app, log) = make_app(Ok(json!({})));
        let body = r#"{"include":["settings"],"method":"shutdown","path":"/etc/passwd"}"#;
        let r = app.handle(&request("/api/backup/export", body), Instant::now());
        assert_eq!(r.status, 200);
        assert_eq!(log.borrow()[0].1, json!({"include":["settings"]}));
        // 未知の項目名は読めない
        let r = app.handle(
            &request("/api/backup/export", r#"{"include":["registry"]}"#),
            Instant::now(),
        );
        assert_eq!(r.status, 400);
        assert_eq!(log.borrow().len(), 1);
    }

    #[test]
    fn malformed_bodies_are_bad_requests_and_never_reach_the_core() {
        for (path, body) in [
            ("/api/backup/export", "{}"),
            ("/api/backup/export", "not json"),
            ("/api/backup/preview", "{}"),
            ("/api/backup/preview", r#"{"backup":{"foo":1}}"#),
            ("/api/backup/apply", r#"{"apply":["settings"]}"#),
            ("/api/backup/restore", r#"{"name":3}"#),
        ] {
            let (mut app, log) = make_app(Ok(json!({})));
            let r = app.handle(&request(path, body), Instant::now());
            assert_eq!(r.status, 400, "{path} {body}");
            assert!(log.borrow().is_empty(), "{path} {body}");
        }
    }

    #[test]
    fn unknown_backup_routes_are_not_found() {
        let (mut app, log) = make_app(Ok(json!({})));
        for path in ["/api/backup/shutdown", "/api/backup/", "/api/backup/list/x"] {
            let r = app.handle(&request(path, "{}"), Instant::now());
            assert_eq!(r.status, 404, "{path}");
        }
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn core_errors_keep_status_and_message() {
        let (mut app, _) = make_app(Err(BackendError::rpc(
            -32602,
            "設定を適用できません（何も変更していません）",
        )));
        let apply = format!(r#"{{"backup":{BACKUP},"apply":["settings"]}}"#);
        let r = app.handle(&request("/api/backup/apply", &apply), Instant::now());
        assert_eq!(r.status, 400);
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["error"], "設定を適用できません（何も変更していません）");
    }

    #[test]
    fn body_limits_differ_between_import_and_the_rest() {
        let now = Instant::now();
        let (mut app, log) = make_app(Ok(json!({})));
        // 64KB を超える通常の経路は拒否
        let big = format!(r#"{{"name":"{}"}}"#, "a".repeat(70 * 1024));
        assert_eq!(
            app.handle(&request("/api/backup/restore", &big), now)
                .status,
            413
        );
        // 読み込み系は 64KB を超えても通る（形式が正しければ中継される）
        let long = "x".repeat(200 * 1024);
        let body = format!(
            r#"{{"backup":{{"version":1,"created_at":"x","app_version":"x","platform":"x","entries":[{{"kind":"user_dict","name":"a.tsv","text":"{long}"}}]}}}}"#
        );
        assert_eq!(
            app.handle(&request("/api/backup/preview", &body), now)
                .status,
            200
        );
        assert_eq!(log.borrow().len(), 1);
        // 21MB を超えると拒否
        let huge = format!(r#"{{"pad":"{}"}}"#, "a".repeat(22 * 1024 * 1024));
        assert_eq!(
            app.handle(&request("/api/backup/preview", &huge), now)
                .status,
            413
        );
        assert_eq!(
            super::max_body_for("/api/backup/export"),
            crate::http::MAX_BODY
        );
    }

    #[test]
    fn backup_apis_require_token_origin_post_json_and_host() {
        let (mut app, log) = make_app(Ok(json!({})));
        let now = Instant::now();
        let mut r = request("/api/backup/list", "{}");
        r.token_header = None;
        assert_eq!(app.handle(&r, now).status, 403);
        let mut r = request("/api/backup/list", "{}");
        r.origin = Some("http://evil.example".into());
        assert_eq!(app.handle(&r, now).status, 403);
        let mut r = request("/api/backup/list", "{}");
        r.host = Some("evil.example".into());
        assert_eq!(app.handle(&r, now).status, 403);
        let mut r = request("/api/backup/list", "{}");
        r.method = "GET".into();
        assert_eq!(app.handle(&r, now).status, 405);
        let mut r = request("/api/backup/list", "{}");
        r.content_type = Some("text/plain".into());
        assert_eq!(app.handle(&r, now).status, 415);
        assert!(log.borrow().is_empty());
    }
}
