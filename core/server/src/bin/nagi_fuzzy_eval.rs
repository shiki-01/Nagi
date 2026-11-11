//! `nagi-fuzzy-eval`: 曖昧入力（タイプミス補正の候補、M1）の評価。
//!
//! Mozc の `evaluation.tsv`（`cargo xtask fetch-dict` で取得）の読みを辞書の文節に分け、各文節を
//! ローマ字の打鍵列に戻してノイズ（隣接キー置換・脱落・挿入・転置を、1 打鍵ごとに確率 p）を注入し、
//! Composer と同じ規則で読みに戻したものを `FuzzyEngine::suggest` に渡す。測るもの:
//!
//! 1. 補正候補の上位 N 件（`--max-candidates`、既定 5）に正解の表記（ノイズ前の読みの最良変換）が入る率
//! 2. クリーンな入力（ノイズ無し）で補正候補が出てしまう率（誤訂正率。0 に近いほど良い）
//! 3. 提案の所要時間（展開 + 辞書変換）の p50 / p95
//!
//! ```text
//! cargo run --release -p nagi-server --bin nagi-fuzzy-eval -- --data-dir ../data \
//!     [--seed 1] [--strength normal] [--max-candidates 5] [--limit N] [--verbose] \
//!     [--noise-profile uniform|phonetic|both] [--tsv <path>]
//! ```
//!
//! ノイズの種類（`--noise-profile`、既定 both）: `uniform` は上の打鍵列への一様なノイズ（と M2・フリック）、
//! `phonetic` は読み（ひらがな）の段階の音韻ノイズ（濁点の付け忘れ・付け過ぎ、拗音の簡略化、長音・促音の脱落、
//! 小書きの通常化。p = 0.05 / 0.10。重みは仮で実測ではない）、`both` は両方。
//! 各集計にはノイズ後の読みのクラス（Exact = 辞書語として完全一致 / Covered = 語の連なりとして成立 /
//! Uncovered）別の内訳を `[class]` / `[phonetic]` で始まる追加行として出す（既存の行は変えない）。
//! `--dump-rows <path>` で、採否ゲートを通す前の補正（`FuzzyEngine::diagnose`）を 1 件 1 行の TSV に
//! 書く（係数の診断用。`--no-time-limit` と併用して決定的にする）。対象は uniform p=0.05 / 0.10・
//! phonetic p=0.05 / 0.10 の集計に入る読みと、クリーンな文節で、既存の乱数列・出力は変えない。
//! `--tsv <path>` で、集計行を `section\tclass\tn\thit1\thitn\tany` の TSV にも書く（前後比較用）。
//!
//! M2 のタッチ座標モデル（`--touch-sigma` で、ノイズ側の指のばらつきをキー間隔に対する比で指定。既定は
//! 0.18 / 0.25 / 0.32 の 3 通り）: 各打鍵を「狙ったキーの中心からのガウス乱数の位置」に置き、
//! その位置にあるキー（最も近い中心）が押されたとして打鍵列を作る。同じ入力を、M1（座標なし。離散コスト）
//! と M2（座標つき。ガウス尤度）で補正して、上位 N 件の正解率とクリーン入力の誤訂正率を比べる。
//! `--model-sigma` で M2 のモデル側の σ（既定は `DEFAULT_TOUCH_SIGMA`）を変えられる。
//!
//! 12 キーのフリック方向の誤り（`--flick-p`、既定は 0.03 / 0.06）: 読みの各文字を確率 p で同じキーの
//! 隣の方向（周回で隣・中央⇔各方向）の文字に取り違え、境界付近の角度つきで補正する。M1（角度なし）と
//! M2（角度つき）を比べる。
//!
//! 文脈つきの再採点（F1）と自動採用（F2）の評価（`--rescore`。feature `llama` とモデルが必要）:
//! 文節の左に同じ文の前の文節を左文脈として付け、補正候補を zenz で再採点した順位（`lmScore - channelCost`）
//! を辞書のスコアの順位と比べる（上位 N 件・1 位の正解率、追加レイテンシ）。あわせて、先頭の提案の
//! `lm_margin`（補正 − 入力そのまま）が閾値以上のときだけ採用する場合の、閾値ごとの採用数・採用が正解だった率と、
//! クリーンな入力（辞書語でない文節）での誤採用率を出す。
//! `--rescore-limit N`（既定 300 件）、`--rescore-p`（ノイズ率。既定 0.05）、`--model <gguf>`、
//! `--rescore-budget-ms`（再採点の上限。既定 5000。実運用の同期予算は 150）。
//!
//! 注意: ノイズは補正側と同じ隣接表から作るので、実際の打ち間違いに対する値より楽観的。
//! データ（辞書・evaluation.tsv）が無ければ skip して正常終了する。

use nagi_config::FuzzyStrength;
use nagi_dict::{Dictionary, Overlay};
use nagi_engine::TypingHint;
use nagi_fuzzy::{Adjacency, EditKinds, KeyLayout, ReverseRomaji, Stroke};
use nagi_romaji::RomajiTable;
use nagi_server::fuzzy::{FuzzyEngine, FuzzySettings};
use nagi_server::fuzzy_eval::{
    ClassTally, DEFAULT_PHONETIC_WEIGHTS, NoiseProfile, ReadingClass, Rng, Tally, add_noise,
    classify_reading, flick_noise, flick_rows, is_kana_reading, percentile, phonetic_noise, tap,
    to_reading, tsv_row,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let mut data_dir: Option<PathBuf> = None;
    let mut seed = 1u64;
    let mut strength = FuzzyStrength::Normal;
    let mut max_candidates = 5usize;
    let mut touch_sigmas: Vec<f64> = vec![0.18, 0.25, 0.32];
    let mut model_sigma = nagi_fuzzy::DEFAULT_TOUCH_SIGMA;
    let mut limit = usize::MAX;
    let mut verbose = false;
    let mut flick_ps: Vec<f64> = vec![0.03, 0.06];
    let mut rescore = false;
    let mut rescore_limit = 300usize;
    let mut rescore_p = 0.05f64;
    let mut rescore_budget_ms = 5000u64;
    let mut model_path: Option<PathBuf> = None;
    let mut profiles: Vec<NoiseProfile> = vec![NoiseProfile::Uniform, NoiseProfile::Phonetic];
    let mut tsv_out: Option<PathBuf> = None;
    let mut no_time_limit = false;
    let mut legacy_penalty = false;
    let mut dump_path: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // 展開の時間上限を外し、結果を実行ごとに同一にする（前後比較用。遅延の数値は参考にならない）
            "--no-time-limit" => no_time_limit = true,
            // 採否の規則を従来の式（covered なら編集コスト 5 倍）に戻す（新旧の比較用）
            "--legacy-penalty" => legacy_penalty = true,
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--seed" => seed = args.next().and_then(|v| v.parse().ok()).unwrap_or(1),
            "--strength" => {
                strength = match args.next().as_deref() {
                    Some("gentle") => FuzzyStrength::Gentle,
                    Some("aggressive") => FuzzyStrength::Aggressive,
                    _ => FuzzyStrength::Normal,
                }
            }
            "--max-candidates" => {
                max_candidates = args.next().and_then(|v| v.parse().ok()).unwrap_or(5)
            }
            "--touch-sigma" => {
                touch_sigmas = args
                    .next()
                    .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
                    .unwrap_or_default()
            }
            "--model-sigma" => {
                model_sigma = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(nagi_fuzzy::DEFAULT_TOUCH_SIGMA)
            }
            "--limit" => {
                limit = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(usize::MAX)
            }
            "--verbose" => verbose = true,
            "--flick-p" => {
                flick_ps = args
                    .next()
                    .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
                    .unwrap_or_default()
            }
            "--rescore" => rescore = true,
            "--rescore-limit" => {
                rescore_limit = args.next().and_then(|v| v.parse().ok()).unwrap_or(300)
            }
            "--rescore-p" => rescore_p = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.05),
            "--rescore-budget-ms" => {
                rescore_budget_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(5000)
            }
            "--noise-profile" => {
                profiles = match args.next().as_deref() {
                    Some("uniform") => vec![NoiseProfile::Uniform],
                    Some("phonetic") => vec![NoiseProfile::Phonetic],
                    Some("both") => vec![NoiseProfile::Uniform, NoiseProfile::Phonetic],
                    other => {
                        eprintln!("--noise-profile は uniform|phonetic|both: {other:?}");
                        std::process::exit(2);
                    }
                }
            }
            "--dump-rows" => dump_path = args.next().map(PathBuf::from),
            "--tsv" => tsv_out = args.next().map(PathBuf::from),
            "--model" => model_path = args.next().map(PathBuf::from),
            other => {
                eprintln!("不明な引数: {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(dir) = data_dir else {
        eprintln!("--data-dir が必要です");
        std::process::exit(2);
    };
    let dict_path = dir.join("dict/nagi-dict.bin");
    let tsv_path = dir.join("dict/mozc/evaluation.tsv");
    let (Ok(dict), Ok(text)) = (
        Dictionary::load(&dict_path),
        std::fs::read_to_string(&tsv_path),
    ) else {
        eprintln!(
            "skip: {} / {} がありません（cargo xtask fetch-dict → build-dict）",
            dict_path.display(),
            tsv_path.display()
        );
        return;
    };

    // 評価の文節: 評価データの読みを辞書の Viterbi で文節に分けたもの（正解表記 = 最良変換）
    let overlay = Overlay::default();
    let mut segments: Vec<(String, String)> = Vec::new();
    // 再採点の評価用: 文節ごとの左文脈（同じ文の前の文節の表記）
    let mut contexted: Vec<(String, String, String)> = Vec::new();
    let mut sentences: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 || !cols[3].starts_with("Conversion") {
            continue;
        }
        let reading = cols[1];
        if !is_kana_reading(reading) {
            continue;
        }
        sentences.push(reading.to_string());
        let mut left = String::new();
        for s in dict.convert(reading) {
            let n = s.reading.chars().count();
            if (3..=24).contains(&n) && is_kana_reading(&s.reading) {
                contexted.push((s.reading.clone(), s.surface.clone(), left.clone()));
                segments.push((s.reading, s.surface.clone()));
            }
            // 範囲外の文節も、後ろの文節の左文脈には入る
            left.push_str(&s.surface);
        }
    }
    segments.truncate(limit);
    contexted.truncate(limit);
    sentences.truncate(limit);
    segments.dedup();
    eprintln!("文節 {} 件、文 {} 件", segments.len(), sentences.len());

    let table = RomajiTable::standard();
    let reverse = ReverseRomaji::new(&table);
    let adjacency = Adjacency::qwerty();
    let engine = FuzzyEngine::new();
    let settings = FuzzySettings {
        enabled: true,
        max_cost: strength.max_cost(),
        max_candidates,
        kinds: EditKinds::ALL,
        use_touch: true,
        touch_sigma: model_sigma,
        no_time_limit,
        legacy_penalty,
        // 再採点・自動採用は `--rescore` の節で別に扱う（ここは辞書のスコアだけの M1 / M2 の比較）
        ..FuzzySettings::default()
    };
    let budget = Duration::from_millis(150);
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    println!(
        "strength={} max_cost={} max_candidates={} seed={seed}",
        strength, settings.max_cost, max_candidates
    );

    let uniform = profiles.contains(&NoiseProfile::Uniform);
    let dumper = DumpCtx {
        dict: &dict,
        overlay: &overlay,
        engine: &engine,
        settings: &settings,
        budget,
    };
    let mut dump_lines: Vec<String> = Vec::new();
    if dump_path.is_some() {
        dump_lines.push(DUMP_HEADER.to_string());
    }
    let mut tsv_rows: Vec<String> = Vec::new();
    let class_lines = |tag: &str, label: &str, ct: &ClassTally, tsv: &mut Vec<String>| {
        for class in ReadingClass::ALL {
            let t = ct.get(class);
            println!(
                "{tag} {label} {}: {} 件: 上位 {max_candidates} 件に {} ({:.1}%)、1 位 {} ({:.1}%)、何か出た {} ({:.1}%)",
                class.label(),
                t.n,
                t.hit_n,
                pct(t.hit_n, t.n),
                t.hit1,
                pct(t.hit1, t.n),
                t.any,
                pct(t.any, t.n)
            );
            tsv.push(tsv_row(label, class.label(), t));
        }
        tsv.push(tsv_row(label, "ALL", &ct.total()));
    };

    if uniform {
        // (2) クリーンな入力での誤訂正
        let mut clean_class = ClassTally::default();
        let mut clean_shown = 0usize;
        let mut clean_total = 0usize;
        let mut clean_multi_shown = 0usize;
        let mut clean_multi_total = 0usize;
        for (reading, _) in &segments {
            let s = engine.suggest(
                &dict,
                &overlay,
                reading,
                &TypingHint::default(),
                &settings,
                false,
                budget,
            );
            clean_total += 1;
            if dump_path.is_some() {
                dumper.dump(&mut dump_lines, "clean", reading, None);
            }
            clean_class.add(classify_reading(&dict, &overlay, reading), &s, "");
            let exact = !dict.lookup(reading).is_empty();
            if !exact {
                clean_multi_total += 1;
            }
            if !s.is_empty() {
                clean_shown += 1;
                if !exact {
                    clean_multi_shown += 1;
                }
                if verbose {
                    println!(
                        "CLEAN {reading} -> {:?}",
                        s.iter()
                            .map(|x| (&x.reading, &x.surface))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
        println!(
            "クリーン文節: 補正候補が出た {clean_shown} / {clean_total} ({:.2}%)、辞書語そのものでない文節に限ると {clean_multi_shown} / {clean_multi_total} ({:.2}%)",
            pct(clean_shown, clean_total),
            pct(clean_multi_shown, clean_multi_total)
        );
        class_lines("[class]", "clean", &clean_class, &mut tsv_rows);
        // 文全体（入力中の候補が読み全体に対して出す場合）
        let mut sent_shown = 0usize;
        let mut sent_total = 0usize;
        for reading in &sentences {
            if reading.chars().count() > 24 {
                continue;
            }
            sent_total += 1;
            if !engine
                .suggest(
                    &dict,
                    &overlay,
                    reading,
                    &TypingHint::default(),
                    &settings,
                    false,
                    budget,
                )
                .is_empty()
            {
                sent_shown += 1;
            }
        }
        println!(
            "クリーンな文（24 文字以下）: 補正候補が出た {sent_shown} / {sent_total} ({:.2}%)",
            pct(sent_shown, sent_total)
        );

        // (1)(3) ノイズ入り
        for p in [0.02, 0.05, 0.10] {
            let mut rng = Rng::new(seed ^ (p * 1000.0) as u64);
            let (mut noisy, mut hit1, mut hit_n, mut any, mut skipped) =
                (0usize, 0usize, 0usize, 0usize, 0usize);
            let mut class_tally = ClassTally::default();
            let mut latencies: Vec<f64> = Vec::new();
            let mut expand_latencies: Vec<f64> = Vec::new();
            for (reading, surface) in &segments {
                let Some(typed) = reverse.to_romaji(reading) else {
                    continue;
                };
                let noisy_typed = add_noise(&typed, p, &adjacency, &mut rng);
                let noisy_reading = to_reading(&noisy_typed, &table);
                if noisy_reading == *reading {
                    continue; // ノイズが入らなかった（または結果が同じ）
                }
                // 通常の変換でそのまま正解になっていれば、補正の対象外
                let direct = dict
                    .segment_candidates_with(&noisy_reading, 1, Some(&overlay))
                    .first()
                    .map(|c| c.surface.clone());
                if direct.as_deref() == Some(surface.as_str()) {
                    skipped += 1;
                    continue;
                }
                noisy += 1;
                if dump_path.is_some() && p >= 0.05 {
                    dumper.dump(
                        &mut dump_lines,
                        &format!("uniform{p:.2}"),
                        &noisy_reading,
                        Some((reading, surface)),
                    );
                }
                let started = Instant::now();
                let _ = engine.expand(
                    &dict,
                    &noisy_reading,
                    &TypingHint::default(),
                    &settings,
                    budget,
                );
                expand_latencies.push(started.elapsed().as_secs_f64() * 1000.0);
                let started = Instant::now();
                let s = engine.suggest(
                    &dict,
                    &overlay,
                    &noisy_reading,
                    &TypingHint::default(),
                    &settings,
                    false,
                    budget,
                );
                latencies.push(started.elapsed().as_secs_f64() * 1000.0);
                class_tally.add(
                    classify_reading(&dict, &overlay, &noisy_reading),
                    &s,
                    surface,
                );
                if !s.is_empty() {
                    any += 1;
                }
                if s.first().is_some_and(|x| &x.surface == surface) {
                    hit1 += 1;
                }
                if s.iter().any(|x| &x.surface == surface) {
                    hit_n += 1;
                } else if verbose {
                    println!(
                        "MISS p={p} {reading}({surface}) typed={noisy_typed} read={noisy_reading} -> {:?}",
                        s.iter()
                            .map(|x| (&x.reading, &x.surface))
                            .collect::<Vec<_>>()
                    );
                }
            }
            latencies.sort_by(f64::total_cmp);
            expand_latencies.sort_by(f64::total_cmp);
            println!(
                "p={p:.2}: ノイズ入り {noisy} 件（通常変換で正解済み {skipped} 件を除く）: 正解が上位 {max_candidates} 件に {hit_n} ({:.1}%)、1 位に {hit1} ({:.1}%)、何か出た {any} ({:.1}%) / 提案全体 p50={:.2}ms p95={:.2}ms max={:.2}ms / 展開のみ p50={:.2}ms p95={:.2}ms",
                pct(hit_n, noisy),
                pct(hit1, noisy),
                pct(any, noisy),
                percentile(&latencies, 0.5),
                percentile(&latencies, 0.95),
                latencies.last().copied().unwrap_or(0.0),
                percentile(&expand_latencies, 0.5),
                percentile(&expand_latencies, 0.95),
            );
            class_lines(
                "[class]",
                &format!("uniform p={p:.2}"),
                &class_tally,
                &mut tsv_rows,
            );
        }

        // M2: タッチ座標モデルでの比較
        let layout = KeyLayout::qwerty();
        println!("model_sigma={model_sigma}");
        for &sigma in &touch_sigmas {
            let mut rng = Rng::new(seed ^ 0x7007_u64 ^ (sigma * 1000.0) as u64);
            let mut tally = [Tally::default(), Tally::default()];
            let mut class_tally = [ClassTally::default(), ClassTally::default()];
            for (reading, surface) in &segments {
                let Some(typed) = reverse.to_romaji(reading) else {
                    continue;
                };
                let strokes = tap(&typed, &layout, sigma, &mut rng);
                let pressed: String = strokes.iter().map(|s| s.ch).collect();
                let noisy_reading = to_reading(&pressed, &table);
                if noisy_reading == *reading {
                    continue;
                }
                let direct = dict
                    .segment_candidates_with(&noisy_reading, 1, Some(&overlay))
                    .first()
                    .map(|c| c.surface.clone());
                if direct.as_deref() == Some(surface.as_str()) {
                    continue;
                }
                let plain: Vec<Stroke> = strokes.iter().map(|s| Stroke::key(s.ch)).collect();
                for (i, hint_strokes) in [&plain, &strokes].into_iter().enumerate() {
                    let hint = TypingHint {
                        strokes: Some(hint_strokes),
                        ..TypingHint::default()
                    };
                    let s = engine.suggest(
                        &dict,
                        &overlay,
                        &noisy_reading,
                        &hint,
                        &settings,
                        false,
                        budget,
                    );
                    tally[i].add(&s, surface);
                    class_tally[i].add(
                        classify_reading(&dict, &overlay, &noisy_reading),
                        &s,
                        surface,
                    );
                }
            }
            // クリーン入力: 全打鍵が狙ったキーに入った（誤打鍵なし）ときの座標つきの誤訂正
            let (mut clean, mut clean_shown) = (0usize, 0usize);
            for (reading, _) in &segments {
                let Some(typed) = reverse.to_romaji(reading) else {
                    continue;
                };
                let mut strokes = tap(&typed, &layout, sigma, &mut rng);
                let ok = strokes.iter().map(|s| s.ch).eq(typed.chars());
                if !ok {
                    // 誤打鍵が出た試行は除き、狙ったキーに入るまで打ち直す（打ち間違いのないクリーン入力の分布）
                    for _ in 0..20 {
                        strokes = tap(&typed, &layout, sigma, &mut rng);
                        if strokes.iter().map(|s| s.ch).eq(typed.chars()) {
                            break;
                        }
                    }
                    if !strokes.iter().map(|s| s.ch).eq(typed.chars()) {
                        continue;
                    }
                }
                clean += 1;
                let hint = TypingHint {
                    strokes: Some(&strokes),
                    ..TypingHint::default()
                };
                if !engine
                    .suggest(&dict, &overlay, reading, &hint, &settings, false, budget)
                    .is_empty()
                {
                    clean_shown += 1;
                }
            }
            println!("touch σ_noise={sigma:.2}:");
            for (name, t) in ["M1 離散", "M2 座標"].iter().zip(&tally) {
                println!(
                    "  {name}: ノイズ入り {} 件: 上位 {max_candidates} 件に {} ({:.1}%)、1 位 {} ({:.1}%)、何か出た {} ({:.1}%)",
                    t.n,
                    t.hit_n,
                    pct(t.hit_n, t.n),
                    t.hit1,
                    pct(t.hit1, t.n),
                    t.any,
                    pct(t.any, t.n)
                );
            }
            println!(
                "  M2 クリーン入力（誤打鍵なし）の誤訂正: {clean_shown} / {clean} ({:.2}%)",
                pct(clean_shown, clean)
            );
            for (name, ct) in ["M1", "M2"].iter().zip(&class_tally) {
                class_lines(
                    "  [class]",
                    &format!("touch σ={sigma:.2} {name}"),
                    ct,
                    &mut tsv_rows,
                );
            }
        }
        // 12 キーのフリック方向の誤り
        let rows = flick_rows();
        for &p in &flick_ps {
            let mut rng = Rng::new(seed ^ 0xF11C_u64 ^ (p * 1000.0) as u64);
            let mut tally = [Tally::default(), Tally::default()];
            for (reading, surface) in &segments {
                let sample = flick_noise(reading, p, &rows, &mut rng);
                if sample.noisy == *reading {
                    continue;
                }
                let direct = dict
                    .segment_candidates_with(&sample.noisy, 1, Some(&overlay))
                    .first()
                    .map(|c| c.surface.clone());
                if direct.as_deref() == Some(surface.as_str()) {
                    continue;
                }
                for (i, with_angle) in [false, true].into_iter().enumerate() {
                    let hint = TypingHint {
                        flicks: with_angle.then_some(&sample.flicks[..]),
                        ..TypingHint::default()
                    };
                    let s = engine.suggest(
                        &dict,
                        &overlay,
                        &sample.noisy,
                        &hint,
                        &settings,
                        false,
                        budget,
                    );
                    tally[i].add(&s, surface);
                }
            }
            println!("flick p={p:.2}:");
            for (name, t) in ["M1 角度なし", "M2 角度つき"].iter().zip(&tally) {
                println!(
                    "  {name}: ノイズ入り {} 件: 上位 {max_candidates} 件に {} ({:.1}%)、1 位 {} ({:.1}%)、何か出た {} ({:.1}%)",
                    t.n,
                    t.hit_n,
                    pct(t.hit_n, t.n),
                    t.hit1,
                    pct(t.hit1, t.n),
                    t.any,
                    pct(t.any, t.n)
                );
            }
        }
    }

    // 音韻ノイズ（読みの段階。打鍵列なし）
    if profiles.contains(&NoiseProfile::Phonetic) {
        for p in [0.05, 0.10] {
            let mut rng = Rng::new(seed ^ 0x9407_1C00_u64 ^ (p * 1000.0) as u64);
            let mut class_tally = ClassTally::default();
            let mut skipped = 0usize;
            for (reading, surface) in &segments {
                let noisy_reading = phonetic_noise(reading, p, &mut rng, &DEFAULT_PHONETIC_WEIGHTS);
                if noisy_reading == *reading {
                    continue;
                }
                let direct = dict
                    .segment_candidates_with(&noisy_reading, 1, Some(&overlay))
                    .first()
                    .map(|c| c.surface.clone());
                if direct.as_deref() == Some(surface.as_str()) {
                    skipped += 1;
                    continue;
                }
                if dump_path.is_some() {
                    dumper.dump(
                        &mut dump_lines,
                        &format!("phonetic{p:.2}"),
                        &noisy_reading,
                        Some((reading, surface)),
                    );
                }
                let s = engine.suggest(
                    &dict,
                    &overlay,
                    &noisy_reading,
                    &TypingHint::default(),
                    &settings,
                    false,
                    budget,
                );
                class_tally.add(
                    classify_reading(&dict, &overlay, &noisy_reading),
                    &s,
                    surface,
                );
                if verbose && !s.iter().any(|x| &x.surface == surface) {
                    println!(
                        "[phonetic] MISS p={p} {reading}({surface}) read={noisy_reading} -> {:?}",
                        s.iter()
                            .map(|x| (&x.reading, &x.surface))
                            .collect::<Vec<_>>()
                    );
                }
            }
            let t = class_tally.total();
            println!(
                "[phonetic] p={p:.2}: 音韻ノイズ入り {} 件（通常変換で正解済み {skipped} 件を除く）: 正解が上位 {max_candidates} 件に {} ({:.1}%)、1 位に {} ({:.1}%)、何か出た {} ({:.1}%)",
                t.n,
                t.hit_n,
                pct(t.hit_n, t.n),
                t.hit1,
                pct(t.hit1, t.n),
                t.any,
                pct(t.any, t.n)
            );
            class_lines(
                "[phonetic]",
                &format!("phonetic p={p:.2}"),
                &class_tally,
                &mut tsv_rows,
            );
        }
    }

    if let Some(path) = &tsv_out {
        let mut body = tsv_rows.join("\n");
        body.push('\n');
        if let Err(e) = std::fs::write(path, body) {
            eprintln!("TSV を書けません: {}: {e}", path.display());
        }
    }

    if let Some(path) = &dump_path {
        let mut body = dump_lines.join("\n");
        body.push('\n');
        if let Err(e) = std::fs::write(path, body) {
            eprintln!("行ダンプを書けません: {}: {e}", path.display());
        }
    }

    // F1 / F2: 文脈つきの再採点と自動採用
    if rescore {
        let model = model_path.or_else(|| first_gguf(&dir.join("models")));
        run_rescore(&RescoreArgs {
            dict: &dict,
            overlay: &overlay,
            engine: &engine,
            settings: &settings,
            samples: &contexted,
            reverse: &reverse,
            table: &table,
            adjacency: &adjacency,
            seed,
            p: rescore_p,
            limit: rescore_limit,
            budget: Duration::from_millis(rescore_budget_ms),
            max_candidates,
            model,
        });
    }
}

/// `--dump-rows` の TSV のヘッダ行
const DUMP_HEADER: &str = "set\tclass\treading_len\ttyped_total\ttyped_words\ttyped_unknown\ttyped_max_word_cost\ttyped_min_word_cost\ttyped_word_costs\tcand_reading\tcand_surface\tcand_cost\tchannel_cost\tkinds\tis_truth";

/// `--dump-rows` で診断を呼ぶのに要るもの
struct DumpCtx<'a> {
    dict: &'a Dictionary,
    overlay: &'a Overlay,
    engine: &'a FuzzyEngine,
    settings: &'a FuzzySettings,
    budget: Duration,
}

impl DumpCtx<'_> {
    /// `reading`（ノイズ後の読み）の補正を全部 `out` に TSV 行として足す。
    /// `truth` は (元の正しい読み, 正解の表記)。`None`（クリーン）の行は `is_truth` が常に 0
    fn dump(&self, out: &mut Vec<String>, set: &str, reading: &str, truth: Option<(&str, &str)>) {
        let Some(d) = self.engine.diagnose(
            self.dict,
            self.overlay,
            reading,
            &TypingHint::default(),
            self.settings,
            self.budget,
        ) else {
            return;
        };
        let class = if d.exact {
            ReadingClass::Exact
        } else if d.covered {
            ReadingClass::Covered
        } else {
            ReadingClass::Uncovered
        };
        let class = match class {
            ReadingClass::Exact => "exact",
            ReadingClass::Covered => "covered",
            ReadingClass::Uncovered => "uncovered",
        };
        let costs: Vec<i32> = d
            .typed
            .as_ref()
            .map(|t| t.words.iter().map(|w| w.word_cost).collect())
            .unwrap_or_default();
        let (total, words, unknown) = d.typed.as_ref().map_or((String::new(), 0, 0), |t| {
            (
                t.total_cost.to_string(),
                t.words.len(),
                t.words.iter().filter(|w| w.unknown).count(),
            )
        });
        let max = costs.iter().max().map_or(String::new(), i32::to_string);
        let min = costs.iter().min().map_or(String::new(), i32::to_string);
        let cost_list = costs
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        for r in &d.rows {
            let is_truth = truth.is_some_and(|(tr, ts)| r.surface == ts || r.reading == tr) as u8;
            out.push(format!(
                "{set}\t{class}\t{}\t{total}\t{words}\t{unknown}\t{max}\t{min}\t{cost_list}\t{}\t{}\t{}\t{}\t{}\t{is_truth}",
                reading.chars().count(),
                r.reading,
                r.surface,
                r.corrected_cost,
                r.channel_cost,
                r.kinds,
            ));
        }
    }
}

fn first_gguf(dir: &std::path::Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "gguf"))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// 再採点の評価に渡すもの（feature `llama` が無いと読まれない）
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
struct RescoreArgs<'a> {
    dict: &'a Dictionary,
    overlay: &'a Overlay,
    engine: &'a FuzzyEngine,
    settings: &'a FuzzySettings,
    /// (読み, 正解の表記, 左文脈)
    samples: &'a [(String, String, String)],
    reverse: &'a ReverseRomaji,
    table: &'a std::sync::Arc<RomajiTable>,
    adjacency: &'a Adjacency,
    seed: u64,
    p: f64,
    limit: usize,
    budget: Duration,
    max_candidates: usize,
    model: Option<PathBuf>,
}

#[cfg(not(feature = "llama"))]
fn run_rescore(_: &RescoreArgs<'_>) {
    eprintln!("skip: --rescore には feature llama が必要です（--features llama）");
}

#[cfg(feature = "llama")]
fn run_rescore(a: &RescoreArgs<'_>) {
    use nagi_infer::ConversionModel;
    use nagi_infer::llama::{LlamaModel, LlamaOptions};
    use nagi_server::fuzzy::ReadingScorer;
    use nagi_server::fuzzy_eval::AdoptionTally;

    /// `ConversionModel::score` を 1 件ずつ呼ぶ（本番の `Pipeline` の再採点と同じ呼び方）
    struct LlamaScorer(std::sync::Mutex<LlamaModel>);

    impl ReadingScorer for LlamaScorer {
        fn score(
            &self,
            left_context: &str,
            items: &[(String, String)],
            budget: Duration,
        ) -> Option<Vec<f32>> {
            let started = Instant::now();
            let mut m = self.0.lock().ok()?;
            let count = left_context.chars().count();
            let left: String = left_context
                .chars()
                .skip(count.saturating_sub(40))
                .collect();
            let mut out = Vec::with_capacity(items.len());
            for (reading, surface) in items {
                if started.elapsed() > budget {
                    return None;
                }
                let req = nagi_infer::ConvertRequest {
                    reading: reading.clone(),
                    left_context: left.clone(),
                    profile: None,
                    max_new_tokens: reading.chars().count() * 3 + 8,
                    beam_width: 1,
                };
                out.push(*m.score(&req, std::slice::from_ref(surface)).ok()?.first()?);
            }
            Some(out)
        }
    }

    let Some(path) = a.model.as_ref() else {
        eprintln!("skip: モデル（data/models/*.gguf）がありません（cargo xtask fetch-model）");
        return;
    };
    let model = match LlamaModel::load(path, LlamaOptions::default()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("skip: モデルを読めません: {e}");
            return;
        }
    };
    let scorer = LlamaScorer(std::sync::Mutex::new(model));
    let mut settings = *a.settings;
    settings.rescore = true;
    // 式の比較用: 辞書のスコアの順で上位 8 件（再採点しない）
    let mut settings8 = *a.settings;
    settings8.rescore = false;
    settings8.max_candidates = nagi_server::fuzzy::RESCORE_MAX;
    let mut variants: Vec<VariantTally> = VARIANTS.iter().map(|v| VariantTally::new(v)).collect();
    let pct = |x: usize, y: usize| 100.0 * x as f64 / y.max(1) as f64;
    let margins = [2.0f32, 4.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0];
    let mut noisy_adopt = vec![AdoptionTally::default(); margins.len()];
    let mut clean_adopt = vec![AdoptionTally::default(); margins.len()];
    let (mut base, mut resc) = (Tally::default(), Tally::default());
    let (mut lat_base, mut lat_resc): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    let mut unscored = 0usize;
    let mut rng = Rng::new(a.seed ^ 0xBEEF ^ (a.p * 1000.0) as u64);
    let hint = TypingHint::default();
    let mut used = 0usize;
    for (reading, surface, left) in a.samples {
        if used >= a.limit {
            break;
        }
        let Some(typed) = a.reverse.to_romaji(reading) else {
            continue;
        };
        let noisy_typed = add_noise(&typed, a.p, a.adjacency, &mut rng);
        let noisy_reading = to_reading(&noisy_typed, a.table);
        if noisy_reading == *reading {
            continue;
        }
        let direct = a
            .dict
            .segment_candidates_with(&noisy_reading, 1, Some(a.overlay))
            .first()
            .map(|c| c.surface.clone());
        if direct.as_deref() == Some(surface.as_str()) {
            continue;
        }
        used += 1;
        let started = Instant::now();
        let b = a.engine.suggest_with(
            a.dict,
            a.overlay,
            &noisy_reading,
            &hint,
            &settings,
            false,
            a.budget,
            None,
        );
        lat_base.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        let r = a.engine.suggest_with(
            a.dict,
            a.overlay,
            &noisy_reading,
            &hint,
            &settings,
            false,
            a.budget,
            Some((&scorer, left)),
        );
        lat_resc.push(started.elapsed().as_secs_f64() * 1000.0);
        base.add(&b, surface);
        resc.add(&r, surface);
        if r.first().is_some_and(|s| s.lm_margin.is_none()) {
            unscored += 1;
        }
        for (t, &m) in noisy_adopt.iter_mut().zip(&margins) {
            t.add(r.first(), Some(surface), m);
        }
        if let Some(ranked) = variant_rankings(a, &scorer, &settings8, &noisy_reading, left) {
            for (t, r) in variants.iter_mut().zip(&ranked) {
                t.add(r, Some(surface), true);
            }
        }
        // 同じ文節のクリーンな入力（辞書語でない読みだけ。辞書語は最初から補正されない）
        if a.dict.lookup(reading).is_empty() {
            let c = a.engine.suggest_with(
                a.dict,
                a.overlay,
                reading,
                &hint,
                &settings,
                false,
                a.budget,
                Some((&scorer, left)),
            );
            for (t, &m) in clean_adopt.iter_mut().zip(&margins) {
                t.add(c.first(), None, m);
            }
            if let Some(ranked) = variant_rankings(a, &scorer, &settings8, reading, left) {
                for (t, r) in variants.iter_mut().zip(&ranked) {
                    t.add(r, None, false);
                }
            }
        }
    }
    lat_base.sort_by(f64::total_cmp);
    lat_resc.sort_by(f64::total_cmp);
    println!(
        "rescore: model={} p={:.2} ノイズ入り {} 件（再採点できなかった {unscored} 件）",
        path.display(),
        a.p,
        base.n
    );
    for (name, t, lat) in [
        ("辞書のスコア", &base, &lat_base),
        ("再採点", &resc, &lat_resc),
    ] {
        println!(
            "  {name}: 上位 {} 件に {} ({:.1}%)、1 位 {} ({:.1}%) / 提案全体 p50={:.1}ms p95={:.1}ms",
            a.max_candidates,
            t.hit_n,
            pct(t.hit_n, t.n),
            t.hit1,
            pct(t.hit1, t.n),
            percentile(lat, 0.5),
            percentile(lat, 0.95),
        );
    }
    println!(
        "  スコア式の比較（同じ候補集合 = 辞書のスコア上位 8 件。1 位・上位 5 件の正解率、先頭の差 >= 閾値での採用）:"
    );
    for v in &variants {
        v.print(a.max_candidates);
    }
    println!("  自動採用の閾値ごと（採用 = 再採点後の先頭の lm_margin >= 閾値）:");
    for ((m, n), c) in margins.iter().zip(&noisy_adopt).zip(&clean_adopt) {
        println!(
            "    margin {m:>4.1}: ノイズ入りで採用 {} / {} ({:.1}%)、うち正解 {} ({:.1}%) / クリーンで誤採用 {} / {} ({:.2}%)",
            n.adopted,
            n.n,
            pct(n.adopted, n.n),
            n.adopted_correct,
            pct(n.adopted_correct, n.adopted),
            c.adopted,
            c.n,
            pct(c.adopted, c.n),
        );
    }
}

/// 比較するスコア式。`lm_a` = 補正後の読みでの対数確率 P(補正後の表記 | 補正後の読み)、
/// `lm_b` = 入力のままの読みでの P(補正後の表記 | 打った読み)、`channel` = 編集コスト、
/// `dict` = 辞書のスコア（Mozc 尺度。500 で割ると自然対数）
#[cfg(feature = "llama")]
const VARIANTS: &[&str] = &[
    "A  lm_a - channel      (現行)",
    "B  lm_b - channel",
    "B0 lm_b",
    "C  lm_a - dict/500",
    "D  lm_b - dict/500",
];

/// 式ごとの集計
#[cfg(feature = "llama")]
struct VariantTally {
    name: &'static str,
    noisy: Tally,
    /// (閾値, ノイズ入りで採用, うち正解, クリーンで誤採用)
    sweep: Vec<(f32, usize, usize, usize)>,
    noisy_n: usize,
    clean_n: usize,
}

#[cfg(feature = "llama")]
impl VariantTally {
    fn new(name: &'static str) -> Self {
        VariantTally {
            name,
            noisy: Tally::default(),
            sweep: [2.0f32, 4.0, 8.0, 12.0, 16.0]
                .iter()
                .map(|&m| (m, 0, 0, 0))
                .collect(),
            noisy_n: 0,
            clean_n: 0,
        }
    }

    /// `ranked` は (表記, 先頭との差) の並び（良い順）。`expected` が `None` ならクリーンな入力
    fn add(&mut self, ranked: &[(String, f32)], expected: Option<&str>, noisy: bool) {
        if noisy {
            self.noisy_n += 1;
            let hits: Vec<nagi_server::fuzzy::Suggestion> = ranked
                .iter()
                .map(|(s, m)| nagi_server::fuzzy::Suggestion {
                    reading: String::new(),
                    surface: s.clone(),
                    score: 0.0,
                    channel_cost: 0.0,
                    lm_margin: Some(*m),
                })
                .collect();
            self.noisy.add(&hits, expected.unwrap_or_default());
        } else {
            self.clean_n += 1;
        }
        for (m, adopted, correct, wrong_clean) in &mut self.sweep {
            if let Some((surface, _)) = ranked.first().filter(|(_, x)| *x >= *m) {
                if noisy {
                    *adopted += 1;
                    if expected == Some(surface.as_str()) {
                        *correct += 1;
                    }
                } else {
                    *wrong_clean += 1;
                }
            }
        }
    }

    fn print(&self, n: usize) {
        let pct = |x: usize, y: usize| 100.0 * x as f64 / y.max(1) as f64;
        let mut line = format!(
            "    {}: 1 位 {:.1}% / 上位 {n} 件 {:.1}% |",
            self.name,
            pct(self.noisy.hit1, self.noisy.n),
            pct(self.noisy.hit_n, self.noisy.n)
        );
        for (m, adopted, correct, wrong) in &self.sweep {
            line.push_str(&format!(
                " m{m:.0}: 採用{adopted}(正解{:.0}%) 誤{wrong}/{}",
                pct(*correct, *adopted),
                self.clean_n
            ));
        }
        println!("{line}");
    }
}

/// 辞書のスコア上位 8 件の候補を、各スコア式で並べ替えた結果（式の順は [`VARIANTS`]）。
/// 各要素は (表記, 先頭の差 = 候補の式の値 − 入力そのままの式の値) の降順
#[cfg(feature = "llama")]
fn variant_rankings(
    a: &RescoreArgs<'_>,
    scorer: &dyn nagi_server::fuzzy::ReadingScorer,
    settings8: &FuzzySettings,
    reading: &str,
    left: &str,
) -> Option<Vec<Vec<(String, f32)>>> {
    let list = a.engine.suggest(
        a.dict,
        a.overlay,
        reading,
        &TypingHint::default(),
        settings8,
        false,
        a.budget,
    );
    if list.is_empty() {
        return Some(vec![Vec::new(); VARIANTS.len()]);
    }
    let typed = a
        .dict
        .segment_candidates_with(reading, 1, Some(a.overlay))
        .into_iter()
        .next();
    let typed_surface = typed
        .as_ref()
        .map_or_else(|| reading.to_string(), |c| c.surface.clone());
    let typed_cost = typed.map_or(f32::MAX / 2.0, |c| c.cost as f32);
    let mut items_a = vec![(reading.to_string(), typed_surface.clone())];
    let mut items_b = items_a.clone();
    for s in &list {
        items_a.push((s.reading.clone(), s.surface.clone()));
        items_b.push((reading.to_string(), s.surface.clone()));
    }
    let la = scorer.score(left, &items_a, a.budget)?;
    let lb = scorer.score(left, &items_b, a.budget)?;
    let floor = |x: f32| {
        if x.is_finite() {
            x.max(-1000.0)
        } else {
            -1000.0
        }
    };
    let dict_nats = |x: f32| x / 500.0;
    let rank = |f: &dyn Fn(usize, &nagi_server::fuzzy::Suggestion) -> f32, base: f32| {
        let mut v: Vec<(String, f32)> = list
            .iter()
            .enumerate()
            .map(|(i, s)| (s.surface.clone(), f(i + 1, s) - base))
            .collect();
        v.sort_by(|x, y| y.1.total_cmp(&x.1));
        v
    };
    Some(vec![
        rank(&|i, s| floor(la[i]) - s.channel_cost, floor(la[0])),
        rank(&|i, s| floor(lb[i]) - s.channel_cost, floor(lb[0])),
        rank(&|i, _| floor(lb[i]), floor(lb[0])),
        rank(
            &|i, s| floor(la[i]) - dict_nats(s.score),
            floor(la[0]) - dict_nats(typed_cost),
        ),
        rank(
            &|i, s| floor(lb[i]) - dict_nats(s.score),
            floor(lb[0]) - dict_nats(typed_cost),
        ),
    ])
}
