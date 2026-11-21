//! `nagi-settings --diag` の入口（コマンドライン）。
//!
//! Windows の `nagi-settings.exe` は GUI サブシステムなので、画面出力を見るには
//! `nagi-settings.exe --diag > diag.txt`（リダイレクト）が要る。レポートは常にファイルにも保存する
//! （`--diag-out <dir>`。既定は `%LOCALAPPDATA%\Nagi\logs`、無ければ一時フォルダ）。

use super::{Ctx, ItemId, Phase, Status, run_selected, save_to};
use crate::cli::Args;
use std::path::PathBuf;

/// 診断を実行して出力する。戻り値は終了コード（0 = NG なし、1 = NG あり、2 = 引数の誤り）
pub fn run_cli(args: &Args) -> Result<i32, String> {
    let mut ctx = Ctx::from_args(args)?;
    ctx.allow_network = !args.diag_no_network;
    let ids: Vec<ItemId> = match &args.diag_item {
        Some(name) => {
            let id = ItemId::parse(name).ok_or_else(|| {
                format!(
                    "不明な診断項目: {name}（使える ID: {}）",
                    ItemId::ALL
                        .iter()
                        .map(|i| i.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            vec![id]
        }
        None => ItemId::ALL.to_vec(),
    };
    let report = run_selected(&ctx, &ids, &mut |p| {
        if p.phase == Phase::Finished
            && let Some(r) = p.result
        {
            eprintln!(
                "[{}/{}] {} {}",
                p.index + 1,
                p.total,
                r.status.label(),
                r.title
            );
        }
    });
    if args.diag_json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    let dir: PathBuf = args
        .diag_out
        .clone()
        .or_else(|| ctx.log_dir.clone())
        .unwrap_or_else(std::env::temp_dir);
    match save_to(&report, &dir, super::report::now_unix()) {
        Ok(saved) => eprintln!("レポートを保存しました: {}", ctx.show(&saved.text_path)),
        Err(e) => eprintln!("レポートを保存できませんでした: {e}"),
    }
    Ok(if report.overall() == Status::Ng { 1 } else { 0 })
}
