//! メインウィンドウ: クラス登録・メッセージループ・各メッセージの処理。

use super::NativeOptions;
use super::controls::{
    create_child, init_accessibility, instance, release_accessibility, send, send_text, set_font,
    set_text, wide,
};
use super::dpi::{
    apply_title_bar_theme, create_fonts, ensure_dpi_awareness, is_high_contrast, outer_size,
    system_dpi, window_dpi,
};
use super::page::{
    edit_text, ensure_visible, find_ctl, rebuild_page, refit_fonts, relayout, role_of,
    scroll_state, scroll_to,
};
use super::pages;
use super::state::{Phase, Role, State, UpdateUi};
use crate::http::{is_safe_notes_url, user_dict_dir};
use crate::native_layout::{
    Rect, control_height, frame_layout, initial_client_size, min_client_size, sidebar_item_height,
    sidebar_item_parts, wheel_step,
};
use crate::native_model::{Page, SettingsView, describe_update, ids, messages, sidebar};
use crate::native_worker::{Command, Reply, WorkerHandle};
use crate::opener::{Opener, SystemOpener};
use nagi_protocol::settings::SettingValue;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::Foundation::{
    COLORREF, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT,
    DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DrawFocusRect, DrawTextW, FillRect,
    GetSysColor, GetSysColorBrush, HBRUSH, HDC, HGDIOBJ, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT, UpdateWindow,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::SystemServices::{
    SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_ETCHEDHORZ, SS_ETCHEDVERT, SS_LEFT, SS_NOPREFIX,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, ODS_FOCUS,
    ODS_NOFOCUSRECT, ODS_SELECTED,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CB_GETCURSEL, CB_GETDROPPEDSTATE, CREATESTRUCTW,
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, FindWindowW, GWLP_USERDATA, GetClientRect, GetMessageW, GetParent,
    GetScrollInfo, GetWindowLongPtrW, HICON, HMENU, IDC_ARROW, IDYES, IsDialogMessageW, IsIconic,
    KillTimer, LB_ADDSTRING, LB_GETCURSEL, LB_GETTEXT, LB_GETTEXTLEN, LB_RESETCONTENT,
    LB_SETCURSEL, LB_SETITEMHEIGHT, LBS_HASSTRINGS, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY,
    LBS_OWNERDRAWFIXED, LoadCursorW, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MINMAXINFO, MSG,
    MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW, SB_VERT, SCROLLINFO,
    SIF_TRACKPOS, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER,
    SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_DRAWITEM, WM_GETMINMAXINFO, WM_MOUSEWHEEL,
    WM_NCCREATE, WM_SETICON, WM_SETTINGCHANGE, WM_SIZE, WM_TIMER, WM_VSCROLL, WNDCLASSEXW,
    WS_CHILD, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON,
};
use windows::core::{PCWSTR, w};

const MAIN_CLASS: &str = "NagiSettingsMainWindow";
const PANEL_CLASS: &str = "NagiSettingsPanel";
const MUTEX_NAME: &str = "Local\\NagiSettingsWindow";
const TITLE: &str = "Nagi 設定";

// コントロール通知コード（`windows` クレートの型に依存しないよう数値で持つ）
const BN_CLICKED: u32 = 0;
const BN_SETFOCUS: u32 = 6;
const EN_SETFOCUS: u32 = 0x0100;
const EN_KILLFOCUS: u32 = 0x0200;
const CBN_SELCHANGE: u32 = 1;
const CBN_SETFOCUS: u32 = 3;
const CBN_SELENDOK: u32 = 9;
const LBN_SELCHANGE: u32 = 1;
const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;
// スクロールバーのコマンド
const SB_LINEUP: i32 = 0;
const SB_LINEDOWN: i32 = 1;
const SB_PAGEUP: i32 = 2;
const SB_PAGEDOWN: i32 = 3;
const SB_THUMBPOSITION: i32 = 4;
const SB_THUMBTRACK: i32 = 5;
const SB_TOP: i32 = 6;
const SB_BOTTOM: i32 = 7;
const BST_CHECKED: isize = 1;
/// ステータス行を出しておく時間
const STATUS_MS: u32 = 6000;
/// ウィンドウを閉じたあと、imserver の終了を待つ最長時間
const SHUTDOWN_WAIT: Duration = Duration::from_secs(6);

fn hwnd_from(raw: isize) -> HWND {
    HWND(std::ptr::without_provenance_mut::<c_void>(raw as usize))
}

// ---- 起動 ----

pub(super) fn run(options: NativeOptions) -> Result<(), String> {
    ensure_dpi_awareness();
    // 読み上げ用の名前（MSAA の属性サービス）に COM が要る。失敗しても設定画面は動く
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    if com {
        init_accessibility();
    }
    let result = run_inner(options);
    if com {
        release_accessibility();
        unsafe { CoUninitialize() };
    }
    result
}

fn run_inner(options: NativeOptions) -> Result<(), String> {
    // 多重起動の抑止: 2 つ目は既存のウィンドウを前面に出して終わる
    let name = wide(MUTEX_NAME);
    let _mutex = match unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) } {
        Ok(handle) => {
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                activate_existing();
                return Ok(());
            }
            Some(handle)
        }
        // ミューテックスを作れなくても、設定画面は開く（多重起動の抑止だけ効かない）
        Err(_) => None,
    };
    let init = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES,
    };
    let _ = unsafe { InitCommonControlsEx(&init) };
    register_classes()?;

    let start_page = if options.open_update || options.auto_check {
        Page::Update
    } else {
        Page::Status
    };
    let state = Box::into_raw(Box::new(State::new(start_page)));

    let dpi = system_dpi();
    let (client_w, client_h) = initial_client_size(dpi);
    let style = WS_OVERLAPPEDWINDOW.0 | WS_CLIPCHILDREN.0;
    let ex_style = WS_EX_CONTROLPARENT.0;
    let (width, height) = outer_size(client_w, client_h, style, ex_style, dpi);
    let class = wide(MAIN_CLASS);
    let title = wide(TITLE);
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(style),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            None,
            None,
            Some(instance()),
            Some(state.cast_const().cast()),
        )
    };
    let Ok(hwnd) = created else {
        drop(unsafe { Box::from_raw(state) });
        return Err("設定ウィンドウを作れません".to_owned());
    };
    // SAFETY: `state` はこの関数の最後まで解放しない。ウィンドウプロシージャは共有参照でしか触らない
    let st = unsafe { &*state };

    // ワーカーを起動して接続を始める（ウィンドウは先に出す。imserver の起動は最大 60 秒かかる）
    let raw = hwnd.0 as isize;
    let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = unsafe {
            PostMessageW(
                Some(hwnd_from(raw)),
                messages::WORKER_REPLY,
                WPARAM(0),
                LPARAM(0),
            )
        };
    });
    let worker = WorkerHandle::start(options.factory, notify);
    worker.send(Command::Connect);
    *st.worker.borrow_mut() = Some(worker);

    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    let _ = unsafe { UpdateWindow(hwnd) };
    let _ = unsafe { SetFocus(Some(st.list.get())) };
    if options.auto_check {
        let _ = unsafe { PostMessageW(Some(hwnd), messages::AUTO_CHECK, WPARAM(0), LPARAM(0)) };
    }

    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
        if r <= 0 {
            break;
        }
        if unsafe { IsDialogMessageW(hwnd, &msg) }.as_bool() {
            continue;
        }
        let _ = unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
    }

    // 後始末: imserver を終了する（間に合わなければ放置。`--parent-pid` で親の終了とともに止まる）
    // SAFETY: ウィンドウは破棄済みで、プロシージャはもう呼ばれない
    let st = unsafe { Box::from_raw(state) };
    let worker = st.worker.borrow_mut().take();
    if let Some(mut worker) = worker {
        worker.shutdown(SHUTDOWN_WAIT);
    }
    if let Some(fonts) = st.fonts.take() {
        fonts.destroy();
    }
    Ok(())
}

fn activate_existing() {
    let class = wide(MAIN_CLASS);
    if let Ok(existing) = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) } {
        if unsafe { IsIconic(existing) }.as_bool() {
            let _ = unsafe { ShowWindow(existing, SW_RESTORE) };
        }
        let _ = unsafe { SetForegroundWindow(existing) };
    }
}

fn register_classes() -> Result<(), String> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let background = HBRUSH(std::ptr::without_provenance_mut::<c_void>(
        COLOR_WINDOW.0 as usize + 1,
    ));
    let main_name = wide(MAIN_CLASS);
    let main_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(main_proc),
        hInstance: instance(),
        hCursor: cursor,
        hbrBackground: background,
        lpszClassName: PCWSTR(main_name.as_ptr()),
        // タスクバー・Alt+Tab は大、タイトルバーは小（exe に埋め込んだ Nagi のロゴ）
        hIcon: app_icon(true, system_dpi()).unwrap_or_default(),
        hIconSm: app_icon(false, system_dpi()).unwrap_or_default(),
        ..Default::default()
    };
    let panel_name = wide(PANEL_CLASS);
    let panel_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(panel_proc),
        hInstance: instance(),
        hCursor: cursor,
        hbrBackground: background,
        lpszClassName: PCWSTR(panel_name.as_ptr()),
        ..Default::default()
    };
    for class in [&main_class, &panel_class] {
        if unsafe { RegisterClassExW(class) } == 0 {
            return Err("ウィンドウクラスを登録できません".to_owned());
        }
    }
    Ok(())
}

/// exe に埋め込んだ Nagi のアイコン（リソース ID 1。`build.rs` が `.ico` から埋め込む）を、
/// `big`（タスクバー・Alt+Tab）か小（タイトルバー）の大きさで読む。共有アイコンなので破棄しない。
/// 埋め込まれていなければ `None`（OS の既定のアイコンになる）
fn app_icon(big: bool, dpi: u32) -> Option<HICON> {
    let (cx, cy) = if big {
        (SM_CXICON, SM_CYICON)
    } else {
        (SM_CXSMICON, SM_CYSMICON)
    };
    let (width, height) = unsafe {
        (
            GetSystemMetricsForDpi(cx, dpi),
            GetSystemMetricsForDpi(cy, dpi),
        )
    };
    // MAKEINTRESOURCE(1)
    let id = PCWSTR(std::ptr::without_provenance::<u16>(1));
    let handle =
        unsafe { LoadImageW(Some(instance()), id, IMAGE_ICON, width, height, LR_SHARED) }.ok()?;
    Some(HICON(handle.0))
}

// ---- ウィンドウプロシージャ ----

unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        // SAFETY: WM_NCCREATE の lparam は有効な CREATESTRUCTW
        let param = unsafe { (*create).lpCreateParams } as isize;
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, param) };
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
    // SAFETY: `run` が作った State は、ウィンドウが破棄されたあとまで解放されない
    match unsafe { ptr.as_ref() } {
        Some(st) => handle_message(st, hwnd, msg, wparam, lparam),
        None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// パネルは背景とスクロールバーを持つだけ。通知はすべてメインウィンドウへ回す
unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND | WM_CTLCOLORSTATIC | WM_VSCROLL | WM_MOUSEWHEEL => {
            match unsafe { GetParent(hwnd) } {
                Ok(parent) => unsafe { SendMessageW(parent, msg, Some(wparam), Some(lparam)) },
                Err(_) => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn handle_message(st: &State, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            if on_create(st, hwnd) {
                LRESULT(0)
            } else {
                LRESULT(-1)
            }
        }
        WM_SIZE => {
            layout_main(st);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let dpi = match unsafe { GetDpiForWindow(hwnd) } {
                0 => 96,
                d => d,
            };
            let (cw, ch) = min_client_size(dpi);
            let (w, h) = outer_size(
                cw,
                ch,
                WS_OVERLAPPEDWINDOW.0 | WS_CLIPCHILDREN.0,
                WS_EX_CONTROLPARENT.0,
                dpi,
            );
            // SAFETY: WM_GETMINMAXINFO の lparam は書き込み可能な MINMAXINFO
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            info.ptMinTrackSize = POINT { x: w, y: h };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            on_dpi_changed(st, hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            apply_title_bar_theme(hwnd);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            on_command(st, id, code);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => on_ctl_color(st, wparam, lparam),
        WM_DRAWITEM => {
            // SAFETY: WM_DRAWITEM の lparam は有効な DRAWITEMSTRUCT
            let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            if item.CtlID == ids::LIST as u32 {
                draw_sidebar_item(st, item);
                LRESULT(1)
            } else {
                LRESULT(0)
            }
        }
        WM_VSCROLL => {
            on_vscroll(st, wparam);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            let (pos, _, _) = scroll_state(st);
            scroll_to(st, pos - delta * wheel_step(st.dpi.get()) / 120);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == ids::TIMER_STATUS {
                let _ = unsafe { KillTimer(Some(hwnd), ids::TIMER_STATUS) };
                set_text(st.status.get(), "");
            }
            LRESULT(0)
        }
        messages::WORKER_REPLY => {
            drain_replies(st);
            LRESULT(0)
        }
        messages::AUTO_CHECK => {
            start_update_check(st);
            LRESULT(0)
        }
        messages::REBUILD => {
            st.rebuild_posted.set(false);
            rebuild_page(st);
            LRESULT(0)
        }
        WM_CLOSE => {
            // すぐ消えたように見せる（imserver の終了待ちは、メッセージループを抜けたあと）
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = unsafe { KillTimer(Some(hwnd), ids::TIMER_STATUS) };
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ---- 作成・レイアウト ----

fn on_create(st: &State, hwnd: HWND) -> bool {
    st.hwnd.set(hwnd);
    let dpi = window_dpi(hwnd);
    st.dpi.set(dpi);
    let fonts = create_fonts(dpi);
    st.fonts.set(Some(fonts));
    let font = fonts.normal;

    // 作る順 = Tab 順: サイドバー → パネルの中 → 「詳細設定を表示」→ 「閉じる」
    let list = create_child(
        hwnd,
        "LISTBOX",
        "",
        LBS_NOTIFY as u32
            | LBS_NOINTEGRALHEIGHT as u32
            | LBS_OWNERDRAWFIXED as u32
            | LBS_HASSTRINGS as u32
            | WS_TABSTOP.0
            | WS_VSCROLL.0,
        0,
        ids::LIST,
        font,
    );
    let panel_class = wide(PANEL_CLASS);
    let panel = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            PCWSTR(panel_class.as_ptr()),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN,
            0,
            0,
            10,
            10,
            Some(hwnd),
            Some(HMENU(std::ptr::without_provenance_mut::<c_void>(
                ids::PANEL as usize,
            ))),
            Some(instance()),
            None,
        )
    };
    let detail = create_child(
        hwnd,
        "BUTTON",
        "詳細設定を表示",
        BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
        0,
        ids::DETAIL_CHECK,
        font,
    );
    let status = create_child(
        hwnd,
        "STATIC",
        "",
        SS_LEFT.0 | SS_NOPREFIX.0 | SS_CENTERIMAGE.0 | SS_ENDELLIPSIS.0,
        0,
        ids::STATUS,
        font,
    );
    let close = create_child(
        hwnd,
        "BUTTON",
        "閉じる",
        BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
        0,
        ids::CLOSE,
        font,
    );
    let sidebar_rule = create_child(
        hwnd,
        "STATIC",
        "",
        SS_ETCHEDVERT.0,
        0,
        ids::SIDEBAR_RULE,
        font,
    );
    let footer_rule = create_child(
        hwnd,
        "STATIC",
        "",
        SS_ETCHEDHORZ.0,
        0,
        ids::FOOTER_RULE,
        font,
    );
    let (
        Some(list),
        Some(detail),
        Some(status),
        Some(close),
        Some(sidebar_rule),
        Some(footer_rule),
        Ok(panel),
    ) = (
        list,
        detail,
        status,
        close,
        sidebar_rule,
        footer_rule,
        panel,
    )
    else {
        return false;
    };
    st.list.set(list);
    st.detail.set(detail);
    st.status.set(status);
    st.close.set(close);
    st.sidebar_rule.set(sidebar_rule);
    st.footer_rule.set(footer_rule);
    st.panel.set(panel);

    apply_title_bar_theme(hwnd);
    // WM_SETICON の種別: 0 = ICON_SMALL（タイトルバー）、1 = ICON_BIG（タスクバー・Alt+Tab）
    for (kind, big) in [(0usize, false), (1usize, true)] {
        if let Some(icon) = app_icon(big, dpi) {
            send(hwnd, WM_SETICON, kind, icon.0 as isize);
        }
    }
    send(list, LB_SETITEMHEIGHT, 0, sidebar_item_height(dpi) as isize);
    layout_main(st);
    rebuild_sidebar(st);
    true
}

/// メインウィンドウの部品を配置する（ページ内の再配置も行う）
fn layout_main(st: &State) {
    let hwnd = st.hwnd.get();
    if hwnd.0.is_null() || st.panel.get().0.is_null() {
        return;
    }
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    let frame = frame_layout(rect.right - rect.left, rect.bottom - rect.top, st.dpi.get());
    for (control, r) in [
        (st.list.get(), frame.list),
        (st.sidebar_rule.get(), frame.sidebar_rule),
        (st.panel.get(), frame.panel),
        (st.footer_rule.get(), frame.footer_rule),
        (st.detail.get(), frame.detail_check),
        (st.status.get(), frame.status),
        (st.close.get(), frame.close),
    ] {
        let _ = unsafe { MoveWindow(control, r.x, r.y, r.w, r.h, true) };
    }
    relayout(st);
}

fn on_dpi_changed(st: &State, hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let dpi = (wparam.0 & 0xFFFF) as u32;
    st.dpi.set(dpi.max(96));
    let old = st.fonts.replace(Some(create_fonts(st.dpi.get())));
    if let Some(fonts) = st.fonts.get() {
        for control in [
            st.list.get(),
            st.detail.get(),
            st.status.get(),
            st.close.get(),
        ] {
            set_font(control, fonts.normal);
        }
    }
    refit_fonts(st);
    send(
        st.list.get(),
        LB_SETITEMHEIGHT,
        0,
        sidebar_item_height(st.dpi.get()) as isize,
    );
    if let Some(old) = old {
        old.destroy();
    }
    // SAFETY: WM_DPICHANGED の lparam は推奨のウィンドウ矩形
    let suggested = unsafe { &*(lparam.0 as *const RECT) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            suggested.left,
            suggested.top,
            suggested.right - suggested.left,
            suggested.bottom - suggested.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    layout_main(st);
}

/// 左の一覧を作り直し、続けてページを作り直す
fn rebuild_sidebar(st: &State) {
    let (titles, selected) = {
        let mut model = st.model.borrow_mut();
        let entries = sidebar(model.view.as_ref(), model.detail, &pages::sidebar_entries());
        if !entries.iter().any(|(p, _)| *p == model.page)
            && let Some((first, _)) = entries.first()
        {
            model.page = *first;
            st.ui.borrow_mut().scroll = 0;
        }
        model.sidebar = entries.iter().map(|(p, _)| *p).collect();
        let selected = model.sidebar.iter().position(|p| *p == model.page);
        (
            entries.into_iter().map(|(_, t)| t).collect::<Vec<_>>(),
            selected,
        )
    };
    let list = st.list.get();
    send(list, LB_RESETCONTENT, 0, 0);
    for title in &titles {
        send_text(list, LB_ADDSTRING, 0, title);
    }
    if let Some(index) = selected {
        send(list, LB_SETCURSEL, index, 0);
    }
    rebuild_page(st);
}

fn request_rebuild(st: &State) {
    if !st.rebuild_posted.replace(true) {
        let _ =
            unsafe { PostMessageW(Some(st.hwnd.get()), messages::REBUILD, WPARAM(0), LPARAM(0)) };
    }
}

// ---- 描画の色 ----

fn on_ctl_color(st: &State, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hdc = HDC(std::ptr::without_provenance_mut::<c_void>(wparam.0));
    let control = hwnd_from(lparam.0);
    let role = if control == st.status.get() {
        st.status_is_error.get().then_some(Role::Error)
    } else {
        role_of(st, control)
    };
    let color = match role {
        Some(Role::Desc | Role::Hint) => unsafe { GetSysColor(COLOR_GRAYTEXT) },
        // 赤（COLORREF は 0x00BBGGRR）。ハイコントラストではシステムの文字色（赤は背景次第で読めない）
        Some(Role::Error) if !is_high_contrast() => 0x0000_00C0,
        _ => unsafe { GetSysColor(COLOR_WINDOWTEXT) },
    };
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(color));
        LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
    }
}

// ---- サイドバー（オーナードロー） ----

/// サイドバーの 1 項目を描く。色はすべてシステム色（ライト・ダーク・ハイコントラストに追従）。
/// 選択は `COLOR_HIGHLIGHT` の角の立った帯、文字は左に余白を取って縦中央
fn draw_sidebar_item(st: &State, item: &DRAWITEMSTRUCT) {
    if item.itemID == u32::MAX {
        return; // 項目が 0 件のときのフォーカス表示
    }
    let hdc = item.hDC;
    let dpi = st.dpi.get();
    let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
    let (fill, text_x) = sidebar_item_parts(
        Rect {
            x: item.rcItem.left,
            y: item.rcItem.top,
            w: item.rcItem.right - item.rcItem.left,
            h: item.rcItem.bottom - item.rcItem.top,
        },
        dpi,
    );
    let fill_rect = RECT {
        left: fill.x,
        top: fill.y,
        right: fill.x + fill.w,
        bottom: fill.y + fill.h,
    };
    unsafe {
        FillRect(hdc, &item.rcItem, GetSysColorBrush(COLOR_WINDOW));
        if selected {
            FillRect(hdc, &fill_rect, GetSysColorBrush(COLOR_HIGHLIGHT));
        }
    }
    let list = st.list.get();
    let len = send(list, LB_GETTEXTLEN, item.itemID as usize, 0);
    if len > 0 {
        let mut buf = vec![0u16; len as usize + 1];
        let n = send(
            list,
            LB_GETTEXT,
            item.itemID as usize,
            buf.as_mut_ptr() as isize,
        );
        buf.truncate(n.max(0) as usize);
        let mut text_rect = RECT {
            left: text_x,
            top: fill.y,
            right: fill_rect.right,
            bottom: fill_rect.bottom,
        };
        unsafe {
            let old = st
                .fonts
                .get()
                .map(|f| SelectObject(hdc, HGDIOBJ(f.normal.0)));
            SetBkMode(hdc, TRANSPARENT);
            let color = if selected {
                GetSysColor(COLOR_HIGHLIGHTTEXT)
            } else {
                GetSysColor(COLOR_WINDOWTEXT)
            };
            SetTextColor(hdc, COLORREF(color));
            DrawTextW(
                hdc,
                &mut buf,
                &mut text_rect,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            if let Some(old) = old {
                SelectObject(hdc, old);
            }
        }
    }
    // キーボードでたどっているときの目印（選択の帯の内側に細い枠）
    if item.itemState.0 & ODS_FOCUS.0 != 0 && item.itemState.0 & ODS_NOFOCUSRECT.0 == 0 {
        let _ = unsafe { DrawFocusRect(hdc, &fill_rect) };
    }
}

// ---- スクロール ----

fn on_vscroll(st: &State, wparam: WPARAM) {
    let code = (wparam.0 & 0xFFFF) as i32;
    let (pos, max, page) = scroll_state(st);
    let line = control_height(st.dpi.get());
    let target = match code {
        SB_LINEUP => pos - line,
        SB_LINEDOWN => pos + line,
        SB_PAGEUP => pos - page * 9 / 10,
        SB_PAGEDOWN => pos + page * 9 / 10,
        SB_TOP => 0,
        SB_BOTTOM => max,
        SB_THUMBTRACK | SB_THUMBPOSITION => {
            let mut info = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_TRACKPOS,
                ..Default::default()
            };
            let _ = unsafe { GetScrollInfo(st.panel.get(), SB_VERT, &mut info) };
            info.nTrackPos
        }
        _ => pos,
    };
    scroll_to(st, target);
}

// ---- ステータス行 ----

fn set_status(st: &State, text: &str, is_error: bool) {
    st.status_is_error.set(is_error);
    let status = st.status.get();
    set_text(status, text);
    // 色（エラーは赤）を反映する
    let _ = unsafe { windows::Win32::Graphics::Gdi::InvalidateRect(Some(status), None, true) };
    unsafe { SetTimer(Some(st.hwnd.get()), ids::TIMER_STATUS, STATUS_MS, None) };
}

// ---- コマンド ----

fn on_command(st: &State, id: i32, code: u32) {
    // Tab で画面外のコントロールにフォーカスが移ったら見える位置までスクロールする
    if let Some((role, _, _)) = find_ctl(st, id) {
        let focused = match role {
            Role::Edit => code == EN_SETFOCUS,
            Role::Combo => code == CBN_SETFOCUS,
            Role::Check | Role::Reset | Role::Button => code == BN_SETFOCUS,
            _ => false,
        };
        if focused {
            ensure_visible(st, id);
            return;
        }
    }
    match id {
        IDOK => commit_focused_edit(st),
        IDCANCEL => close_window(st),
        ids::CLOSE if code == BN_CLICKED => close_window(st),
        ids::LIST if code == LBN_SELCHANGE => select_page(st),
        ids::DETAIL_CHECK if code == BN_CLICKED => {
            let checked = send(st.detail.get(), BM_GETCHECK, 0, 0) == BST_CHECKED;
            {
                let mut model = st.model.borrow_mut();
                model.detail = checked;
                model.errors.clear();
            }
            rebuild_sidebar(st);
        }
        ids::UPDATE_CHECK if code == BN_CLICKED => start_update_check(st),
        ids::UPDATE_APPLY if code == BN_CLICKED => {
            st.model.borrow_mut().update.busy = true;
            send_command(st, Command::ApplyUpdate);
            request_rebuild(st);
        }
        ids::UPDATE_NOTES if code == BN_CLICKED => open_notes(st),
        ids::OPEN_CONFIG | ids::OPEN_CONFIG_DIR | ids::OPEN_USER_DICT if code == BN_CLICKED => {
            open_data(st, id)
        }
        ids::CLEAR_LEARNING if code == BN_CLICKED => confirm_clear_learning(st),
        ids::RETRY if code == BN_CLICKED => {
            {
                let mut model = st.model.borrow_mut();
                model.phase = Phase::Loading;
                model.view = None;
            }
            send_command(st, Command::Connect);
            rebuild_sidebar(st);
        }
        id if id >= ids::DYNAMIC_BASE => on_field_command(st, id, code),
        _ => {}
    }
}

fn close_window(st: &State) {
    let _ = unsafe { PostMessageW(Some(st.hwnd.get()), WM_CLOSE, WPARAM(0), LPARAM(0)) };
}

fn send_command(st: &State, command: Command) {
    if let Some(worker) = st.worker.borrow().as_ref() {
        worker.send(command);
    }
}

fn select_page(st: &State) {
    let index = send(st.list.get(), LB_GETCURSEL, 0, 0);
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    {
        let mut model = st.model.borrow_mut();
        let Some(page) = model.sidebar.get(index).copied() else {
            return;
        };
        model.page = page;
        model.errors.clear();
        model.wanted_focus = None;
    }
    st.ui.borrow_mut().scroll = 0;
    rebuild_page(st);
}

/// 設定項目のコントロールの通知
fn on_field_command(st: &State, id: i32, code: u32) {
    let Some((role, Some(key), hwnd)) = find_ctl(st, id) else {
        return;
    };
    match role {
        Role::Check if code == BN_CLICKED => {
            let checked = send(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED;
            let current = field_of(st, &key).is_some_and(|f| f.bool_value());
            if checked != current {
                send_update(st, key, Some(SettingValue::Bool(checked)));
            }
        }
        Role::Reset if code == BN_CLICKED => send_update(st, key, None),
        Role::Edit if code == EN_KILLFOCUS => commit_edit(st, &key, hwnd),
        Role::Combo => {
            // 閉じたまま矢印キーで変えたとき（SELCHANGE）と、一覧から選んだとき（SELENDOK）だけ書く。
            // 一覧を開いて動かしている間の SELCHANGE は無視する
            let dropped = send(hwnd, CB_GETDROPPEDSTATE, 0, 0) != 0;
            if code == CBN_SELENDOK || (code == CBN_SELCHANGE && !dropped) {
                commit_combo(st, &key, hwnd);
            }
        }
        _ => {}
    }
}

fn field_of(st: &State, key: &str) -> Option<crate::native_model::FieldView> {
    st.model
        .borrow()
        .view
        .as_ref()
        .and_then(|v| v.find(key).map(|(_, _, f)| f.clone()))
}

fn send_update(st: &State, key: String, value: Option<SettingValue>) {
    set_status(st, "保存しています…", false);
    send_command(st, Command::Update { key, value });
}

fn commit_edit(st: &State, key: &str, hwnd: HWND) {
    let Some(field) = field_of(st, key) else {
        return;
    };
    let text = edit_text(hwnd);
    if text == field.display_text() {
        return;
    }
    match field.parse_input(&text) {
        Ok(value) if value == field.value => {
            // 全角などを正規化した結果が現在値と同じ: 表示だけ整える
            st.model.borrow_mut().errors.remove(key);
            request_rebuild(st);
        }
        Ok(value) => send_update(st, key.to_owned(), value),
        Err(message) => {
            // 値は元に戻し（作り直しで現在値が入る）、その項目の下に理由を出す
            st.model.borrow_mut().errors.insert(key.to_owned(), message);
            request_rebuild(st);
        }
    }
}

fn commit_focused_edit(st: &State) {
    let focus = unsafe { GetFocus() };
    let target = {
        let ui = st.ui.borrow();
        ui.ctls
            .iter()
            .find(|c| c.hwnd == focus && c.role == Role::Edit)
            .and_then(|c| c.key.clone().map(|k| (k, c.hwnd)))
    };
    if let Some((key, hwnd)) = target {
        commit_edit(st, &key, hwnd);
    }
}

fn commit_combo(st: &State, key: &str, hwnd: HWND) {
    let Some(field) = field_of(st, key) else {
        return;
    };
    let Ok(index) = usize::try_from(send(hwnd, CB_GETCURSEL, 0, 0)) else {
        return;
    };
    if field.choice_index() == Some(index) {
        return;
    }
    if let Some(value) = field.choice_value_at(index) {
        send_update(st, key.to_owned(), value);
    }
}

// ---- 更新・データのページ ----

fn start_update_check(st: &State) {
    {
        let mut model = st.model.borrow_mut();
        if model.update.busy {
            return;
        }
        model.update = UpdateUi {
            busy: true,
            text: String::new(),
            can_apply: false,
            notes_url: None,
        };
    }
    send_command(st, Command::CheckUpdate);
    request_rebuild(st);
}

fn open_notes(st: &State) {
    let url = st.model.borrow().update.notes_url.clone();
    // ワーカーでも絞っているが、外部に渡す直前にもう一度確認する
    let Some(url) = url.filter(|u| is_safe_notes_url(u)) else {
        return;
    };
    let target = wide(&url);
    let result = unsafe {
        ShellExecuteW(
            Some(st.hwnd.get()),
            w!("open"),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result.0 as usize) <= 32 {
        set_status(st, "エラー: ブラウザを開けませんでした", true);
    }
}

fn open_data(st: &State, id: i32) {
    let config = st
        .model
        .borrow()
        .view
        .as_ref()
        .and_then(|v| v.config_path.clone())
        .map(PathBuf::from);
    let Some(config) = config else {
        set_status(st, "エラー: 設定ファイルの場所が決まっていません", true);
        return;
    };
    let opener = SystemOpener;
    let result = match id {
        ids::OPEN_CONFIG => opener.open_file_in_editor(&config),
        ids::OPEN_CONFIG_DIR => match config.parent() {
            Some(dir) => opener.open_folder(dir),
            None => Err("設定フォルダの場所が決まっていません".to_owned()),
        },
        _ => match user_dict_dir(&config, |p| p.is_dir()) {
            Some(dir) => opener.open_folder(&dir),
            None => Err("ユーザー辞書のフォルダがまだありません".to_owned()),
        },
    };
    if let Err(message) = result {
        set_status(st, &format!("エラー: {message}"), true);
    }
}

fn confirm_clear_learning(st: &State) {
    let text = wide("学習データをすべて削除します。この操作は元に戻せません。\n削除しますか？");
    let caption = wide(TITLE);
    let answer = unsafe {
        MessageBoxW(
            Some(st.hwnd.get()),
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };
    if answer == IDYES {
        set_status(st, "削除しています…", false);
        send_command(st, Command::ClearLearning);
    }
}

// ---- ワーカーの返信 ----

fn drain_replies(st: &State) {
    loop {
        let reply = st.worker.borrow().as_ref().and_then(WorkerHandle::try_recv);
        let Some(reply) = reply else { break };
        on_reply(st, reply);
    }
}

fn on_reply(st: &State, reply: Reply) {
    match reply {
        Reply::Loaded(Ok(loaded)) => {
            let view = SettingsView::from_snapshot(&loaded.snapshot, loaded.config_path);
            let broken = view.error.clone();
            {
                let mut model = st.model.borrow_mut();
                model.phase = Phase::Ready;
                model.view = Some(view);
                model.errors.clear();
            }
            if let Some(error) = broken {
                set_status(st, &format!("エラー: {error}"), true);
            }
            rebuild_sidebar(st);
        }
        Reply::Loaded(Err(error)) => {
            {
                let mut model = st.model.borrow_mut();
                model.phase = Phase::Failed(error.message.clone());
                model.view = None;
            }
            set_status(st, &format!("エラー: {}", error.message), true);
            rebuild_sidebar(st);
        }
        Reply::Updated { key, result } => {
            match result {
                Ok(result) => {
                    let mut message = None;
                    {
                        let mut model = st.model.borrow_mut();
                        model.errors.remove(&key);
                        for e in &result.errors {
                            model.errors.insert(e.key.clone(), e.message.clone());
                            message.get_or_insert_with(|| e.message.clone());
                        }
                        let config_path = model.view.as_ref().and_then(|v| v.config_path.clone());
                        model.view =
                            Some(SettingsView::from_snapshot(&result.snapshot, config_path));
                    }
                    match message {
                        Some(m) => set_status(st, &format!("エラー: {m}"), true),
                        None => set_status(st, "保存しました", false),
                    }
                }
                Err(error) => {
                    st.model
                        .borrow_mut()
                        .errors
                        .insert(key, error.message.clone());
                    set_status(st, &format!("エラー: {}", error.message), true);
                }
            }
            rebuild_page(st);
        }
        Reply::LearningCleared(Ok(())) => set_status(st, "学習データを削除しました", false),
        Reply::LearningCleared(Err(error)) => {
            set_status(st, &format!("エラー: {}", error.message), true);
        }
        Reply::UpdateChecked(result) => {
            {
                let mut model = st.model.borrow_mut();
                model.update = match result {
                    Ok(check) => {
                        let outcome = describe_update(&check);
                        UpdateUi {
                            busy: false,
                            text: outcome.text,
                            can_apply: outcome.can_apply,
                            notes_url: outcome.notes_url,
                        }
                    }
                    Err(error) => UpdateUi {
                        busy: false,
                        text: format!("更新を確認できませんでした: {}", error.message),
                        can_apply: false,
                        notes_url: None,
                    },
                };
            }
            rebuild_page(st);
        }
        Reply::UpdateApplied(result) => {
            {
                let mut model = st.model.borrow_mut();
                model.update.busy = false;
                model.update.text = match result {
                    Ok(()) => "更新の適用を開始しました。新しく開いたコンソールの案内に従ってください（管理者の確認が出ます）。".to_owned(),
                    Err(error) => format!("更新を適用できませんでした: {}", error.message),
                };
            }
            rebuild_page(st);
        }
    }
}
