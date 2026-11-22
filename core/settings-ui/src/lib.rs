//! Nagi 設定画面（Windows 向け。他 OS でも動くがテスト・開発用）。
//!
//! 既定（Windows）は WebView2 のウィンドウ（下の Web 版と同じローカルサーバーの画面を表示する。`webview`）。
//! WebView2 が使えなければ既定のブラウザで開く（H-136）。`--web`（Windows 以外は常に）でも次の Web 版（ブラウザ）になる。
//!
//! Web 版: `nagi-settings.exe` が 127.0.0.1 のランダムポートで小さな HTTP サーバーを起動し、既定のブラウザで
//! 設定画面を開く。画面はコアの `get_settings` / `update_settings` が返す項目記述子から自動生成する
//! （iOS の設定タブと同じ仕組み。`docs/decisions.md` H-97）。値の検証と `config.toml` への書き込みは
//! すべてコア（`imserver` の子プロセス）が行い、ここには設定のロジックを持たない。
//!
//! - `backend`: `imserver` との JSON-RPC（`SettingsBackend` trait と実装）
//! - `view`: 記述子 → 画面用 JSON の変換
//! - `http`: リクエストの検証とルーティング（I/O を持たない純粋な関数。セキュリティの要）
//! - `lifecycle`: 自動終了（操作が無い・ページが閉じられた）のタイマー
//! - `opener`: ブラウザ・メモ帳・エクスプローラーの起動
//! - `run`: HTTP サーバーのループ
//! - `cli`: コマンドライン引数
//! - `diag`: 実機の未確認項目をボタン一つで確認する診断（OS 非依存の判定 + Windows 依存部は `diag::sys`。`--diag` で CLI から実行）
//! - `diag_model` / `update_text` / `theme` / `window_geom` / `ui_state`: 画面の OS 非依存部分（「動作確認」ページの状態・
//!   更新確認の文言・背景色とテーマ・ウィンドウの幾何・UI 状態の保存。`docs/decisions.md` H-118）
//! - `webview_host`: WebView2 ホストの OS 非依存の判断（起動方式の優先順位・ポート・ホスト⇄ページのメッセージ・
//!   ナビゲーションの許可）。`webview`（Windows のみ）がそれを実行する

pub mod assets;
pub mod backend;
pub mod cli;
pub mod diag;
pub mod diag_model;
pub mod http;
pub mod lifecycle;
pub mod opener;
pub mod run;
pub mod theme;
pub mod token;
pub mod ui_state;
pub mod update_text;
pub mod view;
#[cfg(windows)]
pub mod webview;
pub mod webview_host;
pub mod window_geom;
