//! 合成軌跡による認識精度のテスト（正解率の下限をアサートする）。
//!
//! 小さな語彙（`words.txt`）に、乱数で作った紛らわしい読み（語頻度の低い偽の語）を足した索引に対し、
//! シード固定の合成軌跡をなぞって top1 / top5 を測る。下限は測定値より少し緩く置いてある
//! （合成軌跡は実機より楽観的なので、これは回帰検知であり、実機の精度の保証ではない）。
//! 実際の辞書での数値は `nagi-glide-eval`（`docs/glide-input.md`）。

use nagi_glide::spell::{Style, key_path};
use nagi_glide::synth::{self, Rng, SynthParams};
use nagi_glide::{GlideIndex, GlideLayout, Recognizer};

fn words() -> Vec<(String, u16)> {
    include_str!("words.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (r, c) = l.split_once('\t').expect("読み<TAB>コスト");
            (r.to_string(), c.parse().expect("コスト"))
        })
        .collect()
}

/// 乱数で作った偽の読み（2〜5 音節。コストは高め）。本物の語の紛らわしい競合になる
fn distractors(n: usize, rng: &mut Rng) -> Vec<(String, u16)> {
    const SYLLABLES: &[&str] = &[
        "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と",
        "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も",
        "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ん", "が", "ざ", "だ", "ば", "あ",
        "い", "う", "え",
    ];
    (0..n)
        .map(|_| {
            let len = 2 + rng.below(4);
            let r: String = (0..len)
                .map(|_| SYLLABLES[rng.below(SYLLABLES.len())])
                .collect();
            (r, 8000 + rng.below(2000) as u16)
        })
        .filter(|(r, _)| key_path(r, Style::Canon).is_some())
        .collect()
}

fn index_with_distractors() -> (GlideIndex, Vec<(String, u16)>) {
    let words = words();
    let mut rng = Rng::new(99);
    let fakes = distractors(4000, &mut rng);
    let all: Vec<(&str, u16)> = words
        .iter()
        .chain(&fakes)
        .map(|(r, c)| (r.as_str(), *c))
        .collect();
    (GlideIndex::from_readings(all), words)
}

/// (top1 率, top5 率, 試行数)
fn measure(
    layout: &GlideLayout,
    index: &GlideIndex,
    words: &[(String, u16)],
    params: &SynthParams,
    seed: u64,
    per_word: usize,
) -> (f64, f64, usize) {
    let recognizer = Recognizer::new(layout.clone());
    let mut rng = Rng::new(seed);
    let (mut top1, mut top5, mut n) = (0, 0, 0);
    for (reading, _) in words {
        if key_path(reading, Style::Canon).is_none() {
            continue;
        }
        for _ in 0..per_word {
            let Some(trace) = synth::generate(layout, reading, params, &mut rng) else {
                continue;
            };
            n += 1;
            let out = recognizer.recognize(index, &trace);
            match out.iter().position(|m| &m.reading == reading) {
                Some(0) => {
                    top1 += 1;
                    top5 += 1;
                }
                Some(p) if p < 5 => top5 += 1,
                _ => {}
            }
        }
    }
    (top1 as f64 / n as f64, top5 as f64 / n as f64, n)
}

#[test]
fn clean_strokes_are_recognized_with_high_accuracy() {
    let (index, words) = index_with_distractors();
    for layout in [GlideLayout::qwerty_phone(), GlideLayout::qwerty()] {
        let (top1, top5, n) = measure(&layout, &index, &words, &SynthParams::clean(), 1, 3);
        eprintln!("clean: n={n} top1={top1:.3} top5={top5:.3}");
        assert!(top1 >= 0.95, "top1 {top1:.3}");
        assert!(top5 >= 0.98, "top5 {top5:.3}");
    }
}

#[test]
fn normal_strokes_are_recognized_within_top5() {
    let (index, words) = index_with_distractors();
    let layout = GlideLayout::qwerty_phone();
    let (top1, top5, n) = measure(&layout, &index, &words, &SynthParams::normal(), 2, 3);
    eprintln!("normal: n={n} top1={top1:.3} top5={top5:.3}");
    assert!(top1 >= 0.92, "top1 {top1:.3}");
    assert!(top5 >= 0.97, "top5 {top5:.3}");
}

#[test]
fn noisy_strokes_degrade_gracefully() {
    let (index, words) = index_with_distractors();
    let layout = GlideLayout::qwerty_phone();
    let (top1, top5, n) = measure(&layout, &index, &words, &SynthParams::noisy(), 3, 3);
    eprintln!("noisy: n={n} top1={top1:.3} top5={top5:.3}");
    assert!(top1 >= 0.78, "top1 {top1:.3}");
    assert!(top5 >= 0.88, "top5 {top5:.3}");
}

#[test]
fn hepburn_and_loop_variants_are_both_accepted() {
    // ヘボン式・ループの有無を 100% / 0% に固定しても、各流儀で高い正解率になる
    let (index, words) = index_with_distractors();
    let layout = GlideLayout::qwerty_phone();
    for (hepburn, loops) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let params = SynthParams {
            hepburn_prob: hepburn,
            loop_prob: loops,
            ..SynthParams::clean()
        };
        let (_, top5, n) = measure(&layout, &index, &words, &params, 4, 2);
        eprintln!("hepburn={hepburn} loops={loops}: n={n} top5={top5:.3}");
        assert!(
            top5 >= 0.97,
            "hepburn={hepburn} loops={loops}: top5 {top5:.3}"
        );
    }
}

#[test]
fn taps_and_garbage_return_nothing() {
    let (index, _) = index_with_distractors();
    let layout = GlideLayout::qwerty_phone();
    let recognizer = Recognizer::new(layout.clone());
    use nagi_glide::TracePoint;
    // 1 点（タップ）
    let p = |x: f64, y: f64, t: f64| TracePoint { x, y, t };
    assert!(recognizer.recognize(&index, &[p(3.0, 3.0, 0.0)]).is_empty());
    // ほとんど動いていない
    let tiny = [p(3.0, 3.0, 0.0), p(3.1, 3.0, 10.0), p(3.1, 3.1, 20.0)];
    assert!(recognizer.recognize(&index, &tiny).is_empty());
    // 空・非有限
    assert!(recognizer.recognize(&index, &[]).is_empty());
    let nan = [p(f64::NAN, 0.0, 0.0), p(f64::INFINITY, 1.0, 1.0)];
    assert!(recognizer.recognize(&index, &nan).is_empty());
    // 空の索引
    let empty = GlideIndex::from_readings([]);
    let (_, words) = index_with_distractors();
    let trace = synth::generate(
        &layout,
        &words[0].0,
        &SynthParams::clean(),
        &mut Rng::new(1),
    )
    .unwrap();
    assert!(recognizer.recognize(&empty, &trace).is_empty());
}

#[test]
fn recognition_is_deterministic_and_handles_huge_inputs() {
    let (index, words) = index_with_distractors();
    let layout = GlideLayout::qwerty_phone();
    let recognizer = Recognizer::new(layout.clone());
    let trace = synth::generate(
        &layout,
        &words[0].0,
        &SynthParams::normal(),
        &mut Rng::new(7),
    )
    .unwrap();
    assert_eq!(
        recognizer.recognize(&index, &trace),
        recognizer.recognize(&index, &trace)
    );
    // 点数が非常に多くても（サンプリング周期を極端に細かく）止まらず、同じ読みが先頭に来る
    let dense = SynthParams {
        sample_hz: 5000.0,
        drop_rate: 0.0,
        ..SynthParams::clean()
    };
    let big = synth::generate(&layout, &words[0].0, &dense, &mut Rng::new(7)).unwrap();
    assert!(big.len() > nagi_glide::MAX_INPUT_POINTS);
    let out = recognizer.recognize(&index, &big);
    assert_eq!(
        out.first().map(|m| m.reading.as_str()),
        Some(words[0].0.as_str())
    );
}

/// 実際の辞書（`data/dict/nagi-dict.bin`。無ければ skip）での回帰検知。語頻度に比例して選んだ読みの
/// 合成軌跡（normal）が top5 に入る率
#[test]
fn real_dictionary_weighted_accuracy() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dict/nagi-dict.bin");
    let Ok(dict) = nagi_dict::Dictionary::load(&path) else {
        eprintln!("skip: 辞書が見つかりません（cargo xtask fetch-dict && build-dict）");
        return;
    };
    let index = GlideIndex::from_dictionary(&dict);
    let mut targets: Vec<(String, f64)> = Vec::new();
    dict.for_each_reading(|r, c| {
        if key_path(r, Style::Canon).is_some() && c < 6500 {
            targets.push((r.to_string(), (-f64::from(c) / 500.0).exp()));
        }
    });
    let total: f64 = targets.iter().map(|t| t.1).sum();
    let layout = GlideLayout::qwerty_phone();
    let recognizer = Recognizer::new(layout.clone());
    let mut rng = Rng::new(5);
    let (mut top5, mut n) = (0, 0);
    for _ in 0..150 {
        let mut x = f64::from(rng.unit()) * total;
        let reading = &targets
            .iter()
            .find(|t| {
                x -= t.1;
                x <= 0.0
            })
            .unwrap_or(&targets[0])
            .0;
        let Some(trace) = synth::generate(&layout, reading, &SynthParams::normal(), &mut rng)
        else {
            continue;
        };
        n += 1;
        if recognizer
            .recognize(&index, &trace)
            .iter()
            .take(5)
            .any(|m| &m.reading == reading)
        {
            top5 += 1;
        }
    }
    let rate = top5 as f64 / n as f64;
    eprintln!(
        "real dict: n={n} top5={rate:.3} (index {} bytes)",
        index.byte_len()
    );
    assert!(rate >= 0.92, "top5 {rate:.3}");
}
