//! Win32 の共通部品: 文字列・矩形の変換、モニターの作業領域、GDI のフォントと文字幅の計測、
//! ウィンドウクラスの登録管理。判断は持たない（配置・寸法の計算は `nagi_windows`）。

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::Mutex;

use nagi_windows::candidate_style::font_px;
use nagi_windows::candidates::{self, FontRole, Point, Rect, TextMeasure, scale_px};
use nagi_windows::theme::{Rgb, SystemColors, ThemeInput};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT,
    COLOR_WINDOW, COLOR_WINDOWTEXT, ClientToScreen, CreateCompatibleDC, CreateFontW,
    DEFAULT_CHARSET, DEFAULT_PITCH, DeleteDC, DeleteObject, EnumDisplayMonitors, FF_DONTCARE,
    FW_NORMAL, GetDC, GetMonitorInfoW, GetSysColor, GetTextExtentPoint32W, HDC, HFONT, HGDIOBJ,
    HMONITOR, MONITORINFO, OUT_DEFAULT_PRECIS, ReleaseDC, SYS_COLOR_INDEX, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GUITHREADINFO, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo,
    GetWindowRect, HCURSOR, HICON, IsIconic, MONITORINFOF_PRIMARY, RegisterClassExW,
    SPI_GETHIGHCONTRAST, SPI_SETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SystemParametersInfoW, UnregisterClassW, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WNDCLASS_STYLES,
    WNDCLASSEXW, WNDPROC,
};
use windows::core::{BOOL, PCWSTR};

use crate::dll;

/// UTF-16 に変換して NUL で終端する
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `RECT` → 論理矩形
pub fn to_rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// 論理矩形 → `RECT`
pub fn from_rect(r: Rect) -> RECT {
    RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// このスレッド（なければフォアグラウンドのスレッド）の GUI スレッド情報
fn gui_thread_info() -> Option<GUITHREADINFO> {
    for thread in [unsafe { GetCurrentThreadId() }, 0] {
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetGUIThreadInfo(thread, &mut info) }.is_ok() {
            return Some(info);
        }
    }
    None
}

/// フォーカスウィンドウのキャレット（`GetGUIThreadInfo` の `rcCaret`。クラシックなアプリ向け）の
/// スクリーン座標の矩形。取れなければ（キャレットを持たないアプリ・ゼロ矩形）`None`
pub fn gui_caret_rect() -> Option<Rect> {
    let info = gui_thread_info()?;
    let hwnd = info.hwndCaret;
    let rc = info.rcCaret;
    if hwnd.is_invalid() || (rc.left == rc.right && rc.top == rc.bottom) {
        return None;
    }
    let mut top_left = POINT {
        x: rc.left,
        y: rc.top,
    };
    let mut bottom_right = POINT {
        x: rc.right,
        y: rc.bottom,
    };
    // rcCaret は hwndCaret のクライアント座標
    let ok = unsafe { ClientToScreen(hwnd, &mut top_left) }.as_bool()
        && unsafe { ClientToScreen(hwnd, &mut bottom_right) }.as_bool();
    ok.then_some(Rect {
        left: top_left.x,
        top: top_left.y,
        right: bottom_right.x,
        bottom: bottom_right.y,
    })
}

/// フォーカス（なければアクティブ・フォアグラウンド）のウィンドウの矩形（スクリーン座標）。
/// 最小化されている・取れなければ `None`
pub fn focus_window_rect() -> Option<Rect> {
    let info = gui_thread_info();
    let candidates = [
        info.map(|i| i.hwndFocus),
        info.map(|i| i.hwndActive),
        Some(unsafe { GetForegroundWindow() }),
    ];
    candidates.into_iter().flatten().find_map(|hwnd| {
        if hwnd.is_invalid() || unsafe { IsIconic(hwnd) }.as_bool() {
            return None;
        }
        let mut rc = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rc) }.ok()?;
        Some(to_rect(rc))
    })
}

/// マウスカーソルの位置（スクリーン座標）。取れなければ `None`
pub fn try_cursor_position() -> Option<Point> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) }
        .is_ok()
        .then_some(Point { x: p.x, y: p.y })
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<(Rect, bool)>) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        out.push((
            to_rect(info.rcWork),
            info.dwFlags & MONITORINFOF_PRIMARY != 0,
        ));
    }
    BOOL(1)
}

/// 全モニターの作業領域（タスクバーを除く）。先頭がプライマリ
pub fn work_areas() -> Vec<Rect> {
    let mut monitors: Vec<(Rect, bool)> = Vec::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut monitors as *mut Vec<(Rect, bool)> as isize),
        )
    };
    if !ok.as_bool() {
        return Vec::new();
    }
    candidates::primary_first(&monitors)
}

/// この DLL の `HINSTANCE`
pub fn instance() -> windows::Win32::Foundation::HINSTANCE {
    let module = dll::module_handle();
    if module.is_invalid() {
        // DllMain を経ていない場合（ありえないが）、呼び出しプロセスのハンドルで代用
        unsafe { GetModuleHandleW(PCWSTR::null()) }
            .map(|m| windows::Win32::Foundation::HINSTANCE(m.0))
            .unwrap_or_default()
    } else {
        windows::Win32::Foundation::HINSTANCE(module.0)
    }
}

/// ウィンドウクラスの登録管理。DLL がアンロードされるとウィンドウプロシージャが消えるため、
/// 最後のウィンドウを壊したらクラスも登録解除する（再ロード時に古いプロシージャを指さないように）
pub struct WindowClass {
    name: &'static str,
    /// ウィンドウクラスのスタイル（`CS_*`。影の `CS_DROPSHADOW` など）
    style: u32,
    count: Mutex<u32>,
}

impl WindowClass {
    /// 名前を指定して作る（登録はしない）
    pub const fn new(name: &'static str) -> Self {
        Self::with_style(name, 0)
    }

    /// 名前とクラススタイル（`CS_*` の値）を指定して作る（登録はしない）
    pub const fn with_style(name: &'static str, style: u32) -> Self {
        Self {
            name,
            style,
            count: Mutex::new(0),
        }
    }

    /// クラス名（NUL 終端の UTF-16）
    pub fn name_wide(&self) -> Vec<u16> {
        wide(self.name)
    }

    /// ウィンドウを 1 つ作る前に呼ぶ。最初の 1 つで登録する
    pub fn acquire(&self, wndproc: WNDPROC) -> bool {
        let mut n = self.count.lock().unwrap_or_else(|e| e.into_inner());
        if *n == 0 {
            let name = wide(self.name);
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: WNDCLASS_STYLES(self.style),
                lpfnWndProc: wndproc,
                hInstance: instance(),
                hIcon: HICON::default(),
                hCursor: HCURSOR::default(),
                lpszClassName: PCWSTR(name.as_ptr()),
                ..Default::default()
            };
            if unsafe { RegisterClassExW(&class) } == 0 {
                return false;
            }
        }
        *n += 1;
        true
    }

    /// ウィンドウを 1 つ壊した後に呼ぶ。最後の 1 つで登録解除する
    pub fn release(&self) {
        let mut n = self.count.lock().unwrap_or_else(|e| e.into_inner());
        *n = n.saturating_sub(1);
        if *n == 0 {
            let name = wide(self.name);
            let _ = unsafe { UnregisterClassW(PCWSTR(name.as_ptr()), Some(instance())) };
        }
    }
}

/// 既定のウィンドウプロシージャへ
///
/// # Safety
/// ウィンドウプロシージャ内から、受け取った引数をそのまま渡すこと
pub unsafe fn default_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// 用途別のフォントと文字幅の計測（GDI）。文字の倍率（`percent`）と `dpi` を掛けた大きさで作る
/// （基準の大きさは macOS 版 `CandidatePanel` と同じ。`nagi_windows::candidate_style::font_px`）
pub struct Fonts {
    dc: HDC,
    surface: HFONT,
    number: HFONT,
    label: HFONT,
    dpi: u32,
    percent: i32,
}

pub fn make_font(px: i32, dpi: u32) -> HFONT {
    let face = wide("Yu Gothic UI");
    unsafe {
        CreateFontW(
            -scale_px(px, dpi),
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
            u32::from(DEFAULT_PITCH.0) | u32::from(FF_DONTCARE.0),
            PCWSTR(face.as_ptr()),
        )
    }
}

impl Fonts {
    /// `dpi`・倍率 `percent`（%）用のフォントを作る
    pub fn new(dpi: u32, percent: i32) -> Self {
        let screen = unsafe { GetDC(None) };
        let dc = unsafe { CreateCompatibleDC(Some(screen)) };
        unsafe { ReleaseDC(None, screen) };
        Self {
            dc,
            surface: make_font(font_px(FontRole::Surface, percent), dpi),
            number: make_font(font_px(FontRole::Number, percent), dpi),
            label: make_font(font_px(FontRole::Label, percent), dpi),
            dpi,
            percent,
        }
    }

    /// この DPI・倍率で作ったものか（違えば作り直す）
    pub fn matches(&self, dpi: u32, percent: i32) -> bool {
        self.dpi == dpi && self.percent == percent
    }

    /// 用途に対応するフォント
    pub fn font(&self, role: FontRole) -> HFONT {
        match role {
            FontRole::Number => self.number,
            FontRole::Surface => self.surface,
            FontRole::Label => self.label,
        }
    }
}

impl TextMeasure for Fonts {
    fn width(&self, text: &str, role: FontRole) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut size = windows::Win32::Foundation::SIZE::default();
        unsafe {
            let old = SelectObject(self.dc, HGDIOBJ(self.font(role).0));
            let _ = GetTextExtentPoint32W(self.dc, &utf16, &mut size);
            SelectObject(self.dc, old);
        }
        size.cx
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.surface.0));
            let _ = DeleteObject(HGDIOBJ(self.number.0));
            let _ = DeleteObject(HGDIOBJ(self.label.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

/// `*mut c_void` を介さずにユーザーデータを `Box<T>` として保持するための補助（`GWLP_USERDATA` 用）
pub fn into_user_data<T>(value: Box<T>) -> isize {
    Box::into_raw(value) as *mut c_void as isize
}

/// `into_user_data` で渡した値を参照に戻す。0 なら `None`
///
/// # Safety
/// `ptr` は `into_user_data::<T>` が返した値で、`from_user_data` で解放されていないこと。
/// 返す参照はウィンドウプロシージャの呼び出しの間だけ有効
pub unsafe fn user_data_ref<'a, T>(ptr: isize) -> Option<&'a T> {
    unsafe { (ptr as *const T).as_ref() }
}

/// `into_user_data` で渡した値を取り戻して破棄する
///
/// # Safety
/// `ptr` は `into_user_data::<T>` が返した値で、まだ解放されていないこと
pub unsafe fn drop_user_data<T>(ptr: isize) {
    if ptr != 0 {
        drop(unsafe { Box::from_raw(ptr as *mut T) });
    }
}

// ---- テーマ（ダークモード / ハイコントラスト）----

/// 現在のテーマ情報のキャッシュ。無効化されるまでレジストリを読み直さない
static THEME: Mutex<Option<ThemeInput>> = Mutex::new(None);

fn read_apps_use_light_theme() -> Option<u32> {
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").as_ptr()),
            PCWSTR(wide("AppsUseLightTheme").as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast::<c_void>()),
            Some(&raw mut size),
        )
    };
    (err == ERROR_SUCCESS).then_some(data)
}

fn sys_rgb(index: SYS_COLOR_INDEX) -> Rgb {
    let v = unsafe { GetSysColor(index) };
    Rgb(
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    )
}

fn read_high_contrast() -> Option<SystemColors> {
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some((&raw mut hc).cast::<c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()?;
    (hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0).then(|| SystemColors {
        window: sys_rgb(COLOR_WINDOW),
        window_text: sys_rgb(COLOR_WINDOWTEXT),
        highlight: sys_rgb(COLOR_HIGHLIGHT),
        highlight_text: sys_rgb(COLOR_HIGHLIGHTTEXT),
        gray_text: sys_rgb(COLOR_GRAYTEXT),
    })
}

/// 現在のテーマ情報（ライト/ダーク・ハイコントラスト。キャッシュ済み。変更通知で `theme_message` が破棄する）。
/// 配色は設定（`CandidateStyle::palette`）がこれから決める
pub fn theme_input() -> ThemeInput {
    let mut guard = THEME.lock().unwrap_or_else(|e| e.into_inner());
    *guard.get_or_insert_with(|| ThemeInput {
        apps_use_light_theme: read_apps_use_light_theme(),
        high_contrast: read_high_contrast(),
    })
}

/// テーマ変更を知らせるメッセージなら、キャッシュを破棄して true を返す
/// （呼び出し側は再描画する）。`WM_SETTINGCHANGE` はトップレベルウィンドウへ
/// ブロードキャストされるので、候補ウィンドウ・トースト自身で受けられる。
pub fn theme_message(msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    let changed = match msg {
        WM_SYSCOLORCHANGE => true,
        WM_SETTINGCHANGE => {
            // ライト/ダーク切替は lParam が "ImmersiveColorSet"、ハイコントラスト切替は SPI_SETHIGHCONTRAST
            let immersive = lparam.0 != 0
                && unsafe { PCWSTR(lparam.0 as *const u16).to_string() }
                    .is_ok_and(|s| s == "ImmersiveColorSet");
            immersive || wparam.0 == SPI_SETHIGHCONTRAST.0 as usize
        }
        _ => false,
    };
    if changed {
        *THEME.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    changed
}
