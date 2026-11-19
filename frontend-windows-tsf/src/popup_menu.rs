//! 入力インジケーター（言語バーのボタン）の右クリックメニュー。
//!
//! `TF_LBI_STYLE_BTN_BUTTON` のボタンは、右クリックで OS が `ITfLangBarItemButton::InitMenu` を呼ばず
//! `OnClick(TF_LBI_CLK_RIGHT, pt)` を渡すだけ（メニューの表示は項目側の責任。実機 0.1.12 のログで確認。H-96）。
//! そこで `OnClick` から、Win32 のポップアップメニューを自分で出す。
//!
//! - オーナーは専用の非表示トップレベル窓（`HWND_MESSAGE` 窓はメニューのオーナーにできない）。メニューのたびに
//!   作って壊す。`SetForegroundWindow` → `TrackPopupMenu` → `WM_NULL` の定石で、外側のクリックで正しく閉じる。
//!   閉じた後は元の前面ウィンドウへフォーカスを戻す
//! - 1 回の右クリックで `OnClick` が複数回届くことがあるので、表示中・直近 300ms の同座標は無視する
//! - 内容は `Core::lang_bar_menu`（`nagi_windows::status::menu_entries`）、選択は `Core::run_menu_command`

#![allow(unsafe_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use nagi_windows::status::{self, ClickDedup, MenuEntry};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DestroyMenu, DestroyWindow, GetForegroundWindow,
    HMENU, MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_NULL, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::PCWSTR;

use crate::text_service::Core;
use crate::win::{self, WindowClass, wide};

static CLASS: WindowClass = WindowClass::new("NagiMenuOwner");

/// メニューを表示中か（`OnClick` の再入・多重呼び出しを防ぐ）
static SHOWING: AtomicBool = AtomicBool::new(false);

/// 直近のクリック（同座標・300ms 以内の重複を無視する）
static DEDUP: Mutex<Option<ClickDedup>> = Mutex::new(None);

fn is_duplicate(now: Instant, pt: &POINT) -> bool {
    let mut guard = DEDUP.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(ClickDedup::default)
        .is_duplicate(now, pt.x, pt.y)
}

fn touch(now: Instant) {
    let mut guard = DEDUP.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = guard.as_mut() {
        d.touch(now);
    }
}

/// `SHOWING` を戻す
struct ShowingGuard;

impl Drop for ShowingGuard {
    fn drop(&mut self) {
        SHOWING.store(false, Ordering::SeqCst);
        touch(Instant::now());
    }
}

/// 破棄を保証するメニューハンドル
struct Menu(HMENU);

impl Drop for Menu {
    fn drop(&mut self) {
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

/// 非表示のオーナー窓
struct Owner(HWND);

impl Owner {
    fn create() -> Option<Self> {
        // 既定のウィンドウプロシージャのままでよい（メニューは `TPM_RETURNCMD` で結果を直接受ける）
        if !CLASS.acquire(Some(owner_proc)) {
            return None;
        }
        let class = CLASS.name_wide();
        let title = wide("Nagi menu owner");
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(WS_POPUP.0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(win::instance()),
                None,
            )
        };
        match created {
            Ok(hwnd) => Some(Self(hwnd)),
            Err(_) => {
                CLASS.release();
                None
            }
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
        CLASS.release();
    }
}

unsafe extern "system" fn owner_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { win::default_proc(hwnd, msg, wparam, lparam) }
}

/// メニューの 1 行を追加する
fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: u32, label: &str) -> bool {
    let text = wide(label);
    unsafe { AppendMenuW(menu, flags, id as usize, PCWSTR(text.as_ptr())) }.is_ok()
}

fn build(entries: &[MenuEntry]) -> Option<Menu> {
    let handle = unsafe { CreatePopupMenu() }.ok()?;
    let menu = Menu(handle);
    for entry in entries {
        let ok = match entry {
            MenuEntry::Separator => append(handle, MF_SEPARATOR, 0, ""),
            // 状態の行は選べない（id 0 は「選ばれなかった」と同じ値だが、灰色なので選ばれない）
            MenuEntry::Info(text) => append(handle, MF_STRING | MF_GRAYED, 0, text),
            MenuEntry::Command {
                id,
                label,
                checked,
                enabled,
            } => {
                let mut flags = MF_STRING;
                if *checked {
                    flags |= MF_CHECKED;
                }
                if !*enabled {
                    flags |= MF_GRAYED;
                }
                append(handle, flags, *id, label)
            }
        };
        if !ok {
            return None;
        }
    }
    Some(menu)
}

/// 右クリックでポップアップメニューを出し、選ばれた項目を実行する。UI スレッドで呼ぶ（モーダル）
pub fn show(core: &Core, pt: &POINT) {
    let now = Instant::now();
    if SHOWING.load(Ordering::SeqCst) || is_duplicate(now, pt) {
        crate::tsf_log::log("langbar", "popup menu skipped (duplicate click)");
        return;
    }
    if SHOWING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _showing = ShowingGuard;

    // 状態の取得（最大 250ms 待つ）は `InitMenu` 経路と同じ
    let entries = core.lang_bar_menu();
    let Some(menu) = build(&entries) else {
        crate::tsf_log::log("langbar", "popup menu build failed");
        return;
    };
    let Some(owner) = Owner::create() else {
        crate::tsf_log::log("langbar", "popup menu owner window failed");
        return;
    };

    let previous = unsafe { GetForegroundWindow() };
    // 前面にしないと、メニューの外側をクリックしても閉じない
    let _ = unsafe { SetForegroundWindow(owner.0) };
    crate::tsf_log::log(
        "langbar",
        &format!("popup menu shown entries={}", entries.len()),
    );
    let chosen = unsafe {
        TrackPopupMenu(
            menu.0,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            pt.x,
            pt.y,
            None,
            owner.0,
            None,
        )
    }
    .0;
    // メニューが次の操作で閉じ残らないようにする定石
    let _ = unsafe { PostMessageW(Some(owner.0), WM_NULL, WPARAM(0), LPARAM(0)) };
    drop(menu);
    drop(owner);
    // 元の前面ウィンドウへフォーカスを戻す（別プロセスの窓なら失敗してよい）
    if !previous.is_invalid() {
        let _ = unsafe { SetForegroundWindow(previous) };
    }

    let id = u32::try_from(chosen).unwrap_or(0);
    match status::command_from_id(id) {
        Some(command) if id != 0 => {
            crate::tsf_log::log("langbar", &format!("popup menu selected id={id}"));
            core.run_menu_command(command);
        }
        _ => crate::tsf_log::log("langbar", "popup menu dismissed"),
    }
}
