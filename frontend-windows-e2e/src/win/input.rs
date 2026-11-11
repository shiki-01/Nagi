//! `SendInput` によるキー送出（スキャンコードベース。US 配列）。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY,
};

use crate::keys::{KeySpec, Stroke};

/// 1 つのキーイベント
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    stroke: Stroke,
    up: bool,
}

/// 修飾キーを押す → 本体の押下・離上 → 修飾キーを逆順に離す
pub fn key_events(spec: &KeySpec) -> Vec<KeyEvent> {
    let mut events = Vec::new();
    for m in &spec.modifiers {
        events.push(KeyEvent {
            stroke: *m,
            up: false,
        });
    }
    events.push(KeyEvent {
        stroke: spec.key,
        up: false,
    });
    events.push(KeyEvent {
        stroke: spec.key,
        up: true,
    });
    for m in spec.modifiers.iter().rev() {
        events.push(KeyEvent {
            stroke: *m,
            up: true,
        });
    }
    events
}

/// 1 イベントを送る
pub fn send(ev: KeyEvent) -> Result<(), String> {
    let mut flags = KEYEVENTF_SCANCODE;
    if ev.stroke.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if ev.up {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: ev.stroke.scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent == 1 {
        Ok(())
    } else {
        Err(format!(
            "SendInput が入力を受け付けませんでした（scan=0x{:02X}）。別の高権限ウィンドウが前面にある、または対話デスクトップが無い可能性",
            ev.stroke.scan
        ))
    }
}
