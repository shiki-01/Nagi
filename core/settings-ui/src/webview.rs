//! WebView2 の設定ウィンドウ（Windows のみ）。`docs/decisions.md` H-130 候補。
//!
//! 画面の中身は `--web` と同じローカル HTTP サーバー（127.0.0.1・トークンつき。`http` / `run`）が返す Web 画面で、
//! ここは**ウィンドウと WebView2 のホストだけ**を持つ（`webview2-com` を直接使い、`wry` / `tao` は使わない）。
//! 判断（起動方式・ポート・メッセージ・ナビゲーションの許可）は OS 非依存の `webview_host` にあり、
//! unsafe はこのモジュールに隔離する。
//!
//! 起動の流れ: COM（STA）→ WebView2 ランタイムの有無 → 多重起動の抑止（名前付きミューテックス。ネイティブ版と
//! 同じ名前）→ 非表示のウィンドウ → 環境・コントローラー（ここまでで失敗したら `Err`。呼び出し側がネイティブ版へ
//! フォールバックする。ウィンドウはまだ見えていない）→ 設定・ハンドラー → 表示 → サーバースレッドを起動
//! （`imserver` の起動を待ってから URL を `WM_APP` で受け取り、`Navigate`）。ウィンドウを閉じるとすぐ隠し、
//! サーバースレッド（`imserver` の終了を含む）を最大 6 秒待つ。
#![allow(unsafe_code)]

use crate::backend::{FailedBackend, ImserverBackend, ImserverSpec, SettingsBackend};
use crate::http::App;
use crate::native_layout::{Rect, initial_client_size, scale, unscale};
use crate::native_theme::{Rgb, ThemeKind};
use crate::native_ui_state::{self, WindowState, clamp_to_monitors};
use crate::opener::{Opener, SystemOpener};
use crate::webview_host::{
    BASE_TITLE, PageMessage, background_rgb, is_allowed_navigation, is_external_url,
    keep_context_menu_item, parse_page_message, window_title,
};
use crate::{run, token};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, COREWEBVIEW2_MOVE_FOCUS_REASON,
    COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, COREWEBVIEW2_PERMISSION_STATE_DENY,
    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    CreateCoreWebView2EnvironmentWithOptions, GetAvailableCoreWebView2BrowserVersionString,
    ICoreWebView2, ICoreWebView2_11, ICoreWebView2Controller, ICoreWebView2Controller2,
    ICoreWebView2Controller4, ICoreWebView2Environment, ICoreWebView2EnvironmentOptions,
    ICoreWebView2Settings3, ICoreWebView2Settings4, ICoreWebView2Settings5, ICoreWebView2Settings6,
};
use webview2_com::{
    ContextMenuRequestedEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    MoveFocusRequestedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, WebMessageReceivedEventHandler,
    take_pwstr, wait_with_pump,
};
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, E_POINTER, E_UNEXPECTED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE,
    HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOW, CreateSolidBrush, DeleteObject, EnumDisplayMonitors, FillRect, GetMonitorInfoW,
    GetSysColor, HBRUSH, HDC, HGDIOBJ, HMONITOR, InvalidateRect, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
    GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, FindWindowW, GetClientRect, GetMessageW, GetWindowRect, HICON, IDC_ARROW,
    IMAGE_ICON, IsIconic, IsZoomed, LR_SHARED, LoadCursorW, LoadImageW, MINMAXINFO, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, SIZE_MINIMIZED, SM_CXICON, SM_CXSMICON,
    SM_CYICON, SM_CYSMICON, SPI_GETHIGHCONTRAST, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED,
    SWP_NOACTIVATE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetForegroundWindow,
    SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, TranslateMessage,
    UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED,
    WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_MOVE, WM_MOVING, WM_SETFOCUS,
    WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOLORCHANGE, WM_THEMECHANGED, WNDCLASSEXW, WS_CLIPCHILDREN,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR};

/// 多重起動の抑止に使うミューテックス（ネイティブ版 `native/window.rs` と同じ名前。どちらの方式でも 1 つだけ開く）
const MUTEX_NAME: &str = "Local\\NagiSettingsWindow";
/// ウィンドウクラス名（ネイティブ版と同じ。別プロセスの 2 つ目の起動が `FindWindowW` で見つけて前面に出せる）
const MAIN_CLASS: &str = "NagiSettingsMainWindow";
/// サーバースレッドが URL を用意できた（`imserver` の起動後）
const WM_NAVIGATE: u32 = WM_APP + 21;
/// ウィンドウを閉じたあと、サーバースレッド（`imserver` の終了を含む）を待つ最長時間
const SHUTDOWN_WAIT: Duration = Duration::from_secs(6);
/// 最小のクライアント領域（dp）。Web 画面は幅に追従して並び替わるので、ネイティブ版より小さくできる
const MIN_CLIENT_W: i32 = 560;
const MIN_CLIENT_H: i32 = 420;

/// WebView2 ウィンドウの起動条件
pub struct WebViewOptions {
    /// `imserver` の起動条件（サーバースレッドで起動する）
    pub spec: ImserverSpec,
    /// 開いたときの断片（`update` / `update-check`。`--section update [--auto-check]`）
    pub start_fragment: Option<&'static str>,
    /// UI の状態（ウィンドウの位置・サイズ）の保存先。`None` なら保存も復元もしない
    pub ui_state_path: Option<PathBuf>,
    /// WebView2 のユーザーデータフォルダ（`%LOCALAPPDATA%\Nagi\webview2`）。`None` なら WebView2 の既定
    pub user_data_dir: Option<PathBuf>,
    /// DevTools を有効にするか（`NAGI_SETTINGS_DEVTOOLS=1`）
    pub devtools: bool,
    /// 診断の実行（動作確認ページ）。`--no-network`・`--diag-out` などの引数を反映する
    pub diag: Arc<dyn crate::diag::service::DiagService>,
}

/// ウィンドウを開き、閉じられるまで動かす。すでに開いている設定ウィンドウがあれば、それを前面に出して
/// すぐ戻る（`Ok`）。WebView2 が使えない・初期化に失敗した（ウィンドウを表示する前）ときは `Err`
/// （理由つき）。呼び出し側がネイティブ版へフォールバックできるよう、`Err` のときは何も残さない
/// （ミューテックス・ウィンドウクラス・COM を解放済み）
pub fn run(options: WebViewOptions) -> Result<(), String> {
    ensure_dpi_awareness();
    let _com = ComGuard::new();
    // 1. ランタイムの有無（無ければここで諦める。ミューテックスも取らない）
    runtime_version()?;
    // 2. 多重起動の抑止
    let Some(_mutex) = MutexGuard::acquire() else {
        activate_existing();
        return Ok(());
    };
    // 3. 非表示のウィンドウ
    let _class = ClassGuard::register()?;
    let saved = options
        .ui_state_path
        .as_deref()
        .map(native_ui_state::load)
        .unwrap_or_default();
    let hwnd = create_window(saved.window.as_ref())?;
    let result = run_with_window(hwnd, options, saved.window.is_some_and(|w| w.maximized));
    // `run_with_window` は表示前の失敗ではウィンドウを壊さない（ここで破棄する）
    if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool() {
        let _ = unsafe { DestroyWindow(hwnd) };
    }
    result
}

// ---- ホストの状態 ----

/// ウィンドウと WebView2 の状態（UI スレッドだけが触る。ウィンドウプロシージャとイベントハンドラーで共有）
struct Host {
    hwnd: HWND,
    port: u16,
    controller: RefCell<Option<ICoreWebView2Controller>>,
    webview: RefCell<Option<ICoreWebView2>>,
    theme: Cell<ThemeKind>,
    brush: Cell<HBRUSH>,
    ui_state_path: Option<PathBuf>,
    url_rx: mpsc::Receiver<String>,
    /// 最初のナビゲーションが完了したか（完了したらフォーカスをページへ）
    loaded: Cell<bool>,
}

thread_local! {
    static HOST: RefCell<Option<Rc<Host>>> = const { RefCell::new(None) };
}

fn current_host() -> Option<Rc<Host>> {
    HOST.with(|h| h.borrow().clone())
}

fn run_with_window(hwnd: HWND, options: WebViewOptions, maximized: bool) -> Result<(), String> {
    // 4. サーバーのソケット（ポートが要る。失敗したらネイティブ版に任せる）
    let (server, port) = run::bind_preferred()?;
    let token_value = token::generate_token()?;

    // 5. 環境とコントローラー
    let env = create_environment(options.user_data_dir.as_deref())?;
    let controller = create_controller(hwnd, &env)?;
    let webview = unsafe { controller.CoreWebView2() }.map_err(|e| err("WebView2", &e))?;

    let (url_tx, url_rx) = mpsc::channel::<String>();
    let kind = detect_theme();
    let host = Rc::new(Host {
        hwnd,
        port,
        controller: RefCell::new(Some(controller.clone())),
        webview: RefCell::new(Some(webview.clone())),
        theme: Cell::new(kind),
        brush: Cell::new(HBRUSH::default()),
        ui_state_path: options.ui_state_path.clone(),
        url_rx,
        loaded: Cell::new(false),
    });
    apply_theme(&host, true);
    if let Err(e) = configure(&host, &controller, &webview, options.devtools) {
        // 表示前の失敗: WebView2 を閉じてから、呼び出し側がネイティブ版へフォールバックする
        let _ = unsafe { controller.Close() };
        let brush = host.brush.replace(HBRUSH::default());
        if !brush.is_invalid() {
            let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };
        }
        return Err(e);
    }
    HOST.with(|h| *h.borrow_mut() = Some(host.clone()));
    fit_to_client(&host);
    unsafe {
        let _ = controller.SetIsVisible(true);
    }

    // 6. 表示（ここから先の失敗では `Err` を返さない）
    let _ = unsafe { ShowWindow(hwnd, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOW }) };
    focus_page(&host);

    // 7. サーバースレッド
    let stop = Arc::new(AtomicBool::new(false));
    let raw = hwnd.0 as isize;
    let thread = {
        let stop = stop.clone();
        let spec = options.spec;
        let fragment = options.start_fragment;
        let diag = options.diag.clone();
        std::thread::spawn(move || {
            // imserver の起動（最大 60 秒）はここで待つ。ウィンドウは先に出ていて、背景色だけを見せている
            let backend: Box<dyn SettingsBackend> = match ImserverBackend::spawn(&spec) {
                Ok(b) => Box::new(b),
                Err(error) => Box::new(FailedBackend { error }),
            };
            let mut app = App::new(
                backend,
                Box::new(SystemOpener),
                token_value,
                port,
                Instant::now(),
            );
            app.load_config_path();
            app.set_diag_service(diag);
            app.set_start_fragment(fragment);
            if url_tx.send(app.page_url()).is_ok() {
                let _ = unsafe {
                    PostMessageW(Some(hwnd_from(raw)), WM_NAVIGATE, WPARAM(0), LPARAM(0))
                };
            }
            run::serve_until(&server, &mut app, &stop);
            // `app`（= `imserver` の子プロセス）はここで落とす
        })
    };

    // 8. メッセージループ
    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
        if r <= 0 {
            break;
        }
        let _ = unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
    }

    // 9. 後始末: WebView2 を閉じ、サーバースレッド（imserver の終了）を待つ
    HOST.with(|h| *h.borrow_mut() = None);
    if let Some(c) = host.controller.borrow_mut().take() {
        let _ = unsafe { c.Close() };
    }
    host.webview.borrow_mut().take();
    let brush = host.brush.replace(HBRUSH::default());
    if !brush.is_invalid() {
        let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };
    }
    stop.store(true, Ordering::Release);
    let deadline = Instant::now() + SHUTDOWN_WAIT;
    while !thread.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if thread.is_finished() {
        let _ = thread.join();
    }
    // 間に合わなければ放置する（`--parent-pid` で、親の終了とともに imserver も止まる）
    Ok(())
}

fn err(what: &str, e: &windows::core::Error) -> String {
    format!("{what} を初期化できません（{e}）")
}

fn hwnd_from(raw: isize) -> HWND {
    HWND(std::ptr::without_provenance_mut::<c_void>(raw as usize))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---- 前提の確認と RAII ----

/// インストールされている WebView2 ランタイムの版。無ければ `Err`（呼び出し側がフォールバックする）
fn runtime_version() -> Result<String, String> {
    let mut version = PWSTR::null();
    unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) }
        .map_err(|e| format!("WebView2 ランタイムを確認できません（{e}）"))?;
    if version.is_null() {
        return Err("WebView2 ランタイムがインストールされていません".to_owned());
    }
    let text = take_pwstr(version);
    if text.is_empty() {
        Err("WebView2 ランタイムがインストールされていません".to_owned())
    } else {
        Ok(text)
    }
}

/// COM の初期化（WebView2 は STA が要る）。`Drop` で解放する
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// 名前付きミューテックス。`Drop` で閉じる（フォールバック先のネイティブ版が同じ名前で取り直せるように）
struct MutexGuard(HANDLE);

impl MutexGuard {
    /// 取れたら `Some`。すでに別のプロセスが持っていれば `None`。作れなくても開く（多重起動の抑止だけ効かない）
    fn acquire() -> Option<Self> {
        let name = wide(MUTEX_NAME);
        match unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) } {
            Ok(handle) => {
                if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                    let _ = unsafe { CloseHandle(handle) };
                    None
                } else {
                    Some(Self(handle))
                }
            }
            Err(_) => Some(Self(HANDLE::default())),
        }
    }
}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

/// ウィンドウクラス。`Drop` で登録を解除する（フォールバック先のネイティブ版が同じクラス名を登録できるように）
struct ClassGuard;

impl ClassGuard {
    fn register() -> Result<Self, String> {
        let name = wide(MAIN_CLASS);
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: PCWSTR(name.as_ptr()),
            // タスクバー・Alt+Tab は大、タイトルバーは小（exe に埋め込んだ Nagi のロゴ。リソース ID 1）
            hIcon: app_icon(true, system_dpi()).unwrap_or_default(),
            hIconSm: app_icon(false, system_dpi()).unwrap_or_default(),
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&class) } == 0 {
            return Err("ウィンドウクラスを登録できません".to_owned());
        }
        Ok(Self)
    }
}

impl Drop for ClassGuard {
    fn drop(&mut self) {
        let name = wide(MAIN_CLASS);
        let _ = unsafe { UnregisterClassW(PCWSTR(name.as_ptr()), Some(instance())) };
    }
}

fn instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe { GetModuleHandleW(None) }
        .map(|m| windows::Win32::Foundation::HINSTANCE(m.0))
        .unwrap_or_default()
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

// ---- ウィンドウ ----

fn ensure_dpi_awareness() {
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

fn system_dpi() -> u32 {
    match unsafe { GetDpiForSystem() } {
        0 => 96,
        d => d,
    }
}

fn window_dpi(hwnd: HWND) -> u32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 96,
        d => d,
    }
}

const STYLE: u32 = WS_OVERLAPPEDWINDOW.0 | WS_CLIPCHILDREN.0;

/// クライアント領域のサイズ（px）から、ウィンドウ全体のサイズ（px）
fn outer_size(client_w: i32, client_h: i32, dpi: u32) -> (i32, i32) {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };
    let _ = unsafe {
        AdjustWindowRectExForDpi(
            &mut rect,
            WINDOW_STYLE(STYLE),
            false,
            WINDOW_EX_STYLE(0),
            dpi,
        )
    };
    (rect.right - rect.left, rect.bottom - rect.top)
}

fn min_outer_size(dpi: u32) -> (i32, i32) {
    outer_size(scale(MIN_CLIENT_W, dpi), scale(MIN_CLIENT_H, dpi), dpi)
}

/// 非表示のメインウィンドウを作る（前回の位置とサイズがあればモニターの作業領域に収めて使う）
fn create_window(saved: Option<&WindowState>) -> Result<HWND, String> {
    let dpi = system_dpi();
    let (cw, ch) = initial_client_size(dpi);
    let (w, h) = outer_size(cw, ch, dpi);
    let (x, y, w, h) = match saved {
        Some(s) => {
            let r = restore_rect(s);
            (r.x, r.y, r.w, r.h)
        }
        None => (CW_USEDEFAULT, CW_USEDEFAULT, w, h),
    };
    let class = wide(MAIN_CLASS);
    let title = wide(BASE_TITLE);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(STYLE),
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance()),
            None,
        )
    }
    .map_err(|e| err("ウィンドウ", &e))
}

fn restore_rect(w: &WindowState) -> Rect {
    let dpi = monitor_dpi_at(w.x.saturating_add(48), w.y.saturating_add(48));
    let (ow, oh) = outer_size(scale(w.w_dp, dpi), scale(w.h_dp, dpi), dpi);
    let (min_w, min_h) = min_outer_size(dpi);
    clamp_to_monitors(
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

fn monitor_dpi_at(x: i32, y: i32) -> u32 {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
    let ok = !monitor.is_invalid()
        && unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.is_ok();
    if ok && dpi_x != 0 {
        dpi_x
    } else {
        system_dpi()
    }
}

unsafe extern "system" fn collect_work_area(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `work_areas` が渡した `Vec<Rect>` へのポインタ
    let list = unsafe { &mut *(data.0 as *mut Vec<Rect>) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let r = info.rcWork;
        list.push(Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        });
    }
    true.into()
}

fn work_areas() -> Vec<Rect> {
    let mut list: Vec<Rect> = Vec::new();
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_work_area),
            LPARAM(std::ptr::from_mut(&mut list) as isize),
        )
    };
    list
}

/// exe に埋め込んだ Nagi のアイコン（リソース ID 1。`build.rs` が埋め込む）。共有アイコンなので破棄しない
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

// ---- テーマ ----

fn is_high_contrast() -> bool {
    let mut info = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some(std::ptr::from_mut(&mut info).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok();
    ok && (info.dwFlags.0 & HCF_HIGHCONTRASTON.0) != 0
}

/// Windows のアプリのテーマがダークか（レジストリ。取れなければライト）
fn apps_use_dark_theme() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value = wide("AppsUseLightTheme");
    let mut data: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::from_mut(&mut data).cast()),
            Some(&mut size),
        )
    };
    status.0 == 0 && data == 0
}

fn detect_theme() -> ThemeKind {
    ThemeKind::decide(is_high_contrast(), apps_use_dark_theme())
}

/// 背景色（ハイコントラストはシステムの `COLOR_WINDOW`）
fn background(kind: ThemeKind) -> Rgb {
    background_rgb(kind).unwrap_or_else(|| {
        let c = unsafe { GetSysColor(COLOR_WINDOW) };
        Rgb(
            (c & 0xff) as u8,
            ((c >> 8) & 0xff) as u8,
            ((c >> 16) & 0xff) as u8,
        )
    })
}

/// タイトルバー・ウィンドウの背景・WebView2 の既定の背景色（ページが描かれるまでの色）をテーマに合わせる。
/// `force` でなければ、テーマが変わっていないときは何もしない
fn apply_theme(host: &Host, force: bool) {
    let kind = detect_theme();
    if !force && kind == host.theme.get() {
        return;
    }
    host.theme.set(kind);
    let dark: i32 = i32::from(kind == ThemeKind::Dark);
    let _ = unsafe {
        DwmSetWindowAttribute(
            host.hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::from_ref(&dark).cast(),
            std::mem::size_of::<i32>() as u32,
        )
    };
    let Rgb(r, g, b) = background(kind);
    let old = host.brush.replace(unsafe {
        CreateSolidBrush(COLORREF(
            u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16,
        ))
    });
    if !old.is_invalid() {
        let _ = unsafe { DeleteObject(HGDIOBJ(old.0)) };
    }
    if let Some(c2) = host
        .controller
        .borrow()
        .as_ref()
        .and_then(|c| c.cast::<ICoreWebView2Controller2>().ok())
    {
        let _ = unsafe {
            c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 255,
                R: r,
                G: g,
                B: b,
            })
        };
    }
    let _ = unsafe { InvalidateRect(Some(host.hwnd), None, true) };
}

// ---- WebView2 の作成 ----

fn create_environment(
    user_data_dir: Option<&std::path::Path>,
) -> Result<ICoreWebView2Environment, String> {
    let user_data = user_data_dir.map(|dir| {
        let _ = std::fs::create_dir_all(dir);
        HSTRING::from(dir.as_os_str())
    });
    let (tx, rx) = mpsc::channel();
    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        // ミニメニュー・PDF のポップアップ・SmartScreen による URL 検査は使わない（ローカルの自サーバーだけを表示する）
        options.set_additional_browser_arguments(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection".to_owned(),
        );
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &user_data.unwrap_or_default(),
            &ICoreWebView2EnvironmentOptions::from(options),
            &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
                move |error_code, environment| {
                    let result = (|| {
                        error_code?;
                        environment.ok_or_else(|| windows::core::Error::from(E_POINTER))
                    })();
                    tx.send(result)
                        .map_err(|_| windows::core::Error::from(E_UNEXPECTED))
                },
            )),
        )
    }
    .map_err(|e| err("WebView2 の環境", &e))?;
    wait_with_pump(rx)
        .map_err(|e| format!("WebView2 の環境を作れません（{e}）"))?
        .map_err(|e| err("WebView2 の環境", &e))
}

fn create_controller(
    hwnd: HWND,
    env: &ICoreWebView2Environment,
) -> Result<ICoreWebView2Controller, String> {
    let (tx, rx) = mpsc::channel();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |error_code, controller| {
            let result = (|| {
                error_code?;
                controller.ok_or_else(|| windows::core::Error::from(E_POINTER))
            })();
            tx.send(result)
                .map_err(|_| windows::core::Error::from(E_UNEXPECTED))
        },
    ));
    unsafe { env.CreateCoreWebView2Controller(hwnd, &handler) }
        .map_err(|e| err("WebView2 のコントローラー", &e))?;
    wait_with_pump(rx)
        .map_err(|e| format!("WebView2 のコントローラーを作れません（{e}）"))?
        .map_err(|e| err("WebView2 のコントローラー", &e))
}

/// 設定・ハンドラー（セキュリティ: ナビゲーションは自サーバーのオリジンだけ・新しいウィンドウと権限要求は拒否・
/// DevTools は無効・右クリックは編集系だけ・ズームとブラウザのショートカットは無効・外部ファイルのドロップは無効）
fn configure(
    host: &Rc<Host>,
    controller: &ICoreWebView2Controller,
    webview: &ICoreWebView2,
    devtools: bool,
) -> Result<(), String> {
    let port = host.port;
    let mut token = 0i64;
    unsafe {
        let settings = webview.Settings().map_err(|e| err("WebView2 の設定", &e))?;
        let _ = settings.SetIsStatusBarEnabled(false);
        let _ = settings.SetAreDevToolsEnabled(devtools);
        let _ = settings.SetIsZoomControlEnabled(false);
        let _ = settings.SetAreHostObjectsAllowed(false);
        // 既定のメニューは出さない（`ContextMenuRequested` を使えるときは編集系だけ残す）
        let menu_ok = webview.cast::<ICoreWebView2_11>().is_ok();
        let _ = settings.SetAreDefaultContextMenusEnabled(menu_ok);
        if let Ok(s) = settings.cast::<ICoreWebView2Settings3>() {
            // F5・Ctrl+R・Ctrl+P・F12 などのブラウザのショートカット（DevTools の検証時は残す）
            let _ = s.SetAreBrowserAcceleratorKeysEnabled(devtools);
        }
        if let Ok(s) = settings.cast::<ICoreWebView2Settings4>() {
            let _ = s.SetIsPasswordAutosaveEnabled(false);
            let _ = s.SetIsGeneralAutofillEnabled(false);
        }
        if let Ok(s) = settings.cast::<ICoreWebView2Settings5>() {
            let _ = s.SetIsPinchZoomEnabled(false);
        }
        if let Ok(s) = settings.cast::<ICoreWebView2Settings6>() {
            let _ = s.SetIsSwipeNavigationEnabled(false);
        }
        if let Ok(c4) = controller.cast::<ICoreWebView2Controller4>() {
            let _ = c4.SetAllowExternalDrop(false);
        }
    }

    // ナビゲーション: 自サーバーのオリジンだけ。外部の http(s) はユーザー操作のときだけ既定のブラウザで開く
    unsafe {
        webview
            .add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if !is_allowed_navigation(&uri, port) {
                        args.SetCancel(true)?;
                        let mut user = BOOL::default();
                        let _ = args.IsUserInitiated(&mut user);
                        if user.as_bool() && is_external_url(&uri) {
                            let _ = SystemOpener.open_url(&uri);
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| err("WebView2 のナビゲーション", &e))?;

        // 新しいウィンドウは作らない（外部の http(s) は既定のブラウザへ）
        webview
            .add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    args.SetHandled(true)?;
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if is_external_url(&uri) && !is_allowed_navigation(&uri, port) {
                        let _ = SystemOpener.open_url(&uri);
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| err("WebView2 のウィンドウ制御", &e))?;

        // カメラ・位置情報・通知などの権限要求はすべて拒否
        webview
            .add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| err("WebView2 の権限", &e))?;

        // ページ → ホストのメッセージ（送信元が自サーバーのオリジンのときだけ）
        let weak_host = Rc::downgrade(host);
        webview
            .add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut source = PWSTR::null();
                    args.Source(&mut source)?;
                    if !is_allowed_navigation(&take_pwstr(source), port) {
                        return Ok(());
                    }
                    let mut json = PWSTR::null();
                    args.WebMessageAsJson(&mut json)?;
                    let json = take_pwstr(json);
                    if let (Some(host), Some(message)) =
                        (weak_host.upgrade(), parse_page_message(&json))
                    {
                        on_page_message(&host, message);
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| err("WebView2 のメッセージ", &e))?;

        // 最初のナビゲーションが終わったらページにフォーカス
        let weak_host = Rc::downgrade(host);
        webview
            .add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_, _| {
                    if let Some(host) = weak_host.upgrade()
                        && !host.loaded.replace(true)
                    {
                        focus_page(&host);
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| err("WebView2 のナビゲーション", &e))?;

        // 描画プロセスの異常は再読み込み。ブラウザプロセスが落ちたら閉じる（続けられない）
        let weak_host = Rc::downgrade(host);
        webview
            .add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = Default::default();
                    args.ProcessFailedKind(&mut kind)?;
                    let Some(host) = weak_host.upgrade() else {
                        return Ok(());
                    };
                    if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                        || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
                    {
                        if let Some(wv) = host.webview.borrow().as_ref() {
                            let _ = wv.Reload();
                        }
                    } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                        let _ = PostMessageW(Some(host.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| err("WebView2 の異常検知", &e))?;

        // Tab がページの端に達したらページの中で循環させる（ウィンドウに他のコントロールは無い）
        let focus_controller = controller.clone();
        controller
            .add_MoveFocusRequested(
                &MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut reason = COREWEBVIEW2_MOVE_FOCUS_REASON::default();
                    args.Reason(&mut reason)?;
                    args.SetHandled(true)?;
                    let _ = focus_controller.MoveFocus(reason);
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|e| err("WebView2 のフォーカス", &e))?;

        // 右クリックメニューは、入力欄・選択範囲の編集系（切り取り・コピー・貼り付けなど）だけにする
        if let Ok(wv11) = webview.cast::<ICoreWebView2_11>() {
            let _ = wv11.add_ContextMenuRequested(
                &ContextMenuRequestedEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let items = args.MenuItems()?;
                    let mut count = 0u32;
                    items.Count(&mut count)?;
                    // 後ろから消す（添字がずれない）
                    for i in (0..count).rev() {
                        let keep = items
                            .GetValueAtIndex(i)
                            .and_then(|item| {
                                let mut name = PWSTR::null();
                                item.Name(&mut name)?;
                                Ok(keep_context_menu_item(&take_pwstr(name)))
                            })
                            .unwrap_or(false);
                        if !keep {
                            let _ = items.RemoveValueAtIndex(i);
                        }
                    }
                    items.Count(&mut count)?;
                    if count == 0 {
                        args.SetHandled(true)?;
                    }
                    Ok(())
                })),
                &mut token,
            );
        }
    }
    Ok(())
}

// ---- メッセージの処理 ----

fn on_page_message(host: &Host, message: PageMessage) {
    match message {
        PageMessage::CloseRequest => {
            let _ = unsafe { PostMessageW(Some(host.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        PageMessage::OpenExternal(url) => {
            let _ = SystemOpener.open_url(&url);
        }
        PageMessage::SetTitle(title) => {
            let text = wide(&window_title(title.as_deref()));
            let _ = unsafe { SetWindowTextW(host.hwnd, PCWSTR(text.as_ptr())) };
        }
        PageMessage::SaveWindowState => persist_window(host),
    }
}

fn focus_page(host: &Host) {
    if let Some(c) = host.controller.borrow().as_ref() {
        // 最小化中などで失敗してもよい
        let _ = unsafe { c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
    }
}

fn fit_to_client(host: &Host) {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(host.hwnd, &mut rect) }.is_ok()
        && let Some(c) = host.controller.borrow().as_ref()
    {
        let _ = unsafe { c.SetBounds(rect) };
    }
}

/// 今のウィンドウの位置とサイズ。最大化・最小化中は、前回の矩形を保ったまま最大化の有無だけ更新する
fn capture_window(host: &Host, prev: Option<WindowState>) -> Option<WindowState> {
    let zoomed = unsafe { IsZoomed(host.hwnd) }.as_bool();
    if zoomed || unsafe { IsIconic(host.hwnd) }.as_bool() {
        return prev.map(|w| WindowState {
            maximized: zoomed,
            ..w
        });
    }
    let mut outer = RECT::default();
    let mut client = RECT::default();
    unsafe {
        GetWindowRect(host.hwnd, &mut outer).ok()?;
        GetClientRect(host.hwnd, &mut client).ok()?;
    }
    let dpi = window_dpi(host.hwnd);
    Some(WindowState {
        x: outer.left,
        y: outer.top,
        w_dp: unscale(client.right - client.left, dpi),
        h_dp: unscale(client.bottom - client.top, dpi),
        maximized: false,
    })
}

/// ウィンドウの位置・サイズ・最大化を保存する（ネイティブ版の詳細設定・ページは書き換えない）。失敗しても何も出さない
fn persist_window(host: &Host) {
    let Some(path) = host.ui_state_path.as_deref() else {
        return;
    };
    let mut state = native_ui_state::load(path);
    state.window = capture_window(host, state.window);
    let _ = native_ui_state::save(path, &state);
}

fn on_navigate(host: &Host) {
    let Ok(url) = host.url_rx.try_recv() else {
        return;
    };
    if let Some(wv) = host.webview.borrow().as_ref() {
        let _ = unsafe { wv.Navigate(&HSTRING::from(url)) };
    }
}

// ---- ウィンドウプロシージャ ----

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Some(host) = current_host() else {
        // ホストができる前（作成中）・片付け後は既定の処理
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    match msg {
        WM_SIZE => {
            let minimized = wparam.0 == SIZE_MINIMIZED as usize;
            if let Some(c) = host.controller.borrow().as_ref() {
                let _ = unsafe { c.SetIsVisible(!minimized) };
            }
            if !minimized {
                fit_to_client(&host);
            }
            LRESULT(0)
        }
        WM_MOVE | WM_MOVING => {
            if let Some(c) = host.controller.borrow().as_ref() {
                let _ = unsafe { c.NotifyParentWindowPositionChanged() };
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SETFOCUS => {
            focus_page(&host);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            let hdc = HDC(std::ptr::without_provenance_mut::<c_void>(wparam.0));
            let mut rect = RECT::default();
            if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
                unsafe { FillRect(hdc, &rect, host.brush.get()) };
            }
            LRESULT(1)
        }
        WM_GETMINMAXINFO => {
            let (w, h) = min_outer_size(window_dpi(hwnd));
            // SAFETY: WM_GETMINMAXINFO の lparam は書き込み可能な MINMAXINFO
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            info.ptMinTrackSize = POINT { x: w, y: h };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // SAFETY: WM_DPICHANGED の lparam は推奨のウィンドウ矩形
            let r = unsafe { &*(lparam.0 as *const RECT) };
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            LRESULT(0)
        }
        // ダーク / ライト・ハイコントラストの切替に追従する（ページ側は CSS が追従する）
        WM_SETTINGCHANGE => {
            apply_theme(&host, false);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SYSCOLORCHANGE | WM_THEMECHANGED => {
            apply_theme(&host, true);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_EXITSIZEMOVE => {
            persist_window(&host);
            LRESULT(0)
        }
        WM_NAVIGATE => {
            on_navigate(&host);
            LRESULT(0)
        }
        WM_CLOSE => {
            // 位置を保存し、すぐ隠してから破棄する（imserver の終了待ちを見せない）
            persist_window(&host);
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
