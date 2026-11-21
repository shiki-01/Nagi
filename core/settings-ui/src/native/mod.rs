//! ネイティブ（Win32）の設定ウィンドウ。Windows のみ。`docs/decisions.md` H-104。
//!
//! 画面の内容・入力の検証・レイアウト計算・バックエンド用ワーカーは OS 非依存の
//! `native_model` / `native_layout` / `native_worker`（Linux でテスト済み）にあり、ここは
//! それらを Win32 のウィンドウ・コントロールに写すだけの層。unsafe はこのモジュールに隔離する。
#![allow(unsafe_code)]

mod controls;
mod dpi;
mod page;
mod state;
mod window;

use crate::native_worker::Factory;

/// ネイティブ設定ウィンドウの起動条件
pub struct NativeOptions {
    /// バックエンド（`imserver`）を作る関数。ワーカースレッドで呼ばれる（起動に最大 60 秒）
    pub factory: Factory,
    /// 更新ページを開いた状態で始める（`--section update`）
    pub open_update: bool,
    /// 開いたら更新の確認を自動で実行する（`--auto-check`。更新ページを開く）
    pub auto_check: bool,
}

/// ウィンドウを開き、閉じられるまで動かす。すでに開いている設定ウィンドウがあれば、それを前面に出して
/// すぐ戻る。戻る前にバックエンド（`imserver`）を終了する
pub fn run(options: NativeOptions) -> Result<(), String> {
    window::run(options)
}
