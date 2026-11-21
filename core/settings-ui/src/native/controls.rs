//! Win32 コントロールの作成とメッセージ送信の薄いラッパー。

use std::ffi::c_void;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowTextLengthW, GetWindowTextW, HMENU, SendMessageW, SetWindowTextW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WS_CHILD, WS_VISIBLE,
};
use windows::core::PCWSTR;

/// NUL 終端の UTF-16
pub(super) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// このモジュール（exe）のインスタンス
pub(super) fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None) }
        .map(|m| HINSTANCE(m.0))
        .unwrap_or_default()
}

/// 子ウィンドウを作る（`style` に `WS_CHILD | WS_VISIBLE` を足す）。位置は後で決める
pub(super) fn create_child(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    id: i32,
    font: HFONT,
) -> Option<HWND> {
    let class = wide(class);
    let text = wide(text);
    let menu = HMENU(std::ptr::without_provenance_mut::<c_void>(id as usize));
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            PCWSTR(class.as_ptr()),
            PCWSTR(text.as_ptr()),
            WINDOW_STYLE(style | WS_CHILD.0 | WS_VISIBLE.0),
            0,
            0,
            10,
            10,
            Some(parent),
            Some(menu),
            Some(instance()),
            None,
        )
    }
    .ok()?;
    set_font(hwnd, font);
    Some(hwnd)
}

/// フォントを設定する
pub(super) fn set_font(hwnd: HWND, font: HFONT) {
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        )
    };
}

/// メッセージを送る（戻り値は `LRESULT` の中身）
pub(super) fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }.0
}

/// 文字列を `lparam` に渡してメッセージを送る
pub(super) fn send_text(hwnd: HWND, msg: u32, wparam: usize, text: &str) -> isize {
    let text = wide(text);
    send(hwnd, msg, wparam, text.as_ptr() as isize)
}

/// ウィンドウの文字列を設定する
pub(super) fn set_text(hwnd: HWND, text: &str) {
    let text = wide(text);
    let _ = unsafe { SetWindowTextW(hwnd, PCWSTR(text.as_ptr())) };
}

/// ウィンドウの文字列を取り出す
pub(super) fn get_text(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}
