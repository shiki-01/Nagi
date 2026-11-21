//! 実行時のテーマ: ライト / ダーク / ハイコントラストの判定と、パレットの色（`Color`）を実際の色へ解決する処理。
//! 色の表（パレット）と検査は OS 非依存の `native_theme`。ここはシステムの状態を読んでブラシを持つだけ。
//! UI スレッドだけが触る（ウィンドウプロシージャ・サブクラスのどこからでも呼べるよう thread_local に置く）。

use super::dpi::{apps_use_dark_theme, is_high_contrast};
use crate::native_theme::{Color, Palette, Rgb, SysColor, ThemeKind};
use std::cell::RefCell;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT,
    COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateSolidBrush, DeleteObject, GetSysColor,
    HBRUSH, HGDIOBJ, SYS_COLOR_INDEX,
};

/// 背景の塗りに使うブラシの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Surface {
    /// ページ（右の本体）
    Page,
    /// カード
    Card,
    /// 入力欄
    Field,
    /// サイドバー・フッター
    Sidebar,
}

struct Runtime {
    palette: Palette,
    brushes: [HBRUSH; 4],
}

thread_local! {
    static RT: RefCell<Option<Runtime>> = const { RefCell::new(None) };
}

fn sys_index(c: SysColor) -> SYS_COLOR_INDEX {
    match c {
        SysColor::Window => COLOR_WINDOW,
        SysColor::WindowText => COLOR_WINDOWTEXT,
        SysColor::GrayText => COLOR_GRAYTEXT,
        SysColor::Highlight => COLOR_HIGHLIGHT,
        SysColor::HighlightText => COLOR_HIGHLIGHTTEXT,
        SysColor::ButtonFace => COLOR_BTNFACE,
        SysColor::ButtonText => COLOR_BTNTEXT,
        SysColor::HotLight => COLOR_HOTLIGHT,
    }
}

/// パレットの色を実際の RGB にする（システム色は今のテーマの値）
pub(super) fn rgb(c: Color) -> Rgb {
    match c {
        Color::Rgb(rgb) => rgb,
        Color::Sys(sys) => {
            let v = unsafe { GetSysColor(sys_index(sys)) };
            Rgb(
                (v & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                ((v >> 16) & 0xFF) as u8,
            )
        }
    }
}

fn detect_kind() -> ThemeKind {
    ThemeKind::decide(is_high_contrast(), apps_use_dark_theme())
}

fn build() -> Runtime {
    let palette = Palette::for_theme(detect_kind());
    let make = |c: Color| unsafe { CreateSolidBrush(COLORREF(rgb(c).colorref())) };
    Runtime {
        brushes: [
            make(palette.page_bg),
            make(palette.card_bg),
            make(palette.field_bg),
            make(palette.sidebar_bg),
        ],
        palette,
    }
}

fn free(rt: Runtime) {
    for b in rt.brushes {
        let _ = unsafe { DeleteObject(HGDIOBJ(b.0)) };
    }
}

/// 最初の判定（ウィンドウを作る前に呼ぶ）
pub(super) fn init() {
    RT.with(|rt| {
        let mut slot = rt.borrow_mut();
        if slot.is_none() {
            *slot = Some(build());
        }
    });
}

/// テーマ・システム色が変わったかもしれないとき: 判定とブラシを作り直す。種類が変わったら `true`
pub(super) fn refresh() -> bool {
    RT.with(|rt| {
        let mut slot = rt.borrow_mut();
        let old = slot.take();
        let changed = old.as_ref().is_none_or(|o| o.palette.kind != detect_kind());
        if let Some(old) = old {
            free(old);
        }
        *slot = Some(build());
        changed
    })
}

/// 後始末
pub(super) fn destroy() {
    RT.with(|rt| {
        if let Some(rt) = rt.borrow_mut().take() {
            free(rt);
        }
    });
}

/// 今のパレット
pub(super) fn palette() -> Palette {
    init();
    RT.with(|rt| rt.borrow().as_ref().map(|r| r.palette))
        .unwrap_or_else(|| Palette::for_theme(ThemeKind::Light))
}

/// 背景のブラシ（ウィンドウプロシージャの戻り値に使う。呼び出し側は破棄しない）
pub(super) fn brush(surface: Surface) -> HBRUSH {
    init();
    RT.with(|rt| {
        rt.borrow()
            .as_ref()
            .map(|r| r.brushes[surface as usize])
            .unwrap_or_default()
    })
}

/// 背景の色
pub(super) fn surface_rgb(surface: Surface) -> Rgb {
    let p = palette();
    rgb(match surface {
        Surface::Page => p.page_bg,
        Surface::Card => p.card_bg,
        Surface::Field => p.field_bg,
        Surface::Sidebar => p.sidebar_bg,
    })
}
