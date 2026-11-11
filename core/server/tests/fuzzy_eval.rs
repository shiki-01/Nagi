//! 曖昧入力の評価部品（`nagi_server::fuzzy_eval`）のテスト。合成の小辞書で動き、データ取得は要らない。

mod common;

use common::dict_with_extra;
use nagi_config::Config;
use nagi_dict::Overlay;
use nagi_engine::TypingHint;
use nagi_fuzzy::{Adjacency, KeyLayout, ReverseRomaji};
use nagi_romaji::RomajiTable;
use nagi_server::fuzzy::{FuzzyEngine, FuzzySettings, Suggestion};
use nagi_server::fuzzy_eval::{
    AdoptionTally, ClassTally, DEFAULT_PHONETIC_WEIGHTS, PhoneticWeights, ReadingClass, Rng, Tally,
    add_noise, classify_reading, flick_noise, flick_rows, percentile, phonetic_noise, tap,
    to_reading, tsv_row,
};
use std::time::Duration;

const EXTRA: &str = "\
かんじ\t1\t1\t3000\t感じ
かじ\t1\t1\t3200\t火事
がっこう\t1\t1\t3000\t学校
しんぶん\t1\t1\t3000\t新聞
ありがとう\t1\t1\t3000\tありがとう
";

#[test]
fn rng_is_reproducible_and_seed_dependent() {
    let take = |seed| {
        let mut r = Rng::new(seed);
        (0..5).map(|_| r.next_u64()).collect::<Vec<_>>()
    };
    assert_eq!(take(1), take(1));
    assert_ne!(take(1), take(2));
    let mut r = Rng::new(7);
    assert!((0..1000).all(|_| (0.0..1.0).contains(&r.unit())));
    let mean: f64 = (0..4000).map(|_| r.gauss()).sum::<f64>() / 4000.0;
    assert!(mean.abs() < 0.1, "{mean}");
}

#[test]
fn keystroke_noise_is_reproducible_and_stays_near_the_keys() {
    let adj = Adjacency::qwerty();
    let run = |seed| {
        let mut rng = Rng::new(seed);
        add_noise("arigatou", 0.5, &adj, &mut rng)
    };
    assert_eq!(run(3), run(3));
    assert!((0..20).any(|s| run(s) != "arigatou"), "ノイズが入らない");
    // p = 0 なら変えない
    let mut rng = Rng::new(1);
    assert_eq!(add_noise("arigatou", 0.0, &adj, &mut rng), "arigatou");
    // 置換は隣のキーだけ: 文字集合は元の文字と隣接キーに収まる
    let table = RomajiTable::standard();
    let reading = to_reading("kanji", &table);
    assert_eq!(reading, "かんじ");
}

#[test]
fn touch_noise_presses_the_nearest_key_with_relative_coordinates() {
    let layout = KeyLayout::qwerty();
    // σ = 0 なら狙ったキーの中心
    let strokes = tap("kanji", &layout, 0.0, &mut Rng::new(1));
    assert_eq!(strokes.iter().map(|s| s.ch).collect::<String>(), "kanji");
    for s in &strokes {
        let (x, y) = s.touch.unwrap();
        assert!((x - 0.5).abs() < 1e-4 && (y - 0.5).abs() < 1e-4);
    }
    // σ が大きいと外れる。座標は 0..1 に収まる
    let noisy = tap("kanjikanjikanji", &layout, 0.6, &mut Rng::new(2));
    assert_ne!(
        noisy.iter().map(|s| s.ch).collect::<String>(),
        "kanjikanjikanji"
    );
    assert!(noisy.iter().all(|s| {
        let (x, y) = s.touch.unwrap();
        (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)
    }));
}

#[test]
fn flick_noise_swaps_to_an_adjacent_direction_with_a_boundary_angle() {
    let rows = flick_rows();
    assert!(rows.len() >= 10, "{}", rows.len());
    // p = 0: 読みは変わらず、方向キーの文字には角度がつく（あ行の「あ」は中央 = タップ）
    let clean = flick_noise("あいうえお", 0.0, &rows, &mut Rng::new(1));
    assert_eq!(clean.noisy, "あいうえお");
    assert!(clean.flicks[0].is_none());
    assert!(clean.flicks[1..].iter().all(Option::is_some));
    // p = 1: すべて同じキーの別の文字（あ行内）に取り違える
    let all = flick_noise("あいうえお", 1.0, &rows, &mut Rng::new(1));
    assert_eq!(all.noisy.chars().count(), 5);
    for (orig, got) in "あいうえお".chars().zip(all.noisy.chars()) {
        assert_ne!(orig, got);
        assert!("あいうえお".contains(got), "{got}");
    }
    // 再現可能
    assert_eq!(
        flick_noise("かんじ", 0.5, &rows, &mut Rng::new(9)),
        flick_noise("かんじ", 0.5, &rows, &mut Rng::new(9))
    );
    // 行に無い文字（濁点つき）は誤りを入れない
    let voiced = flick_noise("が", 1.0, &rows, &mut Rng::new(1));
    assert_eq!(voiced.noisy, "が");
    assert_eq!(voiced.flicks, [None]);
}

#[test]
fn flick_noise_corrections_are_found_with_a_synthetic_dictionary() {
    let dict = dict_with_extra(EXTRA);
    let engine = FuzzyEngine::new();
    let mut config = Config::default();
    config.input.fuzzy.enabled = true;
    let settings = FuzzySettings::from_config(&config.input.fuzzy);
    let rows = flick_rows();
    let mut tally = Tally::default();
    for seed in 0..40 {
        let sample = flick_noise("しんぶん", 0.3, &rows, &mut Rng::new(seed));
        if sample.noisy == "しんぶん" || !dict.lookup(&sample.noisy).is_empty() {
            continue;
        }
        let hint = TypingHint {
            flicks: Some(&sample.flicks),
            ..TypingHint::default()
        };
        let s = engine.suggest(
            &dict,
            &Overlay::default(),
            &sample.noisy,
            &hint,
            &settings,
            false,
            Duration::from_millis(150),
        );
        tally.add(&s, "新聞");
    }
    assert!(tally.n > 0, "ノイズ入りの試行がない");
    assert!(
        tally.hit_n * 2 >= tally.n,
        "合成辞書なら半数以上は直る: {tally:?}"
    );
}

#[test]
fn typo_evaluation_on_a_synthetic_dictionary_is_deterministic() {
    let dict = dict_with_extra(EXTRA);
    let engine = FuzzyEngine::new();
    let mut config = Config::default();
    config.input.fuzzy.enabled = true;
    let settings = FuzzySettings::from_config(&config.input.fuzzy);
    let table = RomajiTable::standard();
    let reverse = ReverseRomaji::new(&table);
    let adj = Adjacency::qwerty();
    let run = || {
        let mut rng = Rng::new(42);
        let mut tally = Tally::default();
        for (reading, surface) in [
            ("がっこう", "学校"),
            ("しんぶん", "新聞"),
            ("かんじ", "感じ"),
        ] {
            for _ in 0..10 {
                let typed = reverse.to_romaji(reading).unwrap();
                let noisy = to_reading(&add_noise(&typed, 0.15, &adj, &mut rng), &table);
                if noisy == reading || !dict.lookup(&noisy).is_empty() {
                    continue;
                }
                let s = engine.suggest(
                    &dict,
                    &Overlay::default(),
                    &noisy,
                    &TypingHint::default(),
                    &settings,
                    false,
                    Duration::from_millis(150),
                );
                tally.add(&s, surface);
            }
        }
        tally
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b, "シード固定で再現できる");
    assert!(a.n > 0 && a.hit_n > 0, "{a:?}");
}

fn suggestion(surface: &str, margin: Option<f32>) -> Suggestion {
    Suggestion {
        reading: String::new(),
        surface: surface.into(),
        score: 0.0,
        channel_cost: 2.0,
        lm_margin: margin,
    }
}

#[test]
fn tally_and_adoption_counters() {
    let mut t = Tally::default();
    t.add(&[suggestion("A", None), suggestion("B", None)], "B");
    t.add(&[], "B");
    assert_eq!(
        t,
        Tally {
            n: 2,
            hit1: 0,
            hit_n: 1,
            any: 1
        }
    );

    let mut a = AdoptionTally::default();
    let s = suggestion("正", Some(10.0));
    a.add(Some(&s), Some("正"), 8.0); // 採用して正解
    a.add(Some(&s), Some("別"), 8.0); // 採用したが誤り
    a.add(Some(&s), Some("正"), 12.0); // 閾値に届かず不採用
    a.add(Some(&suggestion("正", None)), Some("正"), 1.0); // 再採点なしは不採用
    a.add(None, None, 8.0);
    assert_eq!(
        a,
        AdoptionTally {
            n: 5,
            adopted: 2,
            adopted_correct: 1
        }
    );
    // クリーン入力（正解なし）の採用は常に誤採用
    let mut clean = AdoptionTally::default();
    clean.add(Some(&s), None, 8.0);
    assert_eq!((clean.adopted, clean.adopted_correct), (1, 0));
}

#[test]
fn percentile_handles_empty_and_sorted_input() {
    assert_eq!(percentile(&[], 0.5), 0.0);
    let v = [1.0, 2.0, 3.0, 4.0, 5.0];
    assert_eq!(percentile(&v, 0.0), 1.0);
    assert_eq!(percentile(&v, 0.5), 3.0);
    assert_eq!(percentile(&v, 1.0), 5.0);
}

#[test]
fn classify_reading_distinguishes_exact_covered_and_uncovered() {
    let dict = dict_with_extra(EXTRA);
    let overlay = Overlay::default();
    // 辞書語そのもの
    assert_eq!(
        classify_reading(&dict, &overlay, "しんぶん"),
        ReadingClass::Exact
    );
    // 語の連なり（辞書語どうしの連結だが、全体は辞書語でない）
    assert_eq!(
        classify_reading(&dict, &overlay, "しんぶんかんじ"),
        ReadingClass::Covered
    );
    // 辞書に無い文字列
    assert_eq!(
        classify_reading(&dict, &overlay, "ぬぬぬぬ"),
        ReadingClass::Uncovered
    );
}

#[test]
fn class_tally_splits_by_class() {
    let mut t = ClassTally::default();
    t.add(ReadingClass::Exact, &[], "A");
    t.add(ReadingClass::Covered, &[suggestion("A", None)], "A");
    t.add(ReadingClass::Covered, &[], "A");
    assert_eq!(t.get(ReadingClass::Exact).n, 1);
    assert_eq!(t.get(ReadingClass::Covered).hit1, 1);
    assert_eq!(t.get(ReadingClass::Uncovered).n, 0);
    assert_eq!(t.total().n, 3);
    assert_eq!(
        tsv_row("s", "Covered", t.get(ReadingClass::Covered)),
        "s\tCovered\t2\t1\t1\t1"
    );
}

/// 1 つの操作だけを有効にした重み
fn only(
    dakuten_drop: f64,
    dakuten_add: f64,
    youon: f64,
    long_drop: f64,
    unsmall: f64,
) -> PhoneticWeights {
    PhoneticWeights {
        dakuten_drop,
        dakuten_add,
        youon,
        long_drop,
        unsmall,
    }
}

#[test]
fn phonetic_noise_is_deterministic_and_respects_p() {
    let w = DEFAULT_PHONETIC_WEIGHTS;
    let run = |seed, p| phonetic_noise("がっこうのしゅくだい", p, &mut Rng::new(seed), &w);
    assert_eq!(run(5, 0.5), run(5, 0.5));
    // p = 0 は不変、p = 1 は変化する
    assert_eq!(run(1, 0.0), "がっこうのしゅくだい");
    assert_ne!(run(1, 1.0), "がっこうのしゅくだい");
    // 作用できる位置が無い読みは p = 1 でも不変
    assert_eq!(
        phonetic_noise("あいえ", 1.0, &mut Rng::new(1), &w),
        "あいえ"
    );
}

#[test]
fn phonetic_noise_operations_act_as_expected() {
    let apply = |text: &str, w: PhoneticWeights| phonetic_noise(text, 1.0, &mut Rng::new(3), &w);
    // 濁点の付け忘れ
    assert_eq!(apply("がぎ", only(1.0, 0.0, 0.0, 0.0, 0.0)), "かき");
    assert_eq!(apply("ぱ", only(1.0, 0.0, 0.0, 0.0, 0.0)), "は");
    // 付け過ぎ
    assert_eq!(apply("かき", only(0.0, 1.0, 0.0, 0.0, 0.0)), "がぎ");
    let h = apply("は", only(0.0, 1.0, 0.0, 0.0, 0.0));
    assert!(h == "ば" || h == "ぱ", "{h}");
    // 拗音の簡略化
    assert_eq!(apply("しゅ", only(0.0, 0.0, 1.0, 0.0, 0.0)), "す");
    assert_eq!(apply("ちょ", only(0.0, 0.0, 1.0, 0.0, 0.0)), "と");
    assert_eq!(apply("じゃ", only(0.0, 0.0, 1.0, 0.0, 0.0)), "ざ");
    // 長音・促音・ー の脱落（お段の後の う だけ。あ段の後の う は残す）
    assert_eq!(apply("おうあう", only(0.0, 0.0, 0.0, 1.0, 0.0)), "おあう");
    assert_eq!(apply("がっこう", only(0.0, 0.0, 0.0, 1.0, 0.0)), "がこ");
    assert_eq!(apply("らーめん", only(0.0, 0.0, 0.0, 1.0, 0.0)), "らめん");
    // 小書きの通常化
    assert_eq!(apply("ゃゅょっ", only(0.0, 0.0, 0.0, 0.0, 1.0)), "やゆよつ");
}

#[test]
fn phonetic_noise_corrections_run_on_a_synthetic_dictionary() {
    let dict = dict_with_extra(EXTRA);
    let overlay = Overlay::default();
    let engine = FuzzyEngine::new();
    let mut config = Config::default();
    config.input.fuzzy.enabled = true;
    let settings = FuzzySettings::from_config(&config.input.fuzzy);
    let mut tally = ClassTally::default();
    let mut rng = Rng::new(11);
    for _ in 0..30 {
        for (reading, surface) in [("がっこう", "学校"), ("しんぶん", "新聞")] {
            let noisy = phonetic_noise(reading, 0.3, &mut rng, &DEFAULT_PHONETIC_WEIGHTS);
            if noisy == reading {
                continue;
            }
            let s = engine.suggest(
                &dict,
                &overlay,
                &noisy,
                &TypingHint::default(),
                &settings,
                false,
                Duration::from_millis(150),
            );
            tally.add(classify_reading(&dict, &overlay, &noisy), &s, surface);
        }
    }
    assert!(tally.total().n > 0);
}

#[test]
fn eval_binary_accepts_new_options_and_skips_without_data() {
    let exe = env!("CARGO_BIN_EXE_nagi-fuzzy-eval");
    let empty = std::env::temp_dir().join("nagi-fuzzy-eval-no-data");
    let tsv = empty.join("out.tsv");
    for profile in ["uniform", "phonetic", "both"] {
        let out = std::process::Command::new(exe)
            .args(["--data-dir"])
            .arg(&empty)
            .args(["--noise-profile", profile, "--tsv"])
            .arg(&tsv)
            .output()
            .unwrap();
        assert!(out.status.success(), "{profile}: {out:?}");
    }
    // 不正な値は引数エラー
    let bad = std::process::Command::new(exe)
        .args(["--data-dir"])
        .arg(&empty)
        .args(["--noise-profile", "x"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
}
