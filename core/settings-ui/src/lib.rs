//! Nagi 設定画面（Windows 向け。他 OS でも動くがテスト・開発用）。
//!
//! `nagi-settings.exe` が 127.0.0.1 のランダムポートで小さな HTTP サーバーを起動し、既定のブラウザで
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

pub mod assets;
pub mod backend;
pub mod cli;
pub mod http;
pub mod lifecycle;
pub mod opener;
pub mod run;
pub mod token;
pub mod view;
