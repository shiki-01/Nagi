//! ユーザー辞書・学習・抑制語の管理 API（`/api/userdict/*`・`/api/learning/stats`・`/api/suppressed/*`。
//! すべて POST・JSON）。
//!
//! 検査（Host / Origin / トークン / メソッド / Content-Type / 本文の大きさ）は `http::App::handle` と
//! `App::api` が済ませている。ここでは本文を型に読み、**固定の 10 メソッドだけ**をコアへ中継する
//! （クライアントが任意の JSON-RPC メソッドを呼べないようにする）。読みの検証・重複の判定・保存・
//! 辞書への反映はすべてコア（`imserver`）が行い、ここには持たない。
//! 読み・表記・コメントはログにも応答のエラーにも出さない（コアのエラーメッセージも内容を含まない）。

use super::{Response, backend_error};
use crate::backend::SettingsBackend;
use nagi_protocol::methods::{SuppressCandidateParams, method};
use nagi_protocol::userdict::{
    AddUserDictEntryParams, ListUserDictParams, MergeUserDictParams, RemoveUserDictEntriesParams,
    UpdateUserDictEntryParams, names,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

/// 取り込みの経路（本文の上限だけ他より大きい）
pub const IMPORT_PATH: &str = "/api/userdict/import";
/// 取り込みの本文の上限（ファイル 5MB の base64 + JSON の枠）
pub const MAX_IMPORT_BODY: usize = 7 * 1024 * 1024;
/// 取り込みのファイルの大きさの上限（コアの `MAX_IMPORT_BYTES` と同じ。画面にも伝える）
pub const MAX_IMPORT_FILE_BYTES: usize = 5 * 1024 * 1024;

/// この経路で受ける本文の上限（バイト）
pub fn max_body_for(path: &str) -> usize {
    if path == IMPORT_PATH {
        MAX_IMPORT_BODY
    } else {
        super::MAX_BODY
    }
}

/// このモジュールが扱う経路か
pub fn is_userdict_path(path: &str) -> bool {
    path.starts_with("/api/userdict/")
        || path.starts_with("/api/suppressed/")
        || path == "/api/learning/stats"
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
    Response::error(400, "リクエストの形式が正しくありません")
}

/// 本文を型に読んで中継する
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

/// 経路を処理する
pub fn handle<B: SettingsBackend>(backend: &mut B, path: &str, body: &[u8]) -> Response {
    match path {
        "/api/userdict/list" => {
            typed::<B, ListUserDictParams>(backend, names::LIST_USER_DICT, body)
        }
        "/api/userdict/add" => {
            typed::<B, AddUserDictEntryParams>(backend, names::ADD_USER_DICT_ENTRY, body)
        }
        "/api/userdict/update" => {
            typed::<B, UpdateUserDictEntryParams>(backend, names::UPDATE_USER_DICT_ENTRY, body)
        }
        "/api/userdict/remove" => {
            typed::<B, RemoveUserDictEntriesParams>(backend, names::REMOVE_USER_DICT_ENTRIES, body)
        }
        IMPORT_PATH => match read::<MergeUserDictParams>(body) {
            // base64 の長さの上限（デコード後 5MB）。コアも検査するが、無駄に中継しない
            Some(p) if p.data_base64.len() > MAX_IMPORT_FILE_BYTES.div_ceil(3) * 4 + 4 => {
                Response::error(413, "ファイルが大きすぎます（上限 5 MB）")
            }
            Some(p) => relay(backend, names::MERGE_USER_DICT, &p),
            None => bad_request(),
        },
        "/api/userdict/export" => relay(backend, names::EXPORT_USER_DICT, &json!({})),
        "/api/learning/stats" => relay(backend, names::GET_LEARNING_STATS, &json!({})),
        "/api/suppressed/list" => relay(backend, method::LIST_SUPPRESSED, &json!({})),
        "/api/suppressed/add" => {
            typed::<B, SuppressCandidateParams>(backend, method::SUPPRESS_CANDIDATE, body)
        }
        "/api/suppressed/remove" => {
            typed::<B, SuppressCandidateParams>(backend, method::UNSUPPRESS_CANDIDATE, body)
        }
        _ => Response::error(404, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{App, Request};
    use super::*;
    use crate::backend::{BackendError, SettingsBackend, UpdateCheck};
    use crate::opener::Opener;
    use nagi_protocol::jsonrpc::error_code;
    use nagi_protocol::settings::{SettingChange, SettingsSnapshot, UpdateSettingsResult};
    use serde_json::Value;
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::time::Instant;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const PORT: u16 = 40125;

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
            (
                "/api/userdict/list",
                r#"{"query":"あ","sort":"reading","limit":50}"#,
                "list_user_dict",
            ),
            ("/api/userdict/list", "", "list_user_dict"),
            (
                "/api/userdict/add",
                r#"{"entry":{"reading":"あ","surface":"亜"},"on_duplicate":"skip"}"#,
                "add_user_dict_entry",
            ),
            (
                "/api/userdict/update",
                r#"{"target":{"reading":"あ","surface":"亜","file":"a.tsv"},"entry":{"reading":"あ","surface":"阿"}}"#,
                "update_user_dict_entry",
            ),
            (
                "/api/userdict/remove",
                r#"{"items":[{"reading":"あ","surface":"亜"}]}"#,
                "remove_user_dict_entries",
            ),
            (
                "/api/userdict/import",
                r#"{"data_base64":"YQ==","on_duplicate":"overwrite"}"#,
                "merge_user_dict",
            ),
            ("/api/userdict/export", "", "export_user_dict"),
            ("/api/learning/stats", "", "get_learning_stats"),
            ("/api/suppressed/list", "", "list_suppressed"),
            (
                "/api/suppressed/add",
                r#"{"reading":"こうえん","surface":"講演"}"#,
                "suppress_candidate",
            ),
            (
                "/api/suppressed/remove",
                r#"{"reading":"こうえん","surface":"講演"}"#,
                "unsuppress_candidate",
            ),
        ];
        for (path, body, rpc) in cases {
            let (mut app, log) = make_app(Ok(json!({"ok": true})));
            let r = app.handle(&request(path, body), Instant::now());
            assert_eq!(r.status, 200, "{path}");
            let log = log.borrow();
            assert_eq!(log.len(), 1, "{path}");
            assert_eq!(log[0].0, rpc, "{path}");
        }
    }

    #[test]
    fn params_are_passed_through_typed() {
        let (mut app, log) = make_app(Ok(json!({})));
        app.handle(
            &request(
                "/api/userdict/update",
                r#"{"target":{"reading":"あ","surface":"亜","file":"a.tsv"},"entry":{"reading":"い","surface":"伊","pos":"人名"}}"#,
            ),
            Instant::now(),
        );
        let params = log.borrow()[0].1.clone();
        assert_eq!(params["target"]["file"], "a.tsv");
        assert_eq!(params["entry"]["pos"], "人名");
        assert!(params["entry"].get("comment").is_none());
    }

    #[test]
    fn malformed_bodies_are_rejected_before_reaching_the_core() {
        for path in [
            "/api/userdict/add",
            "/api/userdict/update",
            "/api/userdict/remove",
            "/api/userdict/import",
            "/api/suppressed/add",
            "/api/suppressed/remove",
        ] {
            for body in ["not json", r#"{"entry": 1}"#, r#"{"unknown":true}"#, "[]"] {
                let (mut app, log) = make_app(Ok(json!({})));
                let r = app.handle(&request(path, body), Instant::now());
                assert_eq!(r.status, 400, "{path} {body}");
                assert!(log.borrow().is_empty(), "{path} {body}");
            }
        }
        // 必須の項目が欠けている
        let (mut app, log) = make_app(Ok(json!({})));
        let r = app.handle(
            &request("/api/userdict/add", r#"{"entry":{"reading":"あ"}}"#),
            Instant::now(),
        );
        assert_eq!(r.status, 400);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn core_validation_errors_become_400_with_the_message() {
        let msg = "読みはひらがなで入力してください";
        let (mut app, _) = make_app(Err(BackendError::rpc(error_code::INVALID_PARAMS, msg)));
        let r = app.handle(
            &request(
                "/api/userdict/add",
                r#"{"entry":{"reading":"x","surface":"y"}}"#,
            ),
            Instant::now(),
        );
        assert_eq!(r.status, 400);
        let body: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(body["error"], msg);
        let (mut app, _) = make_app(Err(BackendError::transport("設定エンジンが終了しました")));
        let r = app.handle(&request("/api/learning/stats", ""), Instant::now());
        assert_eq!(r.status, 503);
    }

    #[test]
    fn unknown_paths_and_wrong_methods_never_reach_the_core() {
        let (mut app, log) = make_app(Ok(json!({})));
        for path in [
            "/api/userdict/other",
            "/api/suppressed/other",
            "/api/userdict/",
        ] {
            let r = app.handle(&request(path, "{}"), Instant::now());
            assert_eq!(r.status, 404, "{path}");
        }
        let mut get = request("/api/userdict/list", "");
        get.method = "GET".into();
        assert_eq!(app.handle(&get, Instant::now()).status, 405);
        // トークンが無ければ経路の存在も教えない
        let mut no_token = request("/api/userdict/list", "");
        no_token.token_header = None;
        assert_eq!(app.handle(&no_token, Instant::now()).status, 403);
        let mut bad_origin = request("/api/userdict/list", "");
        bad_origin.origin = Some("http://evil.example".into());
        assert_eq!(app.handle(&bad_origin, Instant::now()).status, 403);
        let mut text = request("/api/userdict/list", "{}");
        text.content_type = Some("text/plain".into());
        assert_eq!(app.handle(&text, Instant::now()).status, 415);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn only_the_import_route_accepts_a_large_body() {
        assert_eq!(max_body_for("/api/userdict/add"), crate::http::MAX_BODY);
        assert_eq!(max_body_for(IMPORT_PATH), MAX_IMPORT_BODY);
        // 通常の経路は 64KB まで
        let (mut app, log) = make_app(Ok(json!({})));
        let big = format!(r#"{{"query":"{}"}}"#, "a".repeat(crate::http::MAX_BODY));
        let r = app.handle(&request("/api/userdict/list", &big), Instant::now());
        assert_eq!(r.status, 413);
        assert!(log.borrow().is_empty());
        // 取り込みは 64KB を超えても通る
        let data = "A".repeat(200 * 1024);
        let body = format!(r#"{{"data_base64":"{data}"}}"#);
        let r = app.handle(&request(IMPORT_PATH, &body), Instant::now());
        assert_eq!(r.status, 200);
        // 5MB を超えるファイル（base64 の長さ）は中継せずに断る
        let huge = "A".repeat(MAX_IMPORT_FILE_BYTES.div_ceil(3) * 4 + 8);
        let body = format!(r#"{{"data_base64":"{huge}"}}"#);
        let before = log.borrow().len();
        let r = app.handle(&request(IMPORT_PATH, &body), Instant::now());
        assert_eq!(r.status, 413);
        assert_eq!(log.borrow().len(), before);
        // 本文そのものが上限（7MB）を超えれば 413
        let too_big = "A".repeat(MAX_IMPORT_BODY);
        let body = format!(r#"{{"data_base64":"{too_big}"}}"#);
        assert_eq!(
            app.handle(&request(IMPORT_PATH, &body), Instant::now())
                .status,
            413
        );
    }
}
