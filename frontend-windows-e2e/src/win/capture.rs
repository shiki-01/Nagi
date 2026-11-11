//! GDI による画面全体のキャプチャと PNG 保存。

use std::path::Path;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use crate::png;

/// プライマリ画面を PNG で保存する（候補ウィンドウなどが写る）
pub fn save_screen_png(path: &Path) -> Result<(), String> {
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    if width <= 0 || height <= 0 {
        return Err("画面サイズを取得できません".to_owned());
    }
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    let screen = unsafe { GetDC(None) };
    let mem = unsafe { CreateCompatibleDC(Some(screen)) };
    let bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
    let old = unsafe { SelectObject(mem, bitmap.into()) };
    let blt = unsafe {
        BitBlt(
            mem,
            0,
            0,
            width,
            height,
            Some(screen),
            0,
            0,
            SRCCOPY | CAPTUREBLT,
        )
    };
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // 負の高さ = 上の行から
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = unsafe {
        GetDIBits(
            mem,
            bitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        SelectObject(mem, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
    blt.map_err(|e| format!("BitBlt: {e}"))?;
    if lines == 0 {
        return Err("GetDIBits に失敗しました".to_owned());
    }
    let png = png::encode_rgb(width as u32, height as u32, &png::bgra_to_rgb(&bgra))?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, png).map_err(|e| format!("{}: {e}", path.display()))
}
