//! テストホスト: 標準 EDIT コントロールを 1 つ持つ最上位ウィンドウ（自プロセス内）。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH,
    FF_DONTCARE, FW_NORMAL, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, SetFocus, VK_MENU, keybd_event,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ES_AUTOHSCROLL, GetClassNameW, GetForegroundWindow,
    GetWindowThreadProcessId, HWND_TOPMOST, RegisterClassExW, SW_SHOW, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow,
    WINDOW_EX_STYLE, WM_GETTEXT, WM_GETTEXTLENGTH, WM_SETFONT, WNDCLASSEXW, WS_BORDER, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use super::pump;

/// 作ったウィンドウ
#[derive(Clone, Copy)]
pub struct HostWindow {
    /// 最上位ウィンドウ
    pub hwnd: HWND,
    /// EDIT コントロール
    pub edit: HWND,
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// ウィンドウと EDIT を作る（まだ前面化はしない）
pub fn create() -> Result<HostWindow, String> {
    let hinstance = unsafe { GetModuleHandleW(None) }
        .map_err(|e| format!("GetModuleHandleW: {e}"))?
        .into();
    let class = w!("NagiE2EHost");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: class,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&wc) } == 0 {
        return Err("RegisterClassExW に失敗しました".to_owned());
    }
    let title = wide("Nagi E2E Host");
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            80,
            80,
            960,
            200,
            None,
            None,
            Some(hinstance),
            None,
        )
    }
    .map_err(|e| format!("ホストウィンドウを作れません: {e}"))?;
    // 単一行の標準 EDIT（Return は挿入せず IME か無視される）
    let edit = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            12,
            12,
            920,
            48,
            Some(hwnd),
            None,
            Some(hinstance),
            None,
        )
    }
    .map_err(|e| format!("EDIT を作れません: {e}"))?;
    // スクリーンショットで読めるよう大きめのフォントにする（失敗しても続ける）
    let face = wide("Yu Gothic UI");
    let font = unsafe {
        CreateFontW(
            -32,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(face.as_ptr()),
        )
    };
    unsafe {
        SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        )
    };
    Ok(HostWindow { hwnd, edit })
}

/// 前面に出してフォーカスを EDIT へ。前面にできなければエラー（SendInput は前面のウィンドウへ届くため）
pub fn bring_to_front(w: &HostWindow) -> Result<(), String> {
    let _ = unsafe { ShowWindow(w.hwnd, SW_SHOW) };
    let _ = unsafe {
        SetWindowPos(
            w.hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        )
    };
    for attempt in 0..6 {
        if attempt > 0 {
            // 前面化の制限を緩める定番の回避策: Alt を一度押して離す
            unsafe {
                keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            }
        }
        let _ = unsafe { SetForegroundWindow(w.hwnd) };
        let _ = unsafe { SetFocus(Some(w.edit)) };
        pump(150);
        if unsafe { GetForegroundWindow() } == w.hwnd {
            return Ok(());
        }
    }
    Err("ホストウィンドウを前面にできません（対話デスクトップが無い可能性）".to_owned())
}

/// EDIT にフォーカスを戻す
pub fn focus(edit: HWND) {
    let _ = unsafe { SetFocus(Some(edit)) };
}

/// EDIT の全文（`WM_GETTEXT`）
pub fn edit_text(edit: HWND) -> String {
    let len = unsafe { SendMessageW(edit, WM_GETTEXTLENGTH, None, None) }
        .0
        .max(0) as usize;
    let mut buf = vec![0u16; len + 1];
    let copied = unsafe {
        SendMessageW(
            edit,
            WM_GETTEXT,
            Some(WPARAM(buf.len())),
            Some(LPARAM(buf.as_mut_ptr() as isize)),
        )
    }
    .0
    .max(0) as usize;
    String::from_utf16_lossy(&buf[..copied.min(len)])
}

/// EDIT の全文を置き換える
pub fn set_edit_text(edit: HWND, text: &str) {
    let w = wide(text);
    let _ = unsafe { SetWindowTextW(edit, PCWSTR(w.as_ptr())) };
}

/// フォーカスを一度外して戻す。IME を有効にする前にフォーカスが入っていたため、
/// TIP のキーシンクが `OnSetFocus` を受けていない可能性への対策
pub fn refocus(w: &HostWindow) {
    let _ = unsafe { SetFocus(None) };
    pump(100);
    let _ = unsafe { SetForegroundWindow(w.hwnd) };
    let _ = unsafe { SetFocus(Some(w.edit)) };
    pump(150);
}

fn window_info(label: &str, h: HWND) -> String {
    if h.is_invalid() {
        return format!("{label}=null");
    }
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut class) }.max(0) as usize;
    format!(
        "{label}={:?} class={} tid={tid} pid={pid}",
        h.0,
        String::from_utf16_lossy(&class[..n])
    )
}

/// 前面・フォーカスの状況（診断用）
pub fn describe_focus(w: &HostWindow) -> String {
    format!(
        "current_tid={} pid={} | host(tid/pid below) {} | {} | {} | edit_text_len={}",
        unsafe { GetCurrentThreadId() },
        std::process::id(),
        window_info("host", w.hwnd),
        window_info("foreground", unsafe { GetForegroundWindow() }),
        window_info("focus", unsafe { GetFocus() }),
        edit_text(w.edit).chars().count()
    )
}
