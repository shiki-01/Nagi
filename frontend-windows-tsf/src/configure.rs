//! 「言語のオプション → キーボード → Nagi → オプション」の入口（`ITfFnConfigure`）。
//!
//! OS は 2 通りで `ITfFnConfigure` を探す: (1) TIP の CLSID を `CoCreateInstance` して直接 QueryInterface
//! （`TextService` 本体が実装）、(2) `ITfFunctionProvider::GetFunction` 経由（[`ConfigureFunction`]）。
//! どちらも設定 GUI の起動は [`show`] を共有する。未アクティブ（`Activate` 前）でも動くこと。
//! decisions.md H-124。

#![allow(unsafe_code)]

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::UI::TextServices::{ITfFnConfigure, ITfFnConfigure_Impl, ITfFunction_Impl};
use windows::core::{BSTR, GUID, Result, implement};

use crate::guids::PROFILE_DESCRIPTION;
use crate::langbar;
use crate::text_service::guarded;

/// 関数の表示名
pub(crate) fn display_name() -> BSTR {
    BSTR::from(format!("{PROFILE_DESCRIPTION} の設定"))
}

/// 設定画面を開く（`nagi-settings.exe` を DLL と同じフォルダから起動）
pub(crate) fn show() -> Result<()> {
    guarded(
        || Err(E_FAIL.into()),
        || {
            crate::tsf_log::log("menu", "configure requested");
            if langbar::open_settings_gui_from_module("configure") {
                Ok(())
            } else {
                crate::tsf_log::log("menu", "configure failed (no settings exe)");
                Err(E_FAIL.into())
            }
        },
    )
}

/// `ITfFunctionProvider::GetFunction` が返す `ITfFnConfigure`
#[implement(ITfFnConfigure)]
pub(crate) struct ConfigureFunction;

impl ITfFunction_Impl for ConfigureFunction_Impl {
    fn GetDisplayName(&self) -> Result<BSTR> {
        Ok(display_name())
    }
}

impl ITfFnConfigure_Impl for ConfigureFunction_Impl {
    fn Show(&self, _hwndparent: HWND, _langid: u16, _rguidprofile: *const GUID) -> Result<()> {
        show()
    }
}
