//! キー割当の編集 API（`/api/keymap/*`。すべて POST・JSON）。
//!
//! 検査（Host / Origin / トークン / メソッド / Content-Type / 本文の大きさ）は `http::App::handle` と
//! `App::api` が済ませている。ここでは本文を型に読み、**固定の 7 メソッドだけ**をコアへ中継する
//! （クライアントが任意の JSON-RPC メソッドを呼べないようにする）。キー・Action・TOML の意味の検査と
//! `config.toml` への書き込みはすべてコア（`imserver`）が行い、ここには持たない。

use super::{Response, backend_error};
use crate::backend::SettingsBackend;
use nagi_protocol::methods::{
    GetKeymapParams, ImportKeymapParams, ReplaceKeymapParams, ResetKeymapParams, SetKeymapParams,
    ValidateKeymapParams, method,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

/// `/api/keymap/*` の経路か
pub fn is_keymap_path(path: &str) -> bool {
    path.starts_with("/api/keymap/")
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
        return Response::error(400, "リクエストの形式が正しくありません");
    };
    match backend.call_rpc(rpc, params) {
        Ok(result) => Response::json(200, &result),
        Err(e) => backend_error(&e),
    }
}

fn bad_request() -> Response {
    Response::error(400, "リクエストの形式が正しくありません")
}

/// `/api/keymap/<name>` を処理する
pub fn handle<B: SettingsBackend>(backend: &mut B, path: &str, body: &[u8]) -> Response {
    match path {
        "/api/keymap/get" => match read::<GetKeymapParams>(body) {
            Some(p) => relay(backend, method::GET_KEYMAP, &p),
            None => bad_request(),
        },
        "/api/keymap/set" => match read::<SetKeymapParams>(body) {
            Some(p) => relay(backend, method::SET_KEYMAP, &p),
            None => bad_request(),
        },
        "/api/keymap/replace" => match read::<ReplaceKeymapParams>(body) {
            Some(p) => relay(backend, method::REPLACE_KEYMAP, &p),
            None => bad_request(),
        },
        "/api/keymap/validate" => match read::<ValidateKeymapParams>(body) {
            Some(p) => relay(backend, method::VALIDATE_KEYMAP, &p),
            None => bad_request(),
        },
        "/api/keymap/reset" => match read::<ResetKeymapParams>(body) {
            Some(p) => relay(backend, method::RESET_KEYMAP, &p),
            None => bad_request(),
        },
        "/api/keymap/export" => relay(backend, method::EXPORT_KEYMAP, &json!({})),
        "/api/keymap/import" => match read::<ImportKeymapParams>(body) {
            Some(p) => relay(backend, method::IMPORT_KEYMAP, &p),
            None => bad_request(),
        },
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
    const PORT: u16 = 40124;

    type Log = Rc<RefCell<Vec<(String, Value)>>>;

    /// 呼ばれた RPC を記録し、決まった結果を返すバックエンド
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

    #[test]
    fn each_route_relays_to_exactly_one_fixed_core_method() {
        let cases = [
            ("/api/keymap/get", "{}", "get_keymap"),
            (
                "/api/keymap/get",
                r#"{"preset":"ctrl_letters"}"#,
                "get_keymap",
            ),
            (
                "/api/keymap/set",
                r#"{"context":"composing","key":"F2","action":"Commit"}"#,
                "set_keymap",
            ),
            (
                "/api/keymap/replace",
                r#"{"context":"composing","old_key":"F8","new_key":"Ctrl+Shift+y","action":"ToKatakana","disable_old":true}"#,
                "replace_keymap",
            ),
            (
                "/api/keymap/validate",
                r#"{"toml":"[keymap.composing]\nF2 = \"Commit\"\n"}"#,
                "validate_keymap",
            ),
            (
                "/api/keymap/reset",
                r#"{"scope":"context","context":"converting"}"#,
                "reset_keymap",
            ),
            ("/api/keymap/export", "", "export_keymap"),
            (
                "/api/keymap/import",
                r#"{"toml":"[keymap]\npreset = \"standard\"\n","merge":true}"#,
                "import_keymap",
            ),
        ];
        for (path, body, rpc) in cases {
            let (mut app, log) = make_app(Ok(json!({"ok": true})));
            let r = app.handle(&request(path, body), Instant::now());
            assert_eq!(r.status, 200, "{path}");
            assert_eq!(
                serde_json::from_slice::<Value>(&r.body).unwrap(),
                json!({"ok": true})
            );
            let log = log.borrow();
            assert_eq!(log.len(), 1, "{path}");
            assert_eq!(log[0].0, rpc, "{path}");
        }
    }

    #[test]
    fn params_are_retyped_so_unknown_fields_never_reach_the_core() {
        let (mut app, log) = make_app(Ok(json!({})));
        let body =
            r#"{"context":"composing","key":"F2","action":"Commit","method":"shutdown","extra":1}"#;
        let r = app.handle(&request("/api/keymap/set", body), Instant::now());
        assert_eq!(r.status, 200);
        assert_eq!(
            log.borrow()[0].1,
            json!({"context":"composing","key":"F2","action":"Commit"})
        );
        // 解除は action を省略（null も同じ）
        let r = app.handle(
            &request(
                "/api/keymap/set",
                r#"{"context":"converting","key":"Tab","action":null}"#,
            ),
            Instant::now(),
        );
        assert_eq!(r.status, 200);
        assert_eq!(
            log.borrow()[1].1,
            json!({"context":"converting","key":"Tab"})
        );
    }

    #[test]
    fn malformed_bodies_are_bad_requests_and_never_reach_the_core() {
        for (path, body) in [
            ("/api/keymap/set", "{}"),
            ("/api/keymap/set", r#"{"context":"bogus","key":"F2"}"#),
            ("/api/keymap/set", "not json"),
            (
                "/api/keymap/replace",
                r#"{"context":"composing","old_key":"F8"}"#,
            ),
            ("/api/keymap/validate", "{}"),
            ("/api/keymap/reset", r#"{"scope":"everything"}"#),
            ("/api/keymap/import", r#"{"merge":true}"#),
            ("/api/keymap/get", r#"{"preset":3}"#),
        ] {
            let (mut app, log) = make_app(Ok(json!({})));
            let r = app.handle(&request(path, body), Instant::now());
            assert_eq!(r.status, 400, "{path} {body}");
            assert!(log.borrow().is_empty(), "{path} {body}");
        }
    }

    #[test]
    fn unknown_keymap_routes_are_not_found() {
        let (mut app, log) = make_app(Ok(json!({})));
        for path in ["/api/keymap/shutdown", "/api/keymap/", "/api/keymap/set/x"] {
            let r = app.handle(&request(path, "{}"), Instant::now());
            assert_eq!(r.status, 404, "{path}");
        }
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn core_errors_keep_status_and_message() {
        let (mut app, _) = make_app(Err(BackendError::rpc(
            -32602,
            "未入力のときの割当は変更できません",
        )));
        let r = app.handle(
            &request(
                "/api/keymap/set",
                r#"{"context":"direct","key":"F2","action":"Commit"}"#,
            ),
            Instant::now(),
        );
        assert_eq!(r.status, 400);
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["error"], "未入力のときの割当は変更できません");
        let (mut app, _) = make_app(Err(BackendError::transport("設定エンジンが終了しました")));
        let r = app.handle(&request("/api/keymap/export", "{}"), Instant::now());
        assert_eq!(r.status, 503);
    }

    #[test]
    fn keymap_apis_require_token_origin_post_json_and_host() {
        let (mut app, log) = make_app(Ok(json!({})));
        let now = Instant::now();
        // トークンなし・違うトークン
        let mut r = request("/api/keymap/export", "{}");
        r.token_header = None;
        assert_eq!(app.handle(&r, now).status, 403);
        r.token_header = Some("x".repeat(64));
        assert_eq!(app.handle(&r, now).status, 403);
        // 他のオリジン・他のホスト
        let mut r = request("/api/keymap/export", "{}");
        r.origin = Some("http://evil.example".into());
        assert_eq!(app.handle(&r, now).status, 403);
        let mut r = request("/api/keymap/export", "{}");
        r.host = Some("evil.example".into());
        assert_eq!(app.handle(&r, now).status, 403);
        // GET は 405、JSON 以外は 415
        let mut r = request("/api/keymap/export", "{}");
        r.method = "GET".into();
        assert_eq!(app.handle(&r, now).status, 405);
        let mut r = request("/api/keymap/export", "{}");
        r.content_type = Some("text/plain".into());
        assert_eq!(app.handle(&r, now).status, 415);
        // 大きすぎる本文
        let big = format!(r#"{{"toml":"{}"}}"#, "a".repeat(70 * 1024));
        assert_eq!(
            app.handle(&request("/api/keymap/validate", &big), now)
                .status,
            413
        );
        assert!(log.borrow().is_empty());
    }
}
