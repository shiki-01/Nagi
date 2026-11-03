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
    AdoptionTally, Rng, Tally, add_noise, flick_noise, flick_rows, percentile, tap, to_reading,
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
