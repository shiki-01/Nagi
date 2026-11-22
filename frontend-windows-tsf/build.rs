//! Windows 向けのビルドで、DLL に文字列リソース（STRINGTABLE）を埋め込む。
//!
//! 入力方式の表示名（ID 101）と説明（ID 102）。レジストリの `Display Description` が
//! `@<dll のパス>,-101` の形でこの文字列を指す（設定アプリが MUI 形式の参照を解決する。H-129）。
//! 値の組み立ては `nagi_windows::registration`、書き込みは `src/register.rs`。
//!
//! リソースコンパイラ（MSVC の `rc.exe` / GNU の `windres`）が無い・失敗したときは、CI（MSVC）ではビルドを
//! 止め、手元では警告にとどめる。埋め込めなかった DLL でも壊れた参照を残さないよう、登録時に実際に
//! `LoadStringW` で読めたときだけ `Display Description` を書く。

use std::path::PathBuf;

/// 埋め込む文字列。ID は `nagi_windows::registration::STRING_ID_NAME` / `STRING_ID_DESCRIPTION` と一致させる
/// （ASCII のみ。rc.exe のコードページ差を避ける）
const RC: &str =
    "STRINGTABLE\nBEGIN\n    101 \"Nagi\"\n    102 \"Nagi Japanese input method\"\nEND\n";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let rc = out_dir.join("nagi_tsf_strings.rc");
    std::fs::write(&rc, RC).expect("rc を書けません");
    let ci = std::env::var_os("CI").is_some()
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    match embed_resource::compile_for_cdylib(&rc, embed_resource::NONE) {
        embed_resource::CompilationResult::NotWindows | embed_resource::CompilationResult::Ok => {}
        embed_resource::CompilationResult::NotAttempted(reason) if !ci => {
            println!(
                "cargo:warning=文字列リソースを埋め込めません（リソースコンパイラが無い）: {reason}"
            );
        }
        other => panic!("文字列リソースを埋め込めません: {other}"),
    }
}
