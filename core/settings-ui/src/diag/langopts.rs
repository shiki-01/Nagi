//! 言語のオプション・プローブ（`tsf.language_options`）。
//!
//! Windows 11 の「設定 → 時刻と言語 → 言語と地域 → 日本語 → 言語のオプション → キーボード」で、Nagi に
//! Microsoft IME のような「オプション」「削除」が出ない原因を 1 クリックで切り分ける（decisions.md H-126 候補）。
//! 判定は OK / NG にせず、登録値・DLL の ACL・設定アプリの DLL 読み込み・Microsoft IME との差分を並べ、
//! 見立て（a）〜（e）それぞれに「該当しそうか」を付ける。レポートをそのまま貼れば原因を追えるよう、根拠を evidence に出す。
//!
//! 事実の収集は PowerShell（`sys::encode_command`・ASCII のみ・`unsafe` なし）。解釈・比較・整形は OS 非依存の
//! 純粋な関数で、Linux で単体テストできる。結果に入力内容は含めない。

use super::ctx::Ctx;
use super::sys::{CLSID, PROFILE_GUID, TSF_DLL};
use super::types::{ItemId, ItemResult};
use serde::{Deserialize, Deserializer};

/// Microsoft IME（日本語）の CLSID
pub const MS_IME_CLSID: &str = "{03B5835F-F03C-411B-9CE2-AA23E1171E36}";
/// Microsoft IME（日本語）のプロファイル GUID
pub const MS_IME_PROFILE: &str = "{A76C93D9-5523-4E90-AAFA-4DB112F9AC76}";

/// ALL APPLICATION PACKAGES
pub const SID_ALL_APP_PACKAGES: &str = "S-1-15-2-1";
/// ALL RESTRICTED APPLICATION PACKAGES
pub const SID_ALL_RESTRICTED_APP_PACKAGES: &str = "S-1-15-2-2";
/// BUILTIN\Users
pub const SID_USERS: &str = "S-1-5-32-545";

/// 設定アプリの本体プロセス名（拡張子なし）。DLL を読み込むかを見る
pub const SETTINGS_PROCESSES: &[&str] = &["SystemSettings", "ApplicationFrameHost"];

fn many<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
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
    Ok(match Repr::<T>::deserialize(d)? {
        Repr::Many(v) => v,
        Repr::One(v) => vec![v],
        Repr::Null(()) => Vec::new(),
    })
}

/// レジストリ値 1 つ
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RegValue {
    /// 値の名前（既定値は空）
    pub name: String,
    /// `String` / `DWord` / `ExpandString` など
    pub kind: String,
    /// 値（文字列化。バイナリは 16 進）
    pub data: String,
}

/// 入力方式 1 つ分のレジストリ事実
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct EntryFacts {
    /// 表示用のラベル
    pub label: String,
    /// `HKLM\SOFTWARE\Microsoft\CTF\TIP\{CLSID}` があるか
    pub tip_exists: bool,
    /// その直下の値
    #[serde(deserialize_with = "many")]
    pub tip_values: Vec<RegValue>,
    /// その直下のサブキー名
    #[serde(deserialize_with = "many")]
    pub tip_subkeys: Vec<String>,
    /// `...\LanguageProfile\0x00000411\{プロファイル}` があるか
    pub profile_exists: bool,
    /// その値
    #[serde(deserialize_with = "many")]
    pub profile_values: Vec<RegValue>,
    /// そのサブキー名
    #[serde(deserialize_with = "many")]
    pub profile_subkeys: Vec<String>,
    /// `Category\Category` 直下の GUID
    #[serde(deserialize_with = "many")]
    pub categories: Vec<String>,
    /// `HKCR\CLSID\{...}\InprocServer32` の既定値
    pub inproc_path: String,
    /// `ThreadingModel`
    pub inproc_threading: String,
    /// `HKCU\Control Panel\International\User Profile\<言語>` にある、この入力方式の ID の値
    /// （名前は `言語\0411:{CLSID}{プロファイル}`）
    #[serde(deserialize_with = "many")]
    pub user_ids: Vec<RegValue>,
}

/// ACL の項目 1 つ
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AclEntry {
    /// SID
    pub sid: String,
    /// `FileSystemRights` の文字列
    pub rights: String,
    /// `FileSystemRights` の数値
    pub mask: i64,
    /// `Allow` / `Deny`
    pub kind: String,
    /// 継承された項目か
    pub inherited: bool,
    /// 子にだけ効く項目（このオブジェクト自身には効かない）か
    pub inherit_only: bool,
}

/// ファイル・フォルダ 1 つの ACL
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AclFacts {
    /// 対象の呼び名（`dll` / `icon` / `install_dir` / `program_files`）
    pub target: String,
    /// 存在するか
    pub exists: bool,
    /// 継承が無効か（親の ACL を引き継いでいない）
    pub protected: bool,
    /// 項目
    #[serde(deserialize_with = "many")]
    pub entries: Vec<AclEntry>,
    /// 取得に失敗した理由
    pub error: String,
}

/// 収集した事実
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Probe {
    /// OS のビルド番号
    pub os_build: String,
    /// Nagi
    pub nagi: EntryFacts,
    /// Microsoft IME（比較の基準）
    pub ms: EntryFacts,
    /// ACL
    #[serde(deserialize_with = "many")]
    pub acls: Vec<AclFacts>,
    /// 動作中の設定アプリ関連プロセス（拡張子なし）
    #[serde(deserialize_with = "many")]
    pub settings_running: Vec<String>,
    /// Nagi の DLL を読み込んでいるプロセス（`tasklist /m`。拡張子つき）
    #[serde(deserialize_with = "many")]
    pub dll_hosts: Vec<String>,
}

/// 出力（JSON 1 つ。前後に余計な行があってもよい）を読む
pub fn parse_probe(output: &str) -> Option<Probe> {
    let start = output.find('{')?;
    let end = output.rfind('}')?;
    (start < end)
        .then(|| serde_json::from_str(&output[start..=end]).ok())
        .flatten()
}

/// 事実を集める PowerShell スクリプト（ASCII のみ。インストール先は環境変数 `NAGI_DIAG_INSTALL`）
pub fn probe_script() -> String {
    PROBE_SCRIPT
        .replace("@@CLSID@@", CLSID)
        .replace("@@PROFILE@@", PROFILE_GUID)
        .replace("@@MSCLSID@@", MS_IME_CLSID)
        .replace("@@MSPROFILE@@", MS_IME_PROFILE)
        .replace("@@DLL@@", TSF_DLL)
}

const PROBE_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$install = $env:NAGI_DIAG_INSTALL
$HKLM = [Microsoft.Win32.Registry]::LocalMachine
$HKCU = [Microsoft.Win32.Registry]::CurrentUser
$HKCR = [Microsoft.Win32.Registry]::ClassesRoot
function Dump($root, $path) {
  $vals = @()
  try {
    $k = $root.OpenSubKey($path)
    if ($k -ne $null) {
      foreach ($n in $k.GetValueNames()) {
        $kind = [string]$k.GetValueKind($n)
        $v = $k.GetValue($n, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if ($v -is [byte[]]) { $v = (($v | ForEach-Object { $_.ToString('x2') }) -join '') }
        elseif ($v -is [string[]]) { $v = ($v -join '|') }
        $vals += [ordered]@{ name = [string]$n; kind = $kind; data = [string]$v }
      }
      $k.Close()
    }
  } catch {}
  return ,$vals
}
function Names($root, $path) {
  try { $k = $root.OpenSubKey($path); if ($k -ne $null) { $n = @($k.GetSubKeyNames()); $k.Close(); return ,$n } } catch {}
  return ,@()
}
function Entry($label, $clsid, $prof) {
  $tp = "SOFTWARE\Microsoft\CTF\TIP\$clsid"
  $pp = "$tp\LanguageProfile\0x00000411\$prof"
  $e = [ordered]@{ label = $label }
  $k = $HKLM.OpenSubKey($tp); $e.tip_exists = ($k -ne $null); if ($k -ne $null) { $k.Close() }
  $e.tip_values = Dump $HKLM $tp
  $e.tip_subkeys = Names $HKLM $tp
  $k = $HKLM.OpenSubKey($pp); $e.profile_exists = ($k -ne $null); if ($k -ne $null) { $k.Close() }
  $e.profile_values = Dump $HKLM $pp
  $e.profile_subkeys = Names $HKLM $pp
  $e.categories = Names $HKLM "$tp\Category\Category"
  $e.inproc_path = ''
  $e.inproc_threading = ''
  try {
    $k = $HKCR.OpenSubKey("CLSID\$clsid\InprocServer32")
    if ($k -ne $null) { $e.inproc_path = [string]$k.GetValue('', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); $e.inproc_threading = [string]$k.GetValue('ThreadingModel'); $k.Close() }
  } catch {}
  $ids = @()
  $up = 'Control Panel\International\User Profile'
  foreach ($lang in (Names $HKCU $up)) {
    $k = $HKCU.OpenSubKey("$up\$lang")
    if ($k -ne $null) {
      foreach ($n in $k.GetValueNames()) {
        if ($n -like "*$clsid*") { $ids += [ordered]@{ name = "$lang\$n"; kind = [string]$k.GetValueKind($n); data = [string]$k.GetValue($n) } }
      }
      $k.Close()
    }
  }
  $e.user_ids = @($ids)
  return $e
}
function AclOf($target, $path) {
  $o = [ordered]@{ target = $target; exists = $false; protected = $false; entries = @(); error = '' }
  try {
    if ($path -and (Test-Path -LiteralPath $path)) {
      $o.exists = $true
      $acl = Get-Acl -LiteralPath $path
      $o.protected = [bool]$acl.AreAccessRulesProtected
      $es = @()
      foreach ($a in $acl.Access) {
        $sid = ''
        try { $sid = $a.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value } catch { $sid = [string]$a.IdentityReference }
        $es += [ordered]@{ sid = $sid; rights = [string]$a.FileSystemRights; mask = [int64][int]$a.FileSystemRights; kind = [string]$a.AccessControlType; inherited = [bool]$a.IsInherited; inherit_only = ((([int]$a.PropagationFlags) -band 2) -ne 0) }
      }
      $o.entries = @($es)
    }
  } catch { $o.error = [string]$_.Exception.Message }
  return $o
}
$r = [ordered]@{}
try { $r.os_build = [string][Environment]::OSVersion.Version.Build } catch {}
$r.nagi = Entry 'Nagi' '@@CLSID@@' '@@PROFILE@@'
$r.ms = Entry 'Microsoft IME' '@@MSCLSID@@' '@@MSPROFILE@@'
$acls = @()
if ($install) {
  $acls += AclOf 'dll' (Join-Path $install '@@DLL@@')
  $acls += AclOf 'icon' (Join-Path $install 'nagi.ico')
  $acls += AclOf 'install_dir' $install
}
$acls += AclOf 'program_files' $env:ProgramFiles
$r.acls = @($acls)
$run = @()
foreach ($n in 'SystemSettings', 'ApplicationFrameHost') { if (Get-Process -Name $n) { $run += $n } }
$r.settings_running = @($run)
$hosts = @()
try {
  $out = & "$env:SystemRoot\System32\tasklist.exe" /m '@@DLL@@' /fo csv /nh 2>$null
  foreach ($line in $out) { if ($line -match '^"([^"]+)","(\d+)"') { $hosts += $Matches[1] } }
} catch {}
$r.dll_hosts = @($hosts | Sort-Object -Unique)
$r | ConvertTo-Json -Depth 6 -Compress
"#;

// ---- 純粋な解釈 ----

/// ReadAndExecute（0x1200A9）を満たすか。GENERIC_ALL / GENERIC_EXECUTE / GENERIC_READ（子に効く総称権限）も許す
pub fn mask_grants_read_execute(mask: i64) -> bool {
    const READ_EXECUTE: u32 = 0x0012_00A9;
    const GENERIC_ALL: u32 = 0x1000_0000;
    const GENERIC_EXECUTE: u32 = 0x2000_0000;
    let m = mask as i32 as u32;
    m & READ_EXECUTE == READ_EXECUTE || m & (GENERIC_ALL | GENERIC_EXECUTE) != 0
}

/// この ACL がその SID に（このオブジェクト自身へ）読み取り・実行を許しているか。Deny があれば `false`
pub fn grants_read_execute(acl: &AclFacts, sid: &str) -> bool {
    let mine = |e: &&AclEntry| e.sid.eq_ignore_ascii_case(sid) && !e.inherit_only;
    if acl
        .entries
        .iter()
        .filter(mine)
        .any(|e| e.kind.eq_ignore_ascii_case("Deny"))
    {
        return false;
    }
    acl.entries
        .iter()
        .filter(mine)
        .any(|e| e.kind.eq_ignore_ascii_case("Allow") && mask_grants_read_execute(e.mask))
}

fn find_acl<'a>(p: &'a Probe, target: &str) -> Option<&'a AclFacts> {
    p.acls.iter().find(|a| a.target == target)
}

/// カテゴリ GUID の名前（TSF の `GUID_TFCAT_*`。名前は参考）
pub fn category_name(guid: &str) -> &'static str {
    match guid.to_ascii_uppercase().as_str() {
        "{34745C63-B2F0-4784-8B67-5E12C8701A31}" => "TIP_KEYBOARD",
        "{046B8C80-1647-40F7-9B21-B93B81AABC1B}" => "DISPLAYATTRIBUTEPROVIDER",
        "{364215D9-75BC-11D7-A6EF-00065B84435C}" => "TIPCAP_COMLESS",
        "{13A016DF-560B-46CD-947A-4C3AF1E0E35D}" => "TIPCAP_IMMERSIVESUPPORT",
        "{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}" => "TIPCAP_SYSTRAYSUPPORT",
        "{49D2F9CE-1F5E-11D7-A6D3-00065B84435C}" => "TIPCAP_UIELEMENTENABLED",
        "{49D2F9CF-1F5E-11D7-A6D3-00065B84435C}" => "TIPCAP_SECUREMODE",
        "{CCF05DD7-4A87-11D7-A6E2-00065B84435C}" => "TIPCAP_INPUTMODECOMPARTMENT",
        _ => "?",
    }
}

/// 値の名前の比較結果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameDiff {
    /// Microsoft IME にだけある
    pub only_ms: Vec<String>,
    /// Nagi にだけある
    pub only_nagi: Vec<String>,
    /// 両方にあるが値の種類が違う（`名前: Nagi の種類 / MS の種類`）
    pub kind_differs: Vec<String>,
}

fn shown_name(name: &str) -> String {
    if name.is_empty() {
        "(既定)".to_owned()
    } else {
        name.to_owned()
    }
}

/// 値の名前（大文字小文字を無視）の有無と種類を比べる
pub fn diff_values(nagi: &[RegValue], ms: &[RegValue]) -> NameDiff {
    let find = |list: &'_ [RegValue], name: &str| {
        list.iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
            .cloned()
    };
    let mut d = NameDiff::default();
    for m in ms {
        match find(nagi, &m.name) {
            None => d.only_ms.push(shown_name(&m.name)),
            Some(n) if !n.kind.eq_ignore_ascii_case(&m.kind) => {
                d.kind_differs
                    .push(format!("{}: {} / {}", shown_name(&m.name), n.kind, m.kind))
            }
            Some(_) => {}
        }
    }
    for n in nagi {
        if find(ms, &n.name).is_none() {
            d.only_nagi.push(shown_name(&n.name));
        }
    }
    d
}

/// 名前（GUID・サブキー名）の集合差（大文字小文字を無視）。`(a にだけある, b にだけある)`
pub fn diff_names(a: &[String], b: &[String]) -> (Vec<String>, Vec<String>) {
    let missing = |x: &[String], y: &[String]| -> Vec<String> {
        x.iter()
            .filter(|n| !y.iter().any(|m| m.eq_ignore_ascii_case(n)))
            .cloned()
            .collect()
    };
    (missing(a, b), missing(b, a))
}

/// 見立ての判定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Likelihood {
    /// 該当しそう
    Likely,
    /// 差分がある（原因かどうかは分からない）
    Differs,
    /// 該当しなさそう
    Unlikely,
    /// 判定できない（材料が取れなかった）
    Unknown,
}

impl Likelihood {
    /// 表示
    pub fn label(self) -> &'static str {
        match self {
            Likelihood::Likely => "該当しそう",
            Likelihood::Differs => "差分あり（原因かは不明）",
            Likelihood::Unlikely => "該当しなさそう",
            Likelihood::Unknown => "判定不能",
        }
    }
}

/// 見立て 1 つへの判定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    /// 記号（a〜e）
    pub key: char,
    /// 見立ての内容
    pub title: &'static str,
    /// 判定
    pub likelihood: Likelihood,
    /// 理由
    pub reason: String,
}

fn assess_acl(p: &Probe) -> Assessment {
    let title = "(a) DLL・アイコンに「すべてのアプリケーション パッケージ」の読み取り・実行が無く、設定アプリが読めない";
    let mut lacking = Vec::new();
    let mut checked = 0;
    for target in ["dll", "icon"] {
        match find_acl(p, target) {
            Some(acl) if acl.exists && acl.error.is_empty() => {
                checked += 1;
                if !grants_read_execute(acl, SID_ALL_APP_PACKAGES) {
                    lacking.push(target);
                }
            }
            _ => {}
        }
    }
    let (likelihood, reason) = if checked == 0 {
        (
            Likelihood::Unknown,
            "DLL・アイコンの ACL を取得できませんでした".to_owned(),
        )
    } else if lacking.is_empty() {
        (
            Likelihood::Unlikely,
            "DLL・アイコンとも S-1-15-2-1 に読み取り・実行があります".to_owned(),
        )
    } else {
        (
            Likelihood::Likely,
            format!(
                "{} に S-1-15-2-1（ALL APPLICATION PACKAGES）の読み取り・実行がありません",
                lacking.join("・")
            ),
        )
    };
    Assessment {
        key: 'a',
        title,
        likelihood,
        reason,
    }
}

fn assess_host(p: &Probe) -> Assessment {
    let title = "(b) 設定アプリのプロセスが Nagi の DLL を読み込めていない（署名なし DLL の拒否・読み込み失敗を含む）";
    let running: Vec<&String> = p
        .settings_running
        .iter()
        .filter(|n| SETTINGS_PROCESSES.iter().any(|s| s.eq_ignore_ascii_case(n)))
        .collect();
    let loaded: Vec<&String> = p
        .dll_hosts
        .iter()
        .filter(|h| {
            SETTINGS_PROCESSES.iter().any(|s| {
                h.trim_end_matches(".exe").eq_ignore_ascii_case(s)
                    || h.eq_ignore_ascii_case(&format!("{s}.exe"))
            })
        })
        .collect();
    let (likelihood, reason) = if !loaded.is_empty() {
        (
            Likelihood::Unlikely,
            "設定アプリ側のプロセスが DLL を読み込んでいます（署名なしでも読めている）".to_owned(),
        )
    } else if running.is_empty() {
        (
            Likelihood::Unknown,
            "設定アプリが動いていません。設定の「言語のオプション」を開いたままもう一度実行してください"
                .to_owned(),
        )
    } else {
        (
            Likelihood::Likely,
            "設定アプリは動いているが DLL を読み込んでいません（弱い手がかり: 読み込みは「オプション」を押す時だけかもしれない。ACL か署名なしの拒否を疑う）"
                .to_owned(),
        )
    };
    Assessment {
        key: 'b',
        title,
        likelihood,
        reason,
    }
}

fn assess_user_profile(p: &Probe) -> Assessment {
    let title =
        "(c) このユーザーの入力方式の登録（HKCU の User Profile）が無い・Microsoft IME と形が違う";
    let (likelihood, reason) = if p.nagi.user_ids.is_empty() {
        (
            Likelihood::Likely,
            "HKCU\\Control Panel\\International\\User Profile に Nagi の ID（0411:{CLSID}{プロファイル}）がありません"
                .to_owned(),
        )
    } else if p.ms.user_ids.is_empty() {
        (
            Likelihood::Unknown,
            "Nagi の ID はあるが、比較する Microsoft IME の ID が無い".to_owned(),
        )
    } else {
        let n = &p.nagi.user_ids[0];
        let m = &p.ms.user_ids[0];
        if n.kind.eq_ignore_ascii_case(&m.kind) && n.data == m.data {
            (
                Likelihood::Unlikely,
                format!("Nagi も Microsoft IME も {} = {}", n.kind, n.data),
            )
        } else {
            (
                Likelihood::Differs,
                format!(
                    "Nagi は {} = {}、Microsoft IME は {} = {}",
                    n.kind, n.data, m.kind, m.data
                ),
            )
        }
    };
    Assessment {
        key: 'c',
        title,
        likelihood,
        reason,
    }
}

fn assess_profile_values(p: &Probe) -> Assessment {
    let title = "(d) LanguageProfile の値（Enable・Display Description・SubstituteLayout など）が Microsoft IME と違う";
    let (likelihood, reason) = if !p.nagi.profile_exists {
        (
            Likelihood::Likely,
            "Nagi の LanguageProfile\\0x00000411\\{プロファイル} が HKLM にありません".to_owned(),
        )
    } else if !p.ms.profile_exists {
        (
            Likelihood::Unknown,
            "比較する Microsoft IME のプロファイルが取れません".to_owned(),
        )
    } else {
        let d = diff_values(&p.nagi.profile_values, &p.ms.profile_values);
        if d.only_ms.is_empty() && d.kind_differs.is_empty() {
            (
                Likelihood::Unlikely,
                "Microsoft IME にあって Nagi に無い値はありません".to_owned(),
            )
        } else {
            (
                Likelihood::Differs,
                format!(
                    "Microsoft IME にだけある値: {}。種類の違い: {}",
                    list_or_none(&d.only_ms),
                    list_or_none(&d.kind_differs)
                ),
            )
        }
    };
    Assessment {
        key: 'd',
        title,
        likelihood,
        reason,
    }
}

fn assess_tip(p: &Probe) -> Assessment {
    let title =
        "(e) CLSID 側の登録（カテゴリ・ThreadingModel・TIP キーの値）が Microsoft IME と違う";
    let (likelihood, reason) = if !p.nagi.tip_exists {
        (
            Likelihood::Likely,
            "HKLM\\SOFTWARE\\Microsoft\\CTF\\TIP\\{CLSID} がありません".to_owned(),
        )
    } else if !p.ms.tip_exists {
        (
            Likelihood::Unknown,
            "比較する Microsoft IME の TIP キーが取れません".to_owned(),
        )
    } else {
        let (only_ms, _) = diff_names(&p.ms.categories, &p.nagi.categories);
        let named: Vec<String> = only_ms
            .iter()
            .map(|g| format!("{}({g})", category_name(g)))
            .collect();
        let vals = diff_values(&p.nagi.tip_values, &p.ms.tip_values);
        if named.is_empty() && vals.only_ms.is_empty() {
            (
                Likelihood::Unlikely,
                "Microsoft IME にあって Nagi に無いカテゴリ・値はありません".to_owned(),
            )
        } else {
            (
                Likelihood::Differs,
                format!(
                    "Microsoft IME にだけあるカテゴリ: {}。TIP キーの値: {}。ThreadingModel は Nagi={} / MS={}",
                    list_or_none(&named),
                    list_or_none(&vals.only_ms),
                    shown_or_none(&p.nagi.inproc_threading),
                    shown_or_none(&p.ms.inproc_threading),
                ),
            )
        }
    };
    Assessment {
        key: 'e',
        title,
        likelihood,
        reason,
    }
}

fn list_or_none(v: &[String]) -> String {
    if v.is_empty() {
        "なし".to_owned()
    } else {
        v.join(", ")
    }
}

fn shown_or_none(s: &str) -> &str {
    if s.is_empty() { "(なし)" } else { s }
}

/// 見立て（a）〜（e）への判定
pub fn assess(p: &Probe) -> Vec<Assessment> {
    vec![
        assess_acl(p),
        assess_host(p),
        assess_user_profile(p),
        assess_profile_values(p),
        assess_tip(p),
    ]
}

fn short(s: &str) -> String {
    const MAX: usize = 120;
    if s.chars().count() <= MAX {
        s.to_owned()
    } else {
        let cut: String = s.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

fn value_lines(prefix: &str, values: &[RegValue]) -> Vec<String> {
    if values.is_empty() {
        return vec![format!("{prefix}(値なし)")];
    }
    values
        .iter()
        .map(|v| {
            format!(
                "{prefix}{} [{}] = {}",
                shown_name(&v.name),
                v.kind,
                short(&v.data)
            )
        })
        .collect()
}

fn entry_lines(e: &EntryFacts) -> Vec<String> {
    let mut lines = vec![format!(
        "[{}] TIP キー: {} / プロファイルキー: {} / ThreadingModel: {} / InprocServer32: {}",
        e.label,
        if e.tip_exists { "あり" } else { "なし" },
        if e.profile_exists { "あり" } else { "なし" },
        shown_or_none(&e.inproc_threading),
        if e.inproc_path.is_empty() {
            "(なし)".to_owned()
        } else {
            short(&e.inproc_path)
        },
    )];
    lines.extend(value_lines("  TIP の値: ", &e.tip_values));
    lines.push(format!(
        "  TIP のサブキー: {}",
        list_or_none(&e.tip_subkeys)
    ));
    lines.extend(value_lines("  プロファイルの値: ", &e.profile_values));
    lines.push(format!(
        "  プロファイルのサブキー: {}",
        list_or_none(&e.profile_subkeys)
    ));
    let cats: Vec<String> = e
        .categories
        .iter()
        .map(|g| format!("{}{g}", category_name_prefix(g)))
        .collect();
    lines.push(format!("  カテゴリ: {}", list_or_none(&cats)));
    lines.extend(value_lines("  HKCU User Profile: ", &e.user_ids));
    lines
}

fn category_name_prefix(guid: &str) -> String {
    match category_name(guid) {
        "?" => String::new(),
        name => format!("{name} "),
    }
}

fn acl_lines(acls: &[AclFacts]) -> Vec<String> {
    let mut lines = Vec::new();
    for a in acls {
        if !a.exists {
            lines.push(format!(
                "[ACL {}] 取得できません{}",
                a.target,
                if a.error.is_empty() {
                    String::new()
                } else {
                    format!("（{}）", short(&a.error))
                }
            ));
            continue;
        }
        let verdicts = [
            ("AllAppPackages", SID_ALL_APP_PACKAGES),
            ("AllRestrictedAppPackages", SID_ALL_RESTRICTED_APP_PACKAGES),
            ("Users", SID_USERS),
        ]
        .map(|(name, sid)| {
            format!(
                "{name}={}",
                if grants_read_execute(a, sid) {
                    "RX あり"
                } else {
                    "RX なし"
                }
            )
        });
        lines.push(format!(
            "[ACL {}] 継承の遮断: {} / {}",
            a.target,
            if a.protected { "あり" } else { "なし" },
            verdicts.join(" / ")
        ));
        for e in &a.entries {
            lines.push(format!(
                "  {} {} {}{}{}",
                e.sid,
                e.kind,
                e.rights,
                if e.inherited { " (継承)" } else { "" },
                if e.inherit_only { " (子のみ)" } else { "" },
            ));
        }
    }
    lines
}

/// 結果に整形する（判定は `Manual`。OK / NG にしない）
pub fn render(p: &Probe) -> ItemResult {
    let id = ItemId::TsfLanguageOptions;
    let assessments = assess(p);
    let summary: Vec<String> = assessments
        .iter()
        .map(|a| format!("({}) {}", a.key, a.likelihood.label()))
        .collect();
    let detail = format!(
        "「言語のオプション」で Nagi に「オプション」「削除」が出ない原因の見立て: {}。\
         Windows のビルド: {}",
        summary.join(" / "),
        shown_or_none(&p.os_build)
    );

    let mut evidence = Vec::new();
    evidence.push("== 見立て ==".to_owned());
    for a in &assessments {
        evidence.push(format!("{} → {}", a.title, a.likelihood.label()));
        evidence.push(format!("    理由: {}", a.reason));
    }
    evidence.push("== 入力方式の登録（HKLM / HKCU） ==".to_owned());
    evidence.extend(entry_lines(&p.nagi));
    evidence.extend(entry_lines(&p.ms));
    let d = diff_values(&p.nagi.profile_values, &p.ms.profile_values);
    evidence.push(format!(
        "プロファイルの値の差分: MS だけ={} / Nagi だけ={} / 種類違い={}",
        list_or_none(&d.only_ms),
        list_or_none(&d.only_nagi),
        list_or_none(&d.kind_differs)
    ));
    let (only_ms, only_nagi) = diff_names(&p.ms.categories, &p.nagi.categories);
    evidence.push(format!(
        "カテゴリの差分: MS だけ={} / Nagi だけ={}",
        list_or_none(
            &only_ms
                .iter()
                .map(|g| format!("{}{g}", category_name_prefix(g)))
                .collect::<Vec<_>>()
        ),
        list_or_none(
            &only_nagi
                .iter()
                .map(|g| format!("{}{g}", category_name_prefix(g)))
                .collect::<Vec<_>>()
        ),
    ));
    evidence.push("== DLL・フォルダの ACL ==".to_owned());
    evidence.extend(acl_lines(&p.acls));
    evidence.push("== 設定アプリと DLL ==".to_owned());
    evidence.push(format!(
        "動作中の設定アプリ関連プロセス: {}",
        list_or_none(&p.settings_running)
    ));
    evidence.push(format!(
        "Nagi の DLL を読み込んでいるプロセス: {}",
        list_or_none(&p.dll_hosts)
    ));

    ItemResult::manual(
        id,
        detail,
        "設定アプリ（設定 > 時刻と言語 > 言語と地域 > 日本語 > 言語のオプション）を開いたままこの項目を実行すると、(b) が正しく判定できます。\
         (a) が「該当しそう」なら管理者の PowerShell で icacls \"%ProgramFiles%\\Nagi\" /grant \"*S-1-15-2-1:(OI)(CI)(RX)\" \"*S-1-15-2-2:(OI)(CI)(RX)\" /C を実行し、設定アプリを開き直してください。\
         (c) が「該当しそう」なら Win+Space で Nagi を一度選ぶか、言語のオプションで「キーボードを追加する」から Nagi を追加してください。\
         レポートをそのまま貼れば原因を追えます。",
    )
    .with_evidence(evidence)
}

/// 項目の入口
pub fn check(ctx: &Ctx) -> ItemResult {
    let id = ItemId::TsfLanguageOptions;
    if !ctx.is_windows() {
        return ItemResult::skipped(id, "Windows でのみ判定します");
    }
    match imp::probe(&ctx.install_dir) {
        Some(p) => render(&p),
        None => ItemResult::manual(
            id,
            "調査を実行できませんでした（PowerShell を実行できない・時間切れ）",
            "もう一度実行してください。",
        ),
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn probe(install_dir: &Path) -> Option<Probe> {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let powershell = Path::new(&root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let mut child = Command::new(powershell)
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
            .arg(super::super::sys::encode_command(&probe_script()))
            .env("NAGI_DIAG_INSTALL", install_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stdout, &mut bytes);
            let _ = tx.send(bytes);
        });
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(bytes) => {
                let _ = child.wait();
                parse_probe(&String::from_utf8_lossy(&bytes))
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                None
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::path::Path;

    pub fn probe(_install_dir: &Path) -> Option<Probe> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::types::Status;

    fn val(name: &str, kind: &str, data: &str) -> RegValue {
        RegValue {
            name: name.into(),
            kind: kind.into(),
            data: data.into(),
        }
    }

    fn ace(sid: &str, mask: i64, kind: &str) -> AclEntry {
        AclEntry {
            sid: sid.into(),
            rights: String::new(),
            mask,
            kind: kind.into(),
            inherited: true,
            inherit_only: false,
        }
    }

    fn acl(target: &str, entries: Vec<AclEntry>) -> AclFacts {
        AclFacts {
            target: target.into(),
            exists: true,
            entries,
            ..AclFacts::default()
        }
    }

    const RX: i64 = 0x1200A9;

    fn healthy() -> Probe {
        let entry = |label: &str| EntryFacts {
            label: label.into(),
            tip_exists: true,
            profile_exists: true,
            profile_values: vec![
                val("Description", "String", "x"),
                val("Enable", "DWord", "1"),
            ],
            categories: vec!["{34745C63-B2F0-4784-8B67-5E12C8701A31}".into()],
            inproc_threading: "Both".into(),
            user_ids: vec![val("ja\\0411:{C}{P}", "DWord", "1")],
            ..EntryFacts::default()
        };
        Probe {
            os_build: "26100".into(),
            nagi: entry("Nagi"),
            ms: entry("Microsoft IME"),
            acls: vec![
                acl("dll", vec![ace(SID_ALL_APP_PACKAGES, RX, "Allow")]),
                acl("icon", vec![ace(SID_ALL_APP_PACKAGES, RX, "Allow")]),
            ],
            settings_running: vec!["SystemSettings".into()],
            dll_hosts: vec!["SystemSettings.exe".into()],
        }
    }

    fn likelihood(p: &Probe, key: char) -> Likelihood {
        assess(p)
            .into_iter()
            .find(|a| a.key == key)
            .unwrap()
            .likelihood
    }

    #[test]
    fn script_is_ascii_and_filled() {
        let s = probe_script();
        assert!(s.is_ascii());
        assert!(!s.contains("@@"));
        assert!(s.contains(CLSID) && s.contains(MS_IME_CLSID) && s.contains(MS_IME_PROFILE));
    }

    #[test]
    fn masks() {
        assert!(mask_grants_read_execute(0x1200A9));
        assert!(mask_grants_read_execute(0x1F01FF));
        assert!(mask_grants_read_execute(0x1301BF));
        assert!(mask_grants_read_execute(-1610612736)); // GENERIC_EXECUTE | GENERIC_READ（符号つき）
        assert!(!mask_grants_read_execute(0x120089)); // Read のみ
        assert!(!mask_grants_read_execute(0x100000));
    }

    #[test]
    fn grants_respect_deny_and_inherit_only() {
        let mut a = acl("dll", vec![ace(SID_ALL_APP_PACKAGES, RX, "Allow")]);
        assert!(grants_read_execute(&a, "s-1-15-2-1"));
        a.entries.push(ace(SID_ALL_APP_PACKAGES, RX, "Deny"));
        assert!(!grants_read_execute(&a, SID_ALL_APP_PACKAGES));
        let mut b = acl("install_dir", vec![ace(SID_ALL_APP_PACKAGES, RX, "Allow")]);
        b.entries[0].inherit_only = true;
        assert!(!grants_read_execute(&b, SID_ALL_APP_PACKAGES));
        assert!(!grants_read_execute(&b, SID_USERS));
    }

    #[test]
    fn healthy_probe_points_nowhere() {
        let p = healthy();
        for a in assess(&p) {
            assert_eq!(a.likelihood, Likelihood::Unlikely, "{a:?}");
        }
    }

    #[test]
    fn missing_acl_is_likely_a() {
        let mut p = healthy();
        p.acls[0].entries.clear();
        assert_eq!(likelihood(&p, 'a'), Likelihood::Likely);
        p.acls.clear();
        assert_eq!(likelihood(&p, 'a'), Likelihood::Unknown);
    }

    #[test]
    fn host_states() {
        let mut p = healthy();
        p.dll_hosts.clear();
        assert_eq!(likelihood(&p, 'b'), Likelihood::Likely);
        p.settings_running.clear();
        assert_eq!(likelihood(&p, 'b'), Likelihood::Unknown);
    }

    #[test]
    fn user_profile_states() {
        let mut p = healthy();
        p.nagi.user_ids.clear();
        assert_eq!(likelihood(&p, 'c'), Likelihood::Likely);
        let mut p = healthy();
        p.nagi.user_ids[0].data = "0".into();
        assert_eq!(likelihood(&p, 'c'), Likelihood::Differs);
    }

    #[test]
    fn value_and_category_diffs() {
        let mut p = healthy();
        p.ms.profile_values
            .push(val("Display Description", "String", "@x,-1"));
        p.ms.profile_values.push(val("enable", "String", "1"));
        let d = diff_values(&p.nagi.profile_values, &p.ms.profile_values);
        assert_eq!(d.only_ms, vec!["Display Description"]);
        assert_eq!(d.kind_differs, vec!["enable: DWord / String"]);
        assert_eq!(likelihood(&p, 'd'), Likelihood::Differs);

        p.ms.categories
            .push("{364215D9-75BC-11D7-A6EF-00065B84435C}".into());
        let r = assess(&p).into_iter().find(|a| a.key == 'e').unwrap();
        assert_eq!(r.likelihood, Likelihood::Differs);
        assert!(r.reason.contains("TIPCAP_COMLESS"), "{}", r.reason);
        let (only_ms, only_nagi) = diff_names(&p.ms.categories, &p.nagi.categories);
        assert_eq!(only_ms.len(), 1);
        assert!(only_nagi.is_empty());
    }

    #[test]
    fn missing_registration_is_likely() {
        let mut p = healthy();
        p.nagi.profile_exists = false;
        p.nagi.tip_exists = false;
        assert_eq!(likelihood(&p, 'd'), Likelihood::Likely);
        assert_eq!(likelihood(&p, 'e'), Likelihood::Likely);
        p.ms.profile_exists = false;
        p.nagi.profile_exists = true;
        assert_eq!(likelihood(&p, 'd'), Likelihood::Unknown);
    }

    #[test]
    fn parse_accepts_scalars_nulls_and_noise() {
        let json = r#"noise {"os_build":"26100","nagi":{"label":"Nagi","tip_exists":true,
            "tip_values":{"name":"","kind":"String","data":"Nagi"},"tip_subkeys":null,
            "categories":["{A}"],"user_ids":[]},"ms":{"label":"MS"},
            "acls":{"target":"dll","exists":true,"entries":{"sid":"S-1-15-2-1","rights":"ReadAndExecute","mask":1179817,"kind":"Allow","inherited":true,"inherit_only":false}},
            "settings_running":"SystemSettings","dll_hosts":[]} tail"#;
        let p = parse_probe(json).unwrap();
        assert_eq!(p.nagi.tip_values.len(), 1);
        assert!(p.nagi.tip_subkeys.is_empty());
        assert_eq!(p.acls[0].entries.len(), 1);
        assert_eq!(p.settings_running, vec!["SystemSettings"]);
        assert!(parse_probe("no json").is_none());
    }

    #[test]
    fn render_is_manual_with_evidence_and_no_verdict() {
        let r = render(&healthy());
        assert_eq!(r.status, Status::Manual);
        assert_eq!(r.id, ItemId::TsfLanguageOptions);
        assert!(r.detail.contains("(a) 該当しなさそう"));
        let text = r.evidence.join("\n");
        assert!(text.contains("[Nagi]") && text.contains("[Microsoft IME]"));
        assert!(text.contains("[ACL dll]") && text.contains("AllAppPackages=RX あり"));
        assert!(text.contains("TIP_KEYBOARD"));
    }

    #[test]
    fn not_windows_is_skipped() {
        use crate::diag::testutil::{FakeSystem, ctx_with_system};
        let ctx = ctx_with_system(FakeSystem::default());
        assert_eq!(check(&ctx).status, Status::Skipped);
    }
}
