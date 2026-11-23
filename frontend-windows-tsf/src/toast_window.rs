//! 通知パネル（トースト）。フォーカスを奪わない最前面の小さなポップアップを、作業領域の右下に
//! 数秒だけ出す。文言は `nagi_windows::status`、配置・表示時間は `nagi_windows::toast` が決める。
//! モード切替ポップアップ（既定はキャレット付近に 1 秒）も同じウィンドウで出す（`show_near` / `show_corner`）。
//! 確定後の見直しの提案（`retro_review`）も、入力中の文字列の上に出す（`show_above`。H-139 候補）。
//! 表示時間・位置・余白・角丸・文字の大きさ・配色は `CandidateStyle`（設定。H-119）に従う。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use std::time::Duration;

use nagi_windows::candidate_style::CandidateStyle;
use nagi_windows::candidates::{Rect, Size, scale_px};
use nagi_windows::toast::{
    TOAST_MARGIN, TOAST_MAX_WIDTH, ToastPosition, place_in_corner, place_mode_popup,
};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateRoundRectRgn, CreateSolidBrush, DT_CALCRECT, DT_NOPREFIX, DT_WORDBREAK,
    DeleteObject, DrawTextW, EndPaint, FillRect, HGDIOBJ, InvalidateRect, PAINTSTRUCT,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOPMOST,
    KillTimer, MA_NOACTIVATE, SW_HIDE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_MOUSEACTIVATE,
    WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_TIMER,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::PCWSTR;

use crate::win::{
    self, Fonts, WindowClass, drop_user_data, into_user_data, user_data_ref, wide, work_areas,
};
use nagi_windows::candidates::FontRole;

static CLASS: WindowClass = WindowClass::new("NagiToastWindow");
const TIMER_ID: usize = 1;

/// 表示位置の決め方
#[derive(Clone, Copy)]
enum Place {
    /// プライマリの作業領域の隅（通知・隅に出すモード表示）
    Corner(ToastPosition),
    /// 基準の矩形の近く（モード切替ポップアップ）
    Near(Rect),
    /// 基準の矩形の上（収まらなければ下。確定後の見直しの提案。候補ウィンドウは下に出るので重ねない）
    Above(Rect),
}

struct Paint {
    text: Vec<u16>,
    fonts: Fonts,
    pad_x: i32,
    pad_y: i32,
    size: Size,
    /// この表示の実効値（ハイコントラストの調整済み。配色は描くたびにここから決める）
    style: CandidateStyle,
}

type PaintCell = RefCell<Option<Paint>>;

/// 通知パネル。UI スレッドでのみ使う
pub struct ToastWindow {
    hwnd: Cell<HWND>,
    paint: Cell<isize>,
    /// アプリ別の位置の補正（96 DPI 基準の dp。キャレット付近に出すポップアップだけに効く）
    offset: Cell<nagi_windows::candidates::Point>,
    /// 設定から解決した見た目（次の表示から効く）
    style: Cell<CandidateStyle>,
}

impl Default for ToastWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastWindow {
    /// 作る（ウィンドウは最初に表示するときに作る）
    pub fn new() -> Self {
        Self {
            hwnd: Cell::new(HWND::default()),
            paint: Cell::new(0),
            offset: Cell::new(nagi_windows::candidates::Point::default()),
            style: Cell::new(CandidateStyle::default()),
        }
    }

    /// 見た目の設定（実効値）を渡す。次の表示から効く
    pub fn set_style(&self, style: CandidateStyle) {
        self.style.set(style);
    }

    /// アプリ別の位置の補正（dp）を設定する。次のポップアップから効く（右下の通知には効かない）
    pub fn set_offset(&self, offset: nagi_windows::candidates::Point) {
        self.offset.set(offset);
    }

    fn ensure_window(&self) -> Option<HWND> {
        let current = self.hwnd.get();
        if !current.is_invalid() {
            return Some(current);
        }
        if !CLASS.acquire(Some(wndproc)) {
            return None;
        }
        let class = CLASS.name_wide();
        let title = wide("Nagi notification");
        let created = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(win::instance()),
                None,
            )
        };
        let Ok(hwnd) = created else {
            CLASS.release();
            return None;
        };
        let ptr = into_user_data(Box::new(RefCell::new(None::<Paint>)));
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr) };
        self.paint.set(ptr);
        self.hwnd.set(hwnd);
        Some(hwnd)
    }

    /// `message` を作業領域の隅（設定 `windows.toast_position`）に表示する。表示中なら置き換えて、表示時間をやり直す
    pub fn show(&self, message: &str) {
        let style = self.style.get();
        self.present(
            &format!("Nagi: {message}"),
            Place::Corner(style.toast_position),
            style.toast_duration,
        );
    }

    /// `text` を `anchor`（キャレットなどのスクリーン座標の矩形）の近くに、設定のモード表示の時間だけ表示する
    /// （モード切替ポップアップ）。表示中なら置き換えて、表示時間をやり直す
    pub fn show_near(&self, text: &str, anchor: Rect) {
        self.present(
            text,
            Place::Near(anchor),
            self.style.get().mode_popup_duration,
        );
    }

    /// `text` を `anchor`（入力中の文字列の矩形）の上に `duration` だけ表示する（確定後の見直しの提案）。
    /// 表示中なら置き換えて、表示時間をやり直す
    pub fn show_above(&self, text: &str, anchor: Rect, duration: Duration) {
        self.present(text, Place::Above(anchor), duration);
    }

    /// `text` を作業領域の隅に、設定のモード表示の時間だけ表示する（モード表示の位置が隅のとき）
    pub fn show_corner(&self, text: &str, position: ToastPosition) {
        self.present(
            text,
            Place::Corner(position),
            self.style.get().mode_popup_duration,
        );
    }

    fn present(&self, text: &str, place: Place, duration: Duration) {
        let Some(hwnd) = self.ensure_window() else {
            return;
        };
        let areas = work_areas();
        let Some(work) = areas.first().copied() else {
            return;
        };
        // 置く場所へ先に動かし、そこのモニターの DPI で寸法を決める（トーストはプライマリの右下）
        let (probe_x, probe_y) = match place {
            Place::Corner(_) => (work.right - 1, work.bottom - 1),
            Place::Near(anchor) => (anchor.left, anchor.bottom),
            Place::Above(anchor) => (anchor.left, anchor.top),
        };
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                probe_x,
                probe_y,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            )
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let Some(cell) = (unsafe { user_data_ref::<PaintCell>(self.paint.get()) }) else {
            return;
        };
        let mut style = self.style.get();
        if win::theme_input().high_contrast.is_some() {
            style = style.for_high_contrast();
        }
        let fonts = Fonts::new(dpi, style.font_percent);
        let text: Vec<u16> = text.encode_utf16().collect();
        let (pad_x, pad_y) = (
            scale_px(style.toast_pad_x, dpi),
            scale_px(style.toast_pad_y, dpi),
        );
        let max_text_width = scale_px(TOAST_MAX_WIDTH, dpi) - pad_x * 2;

        // 折り返したときの文字領域の大きさを計る
        let mut measure = RECT {
            left: 0,
            top: 0,
            right: max_text_width,
            bottom: 0,
        };
        let mut buf = text.clone();
        unsafe {
            let dc = windows::Win32::Graphics::Gdi::GetDC(Some(hwnd));
            let old = SelectObject(dc, HGDIOBJ(fonts.font(FontRole::Surface).0));
            DrawTextW(
                dc,
                &mut buf,
                &mut measure,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(dc, old);
            windows::Win32::Graphics::Gdi::ReleaseDC(Some(hwnd), dc);
        }
        let size = Size {
            width: (measure.right - measure.left) + pad_x * 2,
            height: (measure.bottom - measure.top) + pad_y * 2,
        };
        let origin = match place {
            Place::Corner(position) => {
                place_in_corner(work, size, scale_px(TOAST_MARGIN, dpi), position)
            }
            Place::Near(anchor) => place_mode_popup(
                nagi_windows::placement::shift_rect(anchor, self.offset.get(), dpi),
                size,
                &areas,
            ),
            Place::Above(anchor) => nagi_windows::retro::place_above(
                nagi_windows::placement::shift_rect(anchor, self.offset.get(), dpi),
                size,
                &areas,
                scale_px(4, dpi),
            ),
        };
        *cell.borrow_mut() = Some(Paint {
            text,
            fonts,
            pad_x,
            pad_y,
            size,
            style,
        });
        unsafe {
            let corner = scale_px(style.toast_corner, dpi);
            if corner > 0 {
                let region =
                    CreateRoundRectRgn(0, 0, size.width + 1, size.height + 1, corner, corner);
                // 領域の所有権はウィンドウに移る
                SetWindowRgn(hwnd, Some(region), true);
            } else {
                SetWindowRgn(hwnd, None, true);
            }
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                origin.x,
                origin.y,
                size.width,
                size.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(hwnd), None, true);
            let ms = u32::try_from(duration.as_millis()).unwrap_or(4000);
            SetTimer(Some(hwnd), TIMER_ID, ms, None);
        }
    }

    /// 表示中なら隠す
    pub fn hide(&self) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_invalid() {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ID) };
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }

    /// ウィンドウを壊す
    pub fn destroy(&self) {
        let hwnd = self.hwnd.replace(HWND::default());
        if !hwnd.is_invalid() {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ID) };
            let _ = unsafe { DestroyWindow(hwnd) };
            CLASS.release();
        }
        self.paint.set(0);
    }
}

impl Drop for ToastWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn draw(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.is_invalid() {
        return;
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if let Some(cell) = unsafe { user_data_ref::<PaintCell>(ptr) }
        && let Some(paint) = cell.borrow().as_ref()
    {
        let whole = RECT {
            left: 0,
            top: 0,
            right: paint.size.width,
            bottom: paint.size.height,
        };
        unsafe {
            let pal = paint.style.palette(win::theme_input());
            let brush = CreateSolidBrush(COLORREF(pal.toast_bg.to_colorref()));
            FillRect(hdc, &whole, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(pal.toast_text.to_colorref()));
            let old = SelectObject(hdc, HGDIOBJ(paint.fonts.font(FontRole::Surface).0));
            let mut text_rect = RECT {
                left: paint.pad_x,
                top: paint.pad_y,
                right: paint.size.width - paint.pad_x,
                bottom: paint.size.height - paint.pad_y,
            };
            let mut buf = paint.text.clone();
            DrawTextW(hdc, &mut buf, &mut text_rect, DT_WORDBREAK | DT_NOPREFIX);
            SelectObject(hdc, old);
        }
    }
    let _ = unsafe { EndPaint(hwnd, &ps) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let _ = std::panic::catch_unwind(|| draw(hwnd));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
            if win::theme_message(msg, wparam, lparam) {
                let _ = unsafe { InvalidateRect(Some(hwnd), None, true) };
            }
            unsafe { win::default_proc(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_TIMER if wparam.0 == TIMER_ID => {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ID) };
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop_user_data::<PaintCell>(ptr);
                win::default_proc(hwnd, msg, wparam, lparam)
            }
        }
        _ => unsafe { win::default_proc(hwnd, msg, wparam, lparam) },
    }
}
