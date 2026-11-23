//! リクエストの検証とルーティング。ソケットを持たない純粋な関数（`App::handle`）なので、
//! セキュリティの検査（トークン・Host・Origin・メソッド・Content-Type）を直接テストできる。
//!
//! 検査の順序: Host → Origin → トークン → ルート（メソッド・Content-Type・本文）。
//! トークンが無い・違うリクエストには、どのパスでも同じ 403 を返し、ルートの存在を教えない。

use crate::assets;
use crate::backend::{BackendError, SettingsBackend};
use crate::cli::Args;
use crate::diag::ItemId;
use crate::diag::service::{ArgsDiagService, DiagService};
use crate::lifecycle::Lifecycle;
use crate::opener::Opener;
use crate::token::constant_time_eq;
use crate::view::build_screen;
use diag::{DiagRunner, SaveError, StartError};
use nagi_protocol::settings::SettingChange;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub mod backup;
pub mod diag;
pub mod keymap;
pub mod userdict;

/// トークンを運ぶ HTTP ヘッダー（API 呼び出し）
pub const TOKEN_HEADER: &str = "x-nagi-token";
/// 本文の上限（設定値は小さい）
pub const MAX_BODY: usize = 64 * 1024;

/// この経路で受ける本文の上限（バイト。辞書の取り込み・バックアップの読み込みだけ大きい）
pub fn max_body_for(path: &str) -> usize {
    if backup::is_backup_path(path) {
        backup::max_body_for(path)
    } else {
        userdict::max_body_for(path)
    }
}

/// 検査に必要なリクエストの要素（ヘッダー名は小文字で渡す）
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// HTTP メソッド（大文字）
    pub method: String,
    /// パス（`?` より前）
    pub path: String,
    /// クエリ文字列（`?` より後。無ければ `None`）
    pub query: Option<String>,
    /// `Host` ヘッダー
    pub host: Option<String>,
    /// `Origin` ヘッダー
    pub origin: Option<String>,
    /// `Content-Type` ヘッダー
    pub content_type: Option<String>,
    /// `X-Nagi-Token` ヘッダー
    pub token_header: Option<String>,
    /// 本文
    pub body: Vec<u8>,
}

/// 応答
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// ステータスコード
    pub status: u16,
    /// `Content-Type`
    pub content_type: &'static str,
    /// 本文
    pub body: Vec<u8>,
    /// この応答の CSP（HTML のみページ用の値。それ以外は何も読み込ませない既定の値）
    pub csp: String,
}

impl Response {
    fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: value.to_string().into_bytes(),
            csp: default_csp(),
        }
    }

    fn error(status: u16, message: &str) -> Self {
        Self::json(status, &json!({"error": message}))
    }

    fn text(status: u16, message: &str) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: message.as_bytes().to_vec(),
            csp: default_csp(),
        }
    }

    fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
            csp: default_csp(),
        }
    }

    /// すべての応答に付けるヘッダー（名前, 値）。CORS ヘッダーは付けない
    pub fn security_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Cache-Control", "no-store".to_owned()),
            ("X-Content-Type-Options", "nosniff".to_owned()),
            ("Content-Security-Policy", self.csp.clone()),
            ("Referrer-Policy", "no-referrer".to_owned()),
            ("X-Frame-Options", "DENY".to_owned()),
            ("Cross-Origin-Resource-Policy", "same-origin".to_owned()),
        ]
    }
}

/// HTML 以外の応答の CSP（何も読み込ませない）
pub fn default_csp() -> String {
    "default-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'".to_owned()
}

/// HTML の CSP。script / style は同一オリジンの外部ファイルだけ（インラインは不可）。fetch も同一オリジンのみ。
/// 画像は同一オリジンと `data:`（アイコン）
pub fn page_csp() -> String {
    "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
     connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
        .to_owned()
}

/// 設定画面のアプリケーション本体
pub struct App<B: SettingsBackend> {
    backend: B,
    opener: Box<dyn Opener>,
    token: String,
    port: u16,
    /// 自動終了のタイマー
    pub lifecycle: Lifecycle,
    config_path: Option<PathBuf>,
    /// ブラウザで開く URL に付ける断片（`update` / `update-check`。`--section update [--auto-check]`）
    start_fragment: Option<&'static str>,
    /// 動作確認（診断）の実行
    diag: DiagRunner,
}

#[derive(Deserialize)]
struct DiagStartBody {
    /// 1 項目だけ実行するときの項目 ID（無い・`null` は全項目）
    #[serde(default)]
    item: Option<String>,
}

#[derive(Deserialize)]
struct UpdateBody {
    changes: Vec<SettingChange>,
}

#[derive(Deserialize)]
struct OpenBody {
    target: String,
}

/// クエリ文字列から `name` の値を取り出す（トークンは 16 進なのでパーセントデコードは不要）
fn query_param<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    query?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then_some(v)
    })
}

impl<B: SettingsBackend> App<B> {
    /// `port` は実際にバインドしたポート（Host / Origin の期待値になる）
    pub fn new(
        backend: B,
        opener: Box<dyn Opener>,
        token: String,
        port: u16,
        now: Instant,
    ) -> Self {
        Self {
            backend,
            opener,
            token,
            port,
            lifecycle: Lifecycle::new(now),
            config_path: None,
            start_fragment: None,
            diag: DiagRunner::new(Arc::new(ArgsDiagService::new(Args::default()))),
        }
    }

    /// 動作確認（診断）の実行・保存を頼む相手を差し替える。実行環境では起動引数つきの
    /// [`ArgsDiagService`]（`--no-network`・`--diag-out` などが効く）を渡す
    pub fn set_diag_service(&mut self, service: Arc<dyn DiagService>) {
        self.diag = DiagRunner::new(service);
    }

    /// 開いたときの動作の断片（`update` = 更新セクションへスクロール、`update-check` = さらに確認を自動実行）
    pub fn set_start_fragment(&mut self, fragment: Option<&'static str>) {
        self.start_fragment = fragment;
    }

    /// ブラウザに開かせる URL（トークンはここにだけ載る）
    pub fn page_url(&self) -> String {
        let fragment = self
            .start_fragment
            .map(|f| format!("#{f}"))
            .unwrap_or_default();
        format!("http://127.0.0.1:{}/?t={}{fragment}", self.port, self.token)
    }

    /// 起動直後に設定ファイルの場所を取得しておく（取れなくても画面は出す）
    pub fn load_config_path(&mut self) {
        self.config_path = self.backend.config_path().ok().flatten().map(PathBuf::from);
    }

    fn token_ok(&self, candidate: Option<&str>) -> bool {
        candidate.is_some_and(|c| constant_time_eq(c, &self.token))
    }

    /// リクエストを処理する。`now` はタイマーの更新に使う
    pub fn handle(&mut self, req: &Request, now: Instant) -> Response {
        // DNS リバインディング対策: Host は 127.0.0.1:<port> だけ
        let expected_host = format!("127.0.0.1:{}", self.port);
        if !req
            .host
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(&expected_host))
        {
            return Response::text(403, "forbidden");
        }
        // Origin があれば同一オリジンだけ（ナビゲーションの GET には付かないことがある）
        let expected_origin = format!("http://{expected_host}");
        if req.origin.as_deref().is_some_and(|o| o != expected_origin) {
            return Response::text(403, "forbidden");
        }
        let route = (req.method.as_str(), req.path.as_str());
        let token_ok = match route {
            // 画面・静的ファイルの取得: URL のトークン（`<script src>` などはヘッダーを付けられない）
            ("GET", "/") => self.token_ok(query_param(req.query.as_deref(), "t")),
            ("GET", p) if assets::find_static(p).is_some() => {
                self.token_ok(query_param(req.query.as_deref(), "t"))
            }
            // sendBeacon はヘッダーを付けられないので、/bye だけ URL のトークンも受ける
            (_, "/bye") => {
                self.token_ok(req.token_header.as_deref())
                    || self.token_ok(query_param(req.query.as_deref(), "t"))
            }
            // 既知でない経路・API: ヘッダーのトークン
            _ => self.token_ok(req.token_header.as_deref()),
        };
        if !token_ok {
            return Response::text(403, "forbidden");
        }
        self.lifecycle.touch(now);
        match route {
            ("GET", "/") => self.page(),
            ("GET", p) if assets::find_static(p).is_some() => static_asset(p),
            (_, p) if p == "/" || assets::find_static(p).is_some() => Response::empty(405),
            ("POST", "/bye") => {
                self.lifecycle.bye(now);
                Response::empty(204)
            }
            ("POST", p) if p.starts_with("/api/") => self.api(req, p),
            (_, p) if p == "/bye" || p.starts_with("/api/") => Response::empty(405),
            _ => Response::text(404, "not found"),
        }
    }

    fn page(&self) -> Response {
        Response {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: assets::render_page(&self.token).into_bytes(),
            csp: page_csp(),
        }
    }

    fn api(&mut self, req: &Request, path: &str) -> Response {
        let is_json = req.content_type.as_deref().is_some_and(|c| {
            c.split(';')
                .next()
                .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
        });
        if !is_json {
            return Response::error(415, "Content-Type は application/json にしてください");
        }
        if req.body.len() > max_body_for(path) {
            return Response::error(413, "リクエストが大きすぎます");
        }
        match path {
            "/api/settings" => self.api_settings(),
            "/api/update" => self.api_update(&req.body),
            "/api/open" => self.api_open(&req.body),
            "/api/update/check" => self.api_update_check(),
            // 固定のアクション: 本文は読まない（パス・引数はクライアントから受け取らない）
            "/api/update/apply" => match self.backend.apply_update() {
                Ok(()) => Response::json(200, &json!({"ok": true})),
                Err(e) => backend_error(&e),
            },
            "/api/diag/status" => Response::json(200, &self.diag.status()),
            "/api/diag/start" => self.api_diag_start(&req.body),
            "/api/diag/cancel" => {
                self.diag.cancel();
                Response::json(200, &json!({"ok": true}))
            }
            "/api/diag/report" => match self.diag.report_text() {
                Some(text) => Response::json(200, &json!({"text": text})),
                None => Response::error(409, "コピーできる結果がありません"),
            },
            "/api/diag/save" => match self.diag.save() {
                Ok(path) => Response::json(200, &json!({"path": path})),
                Err(SaveError::NoReport) => Response::error(409, "保存できる結果がありません"),
                Err(SaveError::Failed(message)) => Response::error(500, &message),
            },
            p if keymap::is_keymap_path(p) => keymap::handle(&mut self.backend, p, &req.body),
            p if userdict::is_userdict_path(p) => userdict::handle(&mut self.backend, p, &req.body),
            p if backup::is_backup_path(p) => backup::handle(&mut self.backend, p, &req.body),
            "/api/clear_learning" => match self.backend.clear_learning() {
                Ok(()) => Response::json(200, &json!({"ok": true})),
                Err(e) => backend_error(&e),
            },
            _ => Response::error(404, "not found"),
        }
    }

    fn screen_json(&self, snapshot: &nagi_protocol::settings::SettingsSnapshot) -> Value {
        let path = self
            .config_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let mut screen = serde_json::to_value(build_screen(snapshot, path)).unwrap_or(Value::Null);
        // 現在の版（更新セクションの表示用。この画面と imserver は同じワークスペースの版）
        if let Some(object) = screen.as_object_mut() {
            object.insert("app_version".to_owned(), json!(env!("CARGO_PKG_VERSION")));
        }
        screen
    }

    fn api_update_check(&mut self) -> Response {
        match self.backend.check_update() {
            Ok(mut check) => {
                // リンクにするのは https の URL だけ（マニフェストは配布元の JSON なので、画面に出す前に絞る）
                check.notes_url = check.notes_url.take().filter(|u| is_safe_notes_url(u));
                let view = update_view(&check);
                let mut value = serde_json::to_value(&check).unwrap_or(Value::Null);
                // 既存の項目はそのまま。文言・状態の見出しを `view` として足す
                if let Some(object) = value.as_object_mut() {
                    object.insert("view".to_owned(), view);
                }
                Response::json(200, &value)
            }
            Err(e) => backend_error(&e),
        }
    }

    fn api_settings(&mut self) -> Response {
        if self.config_path.is_none() {
            self.load_config_path();
        }
        match self.backend.get_settings() {
            Ok(snapshot) => Response::json(200, &self.screen_json(&snapshot)),
            Err(e) => backend_error(&e),
        }
    }

    fn api_update(&mut self, body: &[u8]) -> Response {
        let Ok(parsed) = serde_json::from_slice::<UpdateBody>(body) else {
            return Response::error(400, "リクエストの形式が正しくありません");
        };
        match self.backend.update_settings(parsed.changes) {
            Ok(result) => {
                let errors: Vec<Value> = result
                    .errors
                    .iter()
                    .map(|e| json!({"key": e.key, "message": e.message}))
                    .collect();
                Response::json(
                    200,
                    &json!({
                        "applied": result.applied,
                        "errors": errors,
                        "screen": self.screen_json(&result.snapshot),
                    }),
                )
            }
            Err(e) => backend_error(&e),
        }
    }

    fn api_diag_start(&mut self, body: &[u8]) -> Response {
        // 本文は空でもよい（全項目）。項目は ID の固定の一覧から引くだけで、パス・引数は受け取らない
        let parsed = if body.iter().all(u8::is_ascii_whitespace) {
            DiagStartBody { item: None }
        } else {
            match serde_json::from_slice::<DiagStartBody>(body) {
                Ok(p) => p,
                Err(_) => return Response::error(400, "リクエストの形式が正しくありません"),
            }
        };
        let item = match parsed.item.as_deref() {
            None => None,
            Some(id) => match ItemId::parse(id) {
                Some(id) => Some(id),
                None => return Response::error(400, "確認する項目が正しくありません"),
            },
        };
        match self.diag.start(item) {
            Ok(()) => Response::json(200, &json!({"ok": true})),
            Err(StartError::Busy) => Response::error(409, "確認の実行中です"),
            Err(StartError::Spawn) => Response::error(500, "確認を開始できません"),
        }
    }

    fn api_open(&mut self, body: &[u8]) -> Response {
        let Ok(parsed) = serde_json::from_slice::<OpenBody>(body) else {
            return Response::error(400, "リクエストの形式が正しくありません");
        };
        if self.config_path.is_none() {
            self.load_config_path();
        }
        let Some(config) = self.config_path.clone() else {
            return Response::error(409, "設定ファイルの場所が決まっていません");
        };
        // 開く対象はクライアントから受け取らない（固定の 2 種類だけ）
        let result = match parsed.target.as_str() {
            "config" => self.opener.open_file_in_editor(&config),
            "config_dir" => match config.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(dir) => self.opener.open_folder(dir),
                None => Err("設定フォルダの場所が決まっていません".to_owned()),
            },
            "user_dict" => match user_dict_dir(&config, |p| p.is_dir()) {
                Some(dir) => self.opener.open_folder(&dir),
                None => Err("ユーザー辞書のフォルダがまだありません".to_owned()),
            },
            _ => return Response::error(400, "開く対象が正しくありません"),
        };
        match result {
            Ok(()) => Response::json(200, &json!({"ok": true})),
            Err(message) => Response::error(500, &message),
        }
    }
}

/// 更新確認の結果の画面表示（文言と状態の見出しは `update_text`。`notes_url` は安全と確認済みのものだけ）
pub fn update_view(check: &crate::backend::UpdateCheck) -> Value {
    use crate::diag_model::BadgeKind;
    use crate::update_text::{describe_update, update_headline};
    let outcome = describe_update(check);
    let head = update_headline(false, &outcome.text);
    let badge = match head.as_ref().map(|h| h.badge) {
        Some(BadgeKind::Ok) => "ok",
        Some(BadgeKind::Warn) => "warn",
        Some(BadgeKind::Ng) => "ng",
        Some(BadgeKind::Neutral) | None => "neutral",
        Some(BadgeKind::Info) => "info",
    };
    json!({
        "text": outcome.text,
        "can_apply": outcome.can_apply,
        "notes_url": outcome.notes_url,
        "badge": badge,
        "label": head.as_ref().map(|h| h.label),
        "title": head.as_ref().map(|h| h.title),
    })
}

/// 静的ファイルの応答（トークンは検査済み）
fn static_asset(path: &str) -> Response {
    match assets::find_static(path) {
        Some(asset) => Response {
            status: 200,
            content_type: asset.content_type,
            body: asset.body.as_bytes().to_vec(),
            csp: default_csp(),
        },
        None => Response::text(404, "not found"),
    }
}

/// リリースノートの URL として画面に出してよいか（`https:` の絶対 URL。空白・制御文字・引用符を含まない）
pub fn is_safe_notes_url(url: &str) -> bool {
    url.len() <= 2048
        && url.starts_with("https://")
        && url.len() > "https://".len()
        && url
            .chars()
            .all(|c| c.is_ascii() && !c.is_ascii_control() && !matches!(c, ' ' | '"' | '<' | '>'))
}

/// ユーザー辞書フォルダ（`<user-dir>/user_dict`、無ければ user-dir。どちらも無ければ `None`）
pub fn user_dict_dir(config_path: &Path, is_dir: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let user_dir = config_path.parent().filter(|p| !p.as_os_str().is_empty())?;
    let sub = user_dir.join("user_dict");
    if is_dir(&sub) {
        Some(sub)
    } else if is_dir(user_dir) {
        Some(user_dir.to_path_buf())
    } else {
        None
    }
}

/// コアの失敗を HTTP 応答にする。検証エラー（`INVALID_PARAMS`）は 400 でメッセージをそのまま渡す。
/// 通信の失敗は 503
fn backend_error(e: &BackendError) -> Response {
    let status = if e.is_invalid_params() {
        400
    } else if e.code.is_some() {
        502
    } else {
        503
    };
    Response::error(status, &e.message)
}

#[cfg(test)]
mod tests;
