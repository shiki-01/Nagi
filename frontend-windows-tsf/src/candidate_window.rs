//! 候補ウィンドウ（自前の Win32 ウィンドウ）。描画と位置合わせだけを行い、何を出すか・
//! どこに置くか・寸法は `nagi_windows::candidates` が決める。
//!
//! フォーカスを奪わない（`WS_EX_NOACTIVATE`）最前面のポップアップ。マウス操作は受けない
//! （v1 の非目標。クリックは下のウィンドウに通す）。
//!
//! 見た目（寸法・文字の大きさ・角丸・影・不透明度・配色・番号と注釈・選択の強調）は
//! `nagi_windows::candidate_style::CandidateStyle`（設定 `windows.candidate_*` の実効値。H-119）に従う。
//! 設定は `set_style` で渡し、次の表示から効く。ハイコントラスト時はシステムの色を使い、
//! 半透明・角丸・影は使わない。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use nagi_windows::candidate_style::{CandidateStyle, row_colors};
use nagi_windows::candidates::{
    CandidatePage, LayoutOptions, Point, Rect, Size, WindowLayout, anchor_rect, layout_with,
    place_window_with_gap, scale_px,
};
use nagi_windows::placement::shift_rect;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DT_END_ELLIPSIS, DT_LEFT,
    DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect,
    FrameRect, GetStockObject, HDC, HGDIOBJ, InvalidateRect, NULL_BRUSH, PAINTSTRUCT, PS_SOLID,
    RoundRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DestroyWindow, GWL_EXSTYLE, GWLP_USERDATA, GetWindowLongPtrW,
    HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA, MA_NOACTIVATE, SW_HIDE, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT,
    WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::PCWSTR;

use nagi_windows::candidates::FontRole;
use nagi_windows::theme::Rgb;

use crate::win::{
    self, Fonts, WindowClass, cursor_position, drop_user_data, into_user_data, user_data_ref, wide,
    work_areas,
};

/// 影なしのウィンドウクラス
static CLASS_PLAIN: WindowClass = WindowClass::new("NagiCandidateWindow");
/// 影つき（`CS_DROPSHADOW`）。影はクラスのスタイルなので、影の有無が変わったらウィンドウを作り直す
static CLASS_SHADOW: WindowClass =
    WindowClass::with_style("NagiCandidateWindowShadow", CS_DROPSHADOW.0);

fn class_for(shadow: bool) -> &'static WindowClass {
    if shadow { &CLASS_SHADOW } else { &CLASS_PLAIN }
}

/// 候補 1 行の左のバーの太さ（96 DPI 基準）
const BAR_WIDTH: i32 = 3;

/// 描画に必要なもの（ウィンドウプロシージャから参照する）
struct Paint {
    page: CandidatePage,
    layout: WindowLayout,
    fonts: Fonts,
    /// この表示の実効値（ハイコントラストの調整済み）
    style: CandidateStyle,
    dpi: u32,
}

type PaintCell = RefCell<Option<Paint>>;

/// 候補ウィンドウ。UI スレッドでのみ使う
pub struct CandidateWindow {
    hwnd: Cell<HWND>,
    /// ウィンドウのユーザーデータとして渡したポインタ（`WM_NCDESTROY` で解放される）
    paint: Cell<isize>,
    visible: Cell<bool>,
    /// 位置の確定待ちで出していない最初の表示
    deferred: RefCell<Option<CandidatePage>>,
    /// アプリ別の位置の補正（96 DPI 基準の dp。`nagi_windows::placement`）
    offset: Cell<Point>,
    /// 設定から解決した見た目（次の表示から効く）
    style: Cell<CandidateStyle>,
    /// 今のウィンドウを影つきのクラスで作ったか
    shadow_class: Cell<bool>,
}

impl Default for CandidateWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl CandidateWindow {
    /// 作る（ウィンドウは最初に表示するときに作る）
    pub fn new() -> Self {
        Self {
            hwnd: Cell::new(HWND::default()),
            paint: Cell::new(0),
            visible: Cell::new(false),
            deferred: RefCell::new(None),
            offset: Cell::new(Point::default()),
            style: Cell::new(CandidateStyle::default()),
            shadow_class: Cell::new(false),
        }
    }

    /// 見た目の設定（実効値）を渡す。次の表示から効く
    pub fn set_style(&self, style: CandidateStyle) {
        self.style.set(style);
    }

    /// アプリ別の位置の補正（dp）を設定する。次の表示から効く
    pub fn set_offset(&self, offset: Point) {
        self.offset.set(offset);
    }

    /// ウィンドウを作る（無ければ）。影の有無が今のウィンドウと違えば作り直す
    fn ensure_window(&self, shadow: bool) -> Option<HWND> {
        let current = self.hwnd.get();
        if !current.is_invalid() {
            if self.shadow_class.get() == shadow {
                return Some(current);
            }
            self.destroy();
        }
        let class_info = class_for(shadow);
        if !class_info.acquire(Some(wndproc)) {
            return None;
        }
        let class = class_info.name_wide();
        let created = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                PCWSTR(class.as_ptr()),
                PCWSTR(wide("Nagi candidates").as_ptr()),
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
            class_info.release();
            return None;
        };
        let cell: Box<PaintCell> = Box::new(RefCell::new(None));
        let ptr = into_user_data(cell);
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr) };
        self.paint.set(ptr);
        self.hwnd.set(hwnd);
        self.shadow_class.set(shadow);
        Some(hwnd)
    }

    /// 候補を表示（または更新）する。`caret` はキャレット（composition）の矩形（スクリーン座標）。
    /// 取れないときはマウス位置を使う
    pub fn show(&self, page: &CandidatePage, caret: Option<Rect>) {
        // ハイコントラストはシステムの色を使い、半透明・角丸・影は使わない
        let mut style = self.style.get();
        if win::theme_input().high_contrast.is_some() {
            style = style.for_high_contrast();
        }
        let Some(hwnd) = self.ensure_window(style.has_shadow()) else {
            return;
        };
        self.deferred.borrow_mut().take();
        let anchor = anchor_rect(caret, cursor_position());
        // 先に基準位置へ動かして、そこのモニターの DPI を得る（Per-Monitor 対応のホスト向け）
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                anchor.left,
                anchor.bottom,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            )
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        // アプリ別の補正（キャレット位置を正しく測れないアプリ向け）。反転の判定も補正後の位置で行う
        let anchor = shift_rect(anchor, self.offset.get(), dpi);

        let cell = unsafe { user_data_ref::<PaintCell>(self.paint.get()) };
        let Some(cell) = cell else {
            return;
        };
        // DPI・文字の倍率が変わったときだけフォントを作り直す
        let previous = cell.borrow_mut().take();
        let fonts = match previous {
            Some(p) if p.fonts.matches(dpi, style.font_percent) => p.fonts,
            _ => Fonts::new(dpi, style.font_percent),
        };
        let metrics = style.metrics(dpi);
        let options = LayoutOptions {
            number: style.number,
            annotation: style.annotation,
            page_indicator: style.page_indicator,
        };
        let computed = layout_with(page, &fonts, &metrics, &options);
        let placement = place_window_with_gap(
            anchor,
            computed.size,
            &work_areas(),
            scale_px(style.caret_gap, dpi),
        );
        let Size { width, height } = computed.size;
        *cell.borrow_mut() = Some(Paint {
            page: page.clone(),
            layout: computed,
            fonts,
            style,
            dpi,
        });
        // 角丸（領域の所有権はウィンドウに移る）。0 なら領域を外す
        let corner = scale_px(style.corner_radius, dpi);
        unsafe {
            if corner > 0 {
                let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, corner, corner);
                SetWindowRgn(hwnd, Some(region), false);
            } else {
                SetWindowRgn(hwnd, None, false);
            }
        }
        apply_alpha(hwnd, style.alpha());
        let Point { x, y } = placement.origin;
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        };
        let _ = unsafe { InvalidateRect(Some(hwnd), None, true) };
        self.visible.set(true);
    }

    /// 位置が未確定の最初の表示を保留する（再測定の完了後に `reposition` で出す）
    pub fn defer(&self, page: &CandidatePage) {
        *self.deferred.borrow_mut() = Some(page.clone());
    }

    /// 再測定で基準が更新された後に位置を合わせる。保留中の表示があればそれを出し、
    /// 表示中ならその位置だけ更新する。どちらでもなければ何もしない
    pub fn reposition(&self, caret: Option<Rect>) {
        let deferred = self.deferred.borrow_mut().take();
        let page = deferred.or_else(|| {
            if !self.visible.get() {
                return None;
            }
            let cell = unsafe { user_data_ref::<PaintCell>(self.paint.get()) }?;
            cell.borrow().as_ref().map(|p| p.page.clone())
        });
        if let Some(page) = page {
            self.show(&page, caret);
        }
    }

    /// 隠す
    pub fn hide(&self) {
        self.deferred.borrow_mut().take();
        let hwnd = self.hwnd.get();
        if !hwnd.is_invalid() && self.visible.replace(false) {
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }

    /// ウィンドウを壊す（`Deactivate` 時）
    pub fn destroy(&self) {
        let hwnd = self.hwnd.replace(HWND::default());
        self.visible.set(false);
        if !hwnd.is_invalid() {
            // ユーザーデータは WM_NCDESTROY で解放する
            let _ = unsafe { DestroyWindow(hwnd) };
            class_for(self.shadow_class.get()).release();
        }
        self.paint.set(0);
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn color(c: Rgb) -> COLORREF {
    COLORREF(c.to_colorref())
}

/// 単色で矩形を塗る
fn fill(hdc: HDC, r: &RECT, c: Rgb) {
    unsafe {
        let brush = CreateSolidBrush(color(c));
        FillRect(hdc, r, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

/// 単色で矩形の枠（太さ `thickness` px。内側へ向かって重ねる）を描く
fn frame(hdc: HDC, r: &RECT, c: Rgb, thickness: i32) {
    unsafe {
        let brush = CreateSolidBrush(color(c));
        for i in 0..thickness.max(1) {
            let inner = RECT {
                left: r.left + i,
                top: r.top + i,
                right: r.right - i,
                bottom: r.bottom - i,
            };
            if inner.right > inner.left && inner.bottom > inner.top {
                FrameRect(hdc, &inner, brush);
            }
        }
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

/// 角丸の枠線を描く（中は塗らない）
fn rounded_frame(hdc: HDC, r: &RECT, c: Rgb, thickness: i32, corner: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, thickness.max(1), color(c));
        let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        // GDI の線は太さの中心に引かれるので、内側へ半分寄せる
        let half = thickness.max(1) / 2;
        let _ = RoundRect(
            hdc,
            r.left + half,
            r.top + half,
            r.right - half,
            r.bottom - half,
            corner,
            corner,
        );
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

/// ウィンドウ全体の不透明度。255 ならレイヤードウィンドウにしない
/// （レイヤードにするとクラスの影が付かないため、不透明のときは外す）
fn apply_alpha(hwnd: HWND, alpha: u8) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = (ex as u32) & WS_EX_LAYERED.0 != 0;
        if alpha < 255 {
            if !layered {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
            }
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        } else if layered {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as isize));
        }
    }
}

fn rect_of(r: Rect) -> RECT {
    win::from_rect(r)
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
        let style = &paint.style;
        let dpi = paint.dpi;
        let size = paint.layout.size;
        let whole = RECT {
            left: 0,
            top: 0,
            right: size.width,
            bottom: size.height,
        };
        // 配色はテーマの変更（ライト/ダーク・ハイコントラスト）に追従するため、描くたびに決める
        let pal = style.palette(win::theme_input());
        let border = style.border_px(dpi);
        let corner = scale_px(style.corner_radius, dpi);
        fill(hdc, &whole, pal.background);
        if corner > 0 {
            rounded_frame(hdc, &whole, pal.border, border, corner);
        } else {
            frame(hdc, &whole, pal.border, border);
        }
        unsafe { SetBkMode(hdc, TRANSPARENT) };
        let text_flags = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX;
        let draw_text = |text: &str, role: FontRole, mut rect: RECT, align, col| {
            let mut buf: Vec<u16> = text.encode_utf16().collect();
            unsafe {
                let old = SelectObject(hdc, HGDIOBJ(paint.fonts.font(role).0));
                SetTextColor(hdc, col);
                DrawTextW(hdc, &mut buf, &mut rect, text_flags | align);
                SelectObject(hdc, old);
            }
        };
        let lay = &paint.layout;
        let padding = scale_px(style.padding, dpi);
        for (row, rect) in paint.page.rows.iter().zip(&lay.row_rects) {
            let r = rect_of(*rect);
            let colors = row_colors(style.selection, row.is_selected, &pal);
            if let Some(c) = colors.fill {
                fill(hdc, &r, c);
            }
            if let Some(c) = colors.bar {
                let bar = RECT {
                    right: r.left + scale_px(BAR_WIDTH, dpi).max(2),
                    ..r
                };
                fill(hdc, &bar, c);
            }
            if let Some(c) = colors.outline {
                frame(hdc, &r, c, scale_px(2, dpi).max(1));
            }
            let (main, sub) = (color(colors.main), color(colors.sub));
            let from_x = |x: i32| RECT { left: x, ..r };
            if let Some(number_x) = lay.number_x {
                draw_text(
                    &row.number.to_string(),
                    FontRole::Number,
                    from_x(number_x),
                    DT_LEFT,
                    sub,
                );
            }
            // 候補が最大幅を超えるときは末尾を「…」で省略する
            let surface_rect = RECT {
                left: lay.surface_x,
                right: lay.surface_x + lay.surface_width,
                ..r
            };
            draw_text(
                &row.surface,
                FontRole::Surface,
                surface_rect,
                DT_LEFT | DT_END_ELLIPSIS,
                main,
            );
            if let Some(label_x) = lay.label_x {
                let label = row.annotation_text(style.annotation);
                if !label.is_empty() {
                    draw_text(&label, FontRole::Label, from_x(label_x), DT_LEFT, sub);
                }
            }
        }
        if let (Some(footer), Some(text)) = (lay.footer_rect, paint.page.footer_text()) {
            let mut r = rect_of(footer);
            r.right -= padding;
            draw_text(&text, FontRole::Label, r, DT_RIGHT, color(pal.sub_text));
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
        // ライト/ダーク・ハイコントラストの切替に追従する（トップレベルなので受け取れる）
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
            if win::theme_message(msg, wparam, lparam) {
                let _ = unsafe { InvalidateRect(Some(hwnd), None, true) };
            }
            unsafe { win::default_proc(hwnd, msg, wparam, lparam) }
        }
        // フォーカスもクリックも奪わない
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
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
