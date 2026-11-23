//! エンジンは、セッションを触るたびにバックエンドへ状況（secure input・アプリ）を伝える。
//! バックエンドはこれで学習による順位付けを止める（設計書 §4.11）。

use nagi_engine::{
    BackendCandidate, BackendSegment, CommitContext, ConversionBackend, Engine, EngineConfig,
    MockBackend, parse_key,
};
use nagi_protocol::methods::SetContextParams;
use std::sync::{Arc, Mutex};

type Seen = Arc<Mutex<Option<(String, Option<String>, bool)>>>;

/// 最後に伝えられた状況 (session_id, app_id, secure_input) を記録するバックエンド
struct Recording {
    inner: MockBackend,
    seen: Seen,
}

impl ConversionBackend for Recording {
    fn name(&self) -> &str {
        "recording"
    }
    fn convert_sentence(&self, reading: &str, left: &str) -> Vec<BackendSegment> {
        self.inner.convert_sentence(reading, left)
    }
    fn segment_candidates(&self, reading: &str, left: &str, limit: usize) -> Vec<BackendCandidate> {
        self.inner.segment_candidates(reading, left, limit)
    }
    fn set_session_context(&self, ctx: &CommitContext<'_>) {
        *self.seen.lock().unwrap() = Some((
            ctx.session_id.to_string(),
            ctx.app_id.map(str::to_string),
            ctx.secure_input,
        ));
    }
}

#[test]
fn backend_is_told_secure_input_and_app() {
    let seen = Arc::new(Mutex::new(None));
    let backend = Recording {
        inner: MockBackend::new(),
        seen: seen.clone(),
    };
    let mut e = Engine::new(Arc::new(backend), EngineConfig::default());
    e.set_context(
        "s",
        &SetContextParams {
            session_id: "s".into(),
            surrounding_text: None,
            app_id: Some("com.example.vault".into()),
            secure_input: Some(true),
        },
    );
    assert_eq!(
        seen.lock().unwrap().clone(),
        Some(("s".into(), Some("com.example.vault".into()), true))
    );
    // 続くキー処理でも同じ状況が伝わる
    *seen.lock().unwrap() = None;
    e.process_key("s", &parse_key("a").unwrap());
    assert_eq!(
        seen.lock().unwrap().clone(),
        Some(("s".into(), Some("com.example.vault".into()), true))
    );
    // 別セッションは状況が違う
    e.process_key("t", &parse_key("a").unwrap());
    assert_eq!(
        seen.lock().unwrap().clone(),
        Some(("t".into(), None, false))
    );
}
