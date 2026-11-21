//! Windows 向けのビルドでアプリケーションマニフェストを埋め込む（ネイティブ設定ウィンドウ用）。
//!
//! `embed_manifest::new_manifest` の既定値が次を含む:
//! - Per-Monitor V2 の DPI 対応（モニターごとの拡大率でぼやけない）
//! - Common Controls v6（ビジュアルスタイル。ボタン・コンボボックス等が現行の見た目になる）
//! - UTF-8 のアクティブコードページ
//!
//! MSVC はリンカーの `/MANIFEST`、GNU は `embed-manifest` が生成する COFF オブジェクトで埋め込む
//! （外部の `windres` は不要）。Windows 以外のターゲットでは何もしない。

use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // 既定値が Per-Monitor V2・Common Controls v6・UTF-8・Windows 7〜11 対応・asInvoker
        if let Err(error) = embed_manifest(new_manifest("Nagi.Settings")) {
            // マニフェスト無しでも動く（見た目が古くなる）ので、ビルドは止めない
            println!("cargo:warning=マニフェストを埋め込めません: {error}");
        }
    }
}
