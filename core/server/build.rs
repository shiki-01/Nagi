//! Windows 向けのビルドで、`imserver.exe` に Nagi のアイコン（リソース ID 1）を埋め込む（H-115）。
//!
//! imserver は入力方式の裏で動く子プロセスだが、タスクマネージャー等には exe のアイコンが出る。
//! 絵柄は `frontend-windows/assets/nagi.ico`（設定アプリ・セットアップと同じロゴ）。
//! Windows 以外のターゲット（iOS / Android のライブラリとしてのビルドを含む）では何もしない。
//! MSVC は `rc.exe`（Windows SDK）、GNU は `windres` を使う。

use std::path::PathBuf;

/// 埋め込むアイコン（リポジトリ内。絵柄の生成と一致の検査は `nagi-windows` の `icon` モジュール）
const ICON_RELATIVE: &str = "../../frontend-windows/assets/nagi.ico";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let ico = manifest_dir.join(ICON_RELATIVE);
    println!("cargo:rerun-if-changed={}", ico.display());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let rc = out_dir.join("nagi_icon.rc");
    // rc の文字列ではバックスラッシュがエスケープになるので、区切りは `/` にする
    let ico_path = ico.display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{ico_path}\"\n")).expect("rc を書けません");
    // アイコン無しでも動く。リソースコンパイラが無いときは、CI（`CI` 環境変数）の MSVC ターゲットではビルドを止め、手元では警告にとどめる
    let ci = std::env::var_os("CI").is_some()
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    match embed_resource::compile_for(&rc, ["imserver"], embed_resource::NONE) {
        embed_resource::CompilationResult::NotWindows | embed_resource::CompilationResult::Ok => {}
        embed_resource::CompilationResult::NotAttempted(reason) if !ci => {
            println!(
                "cargo:warning=アイコンを埋め込めません（リソースコンパイラが無い）: {reason}"
            );
        }
        other => panic!("アイコンを埋め込めません: {other}"),
    }
}
