//! Win32 コントロールの作成とメッセージ送信の薄いラッパー。

use std::cell::RefCell;
use std::ffi::c_void;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{CAccPropServices, IAccPropServices, PROPID_ACC_NAME};
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, CreateWindowExW, GetWindowTextLengthW, GetWindowTextW, HMENU, OBJID_CLIENT,
    SendMessageW, SetWindowTextW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WS_CHILD, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

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

/// 入力欄・選択欄・スクロールバーをダークモードの見た目にする（`dark` が偽なら既定に戻す）。
/// `DarkMode_CFD`（入力欄・コンボボックス）/ `DarkMode_Explorer`（スクロールバー・一覧）は
/// ドキュメントに載っていないが、ダークモード対応のアプリが広く使っているテーマ名。
/// 効かなくても色は `WM_CTLCOLOR*` で指定するので、読めなくなることはない
pub(super) fn set_dark_theme(hwnd: HWND, dark: bool, explorer: bool) {
    let name = match (dark, explorer) {
        (false, _) => PCWSTR::null(),
        (true, true) => w!("DarkMode_Explorer"),
        (true, false) => w!("DarkMode_CFD"),
    };
    let _ = unsafe { SetWindowTheme(hwnd, name, PCWSTR::null()) };
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
    create_child_with(
        parent,
        class,
        text,
        style | WS_VISIBLE.0,
        ex_style,
        id,
        font,
    )
}

/// 子ウィンドウを非表示で作る。フォント・位置・サイズが決まってから `ShowWindow` で出す
/// （作った瞬間に (0,0) の既定サイズで描かれて、左上に一瞬見えるのを防ぐ）
pub(super) fn create_child_hidden(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    id: i32,
    font: HFONT,
) -> Option<HWND> {
    create_child_with(
        parent,
        class,
        text,
        style & !WS_VISIBLE.0,
        ex_style,
        id,
        font,
    )
}

fn create_child_with(
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
            WINDOW_STYLE(style | WS_CHILD.0),
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

thread_local! {
    /// 読み上げの名前を設定するサービス（UI スレッドのもの。COM の初期化〜終了の間だけ持つ）
    static ACC_PROPS: RefCell<Option<IAccPropServices>> = const { RefCell::new(None) };
}

/// 読み上げ用のサービスを用意する（COM を初期化したあとに呼ぶ。作れなければ名前の設定は何もしない）
pub(super) fn init_accessibility() {
    let svc: Option<IAccPropServices> =
        unsafe { CoCreateInstance(&CAccPropServices, None, CLSCTX_INPROC_SERVER) }.ok();
    ACC_PROPS.with(|slot| *slot.borrow_mut() = svc);
}

/// 読み上げ用のサービスを解放する（COM を終了する前に呼ぶ）
pub(super) fn release_accessibility() {
    ACC_PROPS.with(|slot| *slot.borrow_mut() = None);
}

/// コントロールの「読み上げ・自動操作上の名前」を、画面に見える文字列とは別に設定する
/// （MSAA の `PROPID_ACC_NAME`。UI Automation にも橋渡しされる）。同じ見た目のボタン
/// 「既定に戻す」を項目ごとに区別するのに使う。設定できなければ何もしない（見える文字列が名前になる）
pub(super) fn set_accessible_name(hwnd: HWND, name: &str) {
    let name = wide(name);
    ACC_PROPS.with(|slot| {
        if let Some(svc) = slot.borrow().as_ref() {
            let _ = unsafe {
                svc.SetHwndPropStr(
                    hwnd,
                    OBJID_CLIENT.0 as u32,
                    CHILDID_SELF,
                    PROPID_ACC_NAME,
                    PCWSTR(name.as_ptr()),
                )
            };
        }
    });
}

/// [`set_accessible_name`] で設定した名前を消す（コントロールを破棄する前に呼ぶ）
pub(super) fn clear_accessible_name(hwnd: HWND) {
    ACC_PROPS.with(|slot| {
        if let Some(svc) = slot.borrow().as_ref() {
            let _ = unsafe {
                svc.ClearHwndProps(
                    hwnd,
                    OBJID_CLIENT.0 as u32,
                    CHILDID_SELF,
                    &[PROPID_ACC_NAME],
                )
            };
        }
    });
}
