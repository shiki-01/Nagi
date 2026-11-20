//! `imserver`: Nagi コアエンジンのサーバープロセス（stdin/stdout の JSON-RPC）。
//!
//! 使い方: `imserver [--data-dir <dir>] [--user-dir <dir>] [--config <file>] [--no-llm]
//! [--sync-budget-ms <ms>] [--parent-pid <pid>]`（`NAGI_DATA_DIR` でも指定可）。ログは stderr のみ。入力内容は出力しない。
//!
//! `--parent-pid <pid>`（Windows のみ有効）: 親プロセスが終了したら学習を保存して終了する（`docs/decisions.md` H-96）。
//!
//! データ取得: `imserver --install-data --data-dir <dir> [--model small|xsmall|none]`。辞書とモデルを
//! 取得して終了する（JSON-RPC は起動しない）。進捗は標準出力に 1 行 1 JSON。
//!
//! 手動の更新確認（`docs/decisions.md` H-100）: `imserver --check-update [--stage-updates] [--update-dir <dir>]
//! [--user-dir <dir>] [--data-dir <dir>]`。24 時間の間引きを無視して確認し、結果を標準出力に 1 行の JSON で出して終了する。
//!
//! Windows の自動更新（`docs/decisions.md` H-95）: `--stage-updates`（起動フラグ。新版を取得・検証して
//! `<user-dir>/update/` に保存）と、`imserver --apply-update-prepare --update-dir <dir> --out <dir>`
//! （`apply-update.ps1` が昇格後に実行する再検証。成功は終了コード 0 + 標準出力に版数、失敗は非 0 + 標準エラーに理由）。

use nagi_server::install_data::{self, parse_install_args};
use nagi_server::update_manual::{parse_check_args, run_check_update};
use nagi_server::update_stage::{parse_prepare_args, prepare_apply};
use nagi_server::{Server, build, parse_args};
use std::io::{BufReader, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    // データ取得モード: 処理して終了する（JSON-RPC サーバーとしては起動しない）
    match parse_install_args(std::env::args().skip(1)) {
        Ok(Some(args)) => {
            return match install_data::run(&args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    nagi_server::elog!("imserver: {message}");
                    ExitCode::FAILURE
                }
            };
        }
        Ok(None) => {}
        Err(message) => {
            nagi_server::elog!("imserver: {message}");
            return ExitCode::from(2);
        }
    }
    // 更新の適用前の再検証: 処理して終了する
    match parse_prepare_args(std::env::args().skip(1)) {
        Ok(Some(args)) => {
            return match prepare_apply(&args) {
                Ok(version) => {
                    println!("{version}");
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    nagi_server::elog!("imserver: {message}");
                    ExitCode::FAILURE
                }
            };
        }
        Ok(None) => {}
        Err(message) => {
            nagi_server::elog!("imserver: {message}");
            return ExitCode::from(2);
        }
    }
    // 手動の更新確認（H-100）: 間引きなしで確認し、結果を 1 行の JSON で出して終了する
    match parse_check_args(std::env::args().skip(1)) {
        Ok(Some(options)) => {
            // 親（設定画面）が終了したら、確認の途中でも終了する
            if let Some(pid) = options.parent_pid {
                nagi_parent_watch::spawn(pid, || std::process::exit(0));
            }
            println!("{}", run_check_update(&options).to_json_line());
            return ExitCode::SUCCESS;
        }
        Ok(None) => {}
        Err(message) => {
            nagi_server::elog!("imserver: {message}");
            return ExitCode::from(2);
        }
    }
    let mut options = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(message) => {
            nagi_server::elog!("imserver: {message}");
            return ExitCode::from(2);
        }
    };
    // 起動（initialize への応答）をモデル読み込みで待たせない。読み込みが済むまでは辞書のみで応答する
    options.async_model_load = true;
    let built = build(&options);
    for note in &built.notes {
        nagi_server::elog!("imserver: {note}");
    }
    nagi_server::elog!(
        "imserver: プロファイル {}（{}）",
        built.runtime.profile().as_str(),
        built.runtime.profile_reason()
    );
    // 親（フロントのホストプロセス）が終了したら、学習を書き出して終了する（Windows のみ。H-96）
    if let Some(pid) = options.parent_pid {
        let runtime = std::sync::Arc::clone(&built.runtime);
        let watching = nagi_parent_watch::spawn(pid, move || {
            nagi_server::elog!("imserver: 親プロセス（pid {pid}）が終了したので終了します");
            // 書き出しが詰まっても残り続けないよう、猶予の後に必ず終了する
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(3));
                std::process::exit(0);
            });
            runtime.flush_learning(true);
            std::process::exit(0);
        });
        if cfg!(windows) && !watching {
            nagi_server::elog!(
                "imserver: 親プロセス（pid {pid}）を監視できません（stdin の EOF のみで終了します）"
            );
        }
    }
    let mut server = Server::from_built(built);
    // `Server::run` は標準入力を読む専用スレッドに `input` を move するため、`Send + 'static` が要る。
    // `StdinLock` は（`MutexGuard` を内包するため）`Send` ではないので、ロックせずに `Stdin` を渡す
    // （`Stdin::read` は呼び出しごとに内部で lock/unlock するので、単一スレッドから使う限り挙動は変わらない）。
    let stdout = std::io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    match server.run(BufReader::new(std::io::stdin()), &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            nagi_server::elog!("imserver: 入出力エラー: {e}");
            ExitCode::FAILURE
        }
    }
}
