//! `input.auto_space_alpha`: 英語の候補を選んで確定したときだけ後ろにスペースを足し、
//! 続けて句読点を入力したら消す（H-74）

use nagi_engine::{
    AutoEnglishMode, BackendCandidate, BackendSegment, ConversionBackend, Engine, EngineConfig,
    InitialView, KanaBackend, MockBackend, parse_key,
};
use nagi_protocol::ProcessKeyResult;
use std::sync::Arc;

/// 英語らしさを High に固定するバックエンド（残りは Mock に任せる）
struct English(MockBackend);

impl ConversionBackend for English {
    fn name(&self) -> &str {
        "english"
    }
    fn convert_sentence(&self, reading: &str, left: &str) -> Vec<BackendSegment> {
        self.0.convert_sentence(reading, left)
    }
    fn segment_candidates(&self, reading: &str, left: &str, limit: usize) -> Vec<BackendCandidate> {
        self.0.segment_candidates(reading, left, limit)
    }
    fn english_level(&self, _raw: &str, _reading: &str) -> nagi_english::Level {
        nagi_english::Level::High
    }
}

fn key(e: &mut Engine, name: &str) -> ProcessKeyResult {
    e.process_key("s", &parse_key(name).unwrap())
}

fn type_keys(e: &mut Engine, keys: &str) -> ProcessKeyResult {
    let mut last = None;
    for c in keys.chars() {
        last = Some(key(e, &c.to_string()));
    }
    last.unwrap()
}

fn engine(auto_space_alpha: bool) -> Engine {
    // `typescript` の読み（ローマ字にならなかった ASCII が混ざる）
    let mut probe = Engine::new(Arc::new(KanaBackend), EngineConfig::default());
    type_keys(&mut probe, "typescript");
    let reading = key(&mut probe, "Enter").commit.unwrap();
    let inner = MockBackend::new().with_segments(&reading, vec!["漢字甲", "漢字乙", "漢字丙"]);
    Engine::new(
        Arc::new(English(inner)),
        EngineConfig {
            auto_english: AutoEnglishMode::Conservative,
            initial_view: InitialView::Segment,
            auto_space_alpha,
            apps: nagi_engine::AppsConfig {
                passthrough_chars: vec!['/'],
                ..Default::default()
            },
            ..EngineConfig::default()
        },
    )
}

/// `typescript` を打ち、英語の候補（2 番目）まで進める（確定はしない）
fn select_english(e: &mut Engine) {
    type_keys(e, "typescript");
    key(e, "Space"); // 候補ウィンドウ
    key(e, "Space"); // 2 番目 = typescript
}

#[test]
fn off_by_default_adds_no_space() {
    let mut e = engine(false);
    select_english(&mut e);
    assert_eq!(key(&mut e, "Enter").commit.as_deref(), Some("typescript"));
}

#[test]
fn on_adds_one_space_after_english_candidate() {
    let mut e = engine(true);
    select_english(&mut e);
    assert_eq!(key(&mut e, "Enter").commit.as_deref(), Some("typescript "));
}

#[test]
fn on_does_not_add_space_after_japanese_candidate_or_unselected() {
    let mut e = engine(true);
    // 先頭の候補（日本語）をそのまま確定
    type_keys(&mut e, "typescript");
    key(&mut e, "Space");
    let r = key(&mut e, "Enter");
    assert!(
        r.commit.as_deref().is_some_and(|c| !c.ends_with(' ')),
        "{r:?}"
    );
    // 英語の候補を選ばずに打った通りの英字を確定
    type_keys(&mut e, "/help");
    assert_eq!(key(&mut e, "Enter").commit.as_deref(), Some("/help"));
}

#[test]
fn punctuation_after_auto_space_deletes_it() {
    let mut e = engine(true);
    select_english(&mut e);
    key(&mut e, "Enter");
    let r = key(&mut e, ",");
    assert_eq!(r.delete_before, Some(1), "{r:?}");
    assert_eq!(r.preedit.as_ref().map(|p| p.text.as_str()), Some("、"));
    // 句読点はそのまま確定できる
    assert_eq!(key(&mut e, "Enter").commit.as_deref(), Some("、"));
}

#[test]
fn other_input_after_auto_space_keeps_it() {
    let mut e = engine(true);
    select_english(&mut e);
    key(&mut e, "Enter");
    let r = key(&mut e, "a");
    assert_eq!(r.delete_before, None, "{r:?}");
    // 句読点が 2 文字目以降なら消さない
    let r = key(&mut e, ",");
    assert_eq!(r.delete_before, None, "{r:?}");
}

#[test]
fn space_is_not_deleted_when_punctuation_is_not_next() {
    let mut e = engine(true);
    select_english(&mut e);
    key(&mut e, "Enter");
    key(&mut e, "Left"); // アプリへ渡るキーで「直後」ではなくなる
    let r = key(&mut e, ",");
    assert_eq!(r.delete_before, None, "{r:?}");
}

#[test]
fn punctuation_key_that_commits_english_drops_the_space_in_the_same_response() {
    let mut e = engine(true);
    select_english(&mut e);
    // 変換中に句読点を打つと、確定と新しい入力が同時に起きる。足すスペースは確定文字列から外す
    let r = key(&mut e, ",");
    assert_eq!(r.commit.as_deref(), Some("typescript"), "{r:?}");
    assert_eq!(r.delete_before, None);
}
