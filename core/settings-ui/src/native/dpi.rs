//! DPI・フォント・ダークモード（タイトルバーのみ）。

use super::controls::wide;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DEFAULT_CHARSET, DeleteObject, HFONT, HGDIOBJ, LOGFONTW,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
    GetDpiForWindow, SetProcessDpiAwarenessContext, SystemParametersInfoForDpi,
};
use windows::Win32::UI::WindowsAndMessaging::{
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::PCWSTR;

/// 使うフォント 2 種（DPI ごとに作り直す）
#[derive(Clone, Copy)]
pub(super) struct Fonts {
    /// 本文（システムのメッセージフォント。Segoe UI 9pt 相当）
    pub normal: HFONT,
    /// 説明・案内（本文の 9 割）
    pub small: HFONT,
}

impl Fonts {
    /// 破棄する（コントロールから外してから呼ぶこと）
    pub fn destroy(self) {
        for f in [self.normal, self.small] {
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

/// システムのメッセージフォントを `dpi` に合わせて作る（取れなければ Segoe UI 9pt）
pub(super) fn create_fonts(dpi: u32) -> Fonts {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let got = unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some(std::ptr::from_mut(&mut metrics).cast()),
            0,
            dpi,
        )
    }
    .is_ok();
    let normal_font: LOGFONTW = if got && metrics.lfMessageFont.lfHeight != 0 {
        metrics.lfMessageFont
    } else {
        LOGFONTW {
            // 9pt = 12px @96DPI
            lfHeight: -((9 * dpi as i32 + 36) / 72),
            lfWeight: 400,
            lfCharSet: DEFAULT_CHARSET,
            lfFaceName: face_name("Segoe UI"),
            ..Default::default()
        }
    };
    let mut small_font = normal_font;
    small_font.lfHeight = normal_font.lfHeight * 9 / 10;
    if small_font.lfHeight == 0 {
        small_font.lfHeight = -1;
    }
    Fonts {
        normal: unsafe { CreateFontIndirectW(&normal_font) },
        small: unsafe { CreateFontIndirectW(&small_font) },
    }
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
