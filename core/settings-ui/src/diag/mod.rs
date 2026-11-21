//! Windows 実機の未確認項目をボタン一つで確認する診断（`docs/real-hardware-checklist.md` §8・§9）。
//!
//! 公開 API（UI・CLI から使う。安定させる）:
//! - [`Ctx`] … 場所・待ち時間・OS への問い合わせ。[`Ctx::from_args`] で実環境から作る
//! - [`run_all`] / [`run_all_with_progress`] / [`run_selected`] … 全項目（または選んだ項目）を実行して [`Report`] を返す
//! - [`run_one`] … 1 項目だけ実行して [`ItemResult`] を返す
//! - [`Report::to_text`] / [`Report::to_json`] / [`save_to`] / [`copy_to_clipboard`] … 出力
//! - [`Ctx::cancel`] … 中断の合図（項目の間で見る）
//!
//! 構成: 判定は OS 非依存の純粋な関数（`checks`・`engine`・`input`・`update`・`manual`）で、事実の収集だけが
//! [`sys::System`]（Windows では PowerShell。`unsafe` なし）。診断が起動する imserver・更新確認は
//! すべて作業ディレクトリ（`Ctx::scratch_root`）で動き、利用者の設定・学習・更新フォルダには書き込まない
//! （書き込み先の確認 `env.user` が probe ファイルを作って消すのを除く）。
//! 結果に入力内容は含めない（診断が流す文字列は固定のテスト文字列で、結果には本文を出さない）。

pub mod checks;
pub mod ctx;
pub mod engine;
pub mod entry;
pub mod input;
pub mod manual;
pub mod pe;
pub mod report;
pub mod sha256;
pub mod sys;
#[cfg(test)]
mod testutil;
pub mod tsflog;
pub mod types;
pub mod update;

pub use ctx::{Ctx, Timeouts};
pub use report::{SavedReport, copy_to_clipboard, save_to};
pub use types::{ItemId, ItemResult, Kind, Report, Status, Summary};

use engine::Engines;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

/// 進捗の段階
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 項目の実行を始める
    Started,
    /// 項目の実行が終わった（`Progress::result` に結果）
    Finished,
}

/// 進捗コールバックに渡す情報
pub struct Progress<'a> {
    /// 何番目か（0 始まり）
    pub index: usize,
    /// 全項目数
    pub total: usize,
    /// 項目
    pub id: ItemId,
    /// 段階
    pub phase: Phase,
    /// 終わったときの結果
    pub result: Option<&'a ItemResult>,
}

/// 全項目を実行する
pub fn run_all(ctx: &Ctx) -> Report {
    run_all_with_progress(ctx, &mut |_| {})
}

/// 全項目を実行する（進捗コールバックつき。コールバックは項目ごとに 2 回: 開始・終了）。
/// 長くかかる（imserver の起動・更新確認の待ちで 1 分以上）ので、UI はワーカースレッドで呼ぶこと
pub fn run_all_with_progress(ctx: &Ctx, progress: &mut dyn FnMut(&Progress)) -> Report {
    run_selected(ctx, ItemId::ALL, progress)
}

/// 選んだ項目を（渡した順に）実行する
pub fn run_selected(ctx: &Ctx, ids: &[ItemId], progress: &mut dyn FnMut(&Progress)) -> Report {
    let mut engines = Engines::default();
    let mut items = Vec::with_capacity(ids.len());
    for (index, &id) in ids.iter().enumerate() {
        progress(&Progress {
            index,
            total: ids.len(),
            id,
            phase: Phase::Started,
            result: None,
        });
        let result = if ctx.cancelled() {
            ItemResult::skipped(id, "中断されました")
        } else {
            run_item(ctx, &mut engines, id)
        };
        progress(&Progress {
            index,
            total: ids.len(),
            id,
            phase: Phase::Finished,
            result: Some(&result),
        });
        items.push(result);
    }
    // エンジンを先に止めてから作業ディレクトリを消す
    drop(engines);
    let report = Report {
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        generated_at: report::format_utc(report::now_unix()),
        environment: ctx.redact(&checks::environment_summary(ctx)),
        items,
    };
    ctx.cleanup_scratch();
    report
}

/// 1 項目だけ実行する
pub fn run_one(ctx: &Ctx, id: ItemId) -> ItemResult {
    let mut engines = Engines::default();
    let result = run_item(ctx, &mut engines, id);
    drop(engines);
    ctx.cleanup_scratch();
    result
}

/// 項目を実行して、所要時間・報告用の置き換えを仕上げる。項目の中の panic は NG として返す
fn run_item(ctx: &Ctx, engines: &mut Engines, id: ItemId) -> ItemResult {
    let started = Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| dispatch(ctx, engines, id)));
    let mut result = outcome.unwrap_or_else(|_| {
        ItemResult::ng(
            id,
            "診断の実行中に内部エラーが起きました",
            "もう一度実行し、同じなら報告してください。",
        )
    });
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    result.detail = ctx.redact(&result.detail);
    result.advice = result.advice.map(|a| ctx.redact(&a));
    result.evidence = result.evidence.iter().map(|e| ctx.redact(e)).collect();
    result
}

fn dispatch(ctx: &Ctx, engines: &mut Engines, id: ItemId) -> ItemResult {
    match id {
        ItemId::EnvArch => checks::check_arch(ctx),
        ItemId::EnvUser => checks::check_user(ctx),
        ItemId::EnvVcRuntime => checks::check_vcruntime(ctx),
        ItemId::EnvDisplay => checks::check_display(ctx),
        ItemId::InstallFiles => checks::check_install_files(ctx),
        ItemId::InstallOldFiles => checks::check_old_files(ctx),
        ItemId::InstallSignature => checks::check_signature(ctx),
        ItemId::DataFiles => checks::check_data_files(ctx),
        ItemId::TsfRegistration => checks::check_registration(ctx),
        ItemId::TsfUserProfile => checks::check_user_profile(ctx),
        ItemId::TsfInterfaces => checks::check_interfaces(ctx),
        ItemId::TsfFunctionProvider => checks::check_function_provider(ctx),
        ItemId::TsfLog => checks::check_tsf_log(ctx),
        ItemId::EngineStart => engine::check_start(ctx, engines),
        ItemId::EngineShutdown => engine::check_shutdown(ctx, engines),
        ItemId::EngineParentExit => engine::check_parent_exit(ctx),
        ItemId::EngineMemory => engine::check_memory(ctx, engines),
        ItemId::InputFuzzy => input::check_fuzzy(ctx, engines),
        ItemId::InputReconvert => input::check_reconvert(ctx, engines),
        ItemId::UpdateCheck => update::check_real(ctx),
        ItemId::UpdateOffline => update::check_offline(ctx),
        ItemId::UpdateBadSignature => update::check_bad_signature(ctx),
        ItemId::UpdateState => update::check_state(ctx),
        ItemId::ManualLanguageOptions
        | ItemId::ManualReconversion
        | ItemId::ManualMultiDpi
        | ItemId::ManualSmartScreen
        | ItemId::ManualApplyUpdate
        | ItemId::ManualArm64Emulation => manual::guide(ctx, id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testutil::{FakeSystem, ctx_with_system};

    #[test]
    fn run_all_covers_every_item_in_order_and_reports_progress() {
        let mut ctx = ctx_with_system(FakeSystem::default());
        ctx.allow_network = false;
        let mut events: Vec<(usize, Phase, ItemId)> = Vec::new();
        let report = run_all_with_progress(&ctx, &mut |p| {
            assert_eq!(p.total, ItemId::ALL.len());
            assert_eq!(p.result.is_some(), p.phase == Phase::Finished);
            events.push((p.index, p.phase, p.id));
        });
        let ids: Vec<ItemId> = report.items.iter().map(|i| i.id).collect();
        assert_eq!(ids, ItemId::ALL);
        assert_eq!(events.len(), ItemId::ALL.len() * 2);
        assert_eq!(events[0], (0, Phase::Started, ItemId::ALL[0]));
        assert_eq!(events[1], (0, Phase::Finished, ItemId::ALL[0]));
        // imserver が無い環境でも、全項目が結果を返す（panic しない）
        for item in &report.items {
            assert!(!item.detail.is_empty(), "{item:?}");
            assert_eq!(item.title, item.id.title());
        }
        assert_eq!(report.get(ItemId::EngineStart).unwrap().status, Status::Ng);
        assert_eq!(
            report.get(ItemId::UpdateCheck).unwrap().status,
            Status::Skipped
        );
        // Windows 専用の項目は Linux ではスキップ
        assert_eq!(report.get(ItemId::EnvArch).unwrap().status, Status::Skipped);
        assert!(!report.to_text().is_empty());
    }

    #[test]
    fn cancel_skips_remaining_items() {
        let ctx = ctx_with_system(FakeSystem::default());
        ctx.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let report = run_all(&ctx);
        assert!(report.items.iter().all(|i| i.status == Status::Skipped));
        assert!(report.items[0].detail.contains("中断"));
    }

    #[test]
    fn run_one_returns_that_item() {
        let ctx = ctx_with_system(FakeSystem::default());
        let r = run_one(&ctx, ItemId::EngineStart);
        assert_eq!(r.id, ItemId::EngineStart);
        assert_eq!(r.status, Status::Ng);
        let r = run_one(&ctx, ItemId::ManualSmartScreen);
        assert_eq!(r.status, Status::Manual);
    }

    #[test]
    fn scratch_is_cleaned_up() {
        let ctx = ctx_with_system(FakeSystem::default());
        std::fs::create_dir_all(&ctx.scratch_root).unwrap();
        let _ = run_one(&ctx, ItemId::EngineStart);
        assert!(!ctx.scratch_root.exists());
    }
}
