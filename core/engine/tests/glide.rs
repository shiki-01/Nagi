//! グライド入力（`glide_input`）のテスト。認識そのものは `nagi-glide` の仕事なので、ここでは
//! モックのバックエンドが返す読みを使って、状態機械の振る舞い（入力中の読み・候補・確定・学習）を確認する

use nagi_engine::{
    CommitContext, CommitObserver, CommittedSegment, Engine, EngineConfig, MockBackend, StateKind,
};
use nagi_protocol::methods::{
    GlideInputParams, GlidePoint, LayoutKey, ReadingInput, SetLayoutGeometryParams,
};
use nagi_protocol::view::{CandidateSource, InputMode};
use std::sync::{Arc, Mutex};

fn config(enabled: bool) -> EngineConfig {
    EngineConfig {
        glide_input: enabled,
        ..EngineConfig::default()
    }
}

fn backend() -> MockBackend {
    MockBackend::new()
        .with_glide(vec!["かんじ", "かんい", "かんし"])
        .with_sentence("かんじ", vec![("かんじ", vec!["漢字", "感じ", "幹事"])])
        .with_segments("かんい", vec!["簡易", "艦位"])
        .with_segments("かんし", vec!["監視", "冠詞"])
}

/// 26 キーの QWERTY 配置（座標の値は認識に使わない。配置があるかどうかだけが条件）
fn geometry(layout_id: &str) -> SetLayoutGeometryParams {
    let mut keys = Vec::new();
    for (row, letters) in ["qwertyuiop", "asdfghjkl", "zxcvbnm"].iter().enumerate() {
        for (col, c) in letters.chars().enumerate() {
            keys.push(LayoutKey {
                id: c.to_string(),
                label: c.to_string(),
                x: col as f64 * 40.0,
                y: row as f64 * 50.0,
                w: 36.0,
                h: 46.0,
            });
        }
    }
    SetLayoutGeometryParams {
        session_id: None,
        layout_id: layout_id.to_string(),
        mode: "qwerty".to_string(),
        keys,
        width: 400.0,
        height: 150.0,
    }
}

fn stroke(layout_id: Option<&str>) -> GlideInputParams {
    GlideInputParams {
        session_id: "s".to_string(),
        points: (0..10)
            .map(|i| GlidePoint {
                x: i as f64 * 10.0,
                y: 20.0,
                t: i as f64 * 11.0,
            })
            .collect(),
        layout_id: layout_id.map(str::to_string),
    }
}

fn engine(enabled: bool) -> Engine {
    let mut e = Engine::new(Arc::new(backend()), config(enabled));
    e.set_layout_geometry(&geometry("qwerty-1"));
    e
}

#[test]
fn disabled_by_default_and_without_geometry() {
    // 既定は無効
    assert!(!EngineConfig::default().glide_input);
    let mut e = engine(false);
    let r = e.glide_input(&stroke(None));
    assert!(!r.consumed && r.preedit.is_none() && r.candidates.is_none());
    // 配置が無ければ消費しない
    let mut e = Engine::new(Arc::new(backend()), config(true));
    assert!(!e.glide_input(&stroke(None)).consumed);
    assert_eq!(e.snapshot("s").map(|s| s.state), Some(StateKind::Direct));
}

#[test]
fn recognized_reading_becomes_the_composing_text_with_glide_candidates() {
    let mut e = engine(true);
    let r = e.glide_input(&stroke(Some("qwerty-1")));
    assert!(r.consumed);
    assert!(r.commit.is_none());
    assert_eq!(r.preedit.as_ref().unwrap().text, "かんじ");
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Composing);

    // 同期の応答に、認識した読みの変換結果が出所 Glide の候補として載る
    let c = r.candidates.expect("候補");
    assert!(c.visible);
    let top3: Vec<(&str, Option<&str>)> = c
        .items
        .iter()
        .take(3)
        .map(|i| (i.surface.as_str(), i.reading.as_deref()))
        .collect();
    assert_eq!(
        top3,
        vec![
            ("漢字", Some("かんじ")),
            ("簡易", Some("かんい")),
            ("監視", Some("かんし"))
        ]
    );
    assert!(
        c.items
            .iter()
            .take(3)
            .all(|i| i.sources.first() == Some(&CandidateSource::Glide))
    );
    // 変換結果の出所も併記される（先頭は辞書）
    assert!(c.items[0].sources.contains(&CandidateSource::SystemDict));
    // どの候補も入力全体を置き換える
    assert!(c.items.iter().all(|i| i.covers.is_none()));
}

#[test]
fn background_refresh_keeps_glide_candidates_first_and_adds_the_rest() {
    let mut e = engine(true);
    e.glide_input(&stroke(None));
    // サーバーのデバウンス後の作り直し: 読みは最有力のまま。通常の候補（幹事など）が後ろに足される
    assert_eq!(
        e.composing_candidate_reading("s").as_deref(),
        Some("かんじ")
    );
    let r = e
        .refresh_composing_candidates("s", "かんじ")
        .expect("候補の更新");
    let items = r.candidates.unwrap().items;
    let surfaces: Vec<&str> = items.iter().map(|i| i.surface.as_str()).collect();
    assert_eq!(&surfaces[..3], ["漢字", "簡易", "監視"]);
    assert!(surfaces.contains(&"感じ") && surfaces.contains(&"幹事"));
    assert_eq!(
        surfaces.iter().filter(|s| **s == "漢字").count(),
        1,
        "同じ表記は 1 つにまとまる"
    );
    // 作り直し済みなので、次の作り直しは不要
    assert_eq!(e.composing_candidate_reading("s"), None);
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, String)>>);

impl CommitObserver for Recorder {
    fn on_commit(&self, _ctx: &CommitContext<'_>, segments: &[CommittedSegment]) {
        let mut v = self.0.lock().unwrap();
        for s in segments {
            v.push((s.reading.clone(), s.surface.clone()));
        }
    }

    fn on_forget(&self, _: &CommitContext<'_>, _: &str, _: &str) -> bool {
        false
    }
}

#[test]
fn selecting_an_alternative_commits_it_with_its_own_reading() {
    let recorder = Arc::new(Recorder::default());
    let mut e = engine(true);
    e.set_commit_observer(recorder.clone());
    e.glide_input(&stroke(None));
    // 2 番目の候補（かんい → 簡易）を選ぶ
    let r = e.select_candidate("s", 1);
    assert_eq!(r.commit.as_deref(), Some("簡易"));
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Direct);
    assert_eq!(
        *recorder.0.lock().unwrap(),
        vec![("かんい".to_string(), "簡易".to_string())],
        "選んだ候補の読みで学習する"
    );
}

#[test]
fn space_converts_the_top_reading_like_typed_input() {
    let mut e = engine(true);
    e.glide_input(&stroke(None));
    let r = e.perform_action("s", &nagi_protocol::methods::TouchAction::Space);
    assert!(r.consumed);
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Converting);
    assert_eq!(r.preedit.unwrap().text, "漢字");
}

#[test]
fn next_glide_commits_the_previous_top_candidate() {
    let recorder = Arc::new(Recorder::default());
    let mut e = engine(true);
    e.set_commit_observer(recorder.clone());
    e.glide_input(&stroke(None));
    let r = e.glide_input(&stroke(None));
    // 1 語目は先頭の候補（漢字）で確定し、2 語目が入力中になる
    assert_eq!(r.commit.as_deref(), Some("漢字"));
    // 2 語目も同じ読みなので、入力中の表示は変わらない（差分なし）
    assert!(r.preedit.is_none());
    assert_eq!(e.snapshot("s").unwrap().preedit.text, "かんじ");
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Composing);
    assert_eq!(
        *recorder.0.lock().unwrap(),
        vec![("かんじ".to_string(), "漢字".to_string())]
    );
}

#[test]
fn edited_text_is_committed_as_is_before_the_next_glide() {
    let mut e = engine(true);
    e.glide_input(&stroke(None));
    // 1 文字足して読みが変わったら、先頭の候補ではなく見えている文字列を確定する
    e.insert_reading("s", &ReadingInput::Text("あ".into()));
    let r = e.glide_input(&stroke(None));
    assert_eq!(r.commit.as_deref(), Some("かんじあ"));
}

#[test]
fn stale_layout_id_and_non_hiragana_mode_are_ignored() {
    let mut e = engine(true);
    // 配置が切り替わった直後の古い軌跡
    assert!(!e.glide_input(&stroke(Some("other"))).consumed);
    // 英数モードでは認識しない
    e.perform_action(
        "s",
        &nagi_protocol::methods::TouchAction::SetMode(InputMode::Alphanumeric),
    );
    assert!(!e.glide_input(&stroke(None)).consumed);
}

#[test]
fn unrecognized_stroke_leaves_the_composing_text_alone() {
    let mut e = Engine::new(Arc::new(MockBackend::new()), config(true));
    e.set_layout_geometry(&geometry("qwerty-1"));
    e.insert_reading("s", &ReadingInput::Text("あ".into()));
    let r = e.glide_input(&stroke(None));
    assert!(!r.consumed);
    assert!(r.commit.is_none());
    assert_eq!(e.snapshot("s").unwrap().preedit.text, "あ");
}

#[test]
fn glide_candidates_disappear_with_the_composing_text() {
    let mut e = engine(true);
    e.glide_input(&stroke(None));
    let r = e.perform_action("s", &nagi_protocol::methods::TouchAction::Cancel);
    assert!(r.consumed);
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Direct);
    // 同じ読みを打ち直しても、古い Glide の候補は出ない
    e.insert_reading("s", &ReadingInput::Text("かんじ".into()));
    let items = e
        .refresh_composing_candidates("s", "かんじ")
        .and_then(|r| r.candidates)
        .map(|c| c.items)
        .unwrap_or_default();
    assert!(
        items
            .iter()
            .all(|i| !i.sources.contains(&CandidateSource::Glide))
    );
}
