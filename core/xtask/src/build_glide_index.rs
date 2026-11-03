//! `cargo xtask build-glide-index` — システム辞書からグライド入力（なぞり入力）の索引を生成する。
//!
//! `data/dict/nagi-dict.bin` の全読みから `data/dict/nagi-glide.bin` を作る。サーバーはこのファイルが
//! あれば mmap で読み（無ければ最初のグライド入力で辞書から作る）、iOS 拡張でも常駐は触れたページだけで済む。
//! 辞書を作り直したら、索引も作り直すこと。

use std::path::PathBuf;
use std::time::Instant;

/// タスク実行。`dict` / `out` を省略するとリポジトリの `data/dict/` 内の既定の場所を使う
pub fn run(dict: Option<PathBuf>, out: Option<PathBuf>) -> Result<(), String> {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| format!("CARGO_MANIFEST_DIR が未設定: {e}"))?,
    )
    .join("..")
    .join("..");
    let dict_path = dict.unwrap_or_else(|| root.join("data").join("dict").join("nagi-dict.bin"));
    let out_path = out.unwrap_or_else(|| root.join("data").join("dict").join("nagi-glide.bin"));
    if !dict_path.exists() {
        return Err(format!(
            "辞書が見つかりません: {}。先に `cargo xtask fetch-dict` と `cargo xtask build-dict` を実行してください。",
            dict_path.display()
        ));
    }
    eprintln!("辞書: {}", dict_path.display());
    let dictionary = nagi_dict::Dictionary::load(&dict_path)
        .map_err(|e| format!("辞書の読み込みに失敗しました: {e}"))?;
    eprintln!("グライド索引を構築中...");
    let started = Instant::now();
    let bytes = nagi_glide::GlideIndex::dictionary_bytes(&dictionary);
    let seconds = started.elapsed().as_secs_f64();
    nagi_glide::GlideIndex::write_file(&out_path, &bytes)
        .map_err(|e| format!("索引を書き出せません {}: {e}", out_path.display()))?;
    // 書き出したものを読み直して検査する
    let index = nagi_glide::GlideIndex::load(&out_path).map_err(|e| e.to_string())?;
    eprintln!("完了");
    eprintln!("  構築時間: {seconds:.2}s");
    eprintln!("  項目数: {}", index.len());
    eprintln!(
        "  サイズ: {} bytes ({:.2} MB)",
        index.byte_len(),
        index.byte_len() as f64 / 1_000_000.0
    );
    eprintln!("  出力先: {}", out_path.display());
    Ok(())
}
