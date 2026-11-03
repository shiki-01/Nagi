//! グライド入力の評価（G0 ベンチ）: 合成軌跡の認識精度（top1 / top5 / top10）と 1 ストロークの認識時間、
//! 索引のサイズを測る。`nagi-eval`（変換精度）とは独立。
//!
//! ```text
//! cargo run --release -p nagi-glide --bin nagi-glide-eval -- --data-dir ../data \
//!     [--n 2000] [--seed 1] [--noise clean,normal,noisy] [--sampling uniform,weighted] \
//!     [--layout phone|qwerty] [--index <file>] [--write-index <file>] [--verbose] \
//!     [--sigma-shape 0.07] [--sigma-loc 0.6] [--prior 0.3] [--len-lo 0.5] [--len-hi 1.8] [--keep 256]
//! ```
//!
//! 読みは辞書の読みのうちグライドの対象になるもの（ひらがなでキー 2 つ以上）から選ぶ。
//! `uniform` は読みを一様に、`weighted` は語コストから求めた出現確率（`exp(-cost / 500)`）に比例して選ぶ
//! （よく使う語ほど出る = 実際の入力の分布に近い）。ノイズは `nagi_glide::synth` の 3 段階。
//! 合成軌跡は実機の指の挙動より楽観的なので、数値は相対比較と回帰検知に使うこと。

use nagi_dict::Dictionary;
use nagi_glide::spell::{Style, key_path};
use nagi_glide::synth::{self, Rng, SynthParams};
use nagi_glide::{GlideIndex, GlideLayout, Params, Recognizer};
use std::path::PathBuf;
use std::time::Instant;

struct Args {
    data_dir: PathBuf,
    index: Option<PathBuf>,
    write_index: Option<PathBuf>,
    n: usize,
    seed: u64,
    noise: Vec<String>,
    sampling: Vec<String>,
    layout: String,
    verbose: bool,
    params: Params,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        data_dir: PathBuf::from("../data"),
        index: None,
        write_index: None,
        n: 2000,
        seed: 1,
        noise: vec!["clean".into(), "normal".into(), "noisy".into()],
        sampling: vec!["uniform".into(), "weighted".into()],
        layout: "phone".into(),
        verbose: false,
        params: Params::default(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or(format!("{name} に値がありません"));
        let num = |s: String, name: &str| -> Result<f32, String> {
            s.parse::<f32>()
                .map_err(|_| format!("{name} は数値で指定してください"))
        };
        match arg.as_str() {
            "--data-dir" => a.data_dir = val("--data-dir")?.into(),
            "--index" => a.index = Some(val("--index")?.into()),
            "--write-index" => a.write_index = Some(val("--write-index")?.into()),
            "--n" => a.n = val("--n")?.parse().map_err(|_| "--n は整数です")?,
            "--seed" => a.seed = val("--seed")?.parse().map_err(|_| "--seed は整数です")?,
            "--noise" => a.noise = val("--noise")?.split(',').map(str::to_string).collect(),
            "--sampling" => {
                a.sampling = val("--sampling")?.split(',').map(str::to_string).collect()
            }
            "--layout" => a.layout = val("--layout")?,
            "--verbose" => a.verbose = true,
            "--sigma-shape" => a.params.sigma_shape = num(val("--sigma-shape")?, "--sigma-shape")?,
            "--sigma-loc" => a.params.sigma_location = num(val("--sigma-loc")?, "--sigma-loc")?,
            "--prior" => a.params.prior_weight = num(val("--prior")?, "--prior")?,
            "--len-lo" => a.params.length_ratio.0 = num(val("--len-lo")?, "--len-lo")?,
            "--len-hi" => a.params.length_ratio.1 = num(val("--len-hi")?, "--len-hi")?,
            "--align-points" => {
                a.params.align_points = val("--align-points")?
                    .parse()
                    .map_err(|_| "--align-points は整数です")?
            }
            "--keep1" => {
                a.params.stage1_keep = val("--keep1")?.parse().map_err(|_| "--keep1 は整数です")?
            }
            "--s1-prior" => a.params.stage1_prior_scale = num(val("--s1-prior")?, "--s1-prior")?,
            "--miss" => a.params.max_key_miss = num(val("--miss")?, "--miss")?,
            "--rms" => a.params.max_rms_miss = num(val("--rms")?, "--rms")?,
            "--end-keys" => {
                a.params.max_end_keys = val("--end-keys")?
                    .parse()
                    .map_err(|_| "--end-keys は整数です")?
            }
            "--end-radius" => a.params.end_radius = num(val("--end-radius")?, "--end-radius")?,
            "--max-loc" => a.params.max_location = num(val("--max-loc")?, "--max-loc")?,
            "--max-shape" => a.params.max_shape = num(val("--max-shape")?, "--max-shape")?,
            "--max-aligned" => {
                a.params.max_aligned = val("--max-aligned")?
                    .parse()
                    .map_err(|_| "--max-aligned は整数です")?
            }
            "--keep" => {
                a.params.coarse_keep = val("--keep")?.parse().map_err(|_| "--keep は整数です")?
            }
            "--help" | "-h" => {
                return Err("使い方はファイル先頭のコメントを参照してください".into());
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    Ok(a)
}

/// 常駐メモリ（KB）。Linux の `/proc/self/status` から（他の OS では `None`）
fn rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nagi-glide-eval: {e}");
            std::process::exit(2);
        }
    };
    let dict_path = args.data_dir.join("dict").join("nagi-dict.bin");
    let dict = match Dictionary::load(&dict_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "nagi-glide-eval: 辞書を読めません ({}): {e}\ncargo xtask fetch-dict && cargo xtask build-dict で取得してください",
                dict_path.display()
            );
            std::process::exit(1);
        }
    };

    // 索引
    let started = Instant::now();
    let index = match &args.index {
        Some(path) => GlideIndex::load(path).unwrap_or_else(|e| {
            eprintln!("nagi-glide-eval: {e}");
            std::process::exit(1);
        }),
        None => {
            let bytes = GlideIndex::dictionary_bytes(&dict);
            if let Some(path) = &args.write_index {
                GlideIndex::write_file(path, &bytes).expect("索引を書き出せません");
            }
            GlideIndex::from_vec(bytes).expect("構築した索引は正しい")
        }
    };
    println!(
        "辞書: {} 語 / {} 読み。索引: {} 項目, {} bytes ({:.2} MB), {} ms",
        dict.entry_count(),
        dict.reading_count(),
        index.len(),
        index.byte_len(),
        index.byte_len() as f64 / 1e6,
        started.elapsed().as_millis()
    );

    let rss_after_load = rss_kb();

    // 対象の読み（コスト付き）
    let mut targets: Vec<(String, u16)> = Vec::new();
    dict.for_each_reading(|r, c| {
        if key_path(r, Style::Canon).is_some() {
            targets.push((r.to_string(), c));
        }
    });
    println!("対象の読み: {}", targets.len());
    // 重み付きサンプリングの累積
    let mut cum: Vec<f64> = Vec::with_capacity(targets.len());
    let mut total = 0.0;
    for (_, c) in &targets {
        total += (-f64::from(*c) / 500.0).exp();
        cum.push(total);
    }

    let layout = match args.layout.as_str() {
        "qwerty" => GlideLayout::qwerty(),
        _ => GlideLayout::qwerty_phone(),
    };
    let mut params = args.params.clone();
    params.max_results = 10;
    let recognizer = Recognizer::with_params(layout.clone(), params);
    println!("パラメータ: {:?}\n", recognizer.params());

    let rss_before_eval = rss_kb();
    for sampling in &args.sampling {
        for noise in &args.noise {
            let synth_params = match noise.as_str() {
                "clean" => SynthParams::clean(),
                "noisy" => SynthParams::noisy(),
                _ => SynthParams::normal(),
            };
            let mut rng = Rng::new(args.seed);
            let (mut top1, mut top5, mut top10, mut done) = (0usize, 0usize, 0usize, 0usize);
            let mut times: Vec<f64> = Vec::new();
            let mut empty = 0usize;
            let mut stats_sum = [0.0f64; 5];
            for _ in 0..args.n {
                let idx = if sampling == "weighted" {
                    let x = f64::from(rng.unit()) * total;
                    cum.partition_point(|&c| c < x).min(targets.len() - 1)
                } else {
                    rng.below(targets.len())
                };
                let (reading, _) = &targets[idx];
                let Some(trace) = synth::generate(&layout, reading, &synth_params, &mut rng) else {
                    continue;
                };
                let t0 = Instant::now();
                let (out, st) = recognizer.recognize_stats(&index, &trace);
                stats_sum[0] += st.scanned as f64;
                stats_sum[1] += st.candidates as f64;
                stats_sum[2] += st.collect_us as f64 / 1000.0;
                stats_sum[3] += st.coarse_us as f64 / 1000.0;
                stats_sum[4] += st.full_us as f64 / 1000.0;
                times.push(t0.elapsed().as_secs_f64() * 1000.0);
                done += 1;
                if out.is_empty() {
                    empty += 1;
                }
                match out.iter().position(|m| &m.reading == reading) {
                    Some(0) => {
                        top1 += 1;
                        top5 += 1;
                        top10 += 1;
                    }
                    Some(p) if p < 5 => {
                        top5 += 1;
                        top10 += 1;
                    }
                    Some(_) => top10 += 1,
                    None => {
                        if args.verbose {
                            let got: Vec<&str> =
                                out.iter().take(3).map(|m| m.reading.as_str()).collect();
                            println!("  miss: {reading} -> {got:?}");
                        }
                    }
                }
            }
            times.sort_by(f64::total_cmp);
            let pct = |n: usize| 100.0 * n as f64 / done.max(1) as f64;
            println!(
                "[{sampling:8} / {noise:6}] n={done}  top1 {:.1}%  top5 {:.1}%  top10 {:.1}%  無応答 {empty}  時間 p50 {:.1}ms p95 {:.1}ms max {:.1}ms",
                pct(top1),
                pct(top5),
                pct(top10),
                percentile(&times, 0.5),
                percentile(&times, 0.95),
                times.last().copied().unwrap_or(0.0),
            );
            let k = done.max(1) as f64;
            println!(
                "    平均: 走査 {:.0} 項目 → 候補 {:.0} / 収集 {:.1}ms 粗 {:.1}ms 本 {:.1}ms",
                stats_sum[0] / k,
                stats_sum[1] / k,
                stats_sum[2] / k,
                stats_sum[3] / k,
                stats_sum[4] / k,
            );
        }
    }
    if let (Some(a), Some(b), Some(c)) = (rss_after_load, rss_before_eval, rss_kb()) {
        println!(
            "\n常駐メモリ: 索引の読み込み直後 {} MB / 評価の準備後 {} MB / 評価後 {} MB（--index で mmap した場合、索引の増分は評価前後の差に出る）",
            a / 1024,
            b / 1024,
            c / 1024
        );
    }
}
