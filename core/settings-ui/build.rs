//! Windows 向けのビルドで、アプリケーションマニフェストとアイコンを埋め込む（ネイティブ設定ウィンドウ用）。
//!
//! アイコンは exe のリソース ID 1 に `frontend-windows/assets/nagi.ico`（Nagi のロゴ）を入れる（H-115）。
//! エクスプローラー・タスクバー・Alt+Tab・ショートカットに出るほか、ウィンドウのアイコンも同じリソースを読む
//! （`src/native/window.rs` の `app_icon`）。
//!
//! マニフェスト:
//!
//! `embed_manifest::new_manifest` の既定値が次を含む:
//! - Per-Monitor V2 の DPI 対応（モニターごとの拡大率でぼやけない）
//! - Common Controls v6（ビジュアルスタイル。ボタン・コンボボックス等が現行の見た目になる）
//! - UTF-8 のアクティブコードページ
//!
//! MSVC はリンカーの `/MANIFEST`、GNU は `embed-manifest` が生成する COFF オブジェクトで埋め込む
//! （外部の `windres` は不要）。Windows 以外のターゲットでは何もしない。

use std::path::PathBuf;

use embed_manifest::{embed_manifest, new_manifest};

/// 埋め込むアイコン（リポジトリ内の `frontend-windows/assets/nagi.ico`。絵柄の生成と一致の検査は
/// `nagi-windows` の `icon` モジュール）
const ICON_RELATIVE: &str = "../../frontend-windows/assets/nagi.ico";

/// `.ico` を ID 1 のアイコンとして埋め込む（MSVC は `rc.exe`、GNU は `windres`）。
/// 失敗するとアイコン無しの exe になるので、リソースコンパイラが無い・失敗したときは、CI（`CI` 環境変数）では
/// ビルドを止め、手元では警告にとどめる
fn embed_icon() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let ico = manifest_dir.join(ICON_RELATIVE);
    println!("cargo:rerun-if-changed={}", ico.display());
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let rc = out_dir.join("nagi_icon.rc");
    // rc の文字列ではバックスラッシュがエスケープになるので、区切りは `/` にする
    let ico_path = ico.display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{ico_path}\"\n")).expect("rc を書けません");
    let ci = std::env::var_os("CI").is_some();
    match embed_resource::compile(&rc, embed_resource::NONE) {
        embed_resource::CompilationResult::NotWindows | embed_resource::CompilationResult::Ok => {}
        embed_resource::CompilationResult::NotAttempted(reason) if !ci => {
            println!(
                "cargo:warning=アイコンを埋め込めません（リソースコンパイラが無い）: {reason}"
            );
        }
        other => panic!("アイコンを埋め込めません: {other}"),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_icon();
        // 既定値が Per-Monitor V2・Common Controls v6・UTF-8・Windows 7〜11 対応・asInvoker
        if let Err(error) = embed_manifest(new_manifest("Nagi.Settings")) {
            // マニフェスト無しでも動く（見た目が古くなる）ので、ビルドは止めない
            println!("cargo:warning=マニフェストを埋め込めません: {error}");
        }
    }
}
