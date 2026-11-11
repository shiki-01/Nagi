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
    GUID_TFCAT_TIP_KEYBOARD, ITfCompartmentMgr, ITfInputProcessorProfileMgr,
    ITfInputProcessorProfiles, ITfThreadMgr, TF_INPUTPROCESSORPROFILE,
    TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_IPPMF_ENABLEPROFILE, TF_IPPMF_FORPROCESS,
    TF_LANGUAGEPROFILE, TF_PROFILETYPE_INPUTPROCESSOR,
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

/// 外部コマンドを実行して「コマンド / 終了コード / 出力」を文字列にする（診断用）
fn run_capture(program: &std::ffi::OsStr, args: &[&std::ffi::OsStr]) -> (bool, String) {
    match Command::new(program).args(args).output() {
        Ok(out) => {
            let mut text = format!(
                "$ {} {}\n  exit={:?}\n",
                program.to_string_lossy(),
                args.iter()
                    .map(|a| a.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" "),
                out.status.code()
            );
            for stream in [&out.stdout, &out.stderr] {
                let t = String::from_utf8_lossy(stream);
                if !t.trim().is_empty() {
                    text.push_str(&t);
                    text.push('\n');
                }
            }
            (out.status.success(), text)
        }
        Err(e) => (
            false,
            format!("{} を起動できません: {e}\n", program.to_string_lossy()),
        ),
    }
}

fn run_regsvr32(args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let (ok, text) = run_capture(regsvr32().as_os_str(), args);
    println!("{text}");
    if ok {
        Ok(text)
    } else {
        Err(format!(
            "regsvr32 が失敗しました。管理者権限で実行しているか確認してください\n{text}"
        ))
    }
}

/// TIP の登録に関わるレジストリの内容（診断用）
pub fn registry_dump() -> String {
    let clsid = format!("{{{CLSID_NAGI_TEXT_SERVICE:?}}}");
    let mut out = String::new();
    let queries: [(String, bool); 5] = [
        (format!("HKCR\\CLSID\\{clsid}"), true),
        (
            format!("HKLM\\SOFTWARE\\Microsoft\\CTF\\TIP\\{clsid}"),
            true,
        ),
        (
            "HKCU\\Control Panel\\International\\User Profile".to_owned(),
            true,
        ),
        (
            "HKCU\\Software\\Microsoft\\CTF\\SortOrder\\Language".to_owned(),
            true,
        ),
        ("HKCU\\Keyboard Layout\\Preload".to_owned(), true),
    ];
    let reg = Path::new(&std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32")
        .join("reg.exe");
    for (key, recurse) in queries {
        let mut args: Vec<&std::ffi::OsStr> = vec!["query".as_ref(), key.as_ref()];
        if recurse {
            args.push("/s".as_ref());
        }
        out.push_str(&run_capture(reg.as_os_str(), &args).1);
    }
    out
}

/// `regsvr32 /s <dll>`（管理者権限が必要）。出力と終了コードを標準出力に残す
pub fn register(dll: &Path) -> Result<(), String> {
    run_regsvr32(&["/s".as_ref(), dll.as_os_str()])?;
    println!("--- 登録後のレジストリ ---\n{}", registry_dump());
    Ok(())
}

fn create_profile_mgr() -> Result<ITfInputProcessorProfileMgr, String> {
    // 二重初期化は無害（既に別のモードなら RPC_E_CHANGED_MODE が返るだけ）
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| format!("ITfInputProcessorProfileMgr: {e}"))
}

/// 全言語のプロファイルを列挙する
fn all_profiles(profiles: &ITfInputProcessorProfileMgr) -> Vec<TF_INPUTPROCESSORPROFILE> {
    let Ok(enumerator) = (unsafe { profiles.EnumProfiles(0) }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        let mut p = [TF_INPUTPROCESSORPROFILE::default()];
        let mut fetched = 0u32;
        if unsafe { enumerator.Next(&mut p, &mut fetched) }.is_err() || fetched == 0 {
            break;
        }
        out.push(p[0]);
    }
    out
}

fn nagi_listed(profiles: &ITfInputProcessorProfileMgr) -> bool {
    all_profiles(profiles)
        .iter()
        .any(|p| p.clsid == CLSID_NAGI_TEXT_SERVICE && p.guidProfile == GUID_NAGI_PROFILE)
}

/// 登録し、Nagi がプロファイルの列挙に現れるまで待つ（言語リストの変更直後などで一時的に
/// 列挙されないことがあるため）。15 秒ごとに `regsvr32` をやり直す。
/// 現れなくてもエラーにはしない（その先の有効化で診断を出す）
pub fn register_and_wait(dll: &Path, timeout: Duration) -> Result<(), String> {
    register(dll)?;
    let profiles = create_profile_mgr()?;
    let start = Instant::now();
    let mut last_retry = Instant::now();
    loop {
        // 列挙に出なくても GetProfile で直接取れるなら有効化できる（EnumProfiles は当てにならない）
        let gettable = {
            let mut p = TF_INPUTPROCESSORPROFILE::default();
            unsafe {
                profiles.GetProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_JAPANESE,
                    &CLSID_NAGI_TEXT_SERVICE,
                    &GUID_NAGI_PROFILE,
                    HKL::default(),
                    &mut p,
                )
            }
            .is_ok()
        };
        if gettable || nagi_listed(&profiles) {
            println!(
                "Nagi のプロファイルが列挙または GetProfile で見えました（{}ms）",
                start.elapsed().as_millis()
            );
            return Ok(());
        }
        if start.elapsed() >= timeout {
            println!(
                "警告: {}ms 待っても Nagi が列挙に現れません",
                timeout.as_millis()
            );
            return Ok(());
        }
        if last_retry.elapsed() >= Duration::from_secs(15) {
            println!("Nagi が列挙に現れないため regsvr32 をやり直します");
            register(dll)?;
            last_retry = Instant::now();
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// 言語別のプロファイル列挙と `GetProfile` による直接取得（Nagi が見えないときの診断）
fn dump_language_profiles(
    profiles: &ITfInputProcessorProfileMgr,
    legacy: Option<&ITfInputProcessorProfiles>,
) -> String {
    let mut out = String::new();
    if let Some(l) = legacy {
        for langid in [LANGID_JAPANESE, 0x0409u16] {
            match unsafe { l.EnumLanguageProfiles(langid) } {
                Ok(e) => {
                    let mut n = 0;
                    let mut lines = String::new();
                    loop {
                        let mut p = [TF_LANGUAGEPROFILE::default()];
                        let mut fetched = 0u32;
                        if unsafe { e.Next(&mut p, &mut fetched) }.is_err() || fetched == 0 {
                            break;
                        }
                        n += 1;
                        lines.push_str(&format!(
                            "  clsid={:?} profile={:?} active={}\n",
                            p[0].clsid,
                            p[0].guidProfile,
                            p[0].fActive.as_bool()
                        ));
                    }
                    out.push_str(&format!(
                        "EnumLanguageProfiles(0x{langid:04X}) {n} 件\n{lines}"
                    ));
                }
                Err(e) => {
                    out.push_str(&format!("EnumLanguageProfiles(0x{langid:04X}) 失敗: {e}\n"))
                }
            }
        }
        out.push_str(&format!(
            "IsEnabledLanguageProfile(Nagi) = {:?}\n",
            unsafe {
                l.IsEnabledLanguageProfile(
                    &CLSID_NAGI_TEXT_SERVICE,
                    LANGID_JAPANESE,
                    &GUID_NAGI_PROFILE,
                )
            }
            .map(|b| b.as_bool())
        ));
    }
    let mut p = TF_INPUTPROCESSORPROFILE::default();
    let r = unsafe {
        profiles.GetProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            LANGID_JAPANESE,
            &CLSID_NAGI_TEXT_SERVICE,
            &GUID_NAGI_PROFILE,
            HKL::default(),
            &mut p,
        )
    };
    out.push_str(&format!(
        "GetProfile(Nagi) = {r:?} (flags=0x{:08X} caps=0x{:X})\n",
        p.dwFlags, p.dwCaps
    ));
    out
}

/// `regsvr32 /u /s <dll>`
pub fn unregister(dll: &Path) -> Result<(), String> {
    run_regsvr32(&["/u".as_ref(), "/s".as_ref(), dll.as_os_str()]).map(|_| ())
}

/// このスレッドで有効化した IME
pub struct ImeSession {
    profiles: ITfInputProcessorProfileMgr,
    compartments: ITfCompartmentMgr,
    client_id: u32,
}

/// `profiles` に登録されている全言語のプロファイル（診断用）。Nagi には印を付ける
fn dump_profiles(profiles: &ITfInputProcessorProfileMgr) -> String {
    let mut out = String::new();
    let enumerator = match unsafe { profiles.EnumProfiles(0) } {
        Ok(e) => e,
        Err(e) => return format!("EnumProfiles に失敗: {e}\n"),
    };
    let mut count = 0;
    loop {
        let mut p = [TF_INPUTPROCESSORPROFILE::default()];
        let mut fetched = 0u32;
        if unsafe { enumerator.Next(&mut p, &mut fetched) }.is_err() || fetched == 0 {
            break;
        }
        count += 1;
        let p = p[0];
        let mine = p.clsid == CLSID_NAGI_TEXT_SERVICE;
        // TF_IPP_FLAG_ENABLED = 1, TF_IPP_FLAG_ACTIVE = 0x80000000
        out.push_str(&format!(
            "{} type={} langid=0x{:04X} clsid={:?} profile={:?} enabled={} active={}\n",
            if mine { "* NAGI" } else { "  other" },
            p.dwProfileType,
            p.langid,
            p.clsid,
            p.guidProfile,
            p.dwFlags & 1 != 0,
            p.dwFlags & 0x8000_0000 != 0
        ));
    }
    format!("登録済みプロファイル {count} 件:\n{out}")
}

impl ImeSession {
    /// COM を初期化し、Nagi をこのプロセスの入力方式として有効化する。
    /// `TF_IPPMF_FORPROCESS` なので OS 全体の既定の入力方式は変えない。
    /// 失敗したら複数の方法（有効化 → フラグ違い → 旧 API）を順に試し、全ての結果と診断を返す
    pub fn activate() -> Result<Self, String> {
        let mut log = String::new();
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        log.push_str(&format!("CoInitializeEx(STA) = {hr:?}\n"));
        let thread_mgr: ITfThreadMgr =
            unsafe { CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| format!("ITfThreadMgr: {e}\n{log}"))?;
        let client_id =
            unsafe { thread_mgr.Activate() }.map_err(|e| format!("Activate: {e}\n{log}"))?;
        let compartments: ITfCompartmentMgr = thread_mgr
            .cast()
            .map_err(|e| format!("ITfCompartmentMgr: {e}\n{log}"))?;
        let profiles: ITfInputProcessorProfileMgr = unsafe {
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
        }
        .map_err(|e| format!("ITfInputProcessorProfileMgr: {e}\n{log}"))?;
        let legacy: Option<ITfInputProcessorProfiles> = profiles.cast().ok();
        let session = Self {
            profiles,
            compartments,
            client_id,
        };

        log.push_str(&format!("[前] {}\n", session.describe()));
        log.push_str(&dump_profiles(&session.profiles));
        log.push_str(&dump_language_profiles(&session.profiles, legacy.as_ref()));
        if let Some(l) = &legacy {
            log.push_str(&format!(
                "現在の入力言語 GetCurrentLanguage = {:?}\n",
                unsafe { l.GetCurrentLanguage() }.map(|x| format!("0x{x:04X}"))
            ));
            // プロファイルを有効にしておく（ユーザーの言語リストに無い言語だと拒否されうる）
            let r = unsafe {
                l.EnableLanguageProfile(
                    &CLSID_NAGI_TEXT_SERVICE,
                    LANGID_JAPANESE,
                    &GUID_NAGI_PROFILE,
                    true,
                )
            };
            log.push_str(&format!("EnableLanguageProfile(true) = {r:?}\n"));
        }

        let process = TF_IPPMF_FORPROCESS;
        let attempts: [(&str, u32); 5] = [
            (
                "FORPROCESS|DONTCARE|ENABLE",
                process | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE | TF_IPPMF_ENABLEPROFILE,
            ),
            ("FORPROCESS|ENABLE", process | TF_IPPMF_ENABLEPROFILE),
            ("FORPROCESS", process),
            ("ENABLE", TF_IPPMF_ENABLEPROFILE),
            ("FORSESSION|ENABLE", 0x2000_0000 | TF_IPPMF_ENABLEPROFILE),
        ];
        let mut activated = false;
        for (name, flags) in attempts {
            let r = unsafe {
                session.profiles.ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_JAPANESE,
                    &CLSID_NAGI_TEXT_SERVICE,
                    &GUID_NAGI_PROFILE,
                    HKL::default(),
                    flags,
                )
            };
            log.push_str(&format!("ActivateProfile({name}) = {r:?}\n"));
            if r.is_ok() {
                activated = true;
                break;
            }
        }
        if !activated && let Some(l) = &legacy {
            let r1 = unsafe { l.ChangeCurrentLanguage(LANGID_JAPANESE) };
            let r2 = unsafe {
                l.ActivateLanguageProfile(
                    &CLSID_NAGI_TEXT_SERVICE,
                    LANGID_JAPANESE,
                    &GUID_NAGI_PROFILE,
                )
            };
            log.push_str(&format!(
                "旧 API: ChangeCurrentLanguage = {r1:?}, ActivateLanguageProfile = {r2:?}\n"
            ));
            activated = r2.is_ok();
        }
        println!("--- IME 有効化の試行 ---\n{log}");
        if !activated {
            return Err(format!(
                "ActivateProfile が全て失敗しました\n{log}[後] {}\n{}{}--- レジストリ ---\n{}",
                session.describe(),
                dump_profiles(&session.profiles),
                dump_language_profiles(&session.profiles, legacy.as_ref()),
                registry_dump()
            ));
        }
        // TIP の読み込み（Activate）は非同期に進むことがあるので少し待つ
        let start = Instant::now();
        while !session.is_active() && start.elapsed() < Duration::from_secs(10) {
            pump(100);
        }
        if !session.is_active() {
            return Err(format!(
                "Nagi の入力方式が有効になりません\n{log}[後] {}\n{}",
                session.describe(),
                dump_profiles(&session.profiles)
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

    /// 開閉・変換モードの現在値（診断用。`None` は未設定）
    pub fn describe_modes(&self) -> String {
        format!(
            "open={:?} conversion={:?}",
            self.read_compartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE),
            self.read_compartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION)
        )
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
