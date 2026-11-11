//! 実データでのかな言語モデルの検証（`data/dict/nagi-kana-lm.bin` と評価データがあるときだけ）。
//! `cargo xtask build-kana-lm` で作ったモデルが、評価用の読みを、1 文字を壊した読みより明確に高く評価する。

use nagi_dict::KanaLm;
use nagi_dict::kanalm_train::{corruption_check, evaluation_readings};
use std::path::Path;

#[test]
fn real_model_prefers_natural_readings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dict");
    let Some(lm) = KanaLm::load(&root.join("nagi-kana-lm.bin")) else {
        eprintln!("skip: nagi-kana-lm.bin がありません（cargo xtask build-kana-lm）");
        return;
    };
    let Ok(text) = std::fs::read_to_string(root.join("mozc/evaluation.tsv")) else {
        eprintln!("skip: evaluation.tsv がありません");
        return;
    };
    let readings = evaluation_readings(&text);
    assert!(readings.len() > 100);
    let r = corruption_check(&lm, &readings, 1);
    eprintln!("{r:?}");
    assert!(r.original_mean > r.corrupted_mean + 1.0, "{r:?}");
    assert!(r.win_rate > 0.8, "{r:?}");
}
