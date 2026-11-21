//! Nagi Windows フロントエンドの TSF/COM 層。
//!
//! `nagi-windows`（OS 非依存・`unsafe` 禁止）の上に載せる薄い層で、役割は変換と描画だけ:
//! キー入力を `nagi_windows::keynorm` で正規化してエンジンへ送り、応答の確定・preedit を
//! `ITfComposition` / `ITfEditSession` で反映する。何を確定・表示するかはエンジンが決める
//! （ロジックをフロントに持ち込まない。CLAUDE.md の分離の原則）。
//!
//! - [`render`]: 応答 → 編集計画（OS 非依存。Linux でもテストする）
//! - [`engine`]: エンジン接続のまとめ（OS 非依存。UI スレッドを塞がない起動・短いタイムアウト）
//! - [`e2e_probe`]: E2E テスト専用の観測プローブ（環境変数 `NAGI_E2E_PROBE` が設定されたときだけ動く）
//! - [`tsf_log`]: 診断ログ `%LOCALAPPDATA%\Nagi\logs\tsf.log` への追記（入力内容は書かない。H-54）
//! - Windows のみ: COM/TSF の実装（`dll` / `register` / `text_service` / `edit` / `keys`）。
//!   `unsafe` はこれらのモジュールに隔離し、`#![allow(unsafe_code)]` で明示している
//!
//! # Win32 / TSF 層（H-41）
//! - `candidate_window` / `toast_window`: 自前のポップアップ（GDI。フォーカスを奪わない）。寸法・配置・文言は `nagi_windows`
//! - `message_window`: 隠しメッセージウィンドウ。別スレッドのエンジン通知を `PostMessage` で UI スレッドへ渡して
//!   即時に反映し、定期タイマー（環境の報告・スリープ復帰の検出）と電源通知も受ける
//! - `probe`: `EnvironmentProbe` の Win32 実装 / `context_info`: 周辺テキスト・`InputScope`・アプリ ID
//! - `display_attr`: `ITfDisplayAttributeProvider` と `GUID_PROP_ATTRIBUTE` への書き込み
//! - `mode_comp`: 入力モードのコンパートメント（タスクトレイの「あ / A」）/ `langbar`: 言語バーのボタン
//!   （モード表示・クリックで切り替え・メニュー）/ `reconversion`: `ITfFnReconversion`
//!   （`Reconvert` と `GetReconversion`）
//!
//! # エンジンの寿命（H-96）
//! - imserver は、ホストプロセスが最初にフォーカスを受けたとき（または最初のキー）に起動する（Activate では起動しない）
//! - `job`: 起動した imserver を Job Object（KILL_ON_JOB_CLOSE）に入れ、ホストの終了・クラッシュで OS が確実に終了させる。
//!   加えて imserver 側の `--parent-pid` 監視と、`DllCanUnloadNow` の `S_FALSE`（エンジンがある間はアンロードしない）
//! - `popup_menu`: 入力インジケーターの右クリックメニュー（`TF_LBI_STYLE_BTN_BUTTON` では OS が `InitMenu` を呼ばないため、
//!   `OnClick` の右クリックで自前の Win32 ポップアップを出す）
//!
//! # 未実装（TODO）
//! - 複数のホストプロセスでエンジンを共有する仕組み（現状はホストプロセスごとに imserver を起動する）
//! - 候補ウィンドウのマウス操作（v1 の非目標）
//! - 32bit ホスト用の DLL、ARM64X（ARM64 + ARM64EC。x64 エミュレーションのアプリで使うため。ARM64 ネイティブ版の配布は H-107 で対応済み）
//!
//! 自動更新（H-95）: imserver が `--stage-updates` で取得・検証して保存し、適用は `apply-update.ps1`
//! （UAC 1 回）。右クリックメニューの「更新を適用」から起動する。
//!
//! # 検証状況
//! Linux ホストでの `cargo check/clippy --target x86_64-pc-windows-gnu`（型・シグネチャの確認）と、
//! OS 非依存部のテストのみ。Windows 実機では 0.1.14 で基本動作（登録・キー入力・composition・候補ウィンドウ）を確認済み
//! （`docs/real-hardware-checklist.md` §8）。それ以降の追加分は実機未確認。

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod e2e_probe;
pub mod engine;
pub mod render;
pub mod tsf_log;

#[cfg(windows)]
mod candidate_window;
#[cfg(windows)]
mod configure;
#[cfg(windows)]
mod context_info;
#[cfg(windows)]
mod display_attr;
#[cfg(windows)]
mod dll;
#[cfg(windows)]
mod edit;
#[cfg(windows)]
mod guids;
#[cfg(windows)]
mod job;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod langbar;
#[cfg(windows)]
mod message_window;
#[cfg(windows)]
mod mode_comp;
#[cfg(windows)]
mod popup_menu;
#[cfg(windows)]
mod probe;
#[cfg(windows)]
mod proc_mem;
#[cfg(windows)]
mod reconversion;
#[cfg(windows)]
mod register;
#[cfg(windows)]
mod text_service;
#[cfg(windows)]
mod toast_window;
#[cfg(windows)]
mod ui_element;
#[cfg(windows)]
mod win;
