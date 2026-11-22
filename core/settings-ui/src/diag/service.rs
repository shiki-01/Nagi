//! 設定画面の「動作確認」ページが使う診断サービス。
//!
//! ワーカースレッド（`http::diag::DiagRunner`）が呼ぶ。実行のたびに [`Ctx`] を作り直す（前回の事実の
//! キャッシュを引きずらない・中断の合図を実行ごとに差し替える）。判定そのものは [`super::run_all_with_progress`]
//! と [`super::run_one`]。Windows 依存の部分（PowerShell・クリップボード）は `diag::sys` / `report` の中にある。

use super::{Ctx, ItemId, ItemResult, Progress, Report, copy_to_clipboard, report, save_to};
use crate::cli::Args;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// 診断の実行・出力を頼む相手（ワーカーが呼ぶ。テストでは差し替える）
pub trait DiagService: Send + Sync {
    /// 全項目を実行する。`cancel` が立つと残りの項目はスキップされる
    fn run_all(
        &self,
        cancel: Arc<AtomicBool>,
        progress: &mut dyn FnMut(&Progress),
    ) -> Result<Report, String>;

    /// 1 項目だけ実行する
    fn run_one(&self, id: ItemId, cancel: Arc<AtomicBool>) -> Result<ItemResult, String>;

    /// レポート（テキスト）をクリップボードへコピーする
    fn copy(&self, text: &str) -> Result<(), String>;

    /// レポートを保存し、保存先（利用者名を伏せた表示用のパス）を返す
    fn save(&self, report: &Report) -> Result<String, String>;
}

/// 実環境の診断サービス（コマンドライン引数から [`Ctx`] を作る）
pub struct ArgsDiagService {
    args: Args,
}

impl ArgsDiagService {
    /// 引数から作る
    pub fn new(args: Args) -> Self {
        Self { args }
    }

    fn ctx(&self, cancel: Arc<AtomicBool>) -> Result<Ctx, String> {
        let mut ctx = Ctx::from_args(&self.args)?;
        ctx.allow_network = !self.args.diag_no_network;
        ctx.cancel = cancel;
        Ok(ctx)
    }
}

impl DiagService for ArgsDiagService {
    fn run_all(
        &self,
        cancel: Arc<AtomicBool>,
        progress: &mut dyn FnMut(&Progress),
    ) -> Result<Report, String> {
        let ctx = self.ctx(cancel)?;
        Ok(super::run_all_with_progress(&ctx, progress))
    }

    fn run_one(&self, id: ItemId, cancel: Arc<AtomicBool>) -> Result<ItemResult, String> {
        let ctx = self.ctx(cancel)?;
        Ok(super::run_one(&ctx, id))
    }

    fn copy(&self, text: &str) -> Result<(), String> {
        copy_to_clipboard(text)
    }

    fn save(&self, report: &Report) -> Result<String, String> {
        let ctx = self.ctx(Arc::new(AtomicBool::new(false)))?;
        // `--diag` と同じ保存先（既定はログフォルダ。無ければ一時フォルダ）
        let dir: PathBuf = self
            .args
            .diag_out
            .clone()
            .or_else(|| ctx.log_dir.clone())
            .unwrap_or_else(std::env::temp_dir);
        let saved = save_to(report, &dir, report::now_unix())?;
        Ok(ctx.show(&saved.text_path))
    }
}
