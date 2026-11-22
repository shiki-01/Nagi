//! 登録・登録解除（`DllRegisterServer` / `DllUnregisterServer` の実体）。
//!
//! COM の CLSID（`HKCR\CLSID\{...}\InprocServer32`）と、TSF の入力方式プロファイル・カテゴリを登録する。
//! 管理者権限での `regsvr32` を想定する。

#![allow(unsafe_code)]

use windows::Win32::Foundation::{E_FAIL, ERROR_SUCCESS};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE,
    REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIP_KEYBOARD, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    ITfCategoryMgr, ITfInputProcessorProfileMgr,
};
use windows::Win32::UI::WindowsAndMessaging::LoadStringW;
use windows::core::{GUID, PCWSTR, PWSTR, Result};

use crate::dll;
use crate::guids::{
    CATEGORY_INPUTMODECOMPARTMENT, CATEGORY_SECUREMODE, CATEGORY_UIELEMENTENABLED,
    CLSID_NAGI_TEXT_SERVICE, GUID_NAGI_PROFILE, LANGID_JAPANESE, PROFILE_DESCRIPTION, braced,
};

/// 登録するカテゴリ（Microsoft IME・Mozc が宣言するもののうち Nagi が実際に満たすもの）。
/// `COMLESS` は DLL のロード方式が変わるため入れない（H-108）。`SECUREMODE` は宣言しない（H-129）
fn registered_categories() -> [GUID; 6] {
    [
        GUID_TFCAT_TIP_KEYBOARD,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
        CATEGORY_UIELEMENTENABLED,
        CATEGORY_INPUTMODECOMPARTMENT,
        GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    ]
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn clsid_key() -> String {
    format!("CLSID\\{}", braced(&CLSID_NAGI_TEXT_SERVICE))
}

/// 文字列値（`name` が空なら既定値）を書く
fn set_string(key: HKEY, name: Option<&str>, value: &str) -> Result<()> {
    set_value(key, name, REG_SZ, value)
}

fn set_value(key: HKEY, name: Option<&str>, kind: REG_VALUE_TYPE, value: &str) -> Result<()> {
    let name_w = name.map(wide);
    let value_w = wide(value);
    let bytes: Vec<u8> = value_w.iter().flat_map(|u| u.to_le_bytes()).collect();
    let name_ptr = name_w
        .as_ref()
        .map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    let status = unsafe { RegSetValueExW(key, name_ptr, None, kind, Some(&bytes)) };
    status.ok()
}

fn create_key(parent: HKEY, path: &str) -> Result<HKEY> {
    let path_w = wide(path);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            parent,
            PCWSTR(path_w.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    }
    .ok()?;
    Ok(key)
}

fn register_com_server(dll_path: &str) -> Result<()> {
    let base = clsid_key();
    let key = create_key(HKEY_CLASSES_ROOT, &base)?;
    let r = set_string(key, None, PROFILE_DESCRIPTION);
    let _ = unsafe { RegCloseKey(key) };
    r?;

    let inproc = create_key(HKEY_CLASSES_ROOT, &format!("{base}\\InprocServer32"))?;
    let r = nagi_windows::registration::inproc_server_values(dll_path)
        .iter()
        .try_for_each(|(name, value)| set_string(inproc, *name, value));
    let _ = unsafe { RegCloseKey(inproc) };
    r
}

/// アイコンファイル（.ico）の置き場所と名前。DLL と同じディレクトリ
fn icon_path() -> Option<std::path::PathBuf> {
    Some(dll::module_directory()?.join("nagi.ico"))
}

/// 入力方式アイコンを書き出し、`RegisterProfile` に渡すパスを返す。書き出せなければ DLL 自身のパス
/// （アイコンは無表示になるだけで、登録は続ける）
fn write_icon(dll_path: &str) -> String {
    let Some(path) = icon_path() else {
        return dll_path.to_owned();
    };
    match std::fs::write(&path, nagi_windows::icon::encode_ico()) {
        Ok(()) => path.to_string_lossy().into_owned(),
        Err(_) => dll_path.to_owned(),
    }
}

fn register_tsf(dll_path: &str) -> Result<()> {
    let profiles: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }?;
    let description: Vec<u16> = PROFILE_DESCRIPTION.encode_utf16().collect();
    // アイコンは .ico ファイルをコードで生成して指す（番号 0）。decisions.md H-41
    let icon: Vec<u16> = write_icon(dll_path).encode_utf16().collect();
    unsafe {
        profiles.RegisterProfile(
            &CLSID_NAGI_TEXT_SERVICE,
            LANGID_JAPANESE,
            &GUID_NAGI_PROFILE,
            &description,
            &icon,
            0,
            HKL::default(),
            0,
            true,
            0,
        )
    }?;

    let categories: ITfCategoryMgr =
        unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }?;
    // 宣言していないカテゴリが残っていれば取り除く（SECUREMODE は UIELEMENTENABLED と 1 桁違いで取り違えやすい）。
    // 登録されていなければ失敗するだけなので無視する
    let _ = unsafe {
        categories.UnregisterCategory(
            &CLSID_NAGI_TEXT_SERVICE,
            &CATEGORY_SECUREMODE,
            &CLSID_NAGI_TEXT_SERVICE,
        )
    };
    for category in registered_categories() {
        unsafe {
            categories.RegisterCategory(
                &CLSID_NAGI_TEXT_SERVICE,
                &category,
                &CLSID_NAGI_TEXT_SERVICE,
            )
        }?;
    }
    write_display_description(dll_path);
    Ok(())
}

/// この DLL に文字列リソース（ID 101）が埋め込まれているか。埋め込めなかったビルド
/// （リソースコンパイラが無い手元のビルド）では偽
fn has_name_resource() -> bool {
    let mut buf = [0u16; 8];
    let n = unsafe {
        LoadStringW(
            Some(dll::module_handle().into()),
            nagi_windows::registration::STRING_ID_NAME,
            PWSTR(buf.as_mut_ptr()),
            buf.len() as i32,
        )
    };
    n > 0
}

/// 言語プロファイルに `Display Description`（REG_EXPAND_SZ、`@<dll>,-101`）を書く。設定アプリが
/// Microsoft IME と同じ MUI 形式で表示名を引く（H-129）。DLL にリソースが無ければ書かない
/// （壊れた参照を残さない）。失敗しても登録は続ける（無くても `Description` で表示される）
fn write_display_description(dll_path: &str) {
    if !has_name_resource() {
        return;
    }
    let key_path = nagi_windows::registration::language_profile_key(
        &braced(&CLSID_NAGI_TEXT_SERVICE),
        LANGID_JAPANESE,
        &braced(&GUID_NAGI_PROFILE),
    );
    let Ok(key) = create_key(HKEY_LOCAL_MACHINE, &key_path) else {
        return;
    };
    let _ = set_value(
        key,
        Some("Display Description"),
        REG_EXPAND_SZ,
        &nagi_windows::registration::display_description_value(dll_path),
    );
    let _ = unsafe { RegCloseKey(key) };
}

/// 登録する
pub fn register() -> Result<()> {
    let path = dll::module_path().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let dll_path = path.to_string_lossy().into_owned();
    // regsvr32 は COM を初期化済みだが、他の呼び出し元でも動くよう初期化する（二重初期化は無害）
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    register_com_server(&dll_path)?;
    register_tsf(&dll_path)
}

/// 登録を解除する。途中で失敗しても残りを続ける（最初のエラーを返す）
pub fn unregister() -> Result<()> {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let mut first_error: Option<windows::core::Error> = None;
    let mut keep = |r: Result<()>| {
        if let Err(e) = r {
            first_error.get_or_insert(e);
        }
    };

    if let Ok(profiles) = unsafe {
        CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        )
    } {
        keep(unsafe {
            profiles.UnregisterProfile(
                &CLSID_NAGI_TEXT_SERVICE,
                LANGID_JAPANESE,
                &GUID_NAGI_PROFILE,
                0,
            )
        });
    }
    if let Ok(categories) = unsafe {
        CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
    } {
        for category in registered_categories() {
            keep(unsafe {
                categories.UnregisterCategory(
                    &CLSID_NAGI_TEXT_SERVICE,
                    &category,
                    &CLSID_NAGI_TEXT_SERVICE,
                )
            });
        }
        // 誤登録の掃除用の SECUREMODE。登録していなければ失敗するので、エラーは無視する
        let _ = unsafe {
            categories.UnregisterCategory(
                &CLSID_NAGI_TEXT_SERVICE,
                &CATEGORY_SECUREMODE,
                &CLSID_NAGI_TEXT_SERVICE,
            )
        };
    }
    if let Some(path) = icon_path() {
        // 無ければ何もしない（生成できなかった場合）
        let _ = std::fs::remove_file(path);
    }
    let key_w = wide(&clsid_key());
    let status = unsafe { RegDeleteTreeW(HKEY_CLASSES_ROOT, PCWSTR(key_w.as_ptr())) };
    if status != ERROR_SUCCESS {
        keep(status.ok());
    }
    first_error.map_or(Ok(()), Err)
}
