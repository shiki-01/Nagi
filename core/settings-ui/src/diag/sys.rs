//! OS への問い合わせ（Windows 依存部）の境界。
//!
//! 診断のロジックは [`System`] だけを見る。実装 [`RealSystem`] の Windows 側は PowerShell
//! （`-EncodedCommand`。スクリプトは ASCII のみ）の JSON 出力を [`WinFacts`] に読むだけで、
//! `unsafe` は使わない。出力の解釈（[`parse_facts`] など）は OS 非依存で、Linux で単体テストできる。
//! Windows 以外では [`RealSystem::facts`] は `None` を返す。

use serde::{Deserialize, Deserializer};
use std::path::Path;
use std::process::Child;

/// TSF の CLSID（`frontend-windows-tsf/src/guids.rs` の `CLSID_NAGI_TEXT_SERVICE` と同じ。変えると別物になる）
pub const CLSID: &str = "{B0D6897E-C414-4BB1-B621-46EEE659E0CD}";
/// 入力方式（言語プロファイル）の GUID（同 `GUID_NAGI_PROFILE`）
pub const PROFILE_GUID: &str = "{5B02A01F-6B4F-430F-A6A8-D9B4D83C1918}";
/// DLL のファイル名
pub const TSF_DLL: &str = "nagi_windows_tsf.dll";

/// 1 つの値でも配列でも読める（PowerShell の `ConvertTo-Json` は要素 1 つの配列を単体にすることがある）
fn one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr<T> {
        Many(Vec<T>),
        One(T),
        Null(()),
    }
    Ok(match Repr::<T>::deserialize(deserializer)? {
        Repr::Many(v) => v,
        Repr::One(v) => vec![v],
        Repr::Null(()) => Vec::new(),
    })
}

/// システムの VC++ ランタイム DLL 1 つ
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct VcFile {
    /// ファイル名
    pub name: String,
    /// `System32` にあるか
    pub exists: bool,
    /// ファイルバージョン
    pub version: String,
}

/// VC++ 再頒布可能パッケージのレジストリ記録（アーキテクチャごと）
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct VcReg {
    /// `x64` / `arm64` / `x86`
    pub arch: String,
    /// `Installed` の値（1 = 導入済み）
    pub installed: i64,
    /// バージョン
    pub version: String,
}

/// Authenticode 署名の状態（ファイル 1 つ）
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SigInfo {
    /// ファイル名
    pub file: String,
    /// `Get-AuthenticodeSignature` の Status（`Valid` / `NotSigned` / `HashMismatch` ほか）
    pub status: String,
    /// 署名者の Subject
    pub subject: String,
    /// タイムスタンプがあるか
    pub timestamped: bool,
}

/// プロセス（imserver）1 つ
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ProcInfo {
    /// PID
    pub pid: u32,
    /// 実行ファイルのパス（権限が無ければ空）
    pub path: String,
}

/// PowerShell 1 回で集める Windows の事実。欠けた項目は既定値（取れなかった = 判定不能）
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WinFacts {
    /// OS の名称
    pub os_caption: String,
    /// OS のバージョン
    pub os_version: String,
    /// ビルド番号
    pub os_build: String,
    /// `RuntimeInformation.OSArchitecture`（`X64` / `Arm64` / `X86`）
    pub os_arch: String,
    /// `RuntimeInformation.ProcessArchitecture`（PowerShell 自身）
    pub proc_arch: String,
    /// `PROCESSOR_ARCHITECTURE`
    pub env_arch: String,
    /// `PROCESSOR_ARCHITEW6432`
    pub env_arch_w6432: String,
    /// `elevated_admin` / `admin_not_elevated` / `standard`
    pub account: String,
    /// `HKCR\CLSID\{...}\InprocServer32` の既定値（DLL のパス）
    pub clsid_inproc: String,
    /// `ThreadingModel`
    pub clsid_threading: String,
    /// `HKLM\SOFTWARE\Microsoft\CTF\TIP\{CLSID}\LanguageProfile\0x00000411\{プロファイル}` があるか
    pub tip_profile_exists: bool,
    /// プロファイルの Description
    pub tip_profile_desc: String,
    /// 登録されたカテゴリ（`{GUID}`）
    #[serde(deserialize_with = "one_or_many")]
    pub tip_categories: Vec<String>,
    /// このユーザーの入力方式（`0411:{CLSID}{PROFILE}` 形式）
    #[serde(deserialize_with = "one_or_many")]
    pub user_tips: Vec<String>,
    /// VC++ ランタイム DLL
    #[serde(deserialize_with = "one_or_many")]
    pub vc_files: Vec<VcFile>,
    /// VC++ 再頒布可能パッケージの記録
    #[serde(deserialize_with = "one_or_many")]
    pub vc_reg: Vec<VcReg>,
    /// 画面の DPI（`AppliedDPI`。96 = 100%）
    pub dpi: Option<u32>,
    /// OS のタイムゾーンの UTC からの差（分。現在時刻の値。`.old` のファイル名のローカル時刻を UTC にするのに使う）
    pub utc_offset_minutes: Option<i32>,
    /// アプリのテーマがライトか
    pub apps_light: Option<bool>,
    /// システムのテーマがライトか
    pub system_light: Option<bool>,
    /// ハイコントラストがオンか
    pub high_contrast: Option<bool>,
    /// モニター数
    pub monitors: Option<u32>,
    /// 署名の状態
    #[serde(deserialize_with = "one_or_many")]
    pub signatures: Vec<SigInfo>,
    /// DLL を読み込んでいるプロセス名（重複なし）
    #[serde(deserialize_with = "one_or_many")]
    pub dll_hosts: Vec<String>,
    /// 動作中の imserver
    #[serde(deserialize_with = "one_or_many")]
    pub imserver_procs: Vec<ProcInfo>,
}

/// 出力（JSON 1 つ。前後に余計な行があってもよい）を [`WinFacts`] に読む
pub fn parse_facts(output: &str) -> Option<WinFacts> {
    serde_json::from_str(json_part(output)?).ok()
}

/// 出力から最初の `{` から最後の `}` までを取り出す
fn json_part(output: &str) -> Option<&str> {
    let start = output.find('{')?;
    let end = output.rfind('}')?;
    (start < end).then(|| &output[start..=end])
}

/// DLL を COM で作って調べた結果（`-STA` の PowerShell で `CoCreateInstance` と `QueryInterface`）
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ComProbe {
    /// `CoCreateInstance` できたか
    pub created: bool,
    /// `ITfTextInputProcessor` が取れたか
    #[serde(rename = "ITfTextInputProcessor")]
    pub text_input_processor: bool,
    /// `ITfTextInputProcessorEx` が取れたか
    #[serde(rename = "ITfTextInputProcessorEx")]
    pub text_input_processor_ex: bool,
    /// `ITfFnConfigure` が取れたか（言語のオプションから設定画面を開く入口）
    #[serde(rename = "ITfFnConfigure")]
    pub fn_configure: bool,
    /// `ITfDisplayAttributeProvider` が取れたか
    #[serde(rename = "ITfDisplayAttributeProvider")]
    pub display_attribute_provider: bool,
    /// 失敗の理由（例外メッセージ。入力内容は含まない）
    pub error: String,
}

/// [`ComProbe`] の出力を読む
pub fn parse_com_probe(output: &str) -> Option<ComProbe> {
    serde_json::from_str(json_part(output)?).ok()
}

/// プロセスのメモリ（KB）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcMemory {
    /// ワーキングセット（物理メモリ上の量。mmap したモデルのページを含む）
    pub working_set_kb: u64,
    /// 専用メモリ（Windows の PrivateMemorySize64 / Linux の RssAnon）。取れなければ `None`
    pub private_kb: Option<u64>,
}

/// 親プロセス消滅の検証に使う、親役の子プロセス
pub struct Sleeper {
    child: Child,
}

impl Sleeper {
    /// 親役の PID
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// 終了させる（imserver の親が消えた状態にする）
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        self.kill();
    }
}

/// OS への問い合わせ（テストではフェイクを差し込む）
pub trait System: Send + Sync {
    /// Windows か（Windows 専用の項目を実行するか）
    fn is_windows(&self) -> bool;
    /// Windows の事実（PowerShell 1 回分）。`install_dir` は署名の検査対象。取れなければ `None`
    fn facts(&self, install_dir: &Path) -> Option<WinFacts>;
    /// 登録された TIP を COM で作ってインターフェイスを調べる。実行できなければ `None`
    fn com_probe(&self) -> Option<ComProbe>;
    /// プロセスのメモリ。取れなければ `None`
    fn process_memory(&self, pid: u32) -> Option<ProcMemory>;
    /// 親プロセス役を起動する（Windows のみ。他は `None`）
    fn spawn_sleeper(&self) -> Option<Sleeper>;
}

/// 実機の実装
pub struct RealSystem;

impl System for RealSystem {
    fn is_windows(&self) -> bool {
        cfg!(windows)
    }

    fn facts(&self, install_dir: &Path) -> Option<WinFacts> {
        imp::facts(install_dir)
    }

    fn com_probe(&self) -> Option<ComProbe> {
        imp::com_probe()
    }

    fn process_memory(&self, pid: u32) -> Option<ProcMemory> {
        imp::process_memory(pid)
    }

    fn spawn_sleeper(&self) -> Option<Sleeper> {
        imp::spawn_sleeper()
    }
}

/// PowerShell の `-EncodedCommand` 用に、UTF-16LE にして Base64 にする
pub fn encode_command(script: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 事実を集める PowerShell スクリプト（ASCII のみ。インストール先は環境変数 `NAGI_DIAG_INSTALL`）
pub fn facts_script() -> String {
    FACTS_SCRIPT
        .replace("@@CLSID@@", CLSID)
        .replace("@@PROFILE@@", PROFILE_GUID)
        .replace("@@DLL@@", TSF_DLL)
}

/// COM の調査用スクリプト（登録済みの CLSID から作る。`-STA` で実行すること。
/// TIP は `ThreadingModel=Apartment` なので、MTA だとプロキシ越しになり `ITfFnConfigure` が見えない）
pub fn com_probe_script() -> String {
    COM_PROBE_SCRIPT.replace("@@CLSID@@", CLSID)
}

const FACTS_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$clsid = '@@CLSID@@'
$prof = '@@PROFILE@@'
$install = $env:NAGI_DIAG_INSTALL
$HKLM = [Microsoft.Win32.Registry]::LocalMachine
$HKCU = [Microsoft.Win32.Registry]::CurrentUser
$HKCR = [Microsoft.Win32.Registry]::ClassesRoot
function RegVal($root, $path, $name) {
  try { $k = $root.OpenSubKey($path); if ($k -ne $null) { $v = $k.GetValue($name); $k.Close(); return $v } } catch {}
  return $null
}
function RegNames($root, $path) {
  try { $k = $root.OpenSubKey($path); if ($k -ne $null) { $n = $k.GetSubKeyNames(); $k.Close(); return $n } } catch {}
  return @()
}
$r = [ordered]@{}
try { $r.os_arch = [string][System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture } catch {}
try { $r.proc_arch = [string][System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture } catch {}
$r.env_arch = [string]$env:PROCESSOR_ARCHITECTURE
$r.env_arch_w6432 = [string]$env:PROCESSOR_ARCHITEW6432
try {
  $os = Get-CimInstance Win32_OperatingSystem
  $r.os_caption = [string]$os.Caption
  $r.os_version = [string]$os.Version
  $r.os_build = [string]$os.BuildNumber
} catch {}
try {
  $id = [Security.Principal.WindowsIdentity]::GetCurrent()
  $admin = New-Object Security.Principal.SecurityIdentifier('S-1-5-32-544')
  $member = $false
  foreach ($g in $id.Groups) { if ($g.Value -eq 'S-1-5-32-544') { $member = $true } }
  $elev = (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole($admin)
  if ($elev) { $r.account = 'elevated_admin' } elseif ($member) { $r.account = 'admin_not_elevated' } else { $r.account = 'standard' }
} catch {}
$r.clsid_inproc = [string](RegVal $HKCR "CLSID\$clsid\InprocServer32" '')
$r.clsid_threading = [string](RegVal $HKCR "CLSID\$clsid\InprocServer32" 'ThreadingModel')
$p = "SOFTWARE\Microsoft\CTF\TIP\$clsid\LanguageProfile\0x00000411\$prof"
$pk = $HKLM.OpenSubKey($p)
$r.tip_profile_exists = ($pk -ne $null)
if ($pk -ne $null) { $r.tip_profile_desc = [string]$pk.GetValue('Description'); $pk.Close() }
$r.tip_categories = @(RegNames $HKLM "SOFTWARE\Microsoft\CTF\TIP\$clsid\Category\Category")
$tips = @()
try { foreach ($l in (Get-WinUserLanguageList)) { foreach ($t in $l.InputMethodTips) { $tips += [string]$t } } } catch {}
if ($tips.Count -eq 0) {
  foreach ($n in (RegNames $HKCU 'Control Panel\International\User Profile')) {
    $k = $HKCU.OpenSubKey("Control Panel\International\User Profile\$n")
    if ($k -ne $null) { foreach ($vn in $k.GetValueNames()) { if ($vn -like '*:{*') { $tips += $vn } }; $k.Close() }
  }
}
$r.user_tips = @($tips)
$sys = [Environment]::GetFolderPath('System')
$vc = @()
foreach ($n in 'vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll') {
  $f = Join-Path $sys $n
  if (Test-Path -LiteralPath $f) { $vc += [ordered]@{ name = $n; exists = $true; version = [string](Get-Item -LiteralPath $f).VersionInfo.FileVersion } }
  else { $vc += [ordered]@{ name = $n; exists = $false; version = '' } }
}
$r.vc_files = @($vc)
$vr = @()
foreach ($a in 'x64', 'arm64', 'x86') {
  if ($a -eq 'x86') { $path = 'SOFTWARE\WOW6432Node\Microsoft\VisualStudio\14.0\VC\Runtimes\x86' } else { $path = "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\$a" }
  $k = $HKLM.OpenSubKey($path)
  if ($k -ne $null) { $vr += [ordered]@{ arch = $a; installed = [int]$k.GetValue('Installed'); version = [string]$k.GetValue('Version') }; $k.Close() }
}
$r.vc_reg = @($vr)
try { $r.utc_offset_minutes = [int][TimeZoneInfo]::Local.GetUtcOffset([DateTime]::Now).TotalMinutes } catch {}
$v = RegVal $HKCU 'Control Panel\Desktop\WindowMetrics' 'AppliedDPI'
if ($v -ne $null) { $r.dpi = [int]$v }
$v = RegVal $HKCU 'Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' 'AppsUseLightTheme'
if ($v -ne $null) { $r.apps_light = ([int]$v -eq 1) }
$v = RegVal $HKCU 'Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' 'SystemUsesLightTheme'
if ($v -ne $null) { $r.system_light = ([int]$v -eq 1) }
$v = RegVal $HKCU 'Control Panel\Accessibility\HighContrast' 'Flags'
if ($v -ne $null) { $r.high_contrast = ((([int]$v) -band 1) -eq 1) }
try { Add-Type -AssemblyName System.Windows.Forms; $r.monitors = [int][System.Windows.Forms.Screen]::AllScreens.Count } catch {}
$sig = @()
if ($install) {
  foreach ($n in '@@DLL@@', 'imserver.exe', 'nagi-settings.exe') {
    $f = Join-Path $install $n
    if (Test-Path -LiteralPath $f) {
      $s = Get-AuthenticodeSignature -LiteralPath $f
      $sig += [ordered]@{ file = $n; status = [string]$s.Status; subject = [string]$s.SignerCertificate.Subject; timestamped = ($s.TimeStamperCertificate -ne $null) }
    }
  }
}
$r.signatures = @($sig)
$hosts = @()
try {
  $out = & "$env:SystemRoot\System32\tasklist.exe" /m '@@DLL@@' /fo csv /nh 2>$null
  foreach ($line in $out) { if ($line -match '^"([^"]+)","(\d+)"') { $hosts += $Matches[1] } }
} catch {}
$r.dll_hosts = @($hosts | Sort-Object -Unique)
$ps = @()
foreach ($p in (Get-Process -Name imserver)) {
  $path = $null
  try { $path = $p.Path } catch {}
  $ps += [ordered]@{ pid = [int]$p.Id; path = [string]$path }
}
$r.imserver_procs = @($ps)
$r | ConvertTo-Json -Depth 5 -Compress
"#;

const COM_PROBE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$o = [ordered]@{ created = $false; error = '' }
try {
  $t = [type]::GetTypeFromCLSID([guid]'@@CLSID@@')
  $obj = [Activator]::CreateInstance($t)
  $o.created = $true
  $unk = [System.Runtime.InteropServices.Marshal]::GetIUnknownForObject($obj)
  $pairs = @(
    @('ITfTextInputProcessor', 'aa80e7f7-2021-11d2-93e0-0060b067b86e'),
    @('ITfTextInputProcessorEx', '6e4e2102-f9cd-433d-b496-303ce03a6507'),
    @('ITfFnConfigure', '88f567c6-1757-49f8-a1b2-89234c1eeff9'),
    @('ITfDisplayAttributeProvider', 'fee47777-163c-4769-996a-6e9c50ad8f54')
  )
  foreach ($pair in $pairs) {
    $iid = [guid]$pair[1]
    $ptr = [IntPtr]::Zero
    $hr = [System.Runtime.InteropServices.Marshal]::QueryInterface($unk, [ref]$iid, [ref]$ptr)
    $o[$pair[0]] = ($hr -eq 0)
    if ($ptr -ne [IntPtr]::Zero) { [void][System.Runtime.InteropServices.Marshal]::Release($ptr) }
  }
  [void][System.Runtime.InteropServices.Marshal]::Release($unk)
} catch {
  $o.error = [string]$_.Exception.Message
}
$o | ConvertTo-Json -Compress
"#;

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn powershell_path() -> std::path::PathBuf {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        Path::new(&root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe")
    }

    /// スクリプトを実行して標準出力を返す。`timeout` を過ぎたら強制終了して `None`
    fn run_script(
        script: &str,
        sta: bool,
        envs: &[(&str, &std::ffi::OsStr)],
        timeout: Duration,
    ) -> Option<String> {
        let mut command = Command::new(powershell_path());
        command.args(["-NoProfile", "-NonInteractive"]);
        if sta {
            command.arg("-STA");
        }
        command
            .arg("-EncodedCommand")
            .arg(encode_command(script))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        for (k, v) in envs {
            command.env(k, v);
        }
        let mut child = command.spawn().ok()?;
        let mut stdout = child.stdout.take()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stdout, &mut bytes);
            let _ = tx.send(bytes);
        });
        match rx.recv_timeout(timeout) {
            Ok(bytes) => {
                let _ = child.wait();
                Some(String::from_utf8_lossy(&bytes).into_owned())
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                None
            }
        }
    }

    pub fn facts(install_dir: &Path) -> Option<WinFacts> {
        let out = run_script(
            &facts_script(),
            false,
            &[("NAGI_DIAG_INSTALL", install_dir.as_os_str())],
            Duration::from_secs(90),
        )?;
        parse_facts(&out)
    }

    pub fn com_probe() -> Option<ComProbe> {
        let out = run_script(&com_probe_script(), true, &[], Duration::from_secs(60))?;
        parse_com_probe(&out)
    }

    pub fn process_memory(pid: u32) -> Option<ProcMemory> {
        let script = format!(
            "$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; \
             if ($p) {{ '{{0}} {{1}}' -f $p.WorkingSet64, $p.PrivateMemorySize64 }}"
        );
        let out = run_script(&script, false, &[], Duration::from_secs(30))?;
        parse_memory_line(&out)
    }

    pub fn spawn_sleeper() -> Option<Sleeper> {
        // 約 2 分で自然に終わる。stdout は捨てる（ping の出力でパイプを詰まらせない）
        let child = Command::new("cmd.exe")
            .args(["/C", "ping", "-n", "120", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .ok()?;
        Some(Sleeper { child })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn facts(_install_dir: &Path) -> Option<WinFacts> {
        None
    }

    pub fn com_probe() -> Option<ComProbe> {
        None
    }

    /// Linux（開発・テスト用）: `/proc/<pid>/status` の `VmRSS` と `RssAnon`
    pub fn process_memory(pid: u32) -> Option<ProcMemory> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        parse_proc_status(&text)
    }

    pub fn spawn_sleeper() -> Option<Sleeper> {
        None
    }
}

/// `"<working set> <private>"`（バイト）の 1 行を読む（Windows の `process_memory`）
pub fn parse_memory_line(output: &str) -> Option<ProcMemory> {
    let line = output.lines().find(|l| !l.trim().is_empty())?;
    let mut it = line.split_whitespace();
    let ws: u64 = it.next()?.parse().ok()?;
    let private: Option<u64> = it.next().and_then(|v| v.parse().ok());
    Some(ProcMemory {
        working_set_kb: ws / 1024,
        private_kb: private.map(|p| p / 1024),
    })
}

/// `/proc/<pid>/status` から `VmRSS` と `RssAnon`（KB）を読む
pub fn parse_proc_status(text: &str) -> Option<ProcMemory> {
    let field = |name: &str| -> Option<u64> {
        text.lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next())
            .and_then(|v| v.parse().ok())
    };
    Some(ProcMemory {
        working_set_kb: field("VmRSS")?,
        private_kb: field("RssAnon"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_parse_tolerates_scalars_and_noise() {
        let out = "warning line\n{\"os_arch\":\"Arm64\",\"user_tips\":\"0411:{A}{B}\",\
            \"tip_categories\":[\"{X}\",\"{Y}\"],\"vc_files\":{\"name\":\"a.dll\",\"exists\":true,\"version\":\"14\"},\
            \"signatures\":[],\"dpi\":144,\"imserver_procs\":null,\"high_contrast\":false}\ntrailer";
        let f = parse_facts(out).unwrap();
        assert_eq!(f.os_arch, "Arm64");
        assert_eq!(f.user_tips, vec!["0411:{A}{B}".to_owned()]);
        assert_eq!(f.tip_categories.len(), 2);
        assert_eq!(f.vc_files.len(), 1);
        assert!(f.vc_files[0].exists);
        assert_eq!(f.dpi, Some(144));
        assert_eq!(f.high_contrast, Some(false));
        assert!(f.imserver_procs.is_empty());
        assert!(parse_facts("no json here").is_none());
        assert!(parse_facts("{broken").is_none());
    }

    #[test]
    fn com_probe_parses_interface_names() {
        let out = "{\"created\":true,\"error\":\"\",\"ITfTextInputProcessor\":true,\
            \"ITfTextInputProcessorEx\":true,\"ITfFnConfigure\":false,\"ITfDisplayAttributeProvider\":true}";
        let p = parse_com_probe(out).unwrap();
        assert!(p.created && p.text_input_processor && p.text_input_processor_ex);
        assert!(!p.fn_configure);
        assert!(p.display_attribute_provider);
    }

    #[test]
    fn encode_command_is_base64_of_utf16le() {
        // "A" = 41 00 -> QQA=
        assert_eq!(encode_command("A"), "QQA=");
        // "ab" = 61 00 62 00 -> YQBiAA==
        assert_eq!(encode_command("ab"), "YQBiAA==");
        // "abc" = 61 00 62 00 63 00 -> YQBiAGMA
        assert_eq!(encode_command("abc"), "YQBiAGMA");
        assert_eq!(encode_command(""), "");
    }

    #[test]
    fn scripts_are_ascii_and_fully_substituted() {
        for script in [facts_script(), com_probe_script()] {
            assert!(script.is_ascii());
            assert!(!script.contains("@@"));
            assert!(script.contains(CLSID));
        }
        assert!(facts_script().contains(PROFILE_GUID));
        assert!(facts_script().contains(TSF_DLL));
    }

    #[test]
    fn memory_parsers() {
        let m = parse_memory_line("\r\n 52428800 31457280\r\n").unwrap();
        assert_eq!((m.working_set_kb, m.private_kb), (51200, Some(30720)));
        assert!(parse_memory_line("").is_none());
        assert!(parse_memory_line("abc").is_none());
        let status = "Name:\timserver\nVmRSS:\t  123456 kB\nRssAnon:\t   100000 kB\n";
        let m = parse_proc_status(status).unwrap();
        assert_eq!((m.working_set_kb, m.private_kb), (123_456, Some(100_000)));
        assert!(parse_proc_status("Name: x").is_none());
    }
}
