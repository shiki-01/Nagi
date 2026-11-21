//! DPI・フォント・ダークモード（タイトルバーのみ）。

use super::controls::wide;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DEFAULT_CHARSET, DeleteObject, HFONT, HGDIOBJ, LOGFONTW,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
    GetDpiForWindow, SetProcessDpiAwarenessContext, SystemParametersInfoForDpi,
};
use windows::Win32::UI::WindowsAndMessaging::{
    NONCLIENTMETRICSW, SPI_GETHIGHCONTRAST, SPI_GETNONCLIENTMETRICS,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::PCWSTR;

/// 使うフォント（DPI ごとに作り直す）。本文・説明・案内はすべて `normal`（9pt 相当以上）。
/// 説明・案内は文字の大きさではなく色（グレー）で区別する
#[derive(Clone, Copy)]
pub(super) struct Fonts {
    /// 本文（システムのメッセージフォント。Segoe UI 9pt 相当）
    pub normal: HFONT,
    /// ページの見出し（本文の 1.6 倍・セミボールド）
    pub heading: HFONT,
    /// 節（グループ）の見出し（本文の 1.2 倍・セミボールド）
    pub section: HFONT,
}

impl Fonts {
    /// 破棄する（コントロールから外してから呼ぶこと）
    pub fn destroy(self) {
        for f in [self.normal, self.heading, self.section] {
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
    // 見出しは本文と同じ書体（Segoe UI / Segoe UI Variable の既定）の大きさと太さだけを変える
    let derive = |num: i32, den: i32| -> LOGFONTW {
        let mut f = normal_font;
        f.lfHeight = (normal_font.lfHeight * num / den).min(-1);
        f.lfWeight = 600;
        f
    };
    let heading_font = derive(8, 5);
    let section_font = derive(6, 5);
    Fonts {
        normal: unsafe { CreateFontIndirectW(&normal_font) },
        heading: unsafe { CreateFontIndirectW(&heading_font) },
        section: unsafe { CreateFontIndirectW(&section_font) },
    }
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
