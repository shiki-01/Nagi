//! IME の登録（regsvr32）・プロセス内での有効化（`ITfInputProcessorProfileMgr::ActivateProfile`）・
//! 入力モード（ひらがな）の設定。

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, CLSID_TF_ThreadMgr,
    GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
    GUID_TFCAT_TIP_KEYBOARD, ITfCompartmentMgr, ITfInputProcessorProfileMgr, ITfThreadMgr,
    TF_INPUTPROCESSORPROFILE, TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_IPPMF_ENABLEPROFILE,
    TF_IPPMF_FORPROCESS, TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows::core::{GUID, Interface, w};

use super::pump;

// `frontend-windows-tsf/src/guids.rs` と同じ値（登録済みの入力方式の識別子なので変わらない）
const CLSID_NAGI_TEXT_SERVICE: GUID = GUID::from_u128(0xB0D6897E_C414_4BB1_B621_46EEE659E0CD);
const GUID_NAGI_PROFILE: GUID = GUID::from_u128(0x5B02A01F_6B4F_430F_A6A8_D9B4D83C1918);
const LANGID_JAPANESE: u16 = 0x0411;

/// `nagi_windows::mode::compartment_values(Hiragana)` と同じ（開・日本語・全角・ローマ字）
const CONVERSION_HIRAGANA: i32 = 0x0001 | 0x0008 | 0x0010;

fn regsvr32() -> std::path::PathBuf {
    // 64bit の regsvr32（32bit プロセスからは Sysnative。このドライバーは x64 なので System32）
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join("System32").join("regsvr32.exe")
}

fn run_regsvr32(args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let status = Command::new(regsvr32())
        .args(args)
        .status()
        .map_err(|e| format!("regsvr32 を起動できません: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "regsvr32 が失敗しました（終了コード {:?}）。管理者権限で実行しているか確認してください",
            status.code()
        ))
    }
}

/// `regsvr32 /s <dll>`（管理者権限が必要）
pub fn register(dll: &Path) -> Result<(), String> {
    run_regsvr32(&["/s".as_ref(), dll.as_os_str()])
}

/// `regsvr32 /u /s <dll>`
pub fn unregister(dll: &Path) -> Result<(), String> {
    run_regsvr32(&["/u".as_ref(), "/s".as_ref(), dll.as_os_str()])
}

/// このスレッドで有効化した IME
pub struct ImeSession {
    profiles: ITfInputProcessorProfileMgr,
    compartments: ITfCompartmentMgr,
    client_id: u32,
}

impl ImeSession {
    /// COM を初期化し、Nagi をこのプロセスの入力方式として有効化する。
    /// `TF_IPPMF_FORPROCESS` なので OS 全体の既定の入力方式は変えない
    pub fn activate() -> Result<Self, String> {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let thread_mgr: ITfThreadMgr =
            unsafe { CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| format!("ITfThreadMgr: {e}"))?;
        let client_id = unsafe { thread_mgr.Activate() }.map_err(|e| format!("Activate: {e}"))?;
        let compartments: ITfCompartmentMgr = thread_mgr
            .cast()
            .map_err(|e| format!("ITfCompartmentMgr: {e}"))?;
        let profiles: ITfInputProcessorProfileMgr = unsafe {
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
        }
        .map_err(|e| format!("ITfInputProcessorProfileMgr: {e}"))?;
        unsafe {
            profiles.ActivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                LANGID_JAPANESE,
                &CLSID_NAGI_TEXT_SERVICE,
                &GUID_NAGI_PROFILE,
                HKL::default(),
                TF_IPPMF_FORPROCESS
                    | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE
                    | TF_IPPMF_ENABLEPROFILE,
            )
        }
        .map_err(|e| format!("ActivateProfile に失敗: {e}"))?;
        let session = Self {
            profiles,
            compartments,
            client_id,
        };
        // TIP の読み込み（Activate）は非同期に進むことがあるので少し待つ
        let start = Instant::now();
        while !session.is_active() && start.elapsed() < Duration::from_secs(10) {
            pump(100);
        }
        if !session.is_active() {
            return Err(format!(
                "Nagi の入力方式が有効になりません: {}",
                session.describe()
            ));
        }
        Ok(session)
    }

    fn active_profile(&self) -> windows::core::Result<TF_INPUTPROCESSORPROFILE> {
        let mut profile = TF_INPUTPROCESSORPROFILE::default();
        unsafe {
            self.profiles
                .GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile)
        }?;
        Ok(profile)
    }

    fn is_active(&self) -> bool {
        self.active_profile()
            .is_ok_and(|p| p.clsid == CLSID_NAGI_TEXT_SERVICE && p.guidProfile == GUID_NAGI_PROFILE)
    }

    /// 診断用の状態（DLL が読み込まれたか・有効なプロファイル）
    pub fn describe(&self) -> String {
        let loaded = unsafe { GetModuleHandleW(w!("nagi_windows_tsf.dll")) }
            .is_ok_and(|h: HMODULE| !h.is_invalid());
        let active = self
            .active_profile()
            .map(|p| format!("clsid={:?} langid=0x{:04X}", p.clsid, p.langid))
            .unwrap_or_else(|e| format!("取得失敗 {e}"));
        format!(
            "dll_loaded={loaded} nagi_active={} active_profile=({active})",
            self.is_active()
        )
    }

    fn read_compartment(&self, guid: &GUID) -> Option<i32> {
        let comp = unsafe { self.compartments.GetCompartment(guid) }.ok()?;
        let value = unsafe { comp.GetValue() }.ok()?;
        i32::try_from(&value).ok()
    }

    /// 入力モードを「開・ひらがな」にする。変えたら `true`。
    /// TIP が自分でコンパートメントを書いた後（Activate 直後）はそのままのことが多い
    pub fn ensure_hiragana(&self) -> Result<bool, String> {
        let open = self.read_compartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
        let conversion = self.read_compartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION);
        if open.is_some_and(|o| o != 0) && conversion.is_some_and(|c| c & 0x1 != 0) {
            return Ok(false);
        }
        // 変換モードを先に、開閉を後に書く（TIP の `ModeCompartments::write` と同じ順）
        let set = |guid: &GUID, value: i32| -> Result<(), String> {
            let comp = unsafe { self.compartments.GetCompartment(guid) }
                .map_err(|e| format!("GetCompartment: {e}"))?;
            unsafe { comp.SetValue(self.client_id, &VARIANT::from(value)) }
                .map_err(|e| format!("SetValue: {e}"))
        };
        set(
            &GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
            CONVERSION_HIRAGANA,
        )?;
        set(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, 1)?;
        Ok(true)
    }
}
