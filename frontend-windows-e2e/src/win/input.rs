//! `SendInput` によるキー送出（スキャンコードベース。US 配列）。

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MAPVK_VSC_TO_VK_EX, MapVirtualKeyW, SendInput, VIRTUAL_KEY,
};

use crate::keys::{KeySpec, Stroke};

/// `true` ならスキャンコードに加えて仮想キーも付けて送る（`KEYEVENTF_SCANCODE` を使わない）。
/// 既定の純スキャンコード方式で IME に届かないときの切り替え先
static VK_MODE: AtomicBool = AtomicBool::new(false);

/// 送出方式を切り替える（`true`: 仮想キー + スキャンコード）
pub fn set_vk_mode(on: bool) {
    VK_MODE.store(on, Ordering::Relaxed);
}

/// 現在の送出方式の名前（診断用）
pub fn mode_name() -> &'static str {
    if VK_MODE.load(Ordering::Relaxed) {
        "vk+scan"
    } else {
        "scancode"
    }
}

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
    let vk_mode = VK_MODE.load(Ordering::Relaxed);
    let mut flags = if vk_mode {
        Default::default()
    } else {
        KEYEVENTF_SCANCODE
    };
    if ev.stroke.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if ev.up {
        flags |= KEYEVENTF_KEYUP;
    }
    let vk = if vk_mode {
        let code = u32::from(ev.stroke.scan) | if ev.stroke.extended { 0xE000 } else { 0 };
        VIRTUAL_KEY(unsafe { MapVirtualKeyW(code, MAPVK_VSC_TO_VK_EX) } as u16)
    } else {
        VIRTUAL_KEY(0)
    };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
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
