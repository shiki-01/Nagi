//! 言語バーのボタン（`ITfLangBarItemButton`）。現在の入力モード（あ / A）を表示し、
//! 左クリックでひらがな ⇄ 英数を切り替え、メニュー（右クリック。`popup_menu` が `OnClick` で出す）に
//! モード・状態・設定を出す。
//!
//! 何を出すか・どう切り替えるかは `nagi_windows::status`（`menu_entries` / `toggled` / `button_text`）と
//! `Core` が決める。ここは COM の受け口と、表示用の `HICON`（Nagi のロゴにモードの文字を載せたアイコン）だけ。
//! 絵柄は `nagi_windows::icon::render_status`、カラー/モノクロの選択は設定 `windows.indicator_icon`
//! （`nagi_windows::indicator`。H-115）。
//! モードの正はコアで、切り替えは `mode_comp` のコンパートメントへ書く形に統一している
//! （他のクライアントやトレイの操作と同じ経路。`Core::request_mode`）。
//!
//! Win8 以降（特に 11）のシェルの入力インジケーター（トレイの「あ / A」）は、言語バー項目のうち
//! `guidItem == GUID_LBI_INPUTMODE` のものだけを表示する（独自 GUID の項目はレガシー言語バーにしか出ない）。
//! このボタンがトレイの表示・左クリックでの切り替え・右クリックのメニューの実体になる（decisions.md H-54）。
//! `BTN_BUTTON` の右クリックでは OS が `InitMenu` を呼ばない（H-96）ため、メニューは `OnClick` から自前で出す。
//! `InitMenu` は `BTN_MENU` 系のスタイルや他の経路のために残している。

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::Instant;

use nagi_windows::icon;
use nagi_windows::indicator::{self, IndicatorStyle, StyleCache};
use nagi_windows::status::{self, MenuEntry};
use windows::Win32::Foundation::{
    E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_POINTER, ERROR_SUCCESS, POINT, RECT,
};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::TextServices::{
    GUID_LBI_INPUTMODE, ITfLangBarItem_Impl, ITfLangBarItemButton, ITfLangBarItemButton_Impl,
    ITfLangBarItemMgr, ITfLangBarItemSink, ITfMenu, ITfSource, ITfSource_Impl, ITfThreadMgr,
    TF_LANGBARITEMINFO, TF_LBI_CLK_LEFT, TF_LBI_ICON, TF_LBI_STYLE_BTN_BUTTON,
    TF_LBI_STYLE_SHOWNINTRAY, TF_LBI_TEXT, TF_LBI_TOOLTIP, TF_LBMENUF_CHECKED, TF_LBMENUF_GRAYED,
    TF_LBMENUF_SEPARATOR, TfLBIClick,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIcon, HICON, SM_CXSMICON, SW_SHOWNORMAL};
use windows::core::{BOOL, BSTR, GUID, IUnknown, Interface, PCWSTR, Ref, Result, implement, w};

use crate::guids::CLSID_NAGI_TEXT_SERVICE;
use crate::text_service::Core;
use crate::win::wide;

/// 言語バー内の並び順（他の項目の後ろ）
const SORT_ORDER: u32 = 10;

type Sinks = Rc<RefCell<Vec<(u32, ITfLangBarItemSink)>>>;

#[implement(ITfLangBarItemButton, ITfSource)]
struct Button {
    core: Weak<Core>,
    sinks: Sinks,
    next_cookie: RefCell<u32>,
    /// 設定 `windows.indicator_icon` の読み取り結果（間引いて読み直す）
    style: RefCell<StyleCache>,
}

impl Button {
    /// アイコンの配色の設定。前回の取得から間隔があいていれば、コアへ読み直しに行く
    /// （設定の変更は、次のアイコンの更新で反映される。再起動は要らない）
    fn indicator_style(&self) -> IndicatorStyle {
        let now = Instant::now();
        if self.style.borrow().due(now) {
            let fetched = self
                .core
                .upgrade()
                .and_then(|core| core.fetch_indicator_style());
            self.style.borrow_mut().record(now, fetched);
        }
        self.style.borrow().style()
    }

    fn mode(&self) -> Option<nagi_protocol::InputMode> {
        self.core.upgrade().and_then(|c| c.lang_bar_mode())
    }
}

impl ITfLangBarItem_Impl for Button_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if pinfo.is_null() {
            return Err(E_POINTER.into());
        }
        crate::tsf_log::log("langbar", "GetInfo called");
        let mut info = TF_LANGBARITEMINFO {
            clsidService: CLSID_NAGI_TEXT_SERVICE,
            guidItem: GUID_LBI_INPUTMODE,
            dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
            ulSort: SORT_ORDER,
            szDescription: [0; 32],
        };
        // 説明は 31 UTF-16 + NUL まで
        for (slot, unit) in info
            .szDescription
            .iter_mut()
            .zip("Nagi 入力モード".encode_utf16().take(31))
        {
            *slot = unit;
        }
        unsafe { pinfo.write(info) };
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        crate::tsf_log::log("langbar", "GetStatus called status=0");
        Ok(0)
    }

    fn Show(&self, fshow: BOOL) -> Result<()> {
        crate::tsf_log::log("langbar", &format!("Show called show={}", fshow.as_bool()));
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::from(status::button_tooltip(self.mode()).as_str()))
    }
}

impl ITfLangBarItemButton_Impl for Button_Impl {
    fn OnClick(&self, click: TfLBIClick, pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        // 右クリック等で OS が呼んでいるかの切り分け用（毎回記録する。座標は画面座標）
        crate::tsf_log::log(
            "langbar",
            &format!(
                "OnClick called click={} ({}) x={} y={}",
                click.0,
                if click == TF_LBI_CLK_LEFT {
                    "left"
                } else {
                    "right"
                },
                pt.x,
                pt.y
            ),
        );
        let Some(core) = self.core.upgrade() else {
            return Ok(());
        };
        if click == TF_LBI_CLK_LEFT {
            core.toggle_mode_from_lang_bar();
        } else {
            // `TF_LBI_STYLE_BTN_BUTTON` では OS は右クリックで `InitMenu` を呼ばないので、自前で出す（H-96）
            crate::popup_menu::show(&core, pt);
        }
        Ok(())
    }

    fn InitMenu(&self, pmenu: Ref<ITfMenu>) -> Result<()> {
        let menu = pmenu.ok()?;
        let core = self.core.upgrade().ok_or_else(|| {
            crate::tsf_log::log("langbar", "InitMenu called but core is gone");
            windows::core::Error::from(E_FAIL)
        })?;
        let entries = core.lang_bar_menu();
        crate::tsf_log::log(
            "langbar",
            &format!("InitMenu called entries={}", entries.len()),
        );
        for entry in entries {
            match entry {
                MenuEntry::Separator => add_item(menu, 0, TF_LBMENUF_SEPARATOR, "")?,
                MenuEntry::Info(text) => add_item(menu, 0, TF_LBMENUF_GRAYED, &text)?,
                MenuEntry::Command {
                    id,
                    label,
                    checked,
                    enabled,
                } => {
                    let mut flags = 0;
                    if checked {
                        flags |= TF_LBMENUF_CHECKED;
                    }
                    if !enabled {
                        flags |= TF_LBMENUF_GRAYED;
                    }
                    add_item(menu, id, flags, &label)?;
                }
            }
        }
        Ok(())
    }

    fn OnMenuSelect(&self, wid: u32) -> Result<()> {
        crate::tsf_log::log("langbar", &format!("OnMenuSelect called id={wid}"));
        if let (Some(command), Some(core)) = (status::command_from_id(wid), self.core.upgrade()) {
            core.run_menu_command(command);
        }
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        // システム DPI に合わせた小アイコンの大きさ（100% で 16px）
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) }.max(16);
        let tone = indicator::tone(self.indicator_style(), system_uses_light_theme());
        let rgba = icon::render_status(size as u32, indicator::glyph(self.mode()), tone);
        make_icon(&rgba, size)
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(status::button_text(self.mode())))
    }
}

impl ITfSource_Impl for Button_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<IUnknown>) -> Result<u32> {
        if riid.is_null() {
            return Err(E_POINTER.into());
        }
        if unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(E_NOINTERFACE.into());
        }
        let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
        let mut next = self.next_cookie.borrow_mut();
        *next += 1;
        self.sinks.borrow_mut().push((*next, sink));
        Ok(*next)
    }

    fn UnadviseSink(&self, dwcookie: u32) -> Result<()> {
        let mut sinks = self.sinks.borrow_mut();
        let before = sinks.len();
        sinks.retain(|(cookie, _)| *cookie != dwcookie);
        if sinks.len() == before {
            return Err(E_INVALIDARG.into());
        }
        Ok(())
    }
}

fn add_item(menu: &ITfMenu, id: u32, flags: u32, label: &str) -> Result<()> {
    let text: Vec<u16> = label.encode_utf16().collect();
    unsafe {
        menu.AddMenuItem(
            id,
            flags,
            HBITMAP::default(),
            HBITMAP::default(),
            &text,
            std::ptr::null_mut(),
        )
    }
}

/// システム（タスクバー）がライトテーマか。`SystemUsesLightTheme`（0 = ダーク、1 = ライト）が読めなければ `None`。
/// アプリのテーマ（`AppsUseLightTheme`）とは別の値で、タスクバー・入力インジケーターは前者に従う
fn system_uses_light_theme() -> Option<bool> {
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("SystemUsesLightTheme");
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast::<c_void>()),
            Some(&raw mut size),
        )
    };
    (err == ERROR_SUCCESS).then_some(data != 0)
}

/// 非乗算の RGBA（行は上から下）から `HICON` を作る。呼び出し側（言語バー）が `DestroyIcon` する前提で、
/// 呼ぶたびに新しく作る
fn make_icon(rgba: &[u8], size: i32) -> Result<HICON> {
    // XOR（色）は BGRA の上から下。AND マスクは 1bpp で行は 16 bit 境界、1 = 透明（アルファがあれば無視される）
    let mut xor = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        xor.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    let stride = (size as usize).div_ceil(16) * 2;
    let mut mask = vec![0u8; stride * size as usize];
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        if px[3] == 0 {
            let (row, col) = (i / size as usize, i % size as usize);
            mask[row * stride + col / 8] |= 0x80 >> (col % 8);
        }
    }
    unsafe { CreateIcon(None, size, size, 1, 32, mask.as_ptr(), xor.as_ptr()) }
}

/// `ShellExecuteW("open")` で `file` を開く。戻り値が 32 以下なら失敗で、`[menu] open failed` を
/// tsf.log に残す（パスは出すが入力内容は出さない）。開けなくても入力には影響しないので呼び出し元へは返さない
fn shell_open(kind: &str, file: PCWSTR, parameters: PCWSTR) {
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            file,
            parameters,
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as isize;
    if code <= 32 {
        crate::tsf_log::log("menu", &format!("open failed kind={kind} code={code}"));
    }
}

/// 設定ファイルをメモ帳で開く（`.toml` に関連付けが無くても開けるようメモ帳を明示する）。
/// 無いファイルはメモ帳が作成を尋ねる
pub fn open_in_notepad(kind: &str, path: &Path) {
    let argument = wide(&format!("\"{}\"", path.display()));
    shell_open(kind, w!("notepad.exe"), PCWSTR(argument.as_ptr()));
}

/// 設定画面（`nagi-settings.exe`）を起動する。画面はローカルの Web ページで、実行ファイルが既定のブラウザで開く
pub fn run_settings_gui(kind: &str, exe: &Path) {
    let file = wide(&exe.display().to_string());
    shell_open(kind, PCWSTR(file.as_ptr()), PCWSTR::null());
}

/// DLL と同じディレクトリの `nagi-settings.exe` で設定画面を開く。エンジン・スレッドマネージャに
/// 依存しない（未アクティブの COM オブジェクトからも呼べる。`ITfFnConfigure::Show` 用）。
/// 実行ファイルが無ければ `false`
pub fn open_settings_gui_from_module(kind: &str) -> bool {
    let Some(exe) = crate::dll::module_directory().map(|d| d.join(status::SETTINGS_GUI_EXE)) else {
        return false;
    };
    if !exe.exists() {
        return false;
    }
    run_settings_gui(kind, &exe);
    true
}

/// 設定画面を更新セクションで開き、更新の確認を自動で実行する（`--section update --auto-check`）
pub fn run_check_update(kind: &str, exe: &Path) {
    let file = wide(&exe.display().to_string());
    let arguments = wide(status::CHECK_UPDATE_ARGUMENTS);
    shell_open(kind, PCWSTR(file.as_ptr()), PCWSTR(arguments.as_ptr()));
}

/// フォルダをエクスプローラーで開く
pub fn open_folder(kind: &str, path: &Path) {
    let folder = wide(&path.display().to_string());
    shell_open(kind, PCWSTR(folder.as_ptr()), PCWSTR::null());
}

/// `verify.ps1 status` を新しいコンソールで起動する（実行後もコンソールを閉じない）
pub fn run_verify(kind: &str, script: &Path) {
    let arguments = wide(&status::verify_arguments(script));
    shell_open(kind, w!("powershell.exe"), PCWSTR(arguments.as_ptr()));
}

/// `apply-update.ps1` を新しいコンソールで起動する（取得済みの更新の適用。スクリプトが UAC の昇格を行う）
pub fn run_apply_update(kind: &str, script: &Path) {
    let arguments = wide(&status::apply_update_arguments(script));
    shell_open(kind, w!("powershell.exe"), PCWSTR(arguments.as_ptr()));
}

/// 言語バーに登録したボタン。破棄すると登録を外す
pub struct LangBar {
    manager: ITfLangBarItemMgr,
    item: ITfLangBarItemButton,
    sinks: Sinks,
}

impl LangBar {
    /// ボタンを作って言語バーに追加する
    pub fn new(thread_mgr: &ITfThreadMgr, core: Weak<Core>) -> Result<Self> {
        let manager: ITfLangBarItemMgr = thread_mgr.cast().inspect_err(|e| {
            crate::tsf_log::log(
                "langbar",
                &format!(
                    "ITfLangBarItemMgr unavailable hr={}",
                    nagi_windows::diag::hresult_text(e.code().0)
                ),
            );
        })?;
        let sinks: Sinks = Rc::default();
        let item: ITfLangBarItemButton = Button {
            core,
            sinks: Rc::clone(&sinks),
            next_cookie: RefCell::new(0),
            style: RefCell::new(StyleCache::new()),
        }
        .into();
        unsafe { manager.AddItem(&item) }.inspect_err(|e| {
            crate::tsf_log::log(
                "langbar",
                &format!(
                    "AddItem failed hr={}",
                    nagi_windows::diag::hresult_text(e.code().0)
                ),
            );
        })?;
        crate::tsf_log::log("langbar", "AddItem ok (GUID_LBI_INPUTMODE)");
        Ok(Self {
            manager,
            item,
            sinks,
        })
    }

    /// モードが変わったので、アイコン・文字・ツールチップを描き直させる
    pub fn update(&self) {
        let sinks: Vec<ITfLangBarItemSink> =
            self.sinks.borrow().iter().map(|(_, s)| s.clone()).collect();
        for sink in sinks {
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_TEXT | TF_LBI_TOOLTIP) };
        }
    }
}

impl Drop for LangBar {
    fn drop(&mut self) {
        let result = unsafe { self.manager.RemoveItem(&self.item) };
        crate::tsf_log::log(
            "langbar",
            &format!(
                "RemoveItem hr={}",
                nagi_windows::diag::hresult_text(result.err().map_or(0, |e| e.code().0))
            ),
        );
    }
}
