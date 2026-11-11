//! `nagi-e2e-win`: Windows の TSF フロントを、実際の OS 入力経路（SendInput → TSF → IME → テキスト欄）で
//! 検証する E2E ドライバー。シナリオ仕様は `tests/e2e/README.md`、設計と実行方法は `docs/windows-e2e.md`。
//!
//! テスト専用。IME を `regsvr32` で登録し（管理者権限が要る）、自プロセス内の標準 EDIT コントロールに
//! 対して `SendInput` でキーを送る。未確定文字列は IME 側のプローブ（`NAGI_E2E_PROBE`）から、確定済みの文字列は
//! EDIT の `WM_GETTEXT` から読む。

// Windows 以外では OS 依存の `win` が無く、キー表・Host 等は単体テストからしか使われない
#![cfg_attr(not(windows), allow(dead_code))]

mod cli;
mod keys;
mod png;
mod probe;
mod runner;
mod scenario;
#[cfg(windows)]
mod win;

fn main() {
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nagi-e2e-win: {e}\n\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    #[cfg(windows)]
    std::process::exit(win::run(&args));
    #[cfg(not(windows))]
    {
        let _ = args;
        eprintln!("nagi-e2e-win は Windows 専用です");
        std::process::exit(2);
    }
}
