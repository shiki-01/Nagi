//! `cargo xtask build-kana-lm` — Mozc 辞書からかな文字 n-gram 言語モデルを生成する。
//!
//! `data/dict/mozc/` の辞書と連接コストだけから `data/dict/nagi-kana-lm.bin` を作る
//! （評価データ `evaluation.tsv` は学習に使わず、生成後の検証にだけ読む）。
//! 曖昧入力の補正で、補正前後の読みの自然さを比べる特徴量に使う。無くても動く。

use nagi_dict::kanalm_train::{KanaLmParams, corruption_check, evaluation_readings};
use nagi_dict::{ConnectionLayout, KanaLm};
use std::path::PathBuf;
use std::time::Instant;

/// タスク実行。省略した引数は既定値（`KanaLmParams::default`）を使う
pub fn run(
    out: Option<PathBuf>,
    order: Option<usize>,
    sentences: Option<usize>,
    prune: Option<f64>,
) -> Result<(), String> {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| format!("CARGO_MANIFEST_DIR が未設定: {e}"))?,
    )
    .join("..")
    .join("..");
    let mozc = root.join("data").join("dict").join("mozc");
    let out_path = out.unwrap_or_else(|| root.join("data").join("dict").join("nagi-kana-lm.bin"));
    if !mozc.exists() {
        return Err(format!(
            "Mozc 辞書が見つかりません: {}。先に `cargo xtask fetch-dict` を実行してください。",
            mozc.display()
        ));
    }
    let mut params = KanaLmParams::default();
    if let Some(o) = order {
        params.order = o.clamp(1, 8);
    }
    if let Some(s) = sentences {
        params.sentences = s;
    }
    if let Some(p) = prune {
        params.prune_min_count = p;
    }
    eprintln!("辞書を読み込み中: {}", mozc.display());
    let started = Instant::now();
    let builder = nagi_dict::load_mozc_dir(&mozc, ConnectionLayout::RidMajor)
        .map_err(|e| format!("辞書の読み込みに失敗しました: {e}"))?;
    eprintln!(
        "学習中（次数 {}、疑似文 {} 件、辞書の重み総量 {}、枝刈り下限 {}）...",
        params.order, params.sentences, params.dict_mass, params.prune_min_count
    );
    let (lm, stats) = nagi_dict::kanalm_train::train_from_builder(&builder, &params);
    let seconds = started.elapsed().as_secs_f64();
    lm.write_file(&out_path)
        .map_err(|e| format!("書き出せません {}: {e}", out_path.display()))?;
    let size = std::fs::metadata(&out_path).map_or(0, |m| m.len());
    eprintln!("完了");
    eprintln!("  生成時間: {seconds:.1}s");
    eprintln!(
        "  辞書の異なる読み: {}、疑似文の平均 {:.2} 語 / {:.1} 文字",
        stats.dict_readings, stats.avg_words, stats.avg_chars
    );
    eprintln!("  n-gram 数: {}", lm.gram_count());
    eprintln!(
        "  サイズ: {size} bytes ({:.2} MB)",
        size as f64 / 1_000_000.0
    );
    eprintln!("  出力先: {}", out_path.display());

    // 書き出したものを読み直して、評価データの読みで検証する
    let lm = KanaLm::load(&out_path).ok_or("書き出したファイルを読み直せません")?;
    if let Ok(text) = std::fs::read_to_string(mozc.join("evaluation.tsv")) {
        let readings = evaluation_readings(&text);
        let r = corruption_check(&lm, &readings, 1);
        eprintln!(
            "検証（評価用の読み {} 件、1 文字あたり対数確率の平均）: 元 {:.3} / 1 文字を置換・入れ替え {:.3} / 元の方が高い割合 {:.1}%",
            r.n,
            r.original_mean,
            r.corrupted_mean,
            100.0 * r.win_rate
        );
    }
    Ok(())
}
