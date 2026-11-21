//! DPI・フォント・ダークモード（タイトルバーのみ）。

use super::controls::wide;
use crate::native_layout::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CreateFontIndirectW, DEFAULT_CHARSET, DeleteObject, GetDC, GetTextFaceW,
    HFONT, HGDIOBJ, LOGFONTW, ReleaseDC, SelectObject,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
    GetDpiForSystem, GetDpiForWindow, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::PCWSTR;

/// 使うフォント（DPI ごとに作り直す）。本文・説明・案内はすべて `normal`（10.5pt 相当）。
/// 説明・案内は文字の大きさではなく色（グレー）で区別する
#[derive(Clone, Copy)]
pub(super) struct Fonts {
    /// 本文（システムのメッセージフォント。Segoe UI 9pt 相当）
    pub normal: HFONT,
    /// ページの見出し（本文の 1.8 倍・ボールド）
    pub heading: HFONT,
    /// 節（グループ）の見出し・値の強調（本文の 1.2 倍・セミボールド）
    pub section: HFONT,
    /// 設定項目の名前（本文と同じ大きさ・セミボールド）
    pub label: HFONT,
    /// サイドバーのアイコン（Segoe Fluent Icons / Segoe MDL2 Assets。どちらも無ければ `None`）
    pub icon: Option<HFONT>,
}

impl Fonts {
    /// 破棄する（コントロールから外してから呼ぶこと）
    pub fn destroy(self) {
        for f in [self.normal, self.heading, self.section, self.label]
            .into_iter()
            .chain(self.icon)
        {
            let _ = unsafe { DeleteObject(HGDIOBJ(f.0)) };
        }
    }
}

/// マニフェストが効かなかった場合の保険（効いていれば失敗するだけ）
pub(super) fn ensure_dpi_awareness() {
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// システムの DPI（ウィンドウを作る前の見積もり用）
pub(super) fn system_dpi() -> u32 {
    match unsafe { GetDpiForSystem() } {
        0 => 96,
        d => d,
    }
}

/// ウィンドウの DPI
pub(super) fn window_dpi(hwnd: HWND) -> u32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 96,
        d => d,
    }
}

/// クライアント領域のサイズ（px）から、ウィンドウ全体のサイズ（px）を求める
pub(super) fn outer_size(
    client_w: i32,
    client_h: i32,
    style: u32,
    ex_style: u32,
    dpi: u32,
) -> (i32, i32) {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };
    let _ = unsafe {
        AdjustWindowRectExForDpi(
            &mut rect,
            WINDOW_STYLE(style),
            false,
            WINDOW_EX_STYLE(ex_style),
            dpi,
        )
    };
    (rect.right - rect.left, rect.bottom - rect.top)
}

fn face_name(face: &str) -> [u16; 32] {
    let mut out = [0u16; 32];
    for (slot, unit) in out.iter_mut().zip(wide(face).into_iter().take(31)) {
        *slot = unit;
    }
    out
}

/// 本文の基準サイズ（0.5pt 単位。10.5pt = 14px @96DPI）。システムのメッセージフォント（9pt）より大きくする
pub(super) const BODY_HALF_POINTS: i32 = 21;

/// 0.5pt 単位のサイズ `half_pts` を `dpi` での論理フォントの高さにする（`-MulDiv(pt, dpi, 72)` の負値。四捨五入）
pub(super) fn font_height_px(half_pts: i32, dpi: u32) -> i32 {
    let dpi = i64::from(dpi.max(1));
    let px = (i64::from(half_pts) * dpi + 72) / 144;
    -(px.max(1) as i32)
}

/// 本文のフォントを `dpi` に合わせて作る。Segoe UI Variable Text があればそれ、無ければ Segoe UI。
/// 見出し・ラベルは同じ書体の大きさと太さだけを変える
pub(super) fn create_fonts(dpi: u32) -> Fonts {
    let make = |face: &str| LOGFONTW {
        lfHeight: font_height_px(BODY_HALF_POINTS, dpi),
        lfWeight: 400,
        lfCharSet: DEFAULT_CHARSET,
        // ClearType を明示する（既定だと描く先によってギザギザになりうる）
        lfQuality: CLEARTYPE_QUALITY,
        lfFaceName: face_name(face),
        ..Default::default()
    };
    let mut normal_font = make("Segoe UI");
    let variable = make("Segoe UI Variable Text");
    let probe = unsafe { CreateFontIndirectW(&variable) };
    if !probe.is_invalid() {
        if realized_face_is(probe, "Segoe UI Variable Text") {
            normal_font = variable;
        }
        let _ = unsafe { DeleteObject(HGDIOBJ(probe.0)) };
    }
    // 見出しは本文と同じ書体（Segoe UI / Segoe UI Variable の既定）の大きさと太さだけを変える
    let derive = |num: i32, den: i32, weight: i32| -> LOGFONTW {
        let mut f = normal_font;
        f.lfHeight = (normal_font.lfHeight * num / den).min(-1);
        f.lfWeight = weight;
        f
    };
    let heading_font = derive(9, 5, 700);
    let section_font = derive(6, 5, 600);
    let label_font = derive(1, 1, 600);
    Fonts {
        normal: unsafe { CreateFontIndirectW(&normal_font) },
        heading: unsafe { CreateFontIndirectW(&heading_font) },
        section: unsafe { CreateFontIndirectW(&section_font) },
        label: unsafe { CreateFontIndirectW(&label_font) },
        icon: create_icon_font(dpi),
    }
}

/// アイコンのフォント。Windows 11 の Segoe Fluent Icons、無ければ Windows 10 の Segoe MDL2 Assets。
/// フォントが無いと GDI は別のフォントに置き換えてしまうので、実際に選ばれた書体名で確かめる
fn create_icon_font(dpi: u32) -> Option<HFONT> {
    for face in ["Segoe Fluent Icons", "Segoe MDL2 Assets"] {
        let logfont = LOGFONTW {
            // 16px @96DPI
            lfHeight: -((16 * dpi as i32 + 48) / 96),
            lfWeight: 400,
            lfCharSet: DEFAULT_CHARSET,
            lfQuality: CLEARTYPE_QUALITY,
            lfFaceName: face_name(face),
            ..Default::default()
        };
        let font = unsafe { CreateFontIndirectW(&logfont) };
        if font.is_invalid() {
            continue;
        }
        if realized_face_is(font, face) {
            return Some(font);
        }
        let _ = unsafe { DeleteObject(HGDIOBJ(font.0)) };
    }
    None
}

fn realized_face_is(font: HFONT, face: &str) -> bool {
    let hdc = unsafe { GetDC(None) };
    if hdc.is_invalid() {
        return false;
    }
    let old = unsafe { SelectObject(hdc, HGDIOBJ(font.0)) };
    let mut buf = [0u16; 64];
    let n = unsafe { GetTextFaceW(hdc, Some(&mut buf)) };
    unsafe {
        SelectObject(hdc, old);
        ReleaseDC(None, hdc);
    }
    // `GetTextFaceW` の戻り値は終端の NUL を含む文字数
    let len = (n.max(1) as usize - 1).min(buf.len());
    String::from_utf16_lossy(&buf[..len]).eq_ignore_ascii_case(face)
}

/// ハイコントラストテーマが有効か（有効ならエラーも赤ではなくシステムの文字色にする）
pub(super) fn is_high_contrast() -> bool {
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
pub(super) fn apps_use_dark_theme() -> bool {
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

/// タイトルバーをシステムのテーマ（ライト / ダーク）に合わせる。本体の色は標準のまま
pub(super) fn apply_title_bar_theme(hwnd: HWND) {
    let dark: i32 = i32::from(apps_use_dark_theme());
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::from_ref(&dark).cast(),
            std::mem::size_of::<i32>() as u32,
        )
    };
}

/// 画面上の点 `(x, y)` に最も近いモニターの DPI（取れなければシステムの DPI）
pub(super) fn monitor_dpi_at(x: i32, y: i32) -> u32 {
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
) -> windows::core::BOOL {
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

/// 全モニターの作業領域（タスクバーを除く。仮想スクリーン座標）
pub(super) fn work_areas() -> Vec<Rect> {
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
