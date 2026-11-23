//! 言語バーのボタン（`ITfLangBarItemButton`）。現在の入力モード（あ / カ / ｶ / Ａ / A）を表示し、
//! 左クリックでひらがな ⇄ 英数を切り替え、メニュー（右クリック。`popup_menu` が `OnClick` で出す）に
//! モード・状態・設定を出す。
//!
//! 何を出すか・どう切り替えるかは `nagi_windows::status`（`menu_entries` / `toggled` / `button_text`）と
//! `Core` が決める。ここは COM の受け口と、表示用の `HICON`（Nagi のロゴにモードの文字を載せたアイコン）だけ。
//! 絵柄は `nagi_windows::icon::compose_status`、カラー/モノクロの選択は設定 `windows.indicator_icon`
//! （`nagi_windows::indicator`。H-115）。モードの文字は GDI でシステムの日本語 UI フォントの字形を
//! カバレッジマスクに描いて渡し（`glyph_gdi`）、フォントが無い・GDI が失敗したときだけ手描きストローク
//! （`icon::stroke_mask`）にする（H-122）。
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
    /// 設定 `windows.indicator_icon` の読み取り結果（間引いて読み直す）。`LangBar` と共有する
    style: Rc<RefCell<StyleCache>>,
}

impl Button {
    /// アイコンの配色の設定。前回の取得から間隔があいていれば、コアへ読み直しに行く
    /// （設定の変更は、次のアイコンの更新で反映される。再起動は要らない）
    fn indicator_style(&self) -> IndicatorStyle {
        let now = Instant::now();
        if self.style.borrow().due(now) {
            let engine = self
                .core
                .upgrade()
                .and_then(|core| core.fetch_indicator_style());
            // エンジンが答えないとき（起動前など）は、config.toml を読み直して補う
            let config = engine.is_none().then(load_config_style).flatten();
            let recorded = self.style.borrow_mut().record(now, engine, config);
            if let Some(style) = recorded.persist {
                store_cached_style(style);
            }
            if recorded.changed {
                crate::tsf_log::log(
                    "langbar",
                    &format!(
                        "indicator style={} source={}",
                        indicator::style_text(self.style.borrow().style()),
                        if engine.is_some() { "engine" } else { "config" }
                    ),
                );
            }
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
        let size = size as u32;
        let glyph = indicator::glyph(self.mode());
        let mask = glyph_gdi::render(status::button_text(self.mode()), size)
            .map(|(hires, side)| {
                // 全角英数だけ字形を横に広げる（半角の A と見分けるため）
                icon::fit_glyph(&hires, side, side, size, icon::fill_ratio(tone), glyph)
            })
            .unwrap_or_else(|| icon::stroke_mask(size, glyph, tone));
        let rgba = icon::compose_status(size, tone, &mask);
        make_icon(&rgba, size as i32)
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

/// モードの文字を、システムの日本語 UI フォントの字形で描く（GDI）。
/// 結果はカバレッジマスク（0〜255）で、合成と位置決めは `nagi_windows::icon` が行う
mod glyph_gdi {
    use std::ffi::c_void;

    use nagi_windows::icon::HIRES_SCALE;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::{
        ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS,
        CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_PITCH, DIB_RGB_COLORS,
        DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
        FF_DONTCARE, FW_BOLD, GdiFlush, GetTextFaceW, HDC, HGDIOBJ, OUT_DEFAULT_PRECIS,
        SHIFTJIS_CHARSET, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    };
    use windows::core::PCWSTR;

    /// 使うフォント（この順に、実際にその名前で選ばれたものを採る）。
    /// GDI は無い名前を黙って別のフォントに置き換えるので、選択後に `GetTextFaceW` で確かめる
    const FACES: [&str; 3] = ["Yu Gothic UI", "Meiryo UI", "MS UI Gothic"];

    /// 破棄時に GDI オブジェクトと DC を解放する
    struct Cleanup {
        dc: HDC,
        objects: Vec<HGDIOBJ>,
        /// DC に選択していた元のオブジェクト（解放前に戻す）
        restore: Vec<HGDIOBJ>,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                // 選んだ順の逆に戻す（最後に元のビットマップが選ばれ、作ったものは全て解放できる）
                while let Some(old) = self.restore.pop() {
                    SelectObject(self.dc, old);
                }
                for object in self.objects.drain(..) {
                    let _ = DeleteObject(object);
                }
                let _ = DeleteDC(self.dc);
            }
        }
    }

    /// `size` px のアイコン用に `text` を [`HIRES_SCALE`] 倍の大きさで描く。
    /// 返すのは `(side × side のカバレッジ（行は上から下）, side)`。フォントが無い・GDI が失敗したら `None`
    pub fn render(text: &str, size: u32) -> Option<(Vec<u8>, u32)> {
        let side = size.checked_mul(HIRES_SCALE)?;
        if side == 0 || text.is_empty() {
            return None;
        }
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let mut cleanup = Cleanup {
                dc,
                objects: Vec::new(),
                restore: Vec::new(),
            };
            let header = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: side as i32,
                biHeight: -(side as i32), // 上から下
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            let info = BITMAPINFO {
                bmiHeader: header,
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap =
                CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            cleanup.objects.push(bitmap.into());
            if bits.is_null() {
                return None;
            }
            cleanup.restore.push(SelectObject(dc, bitmap.into()));

            FACES
                .iter()
                .find_map(|face| select_face(&mut cleanup, face, side))?;
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00FF_FFFF));
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: side as i32,
                bottom: side as i32,
            };
            if DrawTextW(
                dc,
                &mut wide,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            ) == 0
            {
                return None;
            }
            let _ = GdiFlush();
            // 黒地に白で描いたので、緑のチャネルがそのままカバレッジ（B, G, R, X の順）
            let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), (side * side * 4) as usize);
            Some((pixels.chunks_exact(4).map(|p| p[1]).collect(), side))
        }
    }

    /// `face` のフォントを作って DC へ選ぶ。実際にその名前で選ばれなければ（置き換えられたら）`None`
    unsafe fn select_face(cleanup: &mut Cleanup, face: &str, side: u32) -> Option<()> {
        let name: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
        // 文字の高さは外接の四角に余裕を持たせた em の 8 割（位置と大きさは字形の外接で決め直す）
        let height = -((side * 8 / 10) as i32);
        let font = unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                SHIFTJIS_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
                PCWSTR(name.as_ptr()),
            )
        };
        if font.is_invalid() {
            return None;
        }
        cleanup.objects.push(font.into());
        let old = unsafe { SelectObject(cleanup.dc, font.into()) };
        cleanup.restore.push(old);
        let mut actual = [0u16; 64];
        let len = unsafe { GetTextFaceW(cleanup.dc, Some(&mut actual)) };
        let actual = String::from_utf16_lossy(&actual[..(len.max(1) as usize - 1).min(63)]);
        actual.eq_ignore_ascii_case(face).then_some(())
    }
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

/// 読み込む設定ファイル・キャッシュの大きさの上限（これを超えるものは読まない）
const MAX_STYLE_FILE_BYTES: u64 = 1024 * 1024;

fn read_small(path: &Path) -> Option<String> {
    let small = std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX_STYLE_FILE_BYTES);
    small.then(|| std::fs::read_to_string(path).ok()).flatten()
}

fn env_text(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `config.toml`（`NAGI_CONFIG_DIR` 優先、無ければ `%APPDATA%\Nagi`）から `[windows] indicator_icon` だけを
/// 同期で読む。読めない・キーが無い・不明な値は `None`
fn load_config_style() -> Option<IndicatorStyle> {
    let path = indicator::config_file_path(
        env_text("NAGI_CONFIG_DIR").as_deref(),
        env_text("APPDATA").as_deref(),
    )?;
    indicator::parse_config_text(&read_small(&path)?)
}

fn cache_file() -> Option<std::path::PathBuf> {
    Some(indicator::cache_path(Path::new(&env_text("LOCALAPPDATA")?)))
}

/// 直近に確定した値（`%LOCALAPPDATA%\Nagi\indicator-style`）。読めなければ `None`
fn load_cached_style() -> Option<IndicatorStyle> {
    indicator::parse_cache_text(&read_small(&cache_file()?)?)
}

/// 確定した値を書く（失敗は無視。次の機会に書き直す）
fn store_cached_style(style: IndicatorStyle) {
    let Some(path) = cache_file() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{}\n", indicator::style_text(style)));
}

/// 言語バーに登録したボタン。破棄すると登録を外す
pub struct LangBar {
    manager: ITfLangBarItemMgr,
    item: ITfLangBarItemButton,
    sinks: Sinks,
    style: Rc<RefCell<StyleCache>>,
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
        // 最初のアイコンの描画までに配色が決まるよう、config.toml と直近の値を同期で読んでおく。
        // エンジンの値（`get_settings`）は、つながってから採用される（優先順位は `indicator::resolve`）
        let style = Rc::new(RefCell::new(StyleCache::seeded(
            load_config_style(),
            load_cached_style(),
        )));
        let item: ITfLangBarItemButton = Button {
            core,
            sinks: Rc::clone(&sinks),
            next_cookie: RefCell::new(0),
            style: Rc::clone(&style),
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
            style,
        })
    }

    /// エンジンの設定を次の機会にすぐ読み直させる（エンジンが（再）起動してつながったとき）
    pub fn invalidate_style(&self) {
        self.style.borrow_mut().invalidate();
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
