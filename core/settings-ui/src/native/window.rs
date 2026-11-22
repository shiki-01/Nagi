//! メインウィンドウ: クラス登録・メッセージループ・各メッセージの処理。

use super::NativeOptions;
use super::controls::{
    create_child, init_accessibility, instance, release_accessibility, send, send_text,
    set_dark_theme, set_font, set_text, wide,
};
use super::dpi::{
    apply_title_bar_theme, create_fonts, ensure_dpi_awareness, monitor_dpi_at, outer_size,
    system_dpi, window_dpi, work_areas,
};
use super::gfx;
use super::page::{
    edit_text, ensure_visible, find_ctl, rebuild_page, refit_fonts, relayout, role_of,
    scroll_state, scroll_tick, scroll_to, scroll_wheel,
};
use super::pages;
use super::state::{Ident, Phase, Role, State, UpdateUi};
use super::theme::{self, Surface};
use super::widgets::{self, BtnKind, NavItem};
use crate::http::{is_safe_notes_url, user_dict_dir};
use crate::native_diag;
use crate::native_layout::{
    Rect, control_height, frame_layout, initial_client_size, min_client_size, scale,
    sidebar_item_height, unscale, wheel_step,
};
use crate::native_model::{Page, SettingsView, describe_update, ids, messages, sidebar};
use crate::native_theme::{ButtonStyle, page_glyph, status_kind};
use crate::native_ui_state::{self, UiState, WindowState};
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
    HBRUSH, HDC, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RedrawWindow, SetBkColor, SetBkMode,
    SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::SystemServices::{SS_NOPREFIX, SS_OWNERDRAW};
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
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, BeginDeferWindowPos, CB_GETCURSEL,
    CB_GETDROPPEDSTATE, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW,
    DefWindowProcW, DeferWindowPos, DestroyWindow, DispatchMessageW, EndDeferWindowPos,
    FindWindowW, GWLP_USERDATA, GetClientRect, GetMessageW, GetParent, GetScrollInfo,
    GetWindowLongPtrW, GetWindowRect, HICON, HMENU, IDC_ARROW, IDYES, IsDialogMessageW, IsIconic,
    IsZoomed, KillTimer, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL,
    LB_SETITEMHEIGHT, LBS_HASSTRINGS, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LBS_OWNERDRAWFIXED,
    LoadCursorW, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MINMAXINFO, MSG, MessageBoxW, MoveWindow,
    PostMessageW, PostQuitMessage, RegisterClassExW, SB_VERT, SCROLLINFO, SIF_TRACKPOS, SW_HIDE,
    SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER,
    SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_DRAWITEM,
    WM_ERASEBKGND, WM_GETMINMAXINFO, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT, WM_SETICON,
    WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOLORCHANGE, WM_THEMECHANGED, WM_TIMER, WM_VSCROLL,
    WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
    WS_VISIBLE, WS_VSCROLL,
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
    // 色（ライト・ダーク・ハイコントラスト）の判定と、角丸・円を描く GDI+。GDI+ が使えなくても動く
    theme::init();
    gfx::startup();
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
    theme::destroy();
    gfx::shutdown();
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

    // 前回の UI の状態（詳細設定・ページ・ウィンドウの位置とサイズ）。読めなければ既定
    let saved = options
        .ui_state_path
        .as_deref()
        .map(native_ui_state::load)
        .unwrap_or_default();
    let (start_page, wanted_page) = if options.open_update || options.auto_check {
        (Page::Update, None)
    } else {
        initial_page(saved.page.as_deref())
    };
    let state = Box::into_raw(Box::new(State::new(
        start_page,
        saved.clone(),
        wanted_page,
        options.ui_state_path.clone(),
    )));

    let dpi = system_dpi();
    let (client_w, client_h) = initial_client_size(dpi);
    let style = WS_OVERLAPPEDWINDOW.0 | WS_CLIPCHILDREN.0;
    let ex_style = WS_EX_CONTROLPARENT.0;
    let (width, height) = outer_size(client_w, client_h, style, ex_style, dpi);
    // 前回の位置とサイズ（モニターの作業領域に収める）。無ければ OS 任せの位置と既定のサイズ
    let (x, y, width, height, maximized) = match saved.window {
        Some(w) => {
            let r = restore_rect(&w, style, ex_style);
            (r.x, r.y, r.w, r.h, w.maximized)
        }
        None => (CW_USEDEFAULT, CW_USEDEFAULT, width, height, false),
    };
    let class = wide(MAIN_CLASS);
    let title = wide(TITLE);
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(style),
            x,
            y,
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
    let worker = WorkerHandle::start(options.factory, options.diag, notify);
    worker.send(Command::Connect);
    *st.worker.borrow_mut() = Some(worker);

    let _ = unsafe { ShowWindow(hwnd, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOW }) };
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
    // 実行中の診断があれば止める（ワーカーが次の項目へ進まず、終了の要求にすぐ応えられるように）
    st.model.borrow().diag.cancel_for_close();
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

/// 保存したページから、最初に開くページを決める。固定のページ（更新・データ・動作確認）はそのまま、
/// カテゴリは設定を読み込めてから戻す（それまでは「設定」の読み込み表示）
fn initial_page(key: Option<&str>) -> (Page, Option<String>) {
    let Some(key) = key else {
        return (Page::Status, None);
    };
    let fixed: Vec<(Page, String)> = pages::sidebar_entries()
        .into_iter()
        .map(|(p, t)| (p, t.to_owned()))
        .collect();
    if let Some(page) = native_ui_state::resolve_page(key, &fixed) {
        (page, None)
    } else if key.starts_with("category:") {
        (Page::Status, Some(key.to_owned()))
    } else {
        (Page::Status, None)
    }
}

/// 保存したウィンドウの位置とサイズを、いまのモニター構成に収めた外側の矩形（px）にする
fn restore_rect(w: &WindowState, style: u32, ex_style: u32) -> Rect {
    // 保存した位置にあるモニターの DPI で、dp のサイズを px にする（別の DPI のモニターに寄せられたら
    // WM_DPICHANGED で作り直される）
    let dpi = monitor_dpi_at(w.x.saturating_add(48), w.y.saturating_add(48));
    let (ow, oh) = outer_size(scale(w.w_dp, dpi), scale(w.h_dp, dpi), style, ex_style, dpi);
    let (min_cw, min_ch) = min_client_size(dpi);
    let (min_w, min_h) = outer_size(min_cw, min_ch, style, ex_style, dpi);
    native_ui_state::clamp_to_monitors(
        Rect {
            x: w.x,
            y: w.y,
            w: ow,
            h: oh,
        },
        &work_areas(),
        min_w,
        min_h,
    )
}

fn register_classes() -> Result<(), String> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    // 背景は WM_ERASEBKGND / WM_PAINT で自前で塗る（ダークモード・カード）
    let background = HBRUSH::default();
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

/// パネルは背景（ページの地とカード）とスクロールバーを持つだけ。通知・色の問い合わせはすべてメインウィンドウへ回す
unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create = lparam.0 as *const CREATESTRUCTW;
            // SAFETY: WM_NCCREATE の lparam は有効な CREATESTRUCTW。lpCreateParams は `on_create` が渡した State
            let param = unsafe { (*create).lpCreateParams } as isize;
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, param) };
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
            // SAFETY: State はメインウィンドウが破棄されたあとまで解放されない
            match unsafe { ptr.as_ref() } {
                Some(st) => {
                    widgets::paint_panel(st, hwnd);
                    LRESULT(0)
                }
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_COMMAND | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_DRAWITEM
        | WM_VSCROLL | WM_MOUSEWHEEL => match unsafe { GetParent(hwnd) } {
            Ok(parent) => unsafe { SendMessageW(parent, msg, Some(wparam), Some(lparam)) },
            Err(_) => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        },
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
        WM_ERASEBKGND => {
            paint_main_background(
                st,
                HDC(std::ptr::without_provenance_mut::<c_void>(wparam.0)),
            );
            LRESULT(1)
        }
        WM_SETTINGCHANGE => {
            // ダーク / ライトの切替・ハイコントラストの切替に追従する（再起動は要らない）
            on_theme_changed(st, false);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SYSCOLORCHANGE | WM_THEMECHANGED => {
            on_theme_changed(st, true);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            on_command(st, id, code);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => on_ctl_color(st, wparam, lparam),
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => on_field_color(wparam),
        WM_DRAWITEM => {
            // SAFETY: WM_DRAWITEM の lparam は有効な DRAWITEMSTRUCT
            let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            if draw_item(st, item) {
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
            scroll_wheel(st, delta, wheel_step(st.dpi.get()));
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == ids::TIMER_SCROLL {
                scroll_tick(st);
            }
            if wparam.0 == ids::TIMER_STATUS {
                let _ = unsafe { KillTimer(Some(hwnd), ids::TIMER_STATUS) };
                set_text(st.status.get(), "");
                let _ = unsafe {
                    windows::Win32::Graphics::Gdi::InvalidateRect(Some(st.status.get()), None, true)
                };
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
            // 詳細設定・ページ・ウィンドウの位置とサイズを保存する（次回の起動で復元）
            persist_ui_state(st, true);
            // すぐ消えたように見せる（imserver の終了待ちは、メッセージループを抜けたあと）
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = unsafe { KillTimer(Some(hwnd), ids::TIMER_STATUS) };
            let _ = unsafe { KillTimer(Some(hwnd), ids::TIMER_SCROLL) };
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
            Some((st as *const State).cast::<c_void>()),
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
    // ステータス・区切り線はオーナードロー（種類で色分けした札・テーマの色の線）。文字は読み上げ用に持つ
    let status = create_child(
        hwnd,
        "STATIC",
        "",
        SS_OWNERDRAW.0 | SS_NOPREFIX.0,
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
        SS_OWNERDRAW.0,
        0,
        ids::SIDEBAR_RULE,
        font,
    );
    let footer_rule = create_child(
        hwnd,
        "STATIC",
        "",
        SS_OWNERDRAW.0,
        0,
        ids::FOOTER_RULE,
        font,
    );
    // ブランド（ロゴ + ワードマーク）。文字は読み上げ用に持つ。Tab の対象にはならない
    let brand = create_child(
        hwnd,
        "STATIC",
        &brand_caption(),
        SS_OWNERDRAW.0 | SS_NOPREFIX.0,
        0,
        ids::BRAND,
        font,
    );
    let (
        Some(list),
        Some(detail),
        Some(status),
        Some(close),
        Some(sidebar_rule),
        Some(footer_rule),
        Some(brand),
        Ok(panel),
    ) = (
        list,
        detail,
        status,
        close,
        sidebar_rule,
        footer_rule,
        brand,
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
    st.brand.set(brand);
    st.panel.set(panel);

    // 標準のコントロールのまま、見た目だけ自前で描く（役割・名前・キー操作は標準のまま）
    widgets::install_list(list);
    for owner_draw in [status, sidebar_rule, footer_rule, brand] {
        widgets::install_static(owner_draw, true);
    }
    widgets::install_button(detail, BtnKind::Toggle, false);
    widgets::install_button(close, BtnKind::Push(ButtonStyle::Normal), false);
    if st.model.borrow().detail {
        send(detail, BM_SETCHECK, 1, 0);
    }
    apply_chrome_theme(st);

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
    let moves = [
        (st.brand.get(), frame.brand),
        (st.list.get(), frame.list),
        (st.sidebar_rule.get(), frame.sidebar_rule),
        (st.panel.get(), frame.panel),
        (st.footer_rule.get(), frame.footer_rule),
        (st.detail.get(), frame.detail_check),
        (st.status.get(), frame.status),
        (st.close.get(), frame.close),
    ];
    // 1 つずつ動かして 1 つずつ再描画すると、途中の状態が見えてちらつく。まとめて動かして一度に描き直す
    unsafe {
        let mut deferred = BeginDeferWindowPos(moves.len() as i32).ok();
        for (control, r) in moves {
            deferred = deferred.and_then(|h| {
                DeferWindowPos(
                    h,
                    control,
                    None,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
                .ok()
            });
        }
        if let Some(h) = deferred {
            let _ = EndDeferWindowPos(h);
        } else {
            // まとめて動かせなかったら、1 つずつ動かす
            for (control, r) in moves {
                let _ = MoveWindow(control, r.x, r.y, r.w, r.h, true);
            }
        }
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
    redraw_all(st);
}

/// 左の一覧を作り直し、続けてページを作り直す
fn rebuild_sidebar(st: &State) {
    let (titles, selected) = {
        let mut model = st.model.borrow_mut();
        let entries = sidebar(model.view.as_ref(), model.detail, &pages::sidebar_entries());
        // 前回のカテゴリのページは、設定を読み込めてから戻す（無くなっていれば先頭のページ）
        if model.view.is_some()
            && let Some(key) = model.wanted_page.take()
            && let Some(page) = native_ui_state::resolve_page(&key, &entries)
        {
            model.page = page;
            st.ui.borrow_mut().scroll = 0;
        }
        if !entries.iter().any(|(p, _)| *p == model.page)
            && let Some((first, _)) = entries.first()
        {
            model.page = *first;
            st.ui.borrow_mut().scroll = 0;
        }
        model.sidebar = entries.iter().map(|(p, _)| *p).collect();
        model.sidebar_titles = entries.iter().map(|(_, t)| t.clone()).collect();
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

// ---- 描画の色・テーマ ----

/// 静的な文字（STATIC）の色と、背景のブラシ。見出し・節の見出しはカードの外（ページの地）、
/// それ以外はカードの上。説明・案内は補助の色、エラーはエラーの色（ハイコントラストはどれもシステム色）
fn on_ctl_color(st: &State, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hdc = HDC(std::ptr::without_provenance_mut::<c_void>(wparam.0));
    let control = hwnd_from(lparam.0);
    let pal = theme::palette();
    let (color, surface) = match role_of(st, control) {
        Some(Role::Heading | Role::Section) => (pal.text, Surface::Page),
        Some(Role::Subtitle) => (pal.text_secondary, Surface::Page),
        Some(Role::Desc | Role::Hint) => (pal.text_secondary, Surface::Card),
        Some(Role::Error) => (pal.error_text, Surface::Card),
        _ => (pal.text, Surface::Card),
    };
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(theme::rgb(color).colorref()));
    }
    LRESULT(theme::brush(surface).0 as isize)
}

/// 入力欄・選択欄（EDIT / COMBOBOX の一覧）の文字と背景
fn on_field_color(wparam: WPARAM) -> LRESULT {
    let hdc = HDC(std::ptr::without_provenance_mut::<c_void>(wparam.0));
    let pal = theme::palette();
    unsafe {
        SetTextColor(hdc, COLORREF(theme::rgb(pal.text).colorref()));
        SetBkColor(hdc, COLORREF(theme::rgb(pal.field_bg).colorref()));
    }
    LRESULT(theme::brush(Surface::Field).0 as isize)
}

/// メインウィンドウの背景: ページの地、サイドバーの列、フッター
fn paint_main_background(st: &State, hdc: HDC) {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(st.hwnd.get(), &mut rect) };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    let frame = frame_layout(w, h, st.dpi.get());
    let page = theme::surface_rgb(Surface::Page);
    let side = theme::surface_rgb(Surface::Sidebar);
    let all = Rect { x: 0, y: 0, w, h };
    let column = Rect {
        x: 0,
        y: 0,
        w: frame.list.w,
        h: frame.footer_rule.y,
    };
    let footer = Rect {
        x: 0,
        y: frame.footer_rule.y,
        w,
        h: (h - frame.footer_rule.y).max(0),
    };
    gfx::fill_rect(hdc, all, page);
    gfx::fill_rect(hdc, column, side);
    gfx::fill_rect(hdc, footer, side);
}

/// 一覧・パネルのスクロールバーなど、枠の部品のテーマ（ダークなら暗いスクロールバー）
fn apply_chrome_theme(st: &State) {
    let dark = theme::palette().kind == crate::native_theme::ThemeKind::Dark;
    set_dark_theme(st.list.get(), dark, true);
    set_dark_theme(st.panel.get(), dark, true);
}

fn redraw_all(st: &State) {
    let _ = unsafe {
        RedrawWindow(
            Some(st.hwnd.get()),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
        )
    };
}

/// テーマ（ライト・ダーク・ハイコントラスト）やシステム色が変わった。`force` は色の変更を必ず反映する
fn on_theme_changed(st: &State, force: bool) {
    if st.panel.get().0.is_null() {
        return;
    }
    let changed = theme::refresh();
    if !changed && !force {
        return;
    }
    apply_title_bar_theme(st.hwnd.get());
    apply_chrome_theme(st);
    // 入力欄・選択欄のテーマもあるので、ページを作り直す（スクロール位置・フォーカスは保つ）
    rebuild_page(st);
    redraw_all(st);
}

fn rect_of(r: &RECT) -> Rect {
    Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

/// オーナードローの部品を描く。描いたら `true`
fn draw_item(st: &State, item: &DRAWITEMSTRUCT) -> bool {
    let hdc = item.hDC;
    let area = rect_of(&item.rcItem);
    let dpi = st.dpi.get();
    let id = item.CtlID as i32;
    match id {
        ids::LIST => {
            draw_sidebar_item(st, item);
            true
        }
        ids::SIDEBAR_RULE | ids::FOOTER_RULE => {
            widgets::draw_rule(hdc, area);
            true
        }
        ids::BRAND => {
            let Some(fonts) = st.fonts.get() else {
                return false;
            };
            widgets::draw_brand(hdc, area, dpi, &fonts, &brand_caption());
            true
        }
        ids::STATUS => {
            let Some(fonts) = st.fonts.get() else {
                return false;
            };
            let text = widget_text(st.status.get());
            let kind = status_kind(&text, st.status_is_error.get());
            widgets::draw_status(hdc, area, dpi, &fonts, kind, &text);
            true
        }
        _ => {
            // 診断の結果行
            let info = st.ui.try_borrow().ok().and_then(|ui| {
                ui.ctls
                    .iter()
                    .find(|c| c.id == id)
                    .and_then(|c| c.result.clone())
            });
            let (Some(info), Some(fonts)) = (info, st.fonts.get()) else {
                return false;
            };
            widgets::draw_result(
                hdc,
                area,
                dpi,
                Some(fonts.normal),
                info.badge,
                &info.label,
                &info.title,
            );
            true
        }
    }
}

/// ブランド領域の補足の文字（設定 + 版）
fn brand_caption() -> String {
    format!("設定  v{}", env!("CARGO_PKG_VERSION"))
}

fn widget_text(hwnd: HWND) -> String {
    super::controls::get_text(hwnd)
}

/// サイドバーの 1 項目（アイコン + 表示名。選択は丸みのある帯とアクセントの印）
fn draw_sidebar_item(st: &State, item: &DRAWITEMSTRUCT) {
    if item.itemID == u32::MAX {
        return; // 項目が 0 件のときのフォーカス表示
    }
    let Some(fonts) = st.fonts.get() else {
        return;
    };
    let index = item.itemID as usize;
    let (title, glyph) = {
        let model = st.model.borrow();
        let title = model.sidebar_titles.get(index).cloned().unwrap_or_default();
        let key = model
            .sidebar
            .get(index)
            .and_then(|p| native_ui_state::page_key(*p, &title))
            .unwrap_or_default();
        let glyph = page_glyph(&key, &title);
        (title, glyph)
    };
    widgets::draw_nav_item(
        item.hDC,
        rect_of(&item.rcItem),
        st.dpi.get(),
        &fonts,
        &NavItem {
            text: &title,
            glyph: Some(glyph),
            selected: item.itemState.0 & ODS_SELECTED.0 != 0,
            hot: widgets::hot_nav() == index as i32,
            focused: item.itemState.0 & ODS_FOCUS.0 != 0
                && item.itemState.0 & ODS_NOFOCUSRECT.0 == 0,
        },
    );
}

// ---- UI の状態の保存 ----

/// 今のウィンドウの位置とサイズ。最小化・最大化中は、前回の矩形を保ったまま最大化の有無だけ更新する
fn capture_window(st: &State) -> Option<WindowState> {
    let hwnd = st.hwnd.get();
    let prev = st.saved.borrow().window;
    let zoomed = unsafe { IsZoomed(hwnd) }.as_bool();
    if zoomed || unsafe { IsIconic(hwnd) }.as_bool() {
        return prev.map(|w| WindowState {
            maximized: zoomed,
            ..w
        });
    }
    let mut outer = RECT::default();
    let mut client = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut outer).ok()?;
        GetClientRect(hwnd, &mut client).ok()?;
    }
    let dpi = st.dpi.get();
    Some(WindowState {
        x: outer.left,
        y: outer.top,
        w_dp: unscale(client.right - client.left, dpi),
        h_dp: unscale(client.bottom - client.top, dpi),
        maximized: false,
    })
}

/// 詳細設定・開いているページ（・`with_window` ならウィンドウの位置とサイズ）を保存する。
/// 保存に失敗しても何も出さない（次回の起動が既定になるだけ）
fn persist_ui_state(st: &State, with_window: bool) {
    let Some(path) = st.ui_state_path.as_deref() else {
        return;
    };
    let next = {
        let model = st.model.borrow();
        let mut next = st.saved.borrow().clone();
        next.detail = model.detail;
        // 「設定」（読み込み中・失敗）は保存しない（前回のページを残す）
        let title = model
            .sidebar
            .iter()
            .position(|p| *p == model.page)
            .and_then(|i| model.sidebar_titles.get(i));
        if let Some(key) = title.and_then(|t| native_ui_state::page_key(model.page, t)) {
            next.page = Some(key);
        }
        next
    };
    let mut next: UiState = next;
    if with_window {
        next.window = capture_window(st);
    }
    let _ = native_ui_state::save(path, &next);
    *st.saved.borrow_mut() = next;
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
            persist_ui_state(st, false);
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
        ids::DIAG_RUN_ALL if code == BN_CLICKED => start_diag_all(st),
        ids::DIAG_CANCEL if code == BN_CLICKED => {
            st.model.borrow_mut().diag.request_cancel();
            rebuild_if_diag(st);
        }
        ids::DIAG_COPY if code == BN_CLICKED => export_diag(st, false),
        ids::DIAG_SAVE if code == BN_CLICKED => export_diag(st, true),
        id if code == BN_CLICKED && native_diag::item_from_button_id(id).is_some() => {
            if let Some(item) = native_diag::item_from_button_id(id) {
                start_diag_one(st, item);
            }
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
    persist_ui_state(st, false);
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

// ---- 動作確認のページ ----

/// 動作確認のページを開いているときだけ作り直す（他のページの入力中の欄を壊さない）
fn rebuild_if_diag(st: &State) {
    if st.model.borrow().page == Page::Extra(native_diag::PAGE_ID) {
        request_rebuild(st);
    }
}

fn start_diag_all(st: &State) {
    let flag = st.model.borrow_mut().diag.begin_all();
    let Some(cancel) = flag else { return };
    set_status(st, "動作確認を始めました", false);
    send_command(st, Command::DiagRunAll { cancel });
    rebuild_if_diag(st);
}

fn start_diag_one(st: &State, id: crate::diag::ItemId) {
    let flag = st.model.borrow_mut().diag.begin_one(id);
    let Some(cancel) = flag else { return };
    send_command(st, Command::DiagRunOne { id, cancel });
    rebuild_if_diag(st);
}

/// レポートのコピー（`save` が偽）・保存（真）をワーカーに頼む
fn export_diag(st: &State, save: bool) {
    let report = {
        let model = st.model.borrow();
        if !model.diag.can_export() {
            return;
        }
        model.diag.report().cloned()
    };
    let Some(report) = report else { return };
    if save {
        set_status(st, "レポートを保存しています…", false);
        send_command(
            st,
            Command::DiagSave {
                report: Box::new(report),
            },
        );
    } else {
        set_status(st, "レポートをコピーしています…", false);
        send_command(
            st,
            Command::DiagCopy {
                text: report.to_text(),
            },
        );
    }
}

/// 状態表示（ページ内の行と、フッターのステータス）を更新する
fn diag_note(st: &State, text: String, is_error: bool) {
    st.model.borrow_mut().diag.set_note(text.clone(), is_error);
    if is_error {
        set_status(st, &format!("エラー: {text}"), true);
    } else {
        set_status(st, &text, false);
    }
    rebuild_if_diag(st);
}

/// 実行が終わったあと、押して外れたボタン（無効になった「中止」など）の代わりに
/// 「すべて確認」へフォーカスを戻す
fn diag_finished(st: &State) {
    {
        let mut model = st.model.borrow_mut();
        if model
            .wanted_focus
            .as_ref()
            .is_none_or(|w| w.id == ids::DIAG_CANCEL)
        {
            model.wanted_focus = Some(Ident {
                role: Role::Button,
                key: None,
                id: ids::DIAG_RUN_ALL,
            });
        }
    }
    rebuild_if_diag(st);
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
        Reply::DiagProgress(progress) => {
            st.model.borrow_mut().diag.on_progress(&progress);
            rebuild_if_diag(st);
        }
        Reply::DiagAllDone(result) => {
            let failed = result.is_err();
            st.model.borrow_mut().diag.finish_all(result);
            let note = st.model.borrow().diag.note().cloned();
            if let Some(note) = note {
                set_status(st, &note.text, failed);
            }
            diag_finished(st);
        }
        Reply::DiagOneDone { id, result } => {
            let failed = result.is_err();
            let status = result.as_ref().ok().map(|r| r.status.label());
            st.model.borrow_mut().diag.finish_one(id, result);
            if failed {
                let note = st.model.borrow().diag.note().cloned();
                if let Some(note) = note {
                    set_status(st, &note.text, true);
                }
            } else if let Some(label) = status {
                set_status(st, &format!("{}: {label}", id.title()), false);
            }
            diag_finished(st);
        }
        Reply::DiagCopied(Ok(())) => diag_note(st, "レポートをコピーしました".to_owned(), false),
        Reply::DiagCopied(Err(e)) => {
            diag_note(st, format!("レポートをコピーできませんでした: {e}"), true)
        }
        Reply::DiagSaved(Ok(path)) => {
            diag_note(st, format!("レポートを保存しました: {path}"), false)
        }
        Reply::DiagSaved(Err(e)) => {
            diag_note(st, format!("レポートを保存できませんでした: {e}"), true)
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
