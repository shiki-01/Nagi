//! Nagi 設定画面（Windows 向け。他 OS でも動くがテスト・開発用）。
//!
//! 既定（Windows）はネイティブの Win32 ウィンドウ。`--web`（Windows 以外は常に）では次の Web 版になる。
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
//! - `native_model` / `native_layout` / `native_worker` / `native_diag` / `native_theme` / `native_ui_state`: ネイティブ設定ウィンドウの
//!   OS 非依存部分（画面モデル・レイアウト計算・バックエンド用ワーカースレッド・「動作確認」ページのモデル・色と形・
//!   UI 状態の保存。`docs/decisions.md` H-104・H-118）
//! - `native`（Windows のみ）: Win32 のウィンドウ層。上の 3 つだけを見る

pub mod assets;
pub mod backend;
pub mod cli;
pub mod diag;
pub mod http;
pub mod lifecycle;
#[cfg(windows)]
pub mod native;
pub mod native_diag;
pub mod native_layout;
pub mod native_model;
pub mod native_scroll;
pub mod native_theme;
pub mod native_ui_state;
pub mod native_worker;
pub mod opener;
pub mod run;
pub mod token;
pub mod view;
