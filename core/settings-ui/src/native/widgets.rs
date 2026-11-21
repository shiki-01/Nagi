//! 自前で描くコントロール: トグルスイッチ（チェックボックス）・ボタン・サイドバー・ブランド・診断の結果のバッジ・
//! ステータス・カードの背景。色と形は OS 非依存の `native_theme`、パレットは `theme`。
//!
//! **ちらつき対策**: 部品はすべてメモリ上のバッファに描いてから 1 回の転送で画面に出す（[`buffered`]）。
//! 標準の `STATIC` / `LISTBOX` も `WM_ERASEBKGND`（背景だけを先に塗る → 文字を重ねる、の 2 段階が見える）を
//! 止めてサブクラスで描く。
//!
//! チェックボックスとボタンは **標準の `BUTTON` クラスのまま**（`BS_AUTOCHECKBOX` / `BS_PUSHBUTTON`）で、
//! サブクラス化して `WM_PAINT` だけを自前で描く。役割（チェックボックス・ボタン）・名前・状態（チェック）・
//! キーボード操作（Space・Tab）は標準のコントロールのままなので、読み上げ・自動操作の挙動が変わらない。

use super::controls::get_text;
use super::dpi::Fonts;
use super::gfx::{Canvas, buffered, fill_rect};
use super::state::State;
use super::theme::{self, Surface};
use crate::native_diag::BadgeKind;
use crate::native_layout::{self as layout, Rect, scale};
use crate::native_model::ids;
use crate::native_theme::{
    BADGE_GAP, BADGE_RADIUS, BADGE_W, BUTTON_RADIUS, ButtonStyle, CARD_RADIUS, NAV_RADIUS, Palette,
    Rgb, StatusKind, radius_px, toggle_geometry,
};
use std::cell::Cell;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    DT_WORDBREAK, DrawTextW, EndPaint, FillRect, GetTextMetricsW, HBRUSH, HDC, HFONT, HGDIOBJ,
    InvalidateRect, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::SystemServices::{SS_CENTERIMAGE, SS_ENDELLIPSIS};
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, ODA_DRAWENTIRE, ODS_FOCUS, ODS_SELECTED, ODT_LISTBOX, ODT_STATIC, WM_MOUSELEAVE,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, IsWindowEnabled, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_GETSTATE, BM_SETCHECK, BM_SETSTATE, BST_PUSHED, DI_NORMAL, DefWindowProcW,
    DrawIconEx, GWL_STYLE, GetClientRect, GetDlgCtrlID, GetParent, GetWindowLongPtrW, HICON,
    IMAGE_ICON, LB_GETCARETINDEX, LB_GETCOUNT, LB_GETCURSEL, LB_GETITEMRECT, LB_GETTOPINDEX,
    LB_ITEMFROMPOINT, LR_SHARED, LoadImageW, SendMessageW, UISF_HIDEFOCUS, WM_CTLCOLORSTATIC,
    WM_DRAWITEM, WM_ENABLE, WM_ERASEBKGND, WM_GETFONT, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_QUERYUISTATE, WM_SETFOCUS, WM_SETTEXT,
    WM_UPDATEUISTATE,
};

/// ボタンのサブクラスの識別（コンボボックスのサブクラスとは別の `(関数, ID)` の組なので衝突しない）
const BUTTON_SUBCLASS_ID: usize = 0x4E41_4731;
/// サイドバーのサブクラスの識別
const LIST_SUBCLASS_ID: usize = 0x4E41_4732;
/// 文字・オーナードローの STATIC のサブクラスの識別
const STATIC_SUBCLASS_ID: usize = 0x4E41_4733;

thread_local! {
    /// 今マウスが乗っているボタン（0 = なし）
    static HOT_BUTTON: Cell<isize> = const { Cell::new(0) };
    /// 今マウスが乗っているサイドバーの項目（-1 = なし）
    static HOT_NAV: Cell<i32> = const { Cell::new(-1) };
}

fn rect_of(rc: &RECT) -> Rect {
    Rect {
        x: rc.left,
        y: rc.top,
        w: rc.right - rc.left,
        h: rc.bottom - rc.top,
    }
}

fn win_rect(r: Rect) -> RECT {
    RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    }
}

fn hwnd_key(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

// ---- 文字 ----

fn with_font<R>(hdc: HDC, font: Option<HFONT>, f: impl FnOnce() -> R) -> R {
    // SAFETY: 選んだフォントは戻す
    let old = font.map(|f| unsafe { SelectObject(hdc, HGDIOBJ(f.0)) });
    let out = f();
    if let Some(old) = old {
        unsafe { SelectObject(hdc, old) };
    }
    out
}

/// 文字を描く（背景は塗らない）
fn draw_text(hdc: HDC, font: Option<HFONT>, text: &str, r: Rect, color: Rgb, flags: u32) {
    if text.is_empty() || r.w <= 0 || r.h <= 0 {
        return;
    }
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    let mut rc = win_rect(r);
    with_font(hdc, font, || unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(color.colorref()));
        DrawTextW(
            hdc,
            &mut buf,
            &mut rc,
            windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(flags),
        );
    });
}

/// 1 行の文字の幅（px）
fn text_width(hdc: HDC, font: Option<HFONT>, text: &str) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    let mut rc = RECT::default();
    with_font(hdc, font, || unsafe {
        DrawTextW(
            hdc,
            &mut buf,
            &mut rc,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
    });
    rc.right - rc.left
}

/// 1 行の文字の高さ（px）
fn text_height(hdc: HDC, font: Option<HFONT>) -> i32 {
    let mut tm = TEXTMETRICW::default();
    with_font(hdc, font, || unsafe {
        let _ = GetTextMetricsW(hdc, &mut tm);
    });
    tm.tmHeight.max(1)
}

const SINGLE: u32 = DT_SINGLELINE.0 | DT_VCENTER.0 | DT_NOPREFIX.0 | DT_END_ELLIPSIS.0;

// ---- ボタン・トグル ----

/// サブクラス化したボタンの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BtnKind {
    /// チェックボックスをトグルスイッチとして描く
    Toggle,
    /// 押しボタン
    Push(ButtonStyle),
}

fn encode(kind: BtnKind, on_card: bool) -> usize {
    let k = match kind {
        BtnKind::Toggle => 0,
        BtnKind::Push(ButtonStyle::Normal) => 1,
        BtnKind::Push(ButtonStyle::Primary) => 2,
        BtnKind::Push(ButtonStyle::Danger) => 3,
        BtnKind::Push(ButtonStyle::Subtle) => 4,
    };
    k | (usize::from(on_card) << 8)
}

fn decode(data: usize) -> (BtnKind, bool) {
    let kind = match data & 0xFF {
        0 => BtnKind::Toggle,
        2 => BtnKind::Push(ButtonStyle::Primary),
        3 => BtnKind::Push(ButtonStyle::Danger),
        4 => BtnKind::Push(ButtonStyle::Subtle),
        _ => BtnKind::Push(ButtonStyle::Normal),
    };
    (kind, data & 0x100 != 0)
}

/// ボタン（`BUTTON`）を自前の描画にする。`on_card` はカードの上（偽ならサイドバー・フッターの地）
pub(super) fn install_button(hwnd: HWND, kind: BtnKind, on_card: bool) {
    // SAFETY: `button_proc` は 'static。ウィンドウの破棄（WM_NCDESTROY）で外す
    let _ = unsafe {
        SetWindowSubclass(
            hwnd,
            Some(button_proc),
            BUTTON_SUBCLASS_ID,
            encode(kind, on_card),
        )
    };
}

fn invalidate(hwnd: HWND) {
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn track_leave(hwnd: HWND) {
    let mut track = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    let _ = unsafe { TrackMouseEvent(&mut track) };
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let (kind, on_card) = decode(data);
            paint_button(hwnd, kind, on_card);
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEMOVE => {
            if HOT_BUTTON.with(|h| h.replace(hwnd_key(hwnd))) != hwnd_key(hwnd) {
                track_leave(hwnd);
                invalidate(hwnd);
            }
        }
        WM_MOUSELEAVE => {
            HOT_BUTTON.with(|h| {
                if h.get() == hwnd_key(hwnd) {
                    h.set(0);
                }
            });
            invalidate(hwnd);
        }
        WM_NCDESTROY => {
            HOT_BUTTON.with(|h| {
                if h.get() == hwnd_key(hwnd) {
                    h.set(0);
                }
            });
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(button_proc), BUTTON_SUBCLASS_ID) };
        }
        _ => {}
    }
    let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    // 見た目に関わる状態が変わったら描き直す（標準の描画は使わないので自分で要求する）
    if matches!(
        msg,
        WM_SETFOCUS
            | WM_KILLFOCUS
            | WM_ENABLE
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | BM_SETCHECK
            | BM_SETSTATE
            | WM_UPDATEUISTATE
    ) {
        invalidate(hwnd);
    }
    result
}

struct ButtonState {
    hot: bool,
    pushed: bool,
    focused: bool,
    disabled: bool,
    checked: bool,
}

fn paint_button(hwnd: HWND, kind: BtnKind, on_card: bool) {
    let mut ps = PAINTSTRUCT::default();
    let target = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut rc = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    let r = rect_of(&rc);
    let dpi = match unsafe { GetDpiForWindow(hwnd) } {
        0 => 96,
        d => d,
    };
    let send = |m: u32| unsafe { SendMessageW(hwnd, m, None, None) }.0;
    let focused = unsafe { GetFocus() } == hwnd
        // キーボードで操作していない間は、フォーカスの輪郭を隠す（標準のボタンと同じ）
        && (send(WM_QUERYUISTATE) as u32 & UISF_HIDEFOCUS) == 0;
    let state = ButtonState {
        hot: HOT_BUTTON.with(Cell::get) == hwnd_key(hwnd),
        pushed: send(BM_GETSTATE) as u32 & BST_PUSHED != 0,
        focused,
        disabled: !unsafe { IsWindowEnabled(hwnd) }.as_bool(),
        checked: send(BM_GETCHECK) == 1,
    };
    let font = {
        let f = send(WM_GETFONT);
        (f != 0).then(|| HFONT(std::ptr::without_provenance_mut(f as usize)))
    };
    let text = get_text(hwnd);
    buffered(target, r.w, r.h, |hdc| {
        let pal = theme::palette();
        let bg = theme::surface_rgb(if on_card {
            Surface::Card
        } else {
            Surface::Sidebar
        });
        fill_rect(hdc, Rect { x: 0, y: 0, ..r }, bg);
        let canvas = Canvas::new(hdc);
        let area = Rect { x: 0, y: 0, ..r };
        match kind {
            BtnKind::Toggle => draw_toggle(hdc, &canvas, &pal, area, &text, font, &state, dpi),
            BtnKind::Push(style) => {
                draw_push(hdc, &canvas, &pal, area, &text, font, style, &state, dpi)
            }
        }
    });
    let _ = unsafe { EndPaint(hwnd, &ps) };
}

#[allow(clippy::too_many_arguments)]
fn draw_toggle(
    hdc: HDC,
    canvas: &Canvas,
    pal: &Palette,
    area: Rect,
    text: &str,
    font: Option<HFONT>,
    st: &ButtonState,
    dpi: u32,
) {
    let text_h = text_height(hdc, font);
    let g = toggle_geometry(area, text_h, st.checked, st.hot || st.pushed, dpi);
    let line = scale(1, dpi).max(1);
    let accent = if st.pushed {
        pal.accent_pressed
    } else if st.hot {
        pal.accent_hover
    } else {
        pal.accent
    };
    let (track_fill, track_line, knob) = if st.disabled {
        let c = theme::rgb(pal.disabled_text);
        (st.checked.then_some(c), c, c)
    } else if st.checked {
        (
            Some(theme::rgb(accent)),
            theme::rgb(accent),
            theme::rgb(pal.on_accent),
        )
    } else {
        let c = theme::rgb(pal.toggle_off);
        (None, c, c)
    };
    let radius = g.track.h / 2;
    if let Some(fill) = track_fill {
        canvas.fill_round(g.track, radius, fill);
    }
    // オフのときは枠線（HC は太く）。オンのときの枠線は塗りと同じ色
    let width = if pal.decorative { line } else { line * 2 };
    canvas.stroke_round(g.track, radius, track_line, width);
    canvas.fill_ellipse(g.knob, knob);
    // 文字: 1 行目をスイッチの中央に合わせる
    let line_h = text_h.max(g.track.h);
    let color = theme::rgb(if st.disabled {
        pal.disabled_text
    } else {
        pal.text
    });
    let text_rect = Rect {
        x: g.text_x,
        y: area.y + (line_h - text_h) / 2,
        w: (area.x + area.w - g.text_x).max(0),
        h: (area.h - (line_h - text_h) / 2).max(0),
    };
    draw_text(
        hdc,
        font,
        text,
        text_rect,
        color,
        DT_WORDBREAK.0 | DT_NOPREFIX.0,
    );
    if st.focused {
        let inset = scale(1, dpi);
        let ring = Rect {
            x: area.x + inset,
            y: area.y + inset,
            w: area.w - 2 * inset,
            h: area.h - 2 * inset,
        };
        canvas.stroke_round(
            ring,
            radius_px(BUTTON_RADIUS, dpi, pal.decorative),
            theme::rgb(pal.focus),
            scale(2, dpi).max(1),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_push(
    hdc: HDC,
    canvas: &Canvas,
    pal: &Palette,
    area: Rect,
    text: &str,
    font: Option<HFONT>,
    style: ButtonStyle,
    st: &ButtonState,
    dpi: u32,
) {
    let radius = radius_px(BUTTON_RADIUS, dpi, pal.decorative);
    let idx = if st.pushed { 2 } else { usize::from(st.hot) };
    let active = st.hot || st.pushed;
    let line = scale(1, dpi).max(1);
    let rgb = theme::rgb;
    // (塗り, 枠, 文字)
    let (fill, border, color) = if st.disabled {
        (
            (style != ButtonStyle::Subtle).then(|| rgb(pal.button_bg[0])),
            (style != ButtonStyle::Subtle).then(|| rgb(pal.divider)),
            rgb(pal.disabled_text),
        )
    } else if !pal.decorative {
        // ハイコントラスト: システム色のみ。ホバー・押下は選択色の反転
        if active {
            (
                Some(rgb(pal.accent)),
                Some(rgb(pal.button_border)),
                rgb(pal.on_accent),
            )
        } else {
            (
                Some(rgb(pal.button_bg[0])),
                Some(rgb(pal.button_border)),
                rgb(pal.button_text),
            )
        }
    } else {
        match style {
            ButtonStyle::Primary => {
                let accent = [pal.accent, pal.accent_hover, pal.accent_pressed][idx];
                (Some(rgb(accent)), None, rgb(pal.on_accent))
            }
            ButtonStyle::Danger => (
                Some(rgb(pal.button_bg[idx])),
                Some(rgb(pal.danger)),
                rgb(pal.danger),
            ),
            ButtonStyle::Subtle => (
                active.then(|| rgb(pal.button_bg[idx])),
                None,
                rgb(pal.accent_text),
            ),
            ButtonStyle::Normal => (
                Some(rgb(pal.button_bg[idx])),
                Some(rgb(pal.button_border)),
                rgb(pal.button_text),
            ),
        }
    };
    if let Some(fill) = fill {
        canvas.fill_round(area, radius, fill);
    }
    if let Some(border) = border {
        canvas.stroke_round(area, radius, border, line);
    }
    if st.focused {
        canvas.stroke_round(area, radius, rgb(pal.focus), scale(2, dpi).max(1));
    }
    let pad = scale(8, dpi);
    let text_rect = Rect {
        x: area.x + pad,
        y: area.y,
        w: (area.w - 2 * pad).max(0),
        h: area.h,
    };
    draw_text(hdc, font, text, text_rect, color, SINGLE | DT_CENTER.0);
}

// ---- サイドバー ----

/// サイドバーの `LISTBOX` を自前の描画にする（地の塗り・ホバーの追跡）
pub(super) fn install_list(hwnd: HWND) {
    // SAFETY: `list_proc` は 'static。ウィンドウの破棄で外す
    let _ = unsafe { SetWindowSubclass(hwnd, Some(list_proc), LIST_SUBCLASS_ID, 0) };
}

unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        // 背景は塗らない（先に地だけが見えて、次に項目が描かれる、というちらつきになる）。
        // 全体を `WM_PAINT` で、バッファにまとめて描く
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            paint_list(hwnd);
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            // 下位ワード = 項目の添字、上位ワードが 0 なら領域の内側
            let r = unsafe { SendMessageW(hwnd, LB_ITEMFROMPOINT, None, Some(lparam)) }.0;
            let mut index = if (r >> 16) & 0xFFFF == 0 {
                (r & 0xFFFF) as i32
            } else {
                -1
            };
            if index >= 0 {
                // 項目の下の空き領域は「最も近い項目」が返るので、実際に項目の上にあるときだけ採る
                let mut item = RECT::default();
                let got = unsafe {
                    SendMessageW(
                        hwnd,
                        LB_GETITEMRECT,
                        Some(WPARAM(index as usize)),
                        Some(LPARAM(std::ptr::from_mut(&mut item) as isize)),
                    )
                }
                .0;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
                if got < 0 || y < item.top || y >= item.bottom {
                    index = -1;
                }
            }
            if HOT_NAV.with(|h| h.replace(index)) != index {
                track_leave(hwnd);
                invalidate(hwnd);
            }
        }
        WM_MOUSELEAVE => {
            HOT_NAV.with(|h| h.set(-1));
            invalidate(hwnd);
        }
        WM_NCDESTROY => {
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(list_proc), LIST_SUBCLASS_ID) };
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// サイドバーを描く: 地を塗り、見えている項目ごとに親へ `WM_DRAWITEM` を送る（描く先はバッファ）
fn paint_list(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let target = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut rc = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    let r = rect_of(&rc);
    let parent = unsafe { GetParent(hwnd) }.unwrap_or_default();
    let send = |m: u32, w: usize| unsafe { SendMessageW(hwnd, m, Some(WPARAM(w)), None) }.0;
    let count = send(LB_GETCOUNT, 0) as i32;
    let top = send(LB_GETTOPINDEX, 0) as i32;
    let current = send(LB_GETCURSEL, 0) as i32;
    let caret = send(LB_GETCARETINDEX, 0) as i32;
    // キーボードで操作していない間は、フォーカスの輪郭を隠す（標準と同じ）
    let show_focus = unsafe { GetFocus() } == hwnd
        && (unsafe { SendMessageW(hwnd, WM_QUERYUISTATE, None, None) }.0 as u32 & UISF_HIDEFOCUS)
            == 0;
    buffered(target, r.w, r.h, |hdc| {
        fill_rect(
            hdc,
            Rect { x: 0, y: 0, ..r },
            theme::surface_rgb(Surface::Sidebar),
        );
        for index in top.max(0)..count {
            let mut item = RECT::default();
            let got = unsafe {
                SendMessageW(
                    hwnd,
                    LB_GETITEMRECT,
                    Some(WPARAM(index as usize)),
                    Some(LPARAM(std::ptr::from_mut(&mut item) as isize)),
                )
            }
            .0;
            if got < 0 || item.top >= rc.bottom {
                break;
            }
            let mut state = 0;
            if index == current {
                state |= ODS_SELECTED.0;
            }
            if show_focus && index == caret {
                state |= ODS_FOCUS.0;
            }
            let dis = DRAWITEMSTRUCT {
                CtlType: ODT_LISTBOX,
                CtlID: ids::LIST as u32,
                itemID: index as u32,
                itemAction: ODA_DRAWENTIRE,
                itemState: windows::Win32::UI::Controls::ODS_FLAGS(state),
                hwndItem: hwnd,
                hDC: hdc,
                rcItem: item,
                itemData: 0,
            };
            unsafe {
                SendMessageW(
                    parent,
                    WM_DRAWITEM,
                    Some(WPARAM(ids::LIST as usize)),
                    Some(LPARAM(std::ptr::from_ref(&dis) as isize)),
                )
            };
        }
    });
    let _ = unsafe { EndPaint(hwnd, &ps) };
}

/// マウスが乗っているサイドバーの項目の添字
pub(super) fn hot_nav() -> i32 {
    HOT_NAV.with(Cell::get)
}

/// サイドバー 1 項目の内容
pub(super) struct NavItem<'a> {
    pub text: &'a str,
    pub glyph: Option<char>,
    pub selected: bool,
    pub hot: bool,
    pub focused: bool,
}

/// サイドバーの 1 項目を描く。選択は丸みのある帯 + 左端のアクセントの印、ホバーは薄い帯。
/// ハイコントラストでは選択色の反転だけ（角丸・印なし）
pub(super) fn draw_nav_item(hdc: HDC, item: Rect, dpi: u32, fonts: &Fonts, nav: &NavItem) {
    let pal = theme::palette();
    fill_rect(hdc, item, theme::surface_rgb(Surface::Sidebar));
    let parts = layout::sidebar_item_parts(item, dpi);
    let canvas = Canvas::new(hdc);
    let radius = radius_px(NAV_RADIUS, dpi, pal.decorative);
    let mut color = pal.text;
    if nav.selected {
        canvas.fill_round(parts.fill, radius, theme::rgb(pal.nav_selected_bg));
        color = pal.nav_selected_text;
        if pal.decorative {
            canvas.fill_round(parts.bar, parts.bar.w / 2, theme::rgb(pal.accent));
        }
    } else if nav.hot {
        canvas.fill_round(parts.fill, radius, theme::rgb(pal.nav_hover_bg));
    }
    let color = theme::rgb(color);
    let text_x = match (nav.glyph, fonts.icon) {
        (Some(glyph), Some(icon)) => {
            let mut buf = [0u16; 2];
            let s = glyph.encode_utf16(&mut buf);
            draw_text(
                hdc,
                Some(icon),
                &String::from_utf16_lossy(s),
                parts.icon,
                if nav.selected && pal.decorative {
                    theme::rgb(pal.accent_text)
                } else {
                    color
                },
                DT_SINGLELINE.0 | DT_VCENTER.0 | DT_CENTER.0 | DT_NOPREFIX.0,
            );
            parts.text_x
        }
        _ => parts.text_x_no_icon,
    };
    let text_rect = Rect {
        x: text_x,
        y: parts.fill.y,
        w: (parts.fill.x + parts.fill.w - text_x).max(0),
        h: parts.fill.h,
    };
    draw_text(hdc, Some(fonts.normal), nav.text, text_rect, color, SINGLE);
    if nav.focused {
        canvas.stroke_round(
            parts.fill,
            radius,
            theme::rgb(pal.focus),
            scale(2, dpi).max(1),
        );
    }
}

// ---- STATIC（文字・オーナードロー）----

/// `STATIC` を、バッファに描くサブクラスにする。`owner_draw` は `SS_OWNERDRAW`（親へ `WM_DRAWITEM` を
/// 送って描かせる）。そうでなければ文字の STATIC（背景と文字色は親の `WM_CTLCOLORSTATIC` が決める）
pub(super) fn install_static(hwnd: HWND, owner_draw: bool) {
    // SAFETY: `static_proc` は 'static。ウィンドウの破棄（WM_NCDESTROY）で外す
    let _ = unsafe {
        SetWindowSubclass(
            hwnd,
            Some(static_proc),
            STATIC_SUBCLASS_ID,
            usize::from(owner_draw),
        )
    };
}

unsafe extern "system" fn static_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    match msg {
        // 背景だけを先に塗らない（ちらつきの原因）。`WM_PAINT` が全面を描く
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            paint_static(hwnd, data != 0);
            return LRESULT(0);
        }
        // 標準の STATIC は文字が変わると自分で直接描き直す（バッファを通らない）ので、文字の保存だけを
        // 既定の処理に任せ、描き直しは `WM_PAINT` に回す
        WM_SETTEXT => {
            let result = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            invalidate(hwnd);
            return result;
        }
        WM_NCDESTROY => {
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(static_proc), STATIC_SUBCLASS_ID) };
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

fn paint_static(hwnd: HWND, owner_draw: bool) {
    let mut ps = PAINTSTRUCT::default();
    let target = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut rc = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    let r = rect_of(&rc);
    let parent = unsafe { GetParent(hwnd) }.unwrap_or_default();
    buffered(target, r.w, r.h, |hdc| {
        if owner_draw {
            let id = unsafe { GetDlgCtrlID(hwnd) };
            let dis = DRAWITEMSTRUCT {
                CtlType: ODT_STATIC,
                CtlID: id as u32,
                itemID: 0,
                itemAction: ODA_DRAWENTIRE,
                itemState: windows::Win32::UI::Controls::ODS_FLAGS(0),
                hwndItem: hwnd,
                hDC: hdc,
                rcItem: rc,
                itemData: 0,
            };
            unsafe {
                SendMessageW(
                    parent,
                    WM_DRAWITEM,
                    Some(WPARAM(id as usize)),
                    Some(LPARAM(std::ptr::from_ref(&dis) as isize)),
                )
            };
            return;
        }
        // 背景のブラシ・文字色は親に問い合わせる（標準の STATIC と同じ手順。文字色・透過は `hdc` に設定される）
        let brush = unsafe {
            SendMessageW(
                parent,
                WM_CTLCOLORSTATIC,
                Some(WPARAM(hdc.0 as usize)),
                Some(LPARAM(hwnd.0 as isize)),
            )
        }
        .0;
        if brush != 0 {
            let b = HBRUSH(std::ptr::without_provenance_mut(brush as usize));
            unsafe {
                FillRect(
                    hdc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: r.w,
                        bottom: r.h,
                    },
                    b,
                )
            };
        }
        let font = {
            let f = unsafe { SendMessageW(hwnd, WM_GETFONT, None, None) }.0;
            (f != 0).then(|| HFONT(std::ptr::without_provenance_mut(f as usize)))
        };
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        let mut flags = DT_NOPREFIX.0;
        if style & SS_CENTERIMAGE.0 != 0 {
            flags |= DT_SINGLELINE.0 | DT_VCENTER.0;
        } else {
            flags |= DT_WORDBREAK.0;
        }
        if style & SS_ENDELLIPSIS.0 != 0 {
            flags |= DT_END_ELLIPSIS.0;
        }
        let text = get_text(hwnd);
        if text.is_empty() {
            return;
        }
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        let mut area = win_rect(Rect { x: 0, y: 0, ..r });
        with_font(hdc, font, || unsafe {
            DrawTextW(
                hdc,
                &mut buf,
                &mut area,
                windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(flags),
            );
        });
    });
    let _ = unsafe { EndPaint(hwnd, &ps) };
}

// ---- ブランド ----

/// exe に埋め込んだ Nagi のロゴ（リソース ID 1）を `px` 四方で読む。共有アイコンなので破棄しない。
/// 埋め込まれていなければ `None`（ロゴを描かない）
fn logo_icon(px: i32) -> Option<HICON> {
    // MAKEINTRESOURCE(1)
    let id = windows::core::PCWSTR(std::ptr::without_provenance::<u16>(1));
    let handle = unsafe {
        LoadImageW(
            Some(super::controls::instance()),
            id,
            IMAGE_ICON,
            px,
            px,
            LR_SHARED,
        )
    }
    .ok()?;
    Some(HICON(handle.0))
}

/// サイドバー上部のブランド領域: Nagi のロゴ + ワードマーク + 補足（「設定 v…」）
pub(super) fn draw_brand(hdc: HDC, area: Rect, dpi: u32, fonts: &Fonts, caption: &str) {
    let pal = theme::palette();
    fill_rect(hdc, area, theme::surface_rgb(Surface::Sidebar));
    let parts = layout::brand_parts(Rect { x: 0, y: 0, ..area }, dpi);
    if let Some(icon) = logo_icon(parts.icon.w) {
        // SAFETY: `icon` は有効な共有アイコン。`hdc` は描画中の有効な DC
        let _ = unsafe {
            DrawIconEx(
                hdc,
                parts.icon.x,
                parts.icon.y,
                icon,
                parts.icon.w,
                parts.icon.h,
                0,
                None,
                DI_NORMAL,
            )
        };
    }
    draw_text(
        hdc,
        Some(fonts.section),
        "Nagi",
        parts.title,
        theme::rgb(pal.text),
        SINGLE,
    );
    draw_text(
        hdc,
        Some(fonts.normal),
        caption,
        parts.caption,
        theme::rgb(pal.text_secondary),
        SINGLE,
    );
}

// ---- 線・バッジ・ステータス ----

/// 区切り線（オーナードローの STATIC）
pub(super) fn draw_rule(hdc: HDC, r: Rect) {
    let pal = theme::palette();
    fill_rect(hdc, r, theme::rgb(pal.divider));
}

fn draw_badge_pill(
    hdc: HDC,
    canvas: &Canvas,
    pal: &Palette,
    pill: Rect,
    badge: BadgeKind,
    dpi: u32,
) -> Rgb {
    let colors = pal.badge(badge);
    let (fg, bg) = (theme::rgb(colors.fg), theme::rgb(colors.bg));
    let radius = radius_px(BADGE_RADIUS, dpi, pal.decorative);
    canvas.fill_round(pill, radius, bg);
    if !pal.decorative {
        // 色が無くても枠で札と分かる
        canvas.stroke_round(pill, 0, fg, 1);
    }
    let _ = hdc;
    fg
}

/// 診断の結果行: 左に状態のバッジ（文字つき）、右に項目名
pub(super) fn draw_result(
    hdc: HDC,
    item: Rect,
    dpi: u32,
    font: Option<HFONT>,
    badge: BadgeKind,
    label: &str,
    title: &str,
) {
    let pal = theme::palette();
    fill_rect(hdc, item, theme::surface_rgb(Surface::Card));
    let canvas = Canvas::new(hdc);
    let badge_h = (text_height(hdc, font) + 2 * scale(3, dpi)).min(item.h);
    let pill = Rect {
        x: item.x,
        y: item.y + (item.h - badge_h) / 2,
        w: scale(BADGE_W, dpi).min(item.w),
        h: badge_h,
    };
    let fg = draw_badge_pill(hdc, &canvas, &pal, pill, badge, dpi);
    draw_text(hdc, font, label, pill, fg, SINGLE | DT_CENTER.0);
    let title_x = pill.x + pill.w + scale(BADGE_GAP, dpi);
    let title_rect = Rect {
        x: title_x,
        y: item.y,
        w: (item.x + item.w - title_x).max(0),
        h: item.h,
    };
    draw_text(hdc, font, title, title_rect, theme::rgb(pal.text), SINGLE);
}

/// フッターのステータス: 種類で色分けした札（アイコン + 文言）。文言が空なら何も描かない
pub(super) fn draw_status(
    hdc: HDC,
    item: Rect,
    dpi: u32,
    fonts: &Fonts,
    kind: StatusKind,
    text: &str,
) {
    let pal = theme::palette();
    fill_rect(hdc, item, theme::surface_rgb(Surface::Sidebar));
    if text.trim().is_empty() {
        return;
    }
    let canvas = Canvas::new(hdc);
    let pad = scale(10, dpi);
    let icon = fonts.icon.map(|f| (f, kind.glyph()));
    let icon_w = icon.map_or(0, |_| scale(16, dpi) + scale(6, dpi));
    let text_w = text_width(hdc, Some(fonts.normal), text);
    let v = scale(2, dpi);
    let pill = Rect {
        x: item.x,
        y: item.y + v,
        w: (pad + icon_w + text_w + pad).min(item.w),
        h: (item.h - 2 * v).max(0),
    };
    let fg = draw_badge_pill(hdc, &canvas, &pal, pill, kind.badge(), dpi);
    let mut x = pill.x + pad;
    if let Some((font, glyph)) = icon {
        let mut buf = [0u16; 2];
        let s = String::from_utf16_lossy(glyph.encode_utf16(&mut buf));
        let r = Rect {
            x,
            y: pill.y,
            w: scale(16, dpi),
            h: pill.h,
        };
        draw_text(
            hdc,
            Some(font),
            &s,
            r,
            fg,
            DT_SINGLELINE.0 | DT_VCENTER.0 | DT_CENTER.0 | DT_NOPREFIX.0,
        );
        x += icon_w;
    }
    let r = Rect {
        x,
        y: pill.y,
        w: (pill.x + pill.w - pad - x).max(0),
        h: pill.h,
    };
    draw_text(hdc, Some(fonts.normal), text, r, fg, SINGLE);
}

// ---- パネル（カードの背景） ----

/// カードのやわらかい影: 少しずつ大きく・薄くした角丸を、下へずらして重ねる（カードの背景はこの上に塗る）
fn draw_card_shadow(canvas: &Canvas, card: Rect, radius: i32, dpi: u32, alpha: u8) {
    let black = Rgb(0, 0, 0);
    // (広がり dp, 下へのずれ dp, 不透明度の割合 /8)
    for (grow, dy, weight) in [(3, 2, 2u32), (2, 2, 3), (1, 1, 4), (0, 1, 8)] {
        let g = scale(grow, dpi);
        let a = (u32::from(alpha) * weight / 8).min(255) as u8;
        canvas.fill_round_alpha(
            Rect {
                x: card.x - g,
                y: card.y - g + scale(dy, dpi),
                w: card.w + 2 * g,
                h: card.h + 2 * g,
            },
            radius + g,
            black,
            a,
        );
    }
}

/// パネルの `WM_PAINT`: ページの地・カード（薄い枠 + 角丸）・カード内の項目の区切り線。
/// 部品（STATIC など）は子ウィンドウが自分で描くので、ここは背景だけ
pub(super) fn paint_panel(st: &State, hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let target = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut rc = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    let r = rect_of(&rc);
    let dpi = st.dpi.get();
    // 描画中に借用が重なったら（再入）カード無しで描く。次の再描画で直る
    let (cards, dividers, hero, markers, scroll) = match st.ui.try_borrow() {
        Ok(ui) => match &ui.layout {
            Some(l) => (
                l.cards.clone(),
                l.dividers.clone(),
                l.hero_bar_y,
                l.markers.clone(),
                ui.scroll,
            ),
            None => (Vec::new(), Vec::new(), None, Vec::new(), 0),
        },
        Err(_) => (Vec::new(), Vec::new(), None, Vec::new(), 0),
    };
    buffered(target, r.w, r.h, |hdc| {
        let pal = theme::palette();
        fill_rect(
            hdc,
            Rect { x: 0, y: 0, ..r },
            theme::surface_rgb(Surface::Page),
        );
        let cols = layout::columns(r.w, dpi);
        let canvas = Canvas::new(hdc);
        let radius = radius_px(CARD_RADIUS, dpi, pal.decorative);
        let line = scale(1, dpi).max(1);
        // ページの見出しの上のアクセントの線・節の見出しの左の印
        let accent = theme::rgb(pal.accent);
        if let Some(y) = hero {
            let bar = layout::hero_bar_rect(&cols, y - scroll, dpi);
            canvas.fill_round(bar, radius_px(bar.h / 2, 96, pal.decorative), accent);
        }
        for span in &markers {
            let mark = layout::section_marker_rect(&cols, *span, dpi);
            let mark = Rect {
                y: mark.y - scroll,
                ..mark
            };
            if mark.y + mark.h < 0 || mark.y > r.h {
                continue;
            }
            canvas.fill_round(mark, radius_px(mark.w / 2, 96, pal.decorative), accent);
        }
        for card in &cards {
            let rect = Rect {
                x: cols.card_x,
                y: card.top - scroll,
                w: cols.card_w,
                h: card.bottom - card.top,
            };
            if rect.y + rect.h < 0 || rect.y > r.h {
                continue;
            }
            if pal.shadow_alpha > 0 {
                draw_card_shadow(&canvas, rect, radius, dpi, pal.shadow_alpha);
            }
            canvas.fill_round(rect, radius, theme::rgb(pal.card_bg));
            canvas.stroke_round(rect, radius, theme::rgb(pal.card_border), line);
        }
        for y in &dividers {
            let y = y - scroll;
            if y < 0 || y > r.h {
                continue;
            }
            fill_rect(
                hdc,
                Rect {
                    x: cols.card_x + line,
                    y,
                    w: cols.card_w - 2 * line,
                    h: line,
                },
                theme::rgb(pal.divider),
            );
        }
    });
    let _ = unsafe { EndPaint(hwnd, &ps) };
}
