//! アプリごとの入力モードの記憶（H-49）の永続化に関わる engine の振る舞い:
//! 保存済みの記憶の投入（`load_app_modes`）、利用者の切り替えの通知（`ModeObserver`）、
//! secure input 中の通知の抑止、記憶の消去。

use nagi_engine::{Engine, EngineConfig, KanaBackend, ModeObserver, parse_key};
use nagi_protocol::methods::SetContextParams;
use nagi_protocol::view::InputMode;
use std::sync::{Arc, Mutex};

type Seen = Arc<Mutex<Vec<(String, InputMode)>>>;

/// 通知された記憶を記録する observer
struct Recording(Seen);

impl ModeObserver for Recording {
    fn on_mode_remembered(&self, app: &str, mode: InputMode) {
        self.0.lock().unwrap().push((app.to_string(), mode));
    }
}

/// `[apps] remember_mode` を有効にした設定（エンジンの既定は無効）
fn config() -> EngineConfig {
    let mut config = EngineConfig::default();
    config.apps.remember_mode = true;
    config
}

fn engine() -> Engine {
    Engine::new(Arc::new(KanaBackend), config())
}

fn engine_with_observer() -> (Engine, Seen) {
    let seen: Seen = Arc::default();
    let mut e = engine();
    e.set_mode_observer(Arc::new(Recording(seen.clone())));
    (e, seen)
}

fn set_app(e: &mut Engine, session: &str, app: &str, secure: Option<bool>) {
    e.set_context(
        session,
        &SetContextParams {
            session_id: session.into(),
            surrounding_text: None,
            app_id: Some(app.into()),
            secure_input: secure,
        },
    );
}

#[test]
fn user_mode_switch_is_notified_with_lowercase_app() {
    let (mut e, seen) = engine_with_observer();
    set_app(&mut e, "s", "Com.Example.Code", None);
    // Eisu は英数モードへの切り替え（利用者の操作）
    e.process_key("s", &parse_key("Eisu").unwrap());
    assert_eq!(
        seen.lock().unwrap().clone(),
        vec![("com.example.code".to_string(), InputMode::Alphanumeric)]
    );
}

#[test]
fn secure_input_switch_is_not_notified() {
    let (mut e, seen) = engine_with_observer();
    set_app(&mut e, "s", "com.example.vault", Some(true));
    e.process_key("s", &parse_key("Eisu").unwrap());
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn loaded_modes_are_applied_on_focus_in() {
    let mut e = engine();
    e.load_app_modes(vec![(
        "Com.Example.Notes".to_string(),
        InputMode::Alphanumeric,
    )]);
    set_app(&mut e, "s", "com.example.notes", None);
    let r = e.focus_in("s");
    assert_eq!(r.mode, Some(InputMode::Alphanumeric));
}

#[test]
fn clear_app_modes_forgets_loaded_modes() {
    let mut e = engine();
    e.load_app_modes(vec![(
        "com.example.notes".to_string(),
        InputMode::Alphanumeric,
    )]);
    e.clear_app_modes();
    set_app(&mut e, "s", "com.example.notes", None);
    let r = e.focus_in("s");
    assert_eq!(r.mode, None);
}

#[test]
fn focus_in_switch_is_not_notified() {
    let (mut e, seen) = engine_with_observer();
    e.load_app_modes(vec![(
        "com.example.notes".to_string(),
        InputMode::Alphanumeric,
    )]);
    set_app(&mut e, "s", "com.example.notes", None);
    let r = e.focus_in("s");
    assert_eq!(r.mode, Some(InputMode::Alphanumeric));
    assert!(seen.lock().unwrap().is_empty());
}
