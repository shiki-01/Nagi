//! `cargo xtask bundle-windows` — Windows 版の配布物（zip）を組み立てる。
//!
//! ビルド自体（MSVC ターゲットの `nagi_windows_tsf.dll` と `imserver.exe`）は CI の windows ランナーで
//! 行い、ここでは成果物の収集・（任意で）Authenticode 署名・チェックサム・zip 化だけを行う
//! （`docs/decisions.md` H-42）。収集と zip 化は OS 非依存で、Linux でも単体テストできる。
//!
//! zip の構成（x86_64 は `Nagi-windows-<version>.zip`、ARM64 は `Nagi-windows-arm64-<version>.zip`。
//! どちらも最上位に `Nagi-windows-<version>/` を 1 つ置く。zip の名前だけがアーキテクチャで変わる。
//! x86_64 の名前は 0.1.15 以前のクライアントの自動更新が参照するため変えない）:
//!
//! ```text
//! nagi_windows_tsf.dll   TSF の入力方式（COM の InprocServer32）
//! imserver.exe           変換エンジン
//! nagi-settings.exe      設定画面（ローカルの Web 画面を既定のブラウザで開く。decisions.md H-97）
//! install.ps1 / uninstall.ps1 / verify.ps1（実機の検証用。docs/windows-verification.md）
//! apply-update.ps1       取得済みの更新の適用（UAC 1 回。decisions.md H-95）
//! README.md              docs/windows-install.md（SmartScreen の手順を含む）
//! LICENSE-MIT / LICENSE-APACHE / NOTICE / THIRD_PARTY_LICENSES.md（元は THIRD_PARTY_LICENSES_WINDOWS.md）
//! VERSION                バージョン（x.y.z）
//! ARCH                   この zip のアーキテクチャ（`x86_64` / `aarch64`。install.ps1 が OS と突き合わせる）
//! SHA256SUMS             上記すべての SHA-256（`<hex>  <相対パス>`。install.ps1 が検証する）
//! ```

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// TSF の DLL の名前（`frontend-windows-tsf` の cdylib）。
pub const DLL_FILE: &str = "nagi_windows_tsf.dll";
/// エンジンの実行ファイル名。
pub const IMSERVER_FILE: &str = "imserver.exe";
/// 設定画面の実行ファイル名（`core/settings-ui` の bin。`nagi_windows::status::SETTINGS_GUI_EXE` と同じ）。
pub const SETTINGS_FILE: &str = "nagi-settings.exe";
/// リポジトリ上の Windows 用ライセンス一覧（`gen-licenses` の出力。iOS 用の
/// `THIRD_PARTY_LICENSES.md` とは別。decisions.md H-43）。
pub const WINDOWS_LICENSES_FILE: &str = "THIRD_PARTY_LICENSES_WINDOWS.md";
/// アーキテクチャを書いたファイルの名前（`x86_64` / `aarch64`）。
pub const ARCH_FILE: &str = "ARCH";
/// チェックサム一覧のファイル名。
pub const CHECKSUM_FILE: &str = "SHA256SUMS";
/// 署名用 PFX のパスワードを渡す環境変数名（コマンドラインには載せない）。
const PFX_PASSWORD_ENV: &str = "NAGI_WINDOWS_PFX_PASSWORD";
/// 既定のタイムスタンプサーバー（RFC 3161）。
const DEFAULT_TIMESTAMP_URL: &str = "http://timestamp.digicert.com";

/// 配布物のアーキテクチャ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Arch {
    /// x64（Intel / AMD）。既定。
    #[value(name = "x86_64")]
    X86_64,
    /// ARM64（Windows on Arm）。
    #[value(name = "aarch64")]
    Aarch64,
}

impl Arch {
    /// `ARCH` ファイルと Rust のターゲットトリプルの先頭に使う名前。
    pub fn as_str(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        }
    }

    /// zip のファイル名の先頭（バージョンの前）。x86_64 は従来の名前のまま。
    pub fn zip_stem(self) -> &'static str {
        match self {
            Arch::X86_64 => "Nagi-windows",
            Arch::Aarch64 => "Nagi-windows-arm64",
        }
    }

    /// PE ヘッダの Machine フィールド（IMAGE_FILE_MACHINE_AMD64 / ARM64）。
    pub fn pe_machine(self) -> u16 {
        match self {
            Arch::X86_64 => 0x8664,
            Arch::Aarch64 => 0xAA64,
        }
    }
}

/// `bundle-windows` のオプション。
pub struct Options {
    /// 配布物のアーキテクチャ。
    pub arch: Arch,
    /// DLL のパス。未指定なら `<target>[/<triple>]/release/nagi_windows_tsf.dll`。
    pub dll: Option<PathBuf>,
    /// imserver.exe のパス。未指定なら `<target>[/<triple>]/release/imserver.exe`。
    pub imserver: Option<PathBuf>,
    /// nagi-settings.exe のパス。未指定なら `<target>[/<triple>]/release/nagi-settings.exe`。
    pub settings: Option<PathBuf>,
    /// 既定パスを探すときのターゲットトリプル（例: `x86_64-pc-windows-msvc`）。
    pub target: Option<String>,
    /// 出力先ディレクトリ。未指定は `<target>/windows-bundle`。
    pub out_dir: Option<PathBuf>,
    /// Authenticode 署名に使う PFX。指定時は DLL と exe を signtool で署名する。
    pub sign_pfx: Option<PathBuf>,
    /// signtool.exe のパス（PATH 上に無い場合）。
    pub signtool: String,
    /// タイムスタンプサーバーの URL。
    pub timestamp_url: Option<String>,
}

/// 組み立てる 1 ファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 最上位ディレクトリの下の相対パス（`/` 区切り）。
    pub name: String,
    /// 中身の取得元。
    pub source: Source,
}

/// ファイルの中身の取得元。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// 既存のファイルをコピーする。
    File(PathBuf),
    /// 生成した内容を書く。
    Bytes(Vec<u8>),
}

/// 収集に使う入力パス。
pub struct Inputs {
    pub dll: PathBuf,
    pub imserver: PathBuf,
    pub settings: PathBuf,
    pub repo: PathBuf,
    pub version: String,
    pub arch: Arch,
}

fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// zip の最上位ディレクトリの名前（アーキテクチャによらず `Nagi-windows-<version>`）。
/// 自動更新の `apply-update.ps1` はこの名前のディレクトリを探すため、ARM64 でも変えない。
pub fn bundle_name(version: &str) -> String {
    format!("Nagi-windows-{version}")
}

/// zip のファイル名（拡張子つき）。x86_64 は `Nagi-windows-<version>.zip`（アップデートの
/// `download_url` と `windows-release.yml` が依存する既存の名前）、ARM64 は `Nagi-windows-arm64-<version>.zip`。
pub fn zip_file_name(arch: Arch, version: &str) -> String {
    format!("{}-{version}.zip", arch.zip_stem())
}

/// 配布物に入れるファイルの一覧（`SHA256SUMS` を除く）。入力ファイルの存在もここで確かめる。
pub fn plan(inputs: &Inputs) -> Result<Vec<Entry>, String> {
    let repo = &inputs.repo;
    let file = |name: &str, path: PathBuf| Entry {
        name: name.to_string(),
        source: Source::File(path),
    };
    let entries = vec![
        file(DLL_FILE, inputs.dll.clone()),
        file(IMSERVER_FILE, inputs.imserver.clone()),
        file(SETTINGS_FILE, inputs.settings.clone()),
        file(
            "install.ps1",
            repo.join("frontend-windows/dist/install.ps1"),
        ),
        file(
            "uninstall.ps1",
            repo.join("frontend-windows/dist/uninstall.ps1"),
        ),
        file("verify.ps1", repo.join("frontend-windows/dist/verify.ps1")),
        file(
            "apply-update.ps1",
            repo.join("frontend-windows/dist/apply-update.ps1"),
        ),
        file("README.md", repo.join("docs/windows-install.md")),
        file("LICENSE-MIT", repo.join("LICENSE-MIT")),
        file("LICENSE-APACHE", repo.join("LICENSE-APACHE")),
        file("NOTICE", repo.join("NOTICE")),
        // Windows の依存閉包の一覧（`cargo xtask gen-licenses --platform windows`）。
        // zip の中の名前は install.ps1 の期待どおり THIRD_PARTY_LICENSES.md
        file("THIRD_PARTY_LICENSES.md", repo.join(WINDOWS_LICENSES_FILE)),
        Entry {
            name: "VERSION".to_string(),
            source: Source::Bytes(format!("{}\n", inputs.version).into_bytes()),
        },
        Entry {
            name: ARCH_FILE.to_string(),
            source: Source::Bytes(format!("{}\n", inputs.arch.as_str()).into_bytes()),
        },
    ];
    for e in &entries {
        if let Source::File(p) = &e.source
            && !p.is_file()
        {
            return Err(format!("{} が見つかりません: {}", e.name, p.display()));
        }
    }
    Ok(entries)
}

/// 先頭が `MZ`（PE / DOS ヘッダ）か。Linux の `.so` などを取り違えて入れないための検査。
pub fn looks_like_pe(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MZ")
}

/// PE ヘッダの Machine フィールド。PE ヘッダを読めないとき（短すぎる等）は `None`。
pub fn pe_machine(bytes: &[u8]) -> Option<u16> {
    let lfanew = u32::from_le_bytes(bytes.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if bytes.get(lfanew..lfanew.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let start = lfanew + 4;
    Some(u16::from_le_bytes(
        bytes.get(start..start + 2)?.try_into().ok()?,
    ))
}

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// PowerShell スクリプトを UTF-8 BOM 付きにする（Windows PowerShell 5.1 は BOM が無いと
/// 日本語を ANSI として読み、文字化けする）。リポジトリのソースは BOM 無しで持つ。
pub fn with_utf8_bom(bytes: &[u8]) -> Vec<u8> {
    if bytes.starts_with(UTF8_BOM) {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(bytes.len() + UTF8_BOM.len());
    out.extend_from_slice(UTF8_BOM);
    out.extend_from_slice(bytes);
    out
}

/// ステージング先に書くときの内容の整形（スクリプトは BOM 付き、それ以外はそのまま）。
fn finalize_content(name: &str, bytes: Vec<u8>) -> Vec<u8> {
    if name.ends_with(".ps1") {
        with_utf8_bom(&bytes)
    } else {
        bytes
    }
}

/// `SHA256SUMS` の内容（`<小文字16進>  <相対パス>`、パス昇順、LF 改行）。
pub fn checksums(files: &[(String, Vec<u8>)]) -> String {
    let mut lines: Vec<(String, String)> = files
        .iter()
        .map(|(name, data)| {
            (
                name.clone(),
                crate::update_sign::hex_encode(&Sha256::digest(data)),
            )
        })
        .collect();
    lines.sort();
    lines
        .into_iter()
        .map(|(name, hash)| format!("{hash}  {name}\n"))
        .collect()
}

/// ファイル群を 1 つの zip にする。最上位に `root/` を置き、更新時刻は固定（再現性のため）。
pub fn write_zip(root: &str, files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (name, data) in files {
        writer
            .start_file(format!("{root}/{name}"), options)
            .map_err(|e| format!("zip への追加に失敗しました（{name}）: {e}"))?;
        writer
            .write_all(data)
            .map_err(|e| format!("zip への書き込みに失敗しました（{name}）: {e}"))?;
    }
    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|e| format!("zip の確定に失敗しました: {e}"))
}

/// `signtool sign` の引数（PFX のパスワードは含めない。呼び出し側が追加する）。
pub fn signtool_sign_args(pfx: &Path, timestamp_url: &str, target: &Path) -> Vec<String> {
    vec![
        "sign".into(),
        "/fd".into(),
        "SHA256".into(),
        "/f".into(),
        pfx.display().to_string(),
        "/tr".into(),
        timestamp_url.into(),
        "/td".into(),
        "SHA256".into(),
        target.display().to_string(),
    ]
}

/// `signtool verify` の引数（既定の認証ポリシー）。
pub fn signtool_verify_args(target: &Path) -> Vec<String> {
    vec![
        "verify".into(),
        "/pa".into(),
        "/q".into(),
        target.display().to_string(),
    ]
}

fn run_signtool(signtool: &str, args: &[String], password: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new(signtool);
    cmd.args(args);
    if let Some(pw) = password {
        cmd.args(["/p", pw]);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("signtool の実行に失敗しました（{signtool}）: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "signtool が失敗しました（終了コード: {}）",
            status.code().unwrap_or(-1)
        ))
    }
}

/// ステージング先のファイルを書き出す。
fn stage(stage_dir: &Path, entries: &[Entry], arch: Arch) -> Result<(), String> {
    for e in entries {
        let bytes = match &e.source {
            Source::File(p) => fs::read(p)
                .map_err(|err| format!("{} の読み込みに失敗しました: {err}", p.display()))?,
            Source::Bytes(b) => b.clone(),
        };
        if (e.name == DLL_FILE || e.name == IMSERVER_FILE || e.name == SETTINGS_FILE)
            && !looks_like_pe(&bytes)
        {
            return Err(format!(
                "{} が Windows の実行形式（PE）ではありません。MSVC ターゲットでビルドした成果物を指定してください",
                e.name
            ));
        }
        // PE ヘッダを読めるときは、アーキテクチャの取り違え（x64 の DLL を ARM64 の zip に入れる等）を弾く
        if (e.name == DLL_FILE || e.name == IMSERVER_FILE || e.name == SETTINGS_FILE)
            && let Some(machine) = pe_machine(&bytes)
            && machine != arch.pe_machine()
        {
            return Err(format!(
                "{} のアーキテクチャ（PE Machine 0x{machine:04X}）が --arch {} と一致しません",
                e.name,
                arch.as_str()
            ));
        }
        let dest = stage_dir.join(&e.name);
        fs::write(&dest, finalize_content(&e.name, bytes))
            .map_err(|err| format!("{} の書き込みに失敗しました: {err}", dest.display()))?;
    }
    Ok(())
}

/// ステージング先のファイル（`SHA256SUMS` 以外）を読み直す。署名後のバイト列で計算するため。
fn read_staged(stage_dir: &Path, entries: &[Entry]) -> Result<Vec<(String, Vec<u8>)>, String> {
    entries
        .iter()
        .map(|e| {
            let p = stage_dir.join(&e.name);
            fs::read(&p)
                .map(|data| (e.name.clone(), data))
                .map_err(|err| format!("{} の読み込みに失敗しました: {err}", p.display()))
        })
        .collect()
}

/// 収集 → ステージング → 署名（任意）→ チェックサム → zip。zip のパスを返す。
pub fn assemble(
    inputs: &Inputs,
    out_dir: &Path,
    sign: Option<(&str, &Path, &str)>,
) -> Result<PathBuf, String> {
    let entries = plan(inputs)?;
    let name = bundle_name(&inputs.version);
    let stage_dir = out_dir.join(&name);
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .map_err(|e| format!("既存のステージングの削除に失敗しました: {e}"))?;
    }
    fs::create_dir_all(&stage_dir).map_err(|e| format!("ディレクトリの作成に失敗しました: {e}"))?;
    stage(&stage_dir, &entries, inputs.arch)?;

    if let Some((signtool, pfx, timestamp_url)) = sign {
        let password = std::env::var(PFX_PASSWORD_ENV).ok();
        for file in [DLL_FILE, IMSERVER_FILE, SETTINGS_FILE] {
            let target = stage_dir.join(file);
            run_signtool(
                signtool,
                &signtool_sign_args(pfx, timestamp_url, &target),
                password.as_deref(),
            )?;
            run_signtool(signtool, &signtool_verify_args(&target), None)?;
            println!("✓ 署名しました: {file}");
        }
    }

    let mut files = read_staged(&stage_dir, &entries)?;
    let sums = checksums(&files);
    fs::write(stage_dir.join(CHECKSUM_FILE), &sums)
        .map_err(|e| format!("{CHECKSUM_FILE} の書き込みに失敗しました: {e}"))?;
    files.push((CHECKSUM_FILE.to_string(), sums.into_bytes()));

    let zip_path = out_dir.join(zip_file_name(inputs.arch, &inputs.version));
    let zip = write_zip(&name, &files)?;
    fs::write(&zip_path, zip)
        .map_err(|e| format!("{} の書き込みに失敗しました: {e}", zip_path.display()))?;
    Ok(zip_path)
}

/// 既定の成果物パス（`<target_dir>[/<triple>]/release/<file>`）。
fn default_artifact(target_dir: &Path, triple: Option<&str>, file: &str) -> PathBuf {
    let mut p = target_dir.to_path_buf();
    if let Some(t) = triple {
        p.push(t);
    }
    p.join("release").join(file)
}

pub fn run(opts: Options) -> Result<(), String> {
    let repo = repo_root();
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("core").join("target"));
    let triple = opts.target.as_deref();
    if let Some(t) = triple
        && !t.starts_with(opts.arch.as_str())
    {
        return Err(format!(
            "--target {t} が --arch {} と一致しません",
            opts.arch.as_str()
        ));
    }
    let inputs = Inputs {
        arch: opts.arch,
        dll: opts
            .dll
            .unwrap_or_else(|| default_artifact(&target_dir, triple, DLL_FILE)),
        imserver: opts
            .imserver
            .unwrap_or_else(|| default_artifact(&target_dir, triple, IMSERVER_FILE)),
        settings: opts
            .settings
            .unwrap_or_else(|| default_artifact(&target_dir, triple, SETTINGS_FILE)),
        repo,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let out_dir = opts
        .out_dir
        .unwrap_or_else(|| target_dir.join("windows-bundle"));
    fs::create_dir_all(&out_dir).map_err(|e| format!("ディレクトリの作成に失敗しました: {e}"))?;
    let timestamp = opts
        .timestamp_url
        .as_deref()
        .unwrap_or(DEFAULT_TIMESTAMP_URL);
    let sign = opts
        .sign_pfx
        .as_deref()
        .map(|pfx| (opts.signtool.as_str(), pfx, timestamp));
    if sign.is_none() {
        eprintln!(
            "注意: Authenticode 署名なし（SmartScreen の警告が出ます。docs/windows-install.md）"
        );
    }
    let zip = assemble(&inputs, &out_dir, sign)?;
    println!("✓ zip を作成しました: {}", zip.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// 一時ディレクトリ（テストごとに別。終了時に削除）。
    struct Temp(PathBuf);
    impl Temp {
        fn new(tag: &str) -> Temp {
            let p = std::env::temp_dir()
                .join(format!("nagi-bundle-windows-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Temp(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 偽のリポジトリ（配布物の元ファイルを一通り置く）と偽の成果物を作る。
    fn fake_inputs(root: &Path) -> Inputs {
        let repo = root.join("repo");
        for (path, content) in [
            (
                "frontend-windows/dist/install.ps1",
                "Write-Host 'インストール'\n",
            ),
            ("frontend-windows/dist/uninstall.ps1", "Write-Host 'x'\n"),
            ("frontend-windows/dist/verify.ps1", "Write-Host 'v'\n"),
            ("frontend-windows/dist/apply-update.ps1", "Write-Host 'a'\n"),
            ("docs/windows-install.md", "# README\n"),
            ("LICENSE-MIT", "mit\n"),
            ("LICENSE-APACHE", "apache\n"),
            ("NOTICE", "notice\n"),
            // iOS 用の一覧（同梱しない）と Windows 用の一覧（同梱する）を区別する
            ("THIRD_PARTY_LICENSES.md", "ios\n"),
            (WINDOWS_LICENSES_FILE, "third\n"),
        ] {
            let p = repo.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        let dll = root.join("a.dll");
        let exe = root.join("b.exe");
        fs::write(&dll, b"MZ-dll").unwrap();
        fs::write(&exe, b"MZ-exe").unwrap();
        let settings = root.join("c.exe");
        fs::write(&settings, b"MZ-settings").unwrap();
        Inputs {
            dll,
            imserver: exe,
            settings,
            repo,
            version: "1.2.3".to_string(),
            arch: Arch::X86_64,
        }
    }

    fn read_zip(bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut f = archive.by_index(i).unwrap();
                let mut data = Vec::new();
                f.read_to_end(&mut data).unwrap();
                (f.name().to_string(), data)
            })
            .collect()
    }

    #[test]
    fn bundle_name_matches_release_asset() {
        // windows-release.yml と update の download_url が参照する名前
        assert_eq!(bundle_name("0.1.1"), "Nagi-windows-0.1.1");
    }

    #[test]
    fn zip_file_name_keeps_x86_64_name() {
        // 0.1.15 以前のクライアント・latest.json・web が参照する既存の名前
        assert_eq!(
            zip_file_name(Arch::X86_64, "0.1.1"),
            "Nagi-windows-0.1.1.zip"
        );
        assert_eq!(
            zip_file_name(Arch::Aarch64, "0.1.1"),
            "Nagi-windows-arm64-0.1.1.zip"
        );
    }

    /// 指定の Machine を持つ最小の PE ヘッダ（`MZ` + e_lfanew + `PE\0\0` + Machine）。
    fn fake_pe(machine: u16) -> Vec<u8> {
        let mut b = vec![0u8; 0x48];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C] = 0x40;
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        b[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
        b
    }

    #[test]
    fn pe_machine_reads_header() {
        assert_eq!(pe_machine(&fake_pe(0x8664)), Some(0x8664));
        assert_eq!(pe_machine(&fake_pe(0xAA64)), Some(0xAA64));
        assert_eq!(pe_machine(b"MZ-dll"), None);
    }

    #[test]
    fn aarch64_bundle_has_own_zip_name_and_arch_file() {
        let t = Temp::new("arm64");
        let mut inputs = fake_inputs(&t.0);
        inputs.arch = Arch::Aarch64;
        for p in [&inputs.dll, &inputs.imserver, &inputs.settings] {
            fs::write(p, fake_pe(0xAA64)).unwrap();
        }
        let out = t.0.join("out");
        fs::create_dir_all(&out).unwrap();
        let zip_path = assemble(&inputs, &out, None).unwrap();
        assert_eq!(zip_path, out.join("Nagi-windows-arm64-1.2.3.zip"));
        let files = read_zip(fs::read(&zip_path).unwrap());
        // 最上位ディレクトリの名前はアーキテクチャによらず同じ（apply-update.ps1 が参照する）
        assert!(
            files
                .iter()
                .all(|(n, _)| n.starts_with("Nagi-windows-1.2.3/"))
        );
        let arch = files
            .iter()
            .find(|(n, _)| n.ends_with("/ARCH"))
            .expect("ARCH が zip に無い");
        assert_eq!(arch.1, b"aarch64\n");
    }

    #[test]
    fn assemble_rejects_wrong_architecture_binaries() {
        let t = Temp::new("wrongarch");
        let mut inputs = fake_inputs(&t.0);
        inputs.arch = Arch::Aarch64;
        fs::write(&inputs.dll, fake_pe(0x8664)).unwrap(); // x64 の DLL
        let err = assemble(&inputs, &t.0.join("out"), None).unwrap_err();
        assert!(err.contains("一致しません"), "{err}");
    }

    #[test]
    fn plan_lists_all_distribution_files() {
        let t = Temp::new("plan");
        let entries = plan(&fake_inputs(&t.0)).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "nagi_windows_tsf.dll",
                "imserver.exe",
                "nagi-settings.exe",
                "install.ps1",
                "uninstall.ps1",
                "verify.ps1",
                "apply-update.ps1",
                "README.md",
                "LICENSE-MIT",
                "LICENSE-APACHE",
                "NOTICE",
                "THIRD_PARTY_LICENSES.md",
                "VERSION",
                "ARCH",
            ]
        );
    }

    #[test]
    fn plan_reports_missing_input() {
        let t = Temp::new("missing");
        let mut inputs = fake_inputs(&t.0);
        inputs.dll = t.0.join("nope.dll");
        let err = plan(&inputs).unwrap_err();
        assert!(err.contains("nagi_windows_tsf.dll"), "{err}");
    }

    #[test]
    fn assemble_rejects_non_pe_binaries() {
        let t = Temp::new("notpe");
        let inputs = fake_inputs(&t.0);
        fs::write(&inputs.imserver, b"\x7fELF").unwrap();
        let err = assemble(&inputs, &t.0.join("out"), None).unwrap_err();
        assert!(err.contains("PE"), "{err}");
    }

    #[test]
    fn assemble_builds_zip_with_checksums() {
        let t = Temp::new("zip");
        let inputs = fake_inputs(&t.0);
        let out = t.0.join("out");
        fs::create_dir_all(&out).unwrap();
        let zip_path = assemble(&inputs, &out, None).unwrap();
        assert_eq!(zip_path, out.join("Nagi-windows-1.2.3.zip"));

        let files = read_zip(fs::read(&zip_path).unwrap());
        let get = |name: &str| -> &Vec<u8> {
            &files
                .iter()
                .find(|(n, _)| n == &format!("Nagi-windows-1.2.3/{name}"))
                .unwrap_or_else(|| panic!("{name} が zip に無い"))
                .1
        };
        // すべて最上位ディレクトリの下にあり、SHA256SUMS を含めて 15 個
        assert_eq!(files.len(), 15);
        assert!(
            files
                .iter()
                .all(|(n, _)| n.starts_with("Nagi-windows-1.2.3/"))
        );
        assert_eq!(get("VERSION"), b"1.2.3\n");
        assert_eq!(get("ARCH"), b"x86_64\n");
        assert_eq!(get("nagi_windows_tsf.dll"), b"MZ-dll");
        assert_eq!(get("nagi-settings.exe"), b"MZ-settings");
        // ライセンス一覧は Windows 用（iOS 用ではない）が THIRD_PARTY_LICENSES.md の名前で入る
        assert_eq!(get("THIRD_PARTY_LICENSES.md"), b"third\n");

        // スクリプトだけ UTF-8 BOM 付き（Windows PowerShell 5.1 の文字化け対策）
        assert!(get("install.ps1").starts_with(UTF8_BOM));
        assert!(get("uninstall.ps1").starts_with(UTF8_BOM));
        assert!(get("verify.ps1").starts_with(UTF8_BOM));
        assert!(get("apply-update.ps1").starts_with(UTF8_BOM));
        assert!(!get("README.md").starts_with(UTF8_BOM));

        // SHA256SUMS は SHA256SUMS 以外のすべてを、zip の中身どおりに列挙する
        let sums = String::from_utf8(get(CHECKSUM_FILE).clone()).unwrap();
        assert_eq!(sums.lines().count(), 14);
        for (name, data) in files.iter().filter(|(n, _)| !n.ends_with(CHECKSUM_FILE)) {
            let short = name.strip_prefix("Nagi-windows-1.2.3/").unwrap();
            let expected = format!("{}  {short}", crate::update_sign::sha256_hex(data));
            assert!(sums.lines().any(|l| l == expected), "missing: {expected}");
        }
        // ステージング先にも同じものがある（署名の確認用に残す）
        assert_eq!(
            fs::read(out.join("Nagi-windows-1.2.3").join(CHECKSUM_FILE)).unwrap(),
            sums.as_bytes()
        );
    }

    #[test]
    fn zip_is_reproducible() {
        let t = Temp::new("repro");
        let inputs = fake_inputs(&t.0);
        let out = t.0.join("out");
        fs::create_dir_all(&out).unwrap();
        let a = fs::read(assemble(&inputs, &out, None).unwrap()).unwrap();
        let b = fs::read(assemble(&inputs, &out, None).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn bom_is_added_once() {
        assert_eq!(with_utf8_bom(b"a"), [0xEF, 0xBB, 0xBF, b'a']);
        let once = with_utf8_bom(b"a");
        assert_eq!(with_utf8_bom(&once), once);
    }

    #[test]
    fn checksums_are_sorted_and_two_space_separated() {
        let s = checksums(&[("b".to_string(), vec![]), ("a".to_string(), vec![])]);
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(s, format!("{empty}  a\n{empty}  b\n"));
    }

    #[test]
    fn signtool_args_keep_password_out() {
        let args = signtool_sign_args(Path::new("c.pfx"), "http://ts", Path::new("x.dll"));
        assert_eq!(
            args,
            [
                "sign",
                "/fd",
                "SHA256",
                "/f",
                "c.pfx",
                "/tr",
                "http://ts",
                "/td",
                "SHA256",
                "x.dll"
            ]
        );
        assert!(!args.iter().any(|a| a == "/p"));
        assert_eq!(
            signtool_verify_args(Path::new("x.dll")),
            ["verify", "/pa", "/q", "x.dll"]
        );
    }

    #[test]
    fn default_artifact_includes_triple() {
        let base = Path::new("t");
        assert_eq!(
            default_artifact(base, Some("x86_64-pc-windows-msvc"), "a.dll"),
            Path::new("t/x86_64-pc-windows-msvc/release/a.dll")
        );
        assert_eq!(
            default_artifact(base, None, "a.dll"),
            Path::new("t/release/a.dll")
        );
    }
}
