//! 描画の下回り: GDI+ による角丸・円のアンチエイリアス描画と、GDI のダブルバッファ。
//!
//! GDI+ を開始できなかったときは GDI（`RoundRect` / `Ellipse`。ギザギザになるが読める）に落とす。

use crate::native_layout::Rect;
use crate::native_theme::Rgb;
use std::cell::Cell;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush, DeleteDC,
    DeleteObject, Ellipse, FillRect, HBRUSH, HDC, HGDIOBJ, PS_SOLID, RoundRect, SRCCOPY,
    SelectObject,
};
use windows::Win32::Graphics::GdiPlus::{
    FillModeAlternate, FlushIntentionFlush, GdipAddPathArc, GdipClosePathFigure, GdipCreateFromHDC,
    GdipCreatePath, GdipCreatePen1, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics,
    GdipDeletePath, GdipDeletePen, GdipDrawPath, GdipDrawRectangle, GdipFillEllipse, GdipFillPath,
    GdipFillRectangle, GdipFlush, GdipSetPixelOffsetMode, GdipSetSmoothingMode, GdiplusShutdown,
    GdiplusStartup, GdiplusStartupInput, GdiplusStartupOutput, GpBrush, GpGraphics, GpPath, GpPen,
    GpSolidFill, SmoothingModeAntiAlias, Status, UnitPixel,
};

thread_local! {
    /// GDI+ のトークン（0 = 開始できていない）。UI スレッドだけが使う
    static TOKEN: Cell<usize> = const { Cell::new(0) };
}

/// GDI+ を開始する（UI スレッドで 1 回）。失敗しても描画は GDI に落ちるだけ
pub(super) fn startup() {
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        ..Default::default()
    };
    let mut token = 0usize;
    let mut output = GdiplusStartupOutput::default();
    // SAFETY: 引数は有効なポインタ。バックグラウンドスレッドは使わない設定にしない（既定）
    let status = unsafe { GdiplusStartup(&mut token, &input, &mut output) };
    if status == Status(0) {
        TOKEN.with(|t| t.set(token));
    }
}

/// GDI+ を終了する（描画をすべて終えたあと）
pub(super) fn shutdown() {
    let token = TOKEN.with(|t| t.replace(0));
    if token != 0 {
        // SAFETY: `startup` で得たトークン
        unsafe { GdiplusShutdown(token) };
    }
}

fn gdiplus_ready() -> bool {
    TOKEN.with(|t| t.get() != 0)
}

/// 1 つの `HDC` への描画。GDI+ が使えればアンチエイリアスつき
pub(super) struct Canvas {
    hdc: HDC,
    graphics: *mut GpGraphics,
}

impl Canvas {
    pub fn new(hdc: HDC) -> Self {
        let mut graphics: *mut GpGraphics = std::ptr::null_mut();
        if gdiplus_ready() {
            // SAFETY: `hdc` は有効。失敗すれば `graphics` は null のまま（GDI に落ちる）
            let status = unsafe { GdipCreateFromHDC(hdc, &mut graphics) };
            if status != Status(0) {
                graphics = std::ptr::null_mut();
            } else {
                unsafe {
                    GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
                    // 4 = PixelOffsetModeHalf（線の位置が半ピクセルずれない）
                    GdipSetPixelOffsetMode(
                        graphics,
                        windows::Win32::Graphics::GdiPlus::PixelOffsetMode(4),
                    );
                }
            }
        }
        Self { hdc, graphics }
    }

    /// GDI+ の描画をその場で反映する。GDI+ は描画をためることがあり、続けて GDI（文字）を同じ `HDC` に
    /// 描くと順序が入れ替わって、文字が塗りに隠れたりちらついたりするため、描くたびに流す
    fn flush(&self) {
        if !self.graphics.is_null() {
            // SAFETY: `new` で作った有効なグラフィックス
            unsafe { GdipFlush(self.graphics, FlushIntentionFlush) };
        }
    }

    /// 塗りつぶし（角丸。`radius` 0 で四角）
    pub fn fill_round(&self, r: Rect, radius: i32, color: Rgb) {
        self.fill_round_alpha(r, radius, color, 0xFF);
    }

    /// 不透明度つきの塗りつぶし（影など。GDI+ が無いときは何も描かない）。`alpha` 255 は不透明
    pub fn fill_round_alpha(&self, r: Rect, radius: i32, color: Rgb, alpha: u8) {
        if r.w <= 0 || r.h <= 0 {
            return;
        }
        if self.graphics.is_null() {
            if alpha == 0xFF {
                self.gdi_round(r, radius, color, color);
            }
            return;
        }
        // SAFETY: GDI+ のオブジェクトはこの関数の中で作って壊す
        unsafe {
            let mut fill: *mut GpSolidFill = std::ptr::null_mut();
            if GdipCreateSolidFill(color.argb_alpha(alpha), &mut fill) != Status(0) {
                return;
            }
            let brush = fill.cast::<GpBrush>();
            if radius <= 0 {
                GdipFillRectangle(
                    self.graphics,
                    brush,
                    r.x as f32,
                    r.y as f32,
                    r.w as f32,
                    r.h as f32,
                );
            } else if let Some(path) = round_path(r, radius, 0.0) {
                GdipFillPath(self.graphics, brush, path);
                GdipDeletePath(path);
            }
            GdipDeleteBrush(brush);
        }
        self.flush();
    }

    /// 枠線（`r` の内側に `width` px の線）。角丸・`radius` 0 で四角
    pub fn stroke_round(&self, r: Rect, radius: i32, color: Rgb, width: i32) {
        if r.w <= 0 || r.h <= 0 || width <= 0 {
            return;
        }
        if self.graphics.is_null() {
            self.gdi_stroke(r, radius, color, width);
            return;
        }
        let half = width as f32 / 2.0;
        // SAFETY: GDI+ のオブジェクトはこの関数の中で作って壊す
        unsafe {
            let mut pen: *mut GpPen = std::ptr::null_mut();
            if GdipCreatePen1(color.argb(), width as f32, UnitPixel, &mut pen) != Status(0) {
                return;
            }
            if radius <= 0 {
                GdipDrawRectangle(
                    self.graphics,
                    pen,
                    r.x as f32 + half,
                    r.y as f32 + half,
                    (r.w - width) as f32,
                    (r.h - width) as f32,
                );
            } else if let Some(path) = round_path(r, radius, half) {
                GdipDrawPath(self.graphics, pen, path);
                GdipDeletePath(path);
            }
            GdipDeletePen(pen);
        }
        self.flush();
    }

    /// 円・楕円の塗りつぶし
    pub fn fill_ellipse(&self, r: Rect, color: Rgb) {
        if r.w <= 0 || r.h <= 0 {
            return;
        }
        if self.graphics.is_null() {
            self.gdi_ellipse(r, color, color);
            return;
        }
        // SAFETY: GDI+ のオブジェクトはこの関数の中で作って壊す
        unsafe {
            let mut fill: *mut GpSolidFill = std::ptr::null_mut();
            if GdipCreateSolidFill(color.argb(), &mut fill) != Status(0) {
                return;
            }
            let brush = fill.cast::<GpBrush>();
            GdipFillEllipse(
                self.graphics,
                brush,
                r.x as f32,
                r.y as f32,
                r.w as f32,
                r.h as f32,
            );
            GdipDeleteBrush(brush);
        }
        self.flush();
    }

    // ---- GDI への代替 ----

    fn gdi_round(&self, r: Rect, radius: i32, fill: Rgb, line: Rgb) {
        // SAFETY: 作ったペン・ブラシは選択を戻してから壊す
        unsafe {
            let pen = CreatePen(PS_SOLID, 1, COLORREF(line.colorref()));
            let brush = CreateSolidBrush(COLORREF(fill.colorref()));
            let old_pen = SelectObject(self.hdc, HGDIOBJ(pen.0));
            let old_brush = SelectObject(self.hdc, HGDIOBJ(brush.0));
            if radius <= 0 {
                let rect = RECT {
                    left: r.x,
                    top: r.y,
                    right: r.x + r.w,
                    bottom: r.y + r.h,
                };
                FillRect(self.hdc, &rect, brush);
            } else {
                let _ = RoundRect(
                    self.hdc,
                    r.x,
                    r.y,
                    r.x + r.w,
                    r.y + r.h,
                    radius * 2,
                    radius * 2,
                );
            }
            SelectObject(self.hdc, old_pen);
            SelectObject(self.hdc, old_brush);
            let _ = DeleteObject(HGDIOBJ(pen.0));
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }

    fn gdi_stroke(&self, r: Rect, radius: i32, color: Rgb, width: i32) {
        // 外側の色で塗ってから内側を塗り戻す代わりに、4 辺を塗る（角丸は無視した四角の枠でよい）
        let _ = radius;
        // SAFETY: 作ったブラシは使い終わったら壊す
        unsafe {
            let brush = CreateSolidBrush(COLORREF(color.colorref()));
            for rect in [
                RECT {
                    left: r.x,
                    top: r.y,
                    right: r.x + r.w,
                    bottom: r.y + width,
                },
                RECT {
                    left: r.x,
                    top: r.y + r.h - width,
                    right: r.x + r.w,
                    bottom: r.y + r.h,
                },
                RECT {
                    left: r.x,
                    top: r.y,
                    right: r.x + width,
                    bottom: r.y + r.h,
                },
                RECT {
                    left: r.x + r.w - width,
                    top: r.y,
                    right: r.x + r.w,
                    bottom: r.y + r.h,
                },
            ] {
                FillRect(self.hdc, &rect, brush);
            }
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }

    fn gdi_ellipse(&self, r: Rect, fill: Rgb, line: Rgb) {
        // SAFETY: 作ったペン・ブラシは選択を戻してから壊す
        unsafe {
            let pen = CreatePen(PS_SOLID, 1, COLORREF(line.colorref()));
            let brush = CreateSolidBrush(COLORREF(fill.colorref()));
            let old_pen = SelectObject(self.hdc, HGDIOBJ(pen.0));
            let old_brush = SelectObject(self.hdc, HGDIOBJ(brush.0));
            let _ = Ellipse(self.hdc, r.x, r.y, r.x + r.w, r.y + r.h);
            SelectObject(self.hdc, old_pen);
            SelectObject(self.hdc, old_brush);
            let _ = DeleteObject(HGDIOBJ(pen.0));
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        if !self.graphics.is_null() {
            // SAFETY: `new` で作ったもの
            unsafe { GdipDeleteGraphics(self.graphics) };
        }
    }
}

/// 角丸の長方形のパス。`inset` だけ内側に寄せる（枠線を内側に描くため）。呼び出し側が `GdipDeletePath` する
fn round_path(r: Rect, radius: i32, inset: f32) -> Option<*mut GpPath> {
    let x = r.x as f32 + inset;
    let y = r.y as f32 + inset;
    let w = r.w as f32 - 2.0 * inset;
    let h = r.h as f32 - 2.0 * inset;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    // 直径は短い辺を超えない
    let d = ((radius as f32 - inset) * 2.0).clamp(1.0, w.min(h));
    let mut path: *mut GpPath = std::ptr::null_mut();
    // SAFETY: 作ったパスは呼び出し側が壊す。失敗したら null を返さず None
    unsafe {
        if GdipCreatePath(FillModeAlternate, &mut path) != Status(0) || path.is_null() {
            return None;
        }
        GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(path);
    }
    Some(path)
}

/// ダブルバッファで描く。`paint` には裏のバッファの `HDC` と、領域（0, 0 起点の大きさ）を渡し、
/// 終わったら `target` に転送する（ちらつき防止）
pub(super) fn buffered(target: HDC, width: i32, height: i32, paint: impl FnOnce(HDC)) {
    if width <= 0 || height <= 0 {
        return;
    }
    // SAFETY: 作った DC・ビットマップは選択を戻してから壊す
    unsafe {
        let mem = CreateCompatibleDC(Some(target));
        let bitmap = CreateCompatibleBitmap(target, width, height);
        if mem.is_invalid() || bitmap.is_invalid() {
            // バッファを作れない: 直接描く
            if !mem.is_invalid() {
                let _ = DeleteDC(mem);
            }
            if !bitmap.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
            paint(target);
            return;
        }
        let old = SelectObject(mem, HGDIOBJ(bitmap.0));
        paint(mem);
        let _ = BitBlt(target, 0, 0, width, height, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem);
    }
}

/// 単色で塗る
pub(super) fn fill_rect(hdc: HDC, r: Rect, color: Rgb) {
    let rect = RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    };
    // SAFETY: 作ったブラシはすぐ壊す
    unsafe {
        let brush: HBRUSH = CreateSolidBrush(COLORREF(color.colorref()));
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}
