//! 環境・インストール・TSF 登録の診断（エンジンを起動しない項目）。
//!
//! 各項目は「事実を集める関数（`check_*`。`Ctx` を読む）」と「事実から判定する純粋な関数（`judge_*`）」に分ける。
//! 判定は OS 非依存で、Linux の単体テストで検証する。

use super::ctx::Ctx;
use super::pe::{Machine, read_machine};
use super::sys::{CLSID, PROFILE_GUID, TSF_DLL, WinFacts};
use super::tsflog;
use super::types::{ItemId, ItemResult, Status};
use std::path::{Path, PathBuf};

const WINDOWS_ONLY: &str = "Windows 専用の項目です（この OS では判定しません）";

// ---- CPU ----

/// CPU の種類
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arch {
    /// x64
    X64,
    /// ARM64
    Arm64,
    /// x86
    X86,
    /// 判定できない
    Unknown,
}

impl Arch {
    /// 表示名
    pub fn label(&self) -> &'static str {
        match self {
            Arch::X64 => "x64",
            Arch::Arm64 => "ARM64",
            Arch::X86 => "x86",
            Arch::Unknown => "不明",
        }
    }

    fn machine(&self) -> Option<Machine> {
        match self {
            Arch::X64 => Some(Machine::X64),
            Arch::Arm64 => Some(Machine::Arm64),
            Arch::X86 => Some(Machine::X86),
            Arch::Unknown => None,
        }
    }
}

fn parse_arch_name(name: &str) -> Arch {
    match name.trim().to_ascii_lowercase().as_str() {
        "x64" | "amd64" | "x86_64" => Arch::X64,
        "arm64" | "aarch64" => Arch::Arm64,
        "x86" | "i386" | "i686" => Arch::X86,
        _ => Arch::Unknown,
    }
}

/// OS（実機）の CPU。`RuntimeInformation.OSArchitecture` を優先し、無ければ環境変数
/// （`PROCESSOR_ARCHITEW6432` → `PROCESSOR_ARCHITECTURE`）。エミュレーション下では環境変数は実機の CPU を
/// 表さないことがあるため、後者は最後の手段
pub fn os_arch(f: &WinFacts) -> Arch {
    for name in [&f.os_arch, &f.env_arch_w6432, &f.env_arch] {
        let a = parse_arch_name(name);
        if a != Arch::Unknown {
            return a;
        }
    }
    Arch::Unknown
}

/// 配布物の CPU の判定に使う入力
pub struct ArchInput<'a> {
    /// OS の CPU
    pub os: Arch,
    /// この診断プログラム自身の CPU（`std::env::consts::ARCH`）
    pub tool: &'a str,
    /// 配布物 `(ファイル名, PE の Machine。読めなければ理由)`
    pub files: Vec<(String, Result<Machine, String>)>,
    /// 同梱の `ARCH` ファイルの内容（無ければ `None`）
    pub arch_file: Option<String>,
}

/// CPU と配布物の整合を判定する（H-107: TSF DLL は ARM64 ネイティブのみ。x64 の DLL を ARM64 機に入れない）
pub fn judge_arch(input: &ArchInput) -> ItemResult {
    let id = ItemId::EnvArch;
    let mut lines = vec![format!("OS の CPU: {}", input.os.label())];
    let tool = parse_arch_name(input.tool);
    lines.push(format!("この診断プログラム: {}", tool.label()));
    let mut problems: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if input.os == Arch::Arm64 && tool == Arch::X64 {
        notes.push(
            "診断プログラムが x64 エミュレーションで動いています（ARM64 版を入れた場合は ARM64 版の nagi-settings.exe が正）"
                .to_owned(),
        );
    }
    let expected = input.os.machine();
    for (name, machine) in &input.files {
        match machine {
            Ok(m) => {
                lines.push(format!("{name}: {}", m.label()));
                if let Some(expected) = expected
                    && *m != expected
                {
                    problems.push(format!(
                        "{name} は {} 用です（この PC は {}）",
                        m.label(),
                        input.os.label()
                    ));
                }
            }
            Err(reason) => lines.push(format!("{name}: 読めません（{reason}）")),
        }
    }
    if let Some(arch_file) = &input.arch_file {
        let declared = parse_arch_name(arch_file);
        lines.push(format!("ARCH ファイル: {}", arch_file.trim()));
        if declared != Arch::Unknown && input.os != Arch::Unknown && declared != input.os {
            problems.push(format!(
                "ARCH ファイルは {} ですが、この PC は {} です",
                declared.label(),
                input.os.label()
            ));
        }
    }
    if input.os == Arch::Unknown {
        return ItemResult::warn(
            id,
            "OS の CPU を判定できませんでした",
            "PowerShell を実行できない環境（制限付き・グループポリシー）の可能性があります。",
        )
        .with_evidence(lines);
    }
    if !problems.is_empty() {
        let advice = if input.os == Arch::Arm64 {
            "ARM64 の PC には Nagi-windows-arm64 の配布物を入れてください（x64 の配布物は ARM64 ネイティブのアプリでは使えません）。"
        } else {
            "この PC の CPU に合う配布物（x64 は Nagi-windows、ARM64 は Nagi-windows-arm64）を入れ直してください。"
        };
        return ItemResult::ng(id, problems.join(" / "), advice).with_evidence(lines);
    }
    let mut detail = format!("{} の PC に合った配布物です", input.os.label());
    if input.os == Arch::Arm64 {
        detail.push_str("（ARM64 ネイティブのアプリで入力できます。x64 エミュレーションで動くアプリでは Nagi は出ません。既知の制約）");
    }
    let mut result = ItemResult::ok(id, detail);
    if !notes.is_empty() {
        result.status = Status::Warn;
        result.advice = Some(notes.join(" / "));
    }
    result.with_evidence(lines)
}

/// CPU 項目
pub fn check_arch(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::EnvArch, WINDOWS_ONLY);
    }
    let os = ctx.facts().map_or(Arch::Unknown, os_arch);
    let files = [
        TSF_DLL.to_owned(),
        file_name_of(&ctx.imserver_exe),
        "nagi-settings.exe".to_owned(),
    ]
    .into_iter()
    .filter_map(|name| {
        let path = if name == file_name_of(&ctx.imserver_exe) {
            ctx.imserver_exe.clone()
        } else {
            ctx.install_dir.join(&name)
        };
        path.is_file().then(|| (name, read_machine(&path)))
    })
    .collect();
    let arch_file = std::fs::read_to_string(ctx.install_dir.join("ARCH")).ok();
    judge_arch(&ArchInput {
        os,
        tool: &ctx.arch,
        files,
        arch_file,
    })
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 報告の先頭に載せる環境の要約（OS・CPU・診断プログラムの版。ユーザー名・パスは含めない）
pub fn environment_summary(ctx: &Ctx) -> String {
    let mut s = String::new();
    match ctx.facts() {
        Some(f) => {
            let caption = if f.os_caption.is_empty() {
                "Windows"
            } else {
                &f.os_caption
            };
            s.push_str(&format!(
                "{caption} {} (build {}) / CPU {}",
                f.os_version,
                f.os_build,
                os_arch(f).label()
            ));
        }
        None => s.push_str(&format!("{} / CPU {}", ctx.os, ctx.arch)),
    }
    s
}

// ---- 実行ユーザーと書き込み先 ----

/// 書き込みテスト（一時ファイルを作って消す）。ディレクトリが無ければ作る
pub fn write_probe(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("フォルダを作れません（{}）", e.kind()))?;
    let probe = dir.join(format!(".nagi-diag-write-{}", std::process::id()));
    std::fs::write(&probe, b"x").map_err(|e| format!("書き込めません（{}）", e.kind()))?;
    std::fs::remove_file(&probe).map_err(|e| format!("削除できません（{}）", e.kind()))
}

/// 書き込み先 1 か所の結果
pub struct WriteTarget {
    /// 表示名
    pub label: &'static str,
    /// 表示用のパス
    pub shown: String,
    /// 結果
    pub result: Result<(), String>,
}

/// 実行ユーザーと書き込み先を判定する
pub fn judge_user(
    account: &str,
    targets: &[WriteTarget],
    install_writable: Option<bool>,
) -> ItemResult {
    let id = ItemId::EnvUser;
    let account_label = match account {
        "standard" => "標準ユーザー",
        "admin_not_elevated" => "管理者アカウント（昇格なしで実行）",
        "elevated_admin" => "管理者として実行中",
        _ => "不明",
    };
    let mut lines = vec![format!("実行ユーザー: {account_label}")];
    let mut failed: Vec<String> = Vec::new();
    for t in targets {
        match &t.result {
            Ok(()) => lines.push(format!("{}: 書き込めます（{}）", t.label, t.shown)),
            Err(e) => {
                lines.push(format!("{}: {e}（{}）", t.label, t.shown));
                failed.push(t.label.to_owned());
            }
        }
    }
    if let Some(w) = install_writable {
        lines.push(format!(
            "インストール先への書き込み: {}",
            if w {
                "できる"
            } else {
                "できない（標準ユーザーでは通常の状態）"
            }
        ));
    }
    if !failed.is_empty() {
        return ItemResult::ng(
            id,
            format!("書き込めない場所があります: {}", failed.join("・")),
            "設定・学習データ・ログが保存できません。フォルダの権限（所有者・ウイルス対策ソフトのフォルダ保護）を確認してください。",
        )
        .with_evidence(lines);
    }
    if account == "elevated_admin" {
        return ItemResult::warn(
            id,
            "管理者として実行しているため、標準ユーザーでの動作の確認にはなりません",
            "通常のダブルクリックで（右クリックの「管理者として実行」を使わずに）起動し直してください。",
        )
        .with_evidence(lines);
    }
    if account == "standard" && install_writable == Some(true) {
        return ItemResult::warn(
            id,
            "標準ユーザーがインストール先に書き込めます",
            "インストール先の DLL・exe を一般ユーザーが差し替えられる状態です。インストール先の権限（Program Files 既定の ACL）を確認してください。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(
        id,
        format!("{account_label}で、設定・学習・ログの保存先に書き込めます"),
    )
    .with_evidence(lines)
}

/// 実行ユーザー項目
pub fn check_user(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::EnvUser, WINDOWS_ONLY);
    }
    let account = ctx.facts().map_or("", |f| f.account.as_str()).to_owned();
    let mut targets = Vec::new();
    let candidates: [(&'static str, Option<PathBuf>); 3] = [
        ("設定・学習の保存先", ctx.user_dir.clone()),
        ("ログの保存先", ctx.log_dir.clone()),
        ("更新の保存先", ctx.update_dir.clone()),
    ];
    for (label, dir) in candidates {
        if let Some(dir) = dir {
            targets.push(WriteTarget {
                label,
                shown: ctx.show(&dir),
                result: write_probe(&dir),
            });
        }
    }
    // 昇格している場合は Program Files に触らない
    let install_writable = (account != "elevated_admin" && ctx.install_dir.is_dir())
        .then(|| write_probe_existing(&ctx.install_dir));
    judge_user(&account, &targets, install_writable)
}

/// 既存のディレクトリにだけ書き込みテストをする（作成はしない）
fn write_probe_existing(dir: &Path) -> bool {
    dir.is_dir() && write_probe(dir).is_ok()
}

// ---- VC++ ランタイム ----

/// VC++ ランタイムを判定する
pub fn judge_vcruntime(f: &WinFacts) -> ItemResult {
    let id = ItemId::EnvVcRuntime;
    let os = os_arch(f);
    let lines: Vec<String> = f
        .vc_files
        .iter()
        .map(|v| {
            if v.exists {
                format!("{}: あり（{}）", v.name, v.version)
            } else {
                format!("{}: なし", v.name)
            }
        })
        .collect();
    let missing: Vec<&str> = f
        .vc_files
        .iter()
        .filter(|v| !v.exists)
        .map(|v| v.name.as_str())
        .collect();
    let mut lines = lines;
    for r in &f.vc_reg {
        lines.push(format!(
            "再頒布パッケージ（{}）: {}（{}）",
            r.arch,
            if r.installed == 1 {
                "導入済み"
            } else {
                "未導入"
            },
            r.version
        ));
    }
    if f.vc_files.is_empty() {
        return ItemResult::warn(
            id,
            "VC++ ランタイムの有無を取得できませんでした",
            "変換エンジンの起動項目（engine.start）の結果を見てください。",
        );
    }
    if !missing.is_empty() {
        let url = if os == Arch::Arm64 {
            "https://aka.ms/vs/17/release/vc_redist.arm64.exe"
        } else {
            "https://aka.ms/vs/17/release/vc_redist.x64.exe"
        };
        return ItemResult::ng(
            id,
            format!("VC++ ランタイムの DLL が見つかりません: {}", missing.join("・")),
            format!(
                "変換エンジン（imserver.exe）が起動しない原因になります。Microsoft の「Visual C++ 再頒布可能パッケージ」を入れてください: {url}（入力方式の DLL は静的リンクで、これが無くても読み込まれます）"
            ),
        )
        .with_evidence(lines);
    }
    ItemResult::ok(id, "VC++ ランタイムの DLL がそろっています").with_evidence(lines)
}

/// VC++ ランタイム項目
pub fn check_vcruntime(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::EnvVcRuntime, WINDOWS_ONLY);
    }
    match ctx.facts() {
        Some(f) => judge_vcruntime(f),
        None => ItemResult::warn(
            ItemId::EnvVcRuntime,
            "Windows の情報を取得できませんでした（PowerShell を実行できません）",
            "変換エンジンの起動項目（engine.start）の結果を見てください。",
        ),
    }
}

// ---- DPI・テーマ ----

/// DPI・テーマを記録する（判定ではなく記録。取得できなければ要確認）
pub fn judge_display(f: &WinFacts) -> ItemResult {
    let id = ItemId::EnvDisplay;
    let mut lines = Vec::new();
    match f.dpi {
        Some(dpi) => lines.push(format!(
            "DPI: {dpi}（拡大率 {}%）",
            u64::from(dpi) * 100 / 96
        )),
        None => lines.push("DPI: 取得できません".to_owned()),
    }
    let theme = |v: Option<bool>| match v {
        Some(true) => "ライト",
        Some(false) => "ダーク",
        None => "不明",
    };
    lines.push(format!("アプリのテーマ: {}", theme(f.apps_light)));
    lines.push(format!("システムのテーマ: {}", theme(f.system_light)));
    lines.push(format!(
        "ハイコントラスト: {}",
        match f.high_contrast {
            Some(true) => "オン",
            Some(false) => "オフ",
            None => "不明",
        }
    ));
    if let Some(n) = f.monitors {
        lines.push(format!("モニター数: {n}"));
    }
    let detail = lines.join(" / ");
    if f.dpi.is_none() && f.apps_light.is_none() {
        return ItemResult::warn(
            id,
            "DPI・テーマを取得できませんでした",
            "表示の確認は設定画面を開いて目視してください。",
        );
    }
    ItemResult::ok(id, detail)
}

/// DPI・テーマ項目
pub fn check_display(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::EnvDisplay, WINDOWS_ONLY);
    }
    match ctx.facts() {
        Some(f) => judge_display(f),
        None => ItemResult::warn(
            ItemId::EnvDisplay,
            "Windows の情報を取得できませんでした",
            "PowerShell を実行できない環境の可能性があります。",
        ),
    }
}

// ---- インストール先 ----

fn size_label(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(m) => format!("{} KB", m.len().div_ceil(1024)),
        Err(_) => "?".to_owned(),
    }
}

/// インストール先のファイル構成
pub fn check_install_files(ctx: &Ctx) -> ItemResult {
    let id = ItemId::InstallFiles;
    let mut required: Vec<(String, PathBuf)> =
        vec![(file_name_of(&ctx.imserver_exe), ctx.imserver_exe.clone())];
    let mut optional: Vec<(String, PathBuf)> = Vec::new();
    if ctx.is_windows() {
        required.push((TSF_DLL.to_owned(), ctx.install_dir.join(TSF_DLL)));
        required.push((
            "nagi-settings.exe".to_owned(),
            ctx.install_dir.join("nagi-settings.exe"),
        ));
        required.push(("VERSION".to_owned(), ctx.install_dir.join("VERSION")));
        for name in ["apply-update.ps1", "verify.ps1", "nagi.ico", "ARCH"] {
            optional.push((name.to_owned(), ctx.install_dir.join(name)));
        }
    }
    let mut lines = Vec::new();
    let mut missing = Vec::new();
    for (name, path) in &required {
        if path.is_file() {
            lines.push(format!("{name}: あり（{}）", size_label(path)));
        } else {
            lines.push(format!("{name}: なし"));
            missing.push(name.clone());
        }
    }
    let mut missing_optional = Vec::new();
    for (name, path) in &optional {
        if path.is_file() {
            lines.push(format!("{name}: あり"));
        } else {
            lines.push(format!("{name}: なし"));
            missing_optional.push(name.clone());
        }
    }
    lines.insert(0, format!("場所: {}", ctx.show(&ctx.install_dir)));
    if !missing.is_empty() {
        return ItemResult::ng(
            id,
            format!("必要なファイルがありません: {}", missing.join("・")),
            "Nagi を入れ直してください（Nagi-setup-<版>.exe、または install.ps1）。",
        )
        .with_evidence(lines);
    }
    if ctx.is_windows() {
        let installed = std::fs::read_to_string(ctx.install_dir.join("VERSION"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let own = env!("CARGO_PKG_VERSION");
        lines.push(format!("VERSION: {installed}（この診断プログラム: {own}）"));
        if installed != own {
            return ItemResult::warn(
                id,
                format!("インストール先の版（{installed}）とこの診断プログラムの版（{own}）が違います"),
                "更新の途中（サインアウト・再起動前）か、別の場所の nagi-settings.exe を実行している可能性があります。サインアウトまたは再起動してから再実行してください。",
            )
            .with_evidence(lines);
        }
    }
    if !missing_optional.is_empty() {
        return ItemResult::warn(
            id,
            format!("なくても動きますが、あるべきファイルがありません: {}", missing_optional.join("・")),
            "更新の適用（apply-update.ps1）・状態確認（verify.ps1）が使えません。Nagi を入れ直すと戻ります。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(id, "必要なファイルがそろっています").with_evidence(lines)
}

/// 更新の残りを判定する（`.old`・staging・旧版の imserver）
pub fn judge_old_files(old_files: &[String], staging: bool, old_procs: &[String]) -> ItemResult {
    const MAX_DLL_OLD: usize = 20;
    let id = ItemId::InstallOldFiles;
    let mut lines: Vec<String> = old_files
        .iter()
        .map(|f| format!("残っているファイル: {f}"))
        .collect();
    if staging {
        lines.push("staging フォルダが残っています".to_owned());
    }
    lines.extend(
        old_procs
            .iter()
            .map(|p| format!("旧版の imserver が動作中: {p}")),
    );
    if old_files.is_empty() && !staging && old_procs.is_empty() {
        return ItemResult::ok(
            id,
            "更新の残り（.old・staging・旧版のエンジン）はありません",
        );
    }
    let dll_old = old_files
        .iter()
        .filter(|f| f.to_ascii_lowercase().starts_with("nagi_windows_tsf.dll."))
        .count();
    let too_many = dll_old > MAX_DLL_OLD;
    if old_procs.is_empty() && !staging && !too_many {
        // DLL の .old は読み込み中は消せない更新後の正常な残り。プロセスの無い imserver.exe.old も無害
        let has_exe_old = old_files
            .iter()
            .any(|f| f.to_ascii_lowercase().starts_with("imserver.exe"));
        let mut detail = format!(
            ".old が {} 個残っています（アプリが DLL を読み込み中は削除できない、更新後の正常な残りです。次回の更新・アンインストールで片付きます）",
            old_files.len()
        );
        if has_exe_old {
            detail.push_str(
                "。imserver.exe.old が残っていますが、そこから動いているエンジンはありません",
            );
        }
        return ItemResult::ok(id, detail).with_evidence(lines);
    }
    let mut parts = Vec::new();
    if !old_files.is_empty() {
        parts.push(format!(".old が {} 個残っています", old_files.len()));
    }
    if staging {
        parts.push("staging が残っています".to_owned());
    }
    if !old_procs.is_empty() {
        parts.push(format!("旧版のエンジンが {} 個動作中です", old_procs.len()));
    }
    ItemResult::warn(
        id,
        parts.join(" / "),
        "サインアウトまたは再起動で旧版が切り替わり、.old は次回のインストール・アンインストールで片付きます。再起動後も残る場合は、残っているファイル名を報告してください（imserver.exe.old が残る = 旧エンジンが止まらなかった）。",
    )
    .with_evidence(lines)
}

/// 更新の残り項目
pub fn check_old_files(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::InstallOldFiles, WINDOWS_ONLY);
    }
    let mut old_files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&ctx.install_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".old") {
                old_files.push(name);
            }
        }
    }
    old_files.sort();
    let staging = ctx.install_dir.join("staging").is_dir();
    let old_procs: Vec<String> = ctx
        .facts()
        .map(|f| {
            f.imserver_procs
                .iter()
                .filter(|p| p.path.to_ascii_lowercase().ends_with(".old"))
                .map(|p| format!("PID {} ({})", p.pid, ctx.redact(&p.path)))
                .collect()
        })
        .unwrap_or_default();
    judge_old_files(&old_files, staging, &old_procs)
}

/// 署名の状態を判定する（記録が目的。署名なし配布が現在の前提）
pub fn judge_signature(f: &WinFacts) -> ItemResult {
    let id = ItemId::InstallSignature;
    if f.signatures.is_empty() {
        return ItemResult::warn(
            id,
            "署名の状態を取得できませんでした（ファイルが無い・PowerShell を実行できない）",
            "インストール先のファイル構成の項目を確認してください。",
        );
    }
    let lines: Vec<String> = f
        .signatures
        .iter()
        .map(|s| {
            let mut l = format!("{}: {}", s.file, s.status);
            if !s.subject.is_empty() {
                l.push_str(&format!("（署名者: {}", s.subject));
                l.push_str(if s.timestamped {
                    "、タイムスタンプあり）"
                } else {
                    "、タイムスタンプなし）"
                });
            }
            l
        })
        .collect();
    let bad: Vec<&str> = f
        .signatures
        .iter()
        .filter(|s| s.status == "HashMismatch")
        .map(|s| s.file.as_str())
        .collect();
    if !bad.is_empty() {
        return ItemResult::ng(
            id,
            format!(
                "署名後に内容が変わっています（HashMismatch）: {}",
                bad.join("・")
            ),
            "ファイルが壊れているか書き換えられています。Nagi を入れ直してください。",
        )
        .with_evidence(lines);
    }
    let unsigned = f
        .signatures
        .iter()
        .filter(|s| s.status == "NotSigned")
        .count();
    let valid = f.signatures.iter().filter(|s| s.status == "Valid").count();
    if unsigned == f.signatures.len() {
        return ItemResult::ok(
            id,
            "署名なし（現在の配布は Authenticode 署名なしが前提。SmartScreen の警告が出ることがあります）",
        )
        .with_evidence(lines);
    }
    if valid == f.signatures.len() {
        return ItemResult::ok(id, "すべて有効な署名があります").with_evidence(lines);
    }
    ItemResult::warn(
        id,
        "署名の状態がそろっていません（一部が署名なし・信頼されない・不明）",
        "状態の一覧を報告してください（署名の運用が始まっていれば、署名漏れのファイルがあります）。",
    )
    .with_evidence(lines)
}

/// 署名項目
pub fn check_signature(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::InstallSignature, WINDOWS_ONLY);
    }
    match ctx.facts() {
        Some(f) => judge_signature(f),
        None => ItemResult::warn(
            ItemId::InstallSignature,
            "Windows の情報を取得できませんでした",
            "PowerShell を実行できない環境の可能性があります。",
        ),
    }
}

// ---- 辞書・モデル ----

/// 辞書・モデルの有無
pub fn check_data_files(ctx: &Ctx) -> ItemResult {
    let id = ItemId::DataFiles;
    let Some(dir) = &ctx.data_dir else {
        return ItemResult::warn(
            id,
            "データの置き場（--data-dir）が決まりません",
            "--data-dir で辞書・モデルの場所を指定するか、環境変数 NAGI_DATA_DIR を設定してください。",
        );
    };
    let dict = dir.join("dict").join("nagi-dict.bin");
    let mut lines = vec![format!("データの場所: {}", ctx.show(dir))];
    let dict_size = std::fs::metadata(&dict).map(|m| m.len()).ok();
    match dict_size {
        Some(n) => lines.push(format!("辞書 dict/nagi-dict.bin: {} MB", n / (1024 * 1024))),
        None => lines.push("辞書 dict/nagi-dict.bin: なし".to_owned()),
    }
    let mut models: Vec<(String, u64)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("models")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".gguf") {
                models.push((name, e.metadata().map_or(0, |m| m.len())));
            }
        }
    }
    models.sort();
    for (name, size) in &models {
        lines.push(format!("モデル {name}: {} MB", size / (1024 * 1024)));
    }
    let install_hint = format!(
        "辞書・モデルを取得してください（Nagi のセットアップ、または `imserver.exe --install-data --data-dir \"{}\"`）。",
        ctx.show(dir)
    );
    if dict_size.is_none_or(|n| n == 0) {
        return ItemResult::ng(
            id,
            "辞書がありません。変換できず、読みがそのまま出ます",
            install_hint,
        )
        .with_evidence(lines);
    }
    if models.is_empty() || models.iter().all(|(_, s)| *s == 0) {
        lines.push("モデル: なし".to_owned());
        return ItemResult::warn(
            id,
            "モデル（.gguf）がありません。辞書だけで変換します（精度が下がります）",
            install_hint,
        )
        .with_evidence(lines);
    }
    ItemResult::ok(id, "辞書とモデルがあります").with_evidence(lines)
}

// ---- TSF 登録 ----

/// 登録されているべきカテゴリ（`register.rs` の `RegisterCategory` と同じ 5 つ）
pub const REQUIRED_CATEGORIES: &[(&str, &str)] = &[
    ("TIP_KEYBOARD", "{34745C63-B2F0-4784-8B67-5E12C8701A31}"),
    (
        "TIPCAP_IMMERSIVESUPPORT",
        "{13A016DF-560B-46CD-947A-4C3AF1E0E35D}",
    ),
    (
        "TIPCAP_SYSTRAYSUPPORT",
        "{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}",
    ),
    (
        "TIPCAP_UIELEMENTENABLED",
        "{49D2F9CF-1F5E-11D7-A6D3-00065B84435C}",
    ),
    (
        "DISPLAYATTRIBUTEPROVIDER",
        "{046B8C80-1647-40F7-9B21-B93B81AABC1B}",
    ),
];

fn norm_path(p: &str) -> String {
    p.trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_ascii_lowercase()
}

/// TSF の COM 登録を判定する。`dll_exists` は登録されたパスのファイルが実在するかの問い合わせ
pub fn judge_registration(
    f: &WinFacts,
    install_dll: &str,
    dll_exists: &dyn Fn(&str) -> bool,
) -> ItemResult {
    let id = ItemId::TsfRegistration;
    let mut lines = vec![format!("CLSID {CLSID}")];
    if f.clsid_inproc.trim().is_empty() {
        return ItemResult::ng(
            id,
            "入力方式（COM サーバー）が登録されていません",
            "管理者権限で登録してください: install.ps1 を再実行、または管理者のコマンドプロンプトで regsvr32 \"<インストール先>\\nagi_windows_tsf.dll\"。",
        )
        .with_evidence(lines);
    }
    lines.push(format!("InprocServer32: {}", f.clsid_inproc));
    lines.push(format!("ThreadingModel: {}", f.clsid_threading));
    let mut problems: Vec<String> = Vec::new();
    let mut warns: Vec<String> = Vec::new();
    if !dll_exists(&f.clsid_inproc) {
        problems.push("登録された DLL のファイルが存在しません".to_owned());
    }
    if !install_dll.is_empty() && norm_path(&f.clsid_inproc) != norm_path(install_dll) {
        warns.push(format!(
            "登録された DLL の場所が、この診断プログラムと同じ場所の DLL と違います（登録: {}）",
            f.clsid_inproc
        ));
    }
    if !f.clsid_threading.eq_ignore_ascii_case("Apartment") {
        problems.push(format!(
            "ThreadingModel が Apartment ではありません（{}）",
            f.clsid_threading
        ));
    }
    if f.tip_profile_exists {
        lines.push(format!(
            "プロファイル {PROFILE_GUID}: あり（{}）",
            f.tip_profile_desc
        ));
    } else {
        problems.push("入力方式のプロファイル（日本語 0x0411）が登録されていません".to_owned());
    }
    let have: Vec<String> = f
        .tip_categories
        .iter()
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let missing: Vec<&str> = REQUIRED_CATEGORIES
        .iter()
        .filter(|(_, guid)| !have.iter().any(|h| h == guid))
        .map(|(name, _)| *name)
        .collect();
    lines.push(format!(
        "カテゴリ: {} / {} 個",
        REQUIRED_CATEGORIES.len() - missing.len(),
        REQUIRED_CATEGORIES.len()
    ));
    if !missing.is_empty() {
        problems.push(format!("カテゴリが足りません: {}", missing.join("・")));
    }
    if !problems.is_empty() {
        return ItemResult::ng(
            id,
            problems.join(" / "),
            "管理者権限で再登録してください（install.ps1 の再実行、または regsvr32）。タスクバーのアイコンが出ない・UILess が効かない原因になります。",
        )
        .with_evidence(lines);
    }
    if !warns.is_empty() {
        return ItemResult::warn(
            id,
            warns.join(" / "),
            "別の場所の DLL を登録している可能性があります。意図しなければ install.ps1 を再実行してください。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(
        id,
        "CLSID・プロファイル・カテゴリ（5 種）が登録されています",
    )
    .with_evidence(lines)
}

/// TSF 登録項目
pub fn check_registration(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::TsfRegistration, WINDOWS_ONLY);
    }
    let Some(f) = ctx.facts() else {
        return ItemResult::warn(
            ItemId::TsfRegistration,
            "レジストリを読めませんでした（PowerShell を実行できません）",
            "verify.ps1 status で登録状態を確認してください。",
        );
    };
    let dll = ctx.install_dir.join(TSF_DLL);
    judge_registration(f, &dll.display().to_string(), &|p| {
        Path::new(p.trim().trim_matches('"')).is_file()
    })
}

/// このユーザーで入力方式が有効かを判定する
pub fn judge_user_profile(f: &WinFacts) -> ItemResult {
    let id = ItemId::TsfUserProfile;
    let want = format!("0411:{CLSID}{PROFILE_GUID}").to_ascii_uppercase();
    if f.user_tips.is_empty() {
        return ItemResult::warn(
            id,
            "このユーザーの入力方式の一覧を取得できませんでした",
            "設定 → 時刻と言語 → 言語と地域 → 日本語 → 言語のオプション → キーボードで Nagi があるか目視してください。",
        );
    }
    if f.user_tips.iter().any(|t| t.to_ascii_uppercase() == want) {
        return ItemResult::ok(id, "このユーザーのキーボード一覧に Nagi があります");
    }
    ItemResult::warn(
        id,
        "このユーザーのキーボード一覧に Nagi がありません",
        "設定 → 時刻と言語 → 言語と地域 → 日本語 → 言語のオプション → キーボードを追加する → Nagi を選んでください（標準ユーザーでインストールした場合など、インストールしたアカウントと別のアカウントでは追加が必要です）。",
    )
}

/// ユーザープロファイル項目
pub fn check_user_profile(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::TsfUserProfile, WINDOWS_ONLY);
    }
    match ctx.facts() {
        Some(f) => judge_user_profile(f),
        None => ItemResult::warn(
            ItemId::TsfUserProfile,
            "Windows の情報を取得できませんでした",
            "設定 → 時刻と言語 → 言語と地域で Nagi があるか目視してください。",
        ),
    }
}

/// TIP の COM インターフェイスを判定する
pub fn judge_interfaces(p: &super::sys::ComProbe) -> ItemResult {
    let id = ItemId::TsfInterfaces;
    let mark = |b: bool| {
        if b {
            "取得できた"
        } else {
            "取得できない"
        }
    };
    let lines = vec![
        format!("ITfTextInputProcessor: {}", mark(p.text_input_processor)),
        format!(
            "ITfTextInputProcessorEx: {}",
            mark(p.text_input_processor_ex)
        ),
        format!("ITfFnConfigure: {}", mark(p.fn_configure)),
        format!(
            "ITfDisplayAttributeProvider: {}",
            mark(p.display_attribute_provider)
        ),
    ];
    if !p.created {
        return ItemResult::ng(
            id,
            format!(
                "登録された TIP（COM）を作れません{}",
                if p.error.is_empty() { String::new() } else { format!("（{}）", p.error) }
            ),
            "DLL が無い・壊れている・CPU が合わない・登録が古い可能性があります。登録の項目（tsf.registration）と CPU の項目（env.arch）を確認し、install.ps1 を再実行してください。",
        )
        .with_evidence(lines);
    }
    if !p.text_input_processor || !p.text_input_processor_ex {
        return ItemResult::ng(
            id,
            "TIP の基本インターフェイスが取得できません",
            "DLL が壊れている可能性があります。Nagi を入れ直してください。",
        )
        .with_evidence(lines);
    }
    if !p.fn_configure {
        return ItemResult::ng(
            id,
            "ITfFnConfigure が取得できません。言語のオプションから設定画面を開けません",
            "報告してください（DLL の実装の不具合です）。設定画面はスタートメニューの「Nagi 設定」・右クリックメニューから開けます。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(
        id,
        "ITfFnConfigure を含むインターフェイスが取得できました（言語のオプションから設定画面を開く入口は実装されています。実際に開くかは手動項目で確認）",
    )
    .with_evidence(lines)
}

/// TIP インターフェイス項目
pub fn check_interfaces(ctx: &Ctx) -> ItemResult {
    if !ctx.is_windows() {
        return ItemResult::skipped(ItemId::TsfInterfaces, WINDOWS_ONLY);
    }
    // 登録が無いときは作れないのが当然なので、登録の項目に任せる
    if ctx
        .facts()
        .is_some_and(|f| f.clsid_inproc.trim().is_empty())
    {
        return ItemResult::skipped(
            ItemId::TsfInterfaces,
            "入力方式が登録されていないため判定しません（tsf.registration を参照）",
        );
    }
    match ctx.system.com_probe() {
        Some(p) => judge_interfaces(&p),
        None => ItemResult::warn(
            ItemId::TsfInterfaces,
            "COM の調査を実行できませんでした（PowerShell を実行できない・時間切れ）",
            "言語のオプションから設定画面が開くかを手動項目で確認してください。",
        ),
    }
}

// ---- tsf.log ----

fn read_tsf_log(ctx: &Ctx) -> Option<String> {
    let path = ctx.log_dir.as_ref()?.join("tsf.log");
    let bytes = std::fs::read(path).ok()?;
    // 上限 256KB のログだが、念のため末尾 512KB だけ読む
    let start = bytes.len().saturating_sub(512 * 1024);
    Some(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

/// 再変換の入口（`ITfFunctionProvider`）の項目
pub fn check_function_provider(ctx: &Ctx) -> ItemResult {
    let id = ItemId::TsfFunctionProvider;
    if !ctx.is_windows() {
        return ItemResult::skipped(id, WINDOWS_ONLY);
    }
    let analysis = read_tsf_log(ctx).map(|t| tsflog::analyze(&t));
    let hosts = ctx.facts().map(|f| f.dll_hosts.clone()).unwrap_or_default();
    let mut lines = vec![format!(
        "Nagi の DLL を読み込んでいるアプリ: {}",
        if hosts.is_empty() {
            "なし".to_owned()
        } else {
            hosts.join(", ")
        }
    )];
    match analysis {
        Some(a) if a.activate_done => {
            lines.push("tsf.log: Activate 完了の記録あり".to_owned());
            ItemResult::ok(
                id,
                "入力方式が有効化されています（ITfFunctionProvider はこの有効化の中で登録を試みます）。登録の成否は記録に残らないため、入口が実際に呼ばれるかは実アプリの再変換（手動項目）で確認してください",
            )
            .with_evidence(lines)
        }
        Some(_) | None => ItemResult::warn(
            id,
            "入力方式が有効化された記録がありません（tsf.log が無い、または Activate の記録が無い）",
            "Nagi に切り替えて（Win+Space）メモ帳などで何か入力してから、もう一度実行してください。",
        )
        .with_evidence(lines),
    }
}

/// `tsf.log` を判定する
pub fn judge_tsf_log(a: &tsflog::LogAnalysis, now_unix: i64) -> ItemResult {
    judge_tsf_log_at(a, now_unix, None, None)
}

/// 更新直後に出ることがある旨の案内を出す期間（インストールからの秒数）
const FRESH_INSTALL_SECS: i64 = 600;

/// `install_unix` はインストール・更新の時刻、`engine_log` は imserver ログの該当行（`None` は未取得）
pub fn judge_tsf_log_at(
    a: &tsflog::LogAnalysis,
    now_unix: i64,
    install_unix: Option<i64>,
    engine_log: Option<&[String]>,
) -> ItemResult {
    let id = ItemId::TsfLog;
    let mut lines = vec![format!("読んだ行数: {}", a.lines)];
    lines.push(format!(
        "言語のオプションからの起動の記録: {}",
        if a.configure_requested {
            "あり"
        } else {
            "なし"
        }
    ));
    lines.push(format!(
        "UILess（アプリ側の候補表示）の記録: {}",
        if a.uiless { "あり" } else { "なし" }
    ));
    if let Some(t) = install_unix {
        lines.push(format!(
            "インストール・更新から {} 秒経過。その影響期間（+{} 秒）に起きたエンジン停止の事象: {} 回（数えていません）",
            (now_unix - t).max(0),
            tsflog::INSTALL_GRACE_SECS,
            a.ignored_by_install
        ));
    }
    lines.extend(a.evidence.iter().cloned());
    let mut ng: Vec<String> = Vec::new();
    let mut warn: Vec<String> = Vec::new();
    if a.minimal_entries > 0 {
        ng.push(format!(
            "変換エンジンの起動に失敗して辞書のみの動作に切り替わった記録が {} 回あります",
            a.minimal_entries
        ));
    } else if a.launch_failures > 0 {
        warn.push(format!(
            "変換エンジンの起動失敗の記録が {} 回あります",
            a.launch_failures
        ));
    }
    if a.panicked {
        ng.push("入力方式の有効化中に例外が起きた記録があります".to_owned());
    }
    if a.configure_failed {
        ng.push("言語のオプションからの設定画面の起動に失敗した記録があります（nagi-settings.exe が見つからない）".to_owned());
    }
    if a.langbar_failed || a.add_item_failed {
        warn.push("言語バー（タスクバーの「あ / A」）の登録に失敗した記録があります".to_owned());
    }
    // 同一秒の複数 pid は 1 事象。要確認は「別の秒の事象が 3 回以上、かつ最新が直近 24 時間以内」のみ
    let stopped_recent = a
        .stopped_last
        .as_deref()
        .and_then(tsflog::parse_utc)
        .is_none_or(|t| now_unix - t <= 24 * 3600);
    if a.stopped >= 3 && stopped_recent {
        warn.push(format!(
            "エンジンが止まった事象が {} 回あります（最新 {}）",
            a.stopped,
            a.stopped_last.as_deref().unwrap_or("-")
        ));
    } else if a.stopped > 0 {
        lines.push(format!(
            "エンジン停止の事象: {} 回（最新 {}。更新・差し替えで全アプリのエンジンが一斉に止まる正常な動作を含む）",
            a.stopped,
            a.stopped_last.as_deref().unwrap_or("-")
        ));
    }
    if a.dropped_non_json {
        warn.push("エンジンの出力に JSON 以外の行が混ざった記録があります".to_owned());
    }
    let fresh = install_unix.is_some_and(|t| (0..=FRESH_INSTALL_SECS).contains(&(now_unix - t)));
    let fresh_note = if fresh {
        " 更新直後は一時的に出ることがあります。Nagi を選び直して数文字入力してから、もう一度診断してください。"
    } else {
        ""
    };
    if !ng.is_empty() || !warn.is_empty() {
        match engine_log {
            Some([]) => lines.push("imserver ログのエラー行: 該当なし".to_owned()),
            Some(l) => {
                lines.push("imserver ログのエラー行（末尾）:".to_owned());
                lines.extend(l.iter().cloned());
            }
            None => {}
        }
    }
    if !ng.is_empty() {
        return ItemResult::ng(
            id,
            ng.into_iter().chain(warn).collect::<Vec<_>>().join(" / "),
            format!("imserver の起動項目（engine.start）と VC++ ランタイム・辞書の項目を確認してください。%LOCALAPPDATA%\\Nagi\\logs の imserver-*.log に理由があります。{fresh_note}"),
        )
        .with_evidence(lines);
    }
    if !warn.is_empty() {
        return ItemResult::warn(
            id,
            warn.join(" / "),
            format!("再現するときは、Nagi を選び直して数文字入力し、もう一度診断してください。続く場合は tsf.log の該当行を報告してください。{fresh_note}"),
        )
        .with_evidence(lines);
    }
    ItemResult::ok(id, "起動失敗・登録失敗の記録はありません").with_evidence(lines)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mtime_unix(path: &std::path::Path) -> Option<i64> {
    let m = std::fs::metadata(path).ok()?.modified().ok()?;
    let d = m.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_secs() as i64)
}

/// インストール・更新の時刻（VERSION・TSF DLL の更新時刻の新しい方。取れなければ `None`）
fn install_unix(ctx: &Ctx) -> Option<i64> {
    ["VERSION", "nagi_windows_tsf.dll"]
        .iter()
        .filter_map(|n| mtime_unix(&ctx.install_dir.join(n)))
        .max()
}

/// imserver-*.log（最新 2 ファイル）の末尾から、エラーらしい行を最大 10 行（パスはユーザー名を置換）
fn read_engine_log_evidence(ctx: &Ctx) -> Option<Vec<String>> {
    let dir = ctx.log_dir.as_ref()?;
    let mut files: Vec<(i64, std::path::PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            n.starts_with("imserver-") && n.ends_with(".log")
        })
        .filter_map(|e| Some((mtime_unix(&e.path())?, e.path())))
        .collect();
    files.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    let mut text = String::new();
    // 古い方から連結して、末尾側（最新）を残す
    for (_, p) in files.iter().take(2).rev() {
        if let Ok(b) = std::fs::read(p) {
            let start = b.len().saturating_sub(64 * 1024);
            text += &String::from_utf8_lossy(&b[start..]);
            text.push('\n');
        }
    }
    Some(
        tsflog::engine_log_evidence(&text, 10)
            .into_iter()
            .map(|l| ctx.redact(&l))
            .collect(),
    )
}

/// `tsf.log` 項目
pub fn check_tsf_log(ctx: &Ctx) -> ItemResult {
    let id = ItemId::TsfLog;
    if !ctx.is_windows() {
        return ItemResult::skipped(id, WINDOWS_ONLY);
    }
    match read_tsf_log(ctx) {
        Some(text) => {
            let install = install_unix(ctx);
            let a = tsflog::analyze_with(&text, install.map(|t| t + tsflog::INSTALL_GRACE_SECS));
            let engine_log = read_engine_log_evidence(ctx);
            judge_tsf_log_at(&a, now_unix(), install, engine_log.as_deref())
        }
        None => ItemResult::warn(
            id,
            "tsf.log がありません（Nagi をまだ一度も選んでいない）",
            "Nagi に切り替えて（Win+Space）何か入力してから、もう一度実行してください。",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::pe::Machine;
    use crate::diag::sys::{ComProbe, SigInfo, VcFile};

    fn facts_with_arch(os: &str) -> WinFacts {
        WinFacts {
            os_arch: os.to_owned(),
            ..WinFacts::default()
        }
    }

    #[test]
    fn os_arch_prefers_runtime_information_then_env() {
        assert_eq!(os_arch(&facts_with_arch("Arm64")), Arch::Arm64);
        assert_eq!(os_arch(&facts_with_arch("X64")), Arch::X64);
        let f = WinFacts {
            env_arch: "AMD64".into(),
            env_arch_w6432: "ARM64".into(),
            ..WinFacts::default()
        };
        assert_eq!(os_arch(&f), Arch::Arm64);
        let f = WinFacts {
            env_arch: "AMD64".into(),
            ..WinFacts::default()
        };
        assert_eq!(os_arch(&f), Arch::X64);
        assert_eq!(os_arch(&WinFacts::default()), Arch::Unknown);
    }

    fn arch_input(os: Arch, machine: Machine, arch_file: Option<&str>) -> ArchInput<'static> {
        ArchInput {
            os,
            tool: "x86_64",
            files: vec![
                ("nagi_windows_tsf.dll".into(), Ok(machine)),
                ("imserver.exe".into(), Ok(machine)),
            ],
            arch_file: arch_file.map(str::to_owned),
        }
    }

    #[test]
    fn arch_matching_distribution_is_ok() {
        let r = judge_arch(&arch_input(Arch::X64, Machine::X64, Some("x86_64\n")));
        assert_eq!(r.status, Status::Ok, "{r:?}");
        let mut input = arch_input(Arch::Arm64, Machine::Arm64, Some("aarch64"));
        input.tool = "aarch64";
        let r = judge_arch(&input);
        assert_eq!(r.status, Status::Ok, "{r:?}");
        assert!(r.detail.contains("エミュレーション"));
    }

    #[test]
    fn x64_files_on_arm64_machine_is_ng_with_distribution_advice() {
        let r = judge_arch(&arch_input(Arch::Arm64, Machine::X64, Some("x86_64")));
        assert_eq!(r.status, Status::Ng);
        assert!(r.advice.unwrap().contains("arm64"));
        assert!(r.detail.contains("ARCH ファイル"));
    }

    #[test]
    fn arm64_files_on_x64_machine_is_ng() {
        let r = judge_arch(&arch_input(Arch::X64, Machine::Arm64, None));
        assert_eq!(r.status, Status::Ng);
    }

    #[test]
    fn emulated_tool_on_arm64_is_warn_and_unknown_os_is_warn() {
        let r = judge_arch(&arch_input(Arch::Arm64, Machine::Arm64, None));
        assert_eq!(
            r.status,
            Status::Warn,
            "x64 の診断プログラムを ARM64 機で動かしている"
        );
        let r = judge_arch(&arch_input(Arch::Unknown, Machine::X64, None));
        assert_eq!(r.status, Status::Warn);
    }

    #[test]
    fn user_judgement() {
        let ok = |l: &'static str| WriteTarget {
            label: l,
            shown: "p".into(),
            result: Ok(()),
        };
        let r = judge_user("standard", &[ok("a"), ok("b")], Some(false));
        assert_eq!(r.status, Status::Ok);
        assert!(r.detail.contains("標準ユーザー"));
        let bad = WriteTarget {
            label: "設定・学習の保存先",
            shown: "p".into(),
            result: Err("書き込めません（PermissionDenied）".into()),
        };
        let r = judge_user("standard", &[ok("a"), bad], Some(false));
        assert_eq!(r.status, Status::Ng);
        assert!(r.detail.contains("設定・学習の保存先"));
        assert_eq!(
            judge_user("elevated_admin", &[ok("a")], None).status,
            Status::Warn
        );
        assert_eq!(
            judge_user("standard", &[ok("a")], Some(true)).status,
            Status::Warn
        );
        assert_eq!(
            judge_user("admin_not_elevated", &[ok("a")], Some(true)).status,
            Status::Ok
        );
    }

    #[test]
    fn write_probe_creates_dir_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("nagi-diag-wp-{}", std::process::id()));
        let sub = dir.join("a").join("b");
        assert!(write_probe(&sub).is_ok());
        assert_eq!(std::fs::read_dir(&sub).unwrap().count(), 0);
        // ファイルをディレクトリの代わりに置くと失敗する
        let file = dir.join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(write_probe(&file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn vc(files: &[(&str, bool)]) -> WinFacts {
        WinFacts {
            os_arch: "X64".into(),
            vc_files: files
                .iter()
                .map(|(n, e)| VcFile {
                    name: (*n).into(),
                    exists: *e,
                    version: "14.38".into(),
                })
                .collect(),
            ..WinFacts::default()
        }
    }

    #[test]
    fn vcruntime_judgement() {
        let r = judge_vcruntime(&vc(&[("vcruntime140.dll", true), ("msvcp140.dll", true)]));
        assert_eq!(r.status, Status::Ok);
        let r = judge_vcruntime(&vc(&[("vcruntime140.dll", true), ("msvcp140.dll", false)]));
        assert_eq!(r.status, Status::Ng);
        assert!(r.advice.unwrap().contains("vc_redist.x64.exe"));
        let mut f = vc(&[("vcruntime140.dll", false)]);
        f.os_arch = "Arm64".into();
        assert!(judge_vcruntime(&f).advice.unwrap().contains("arm64"));
        assert_eq!(judge_vcruntime(&WinFacts::default()).status, Status::Warn);
    }

    #[test]
    fn display_is_recorded_with_scale_percent() {
        let f = WinFacts {
            dpi: Some(144),
            apps_light: Some(false),
            high_contrast: Some(false),
            monitors: Some(2),
            ..WinFacts::default()
        };
        let r = judge_display(&f);
        assert_eq!(r.status, Status::Ok);
        assert!(r.detail.contains("150%"));
        assert!(r.detail.contains("ダーク"));
        assert_eq!(judge_display(&WinFacts::default()).status, Status::Warn);
    }

    #[test]
    fn old_files_judgement() {
        assert_eq!(judge_old_files(&[], false, &[]).status, Status::Ok);
        // imserver.exe.old だけでプロセスが無ければ OK
        let r = judge_old_files(&["imserver.exe.old".into()], false, &[]);
        assert_eq!(r.status, Status::Ok);
        assert!(r.detail.contains("次回"));
        // DLL の .old だけ（実レポート）は OK
        let dll: Vec<String> = (0..5)
            .map(|i| format!("nagi_windows_tsf.dll.20261121T0{i}0000.old"))
            .collect();
        let r = judge_old_files(&dll, false, &[]);
        assert_eq!(r.status, Status::Ok);
        assert!(r.detail.contains('5'));
        // 多すぎる
        let many: Vec<String> = (0..21)
            .map(|i| format!("nagi_windows_tsf.dll.{i}.old"))
            .collect();
        assert_eq!(judge_old_files(&many, false, &[]).status, Status::Warn);
        assert_eq!(judge_old_files(&[], true, &[]).status, Status::Warn);
        assert_eq!(
            judge_old_files(&[], false, &["PID 1 (x.old)".into()]).status,
            Status::Warn
        );
    }

    fn sig(file: &str, status: &str) -> SigInfo {
        SigInfo {
            file: file.into(),
            status: status.into(),
            subject: String::new(),
            timestamped: false,
        }
    }

    #[test]
    fn signature_judgement() {
        let f = |s: Vec<SigInfo>| WinFacts {
            signatures: s,
            ..WinFacts::default()
        };
        assert_eq!(judge_signature(&WinFacts::default()).status, Status::Warn);
        let r = judge_signature(&f(vec![
            sig("a.dll", "NotSigned"),
            sig("b.exe", "NotSigned"),
        ]));
        assert_eq!(r.status, Status::Ok);
        assert!(r.detail.contains("署名なし"));
        assert_eq!(
            judge_signature(&f(vec![sig("a", "Valid")])).status,
            Status::Ok
        );
        assert_eq!(
            judge_signature(&f(vec![sig("a", "Valid"), sig("b", "NotSigned")])).status,
            Status::Warn
        );
        assert_eq!(
            judge_signature(&f(vec![sig("a", "HashMismatch")])).status,
            Status::Ng
        );
    }

    fn registered() -> WinFacts {
        WinFacts {
            clsid_inproc: "C:\\Program Files\\Nagi\\nagi_windows_tsf.dll".into(),
            clsid_threading: "Apartment".into(),
            tip_profile_exists: true,
            tip_profile_desc: "Nagi".into(),
            tip_categories: REQUIRED_CATEGORIES
                .iter()
                .map(|(_, g)| (*g).to_lowercase())
                .collect(),
            ..WinFacts::default()
        }
    }

    const DLL: &str = "c:/program files/nagi/NAGI_WINDOWS_TSF.dll";

    #[test]
    fn registration_ok_when_everything_is_registered() {
        let r = judge_registration(&registered(), DLL, &|_| true);
        assert_eq!(r.status, Status::Ok, "{r:?}");
    }

    #[test]
    fn registration_failures() {
        let r = judge_registration(&WinFacts::default(), DLL, &|_| true);
        assert_eq!(r.status, Status::Ng);
        assert!(r.advice.unwrap().contains("regsvr32"));

        let mut f = registered();
        f.tip_categories.pop();
        let r = judge_registration(&f, DLL, &|_| true);
        assert_eq!(r.status, Status::Ng);
        assert!(r.detail.contains("DISPLAYATTRIBUTEPROVIDER"));

        let mut f = registered();
        f.tip_profile_exists = false;
        assert_eq!(judge_registration(&f, DLL, &|_| true).status, Status::Ng);

        let mut f = registered();
        f.clsid_threading = "Both".into();
        assert_eq!(judge_registration(&f, DLL, &|_| true).status, Status::Ng);

        assert_eq!(
            judge_registration(&registered(), DLL, &|_| false).status,
            Status::Ng
        );

        let r = judge_registration(&registered(), "D:\\other\\nagi_windows_tsf.dll", &|_| true);
        assert_eq!(r.status, Status::Warn);
    }

    #[test]
    fn user_profile_judgement() {
        let tip = format!("0411:{CLSID}{PROFILE_GUID}");
        let f = WinFacts {
            user_tips: vec!["0411:{AAAA}{BBBB}".into(), tip.to_lowercase()],
            ..WinFacts::default()
        };
        assert_eq!(judge_user_profile(&f).status, Status::Ok);
        let f = WinFacts {
            user_tips: vec!["0411:{AAAA}{BBBB}".into()],
            ..WinFacts::default()
        };
        let r = judge_user_profile(&f);
        assert_eq!(r.status, Status::Warn);
        assert!(r.advice.unwrap().contains("キーボードを追加"));
        assert_eq!(
            judge_user_profile(&WinFacts::default()).status,
            Status::Warn
        );
    }

    #[test]
    fn interfaces_judgement() {
        let good = ComProbe {
            created: true,
            text_input_processor: true,
            text_input_processor_ex: true,
            fn_configure: true,
            display_attribute_provider: true,
            error: String::new(),
        };
        assert_eq!(judge_interfaces(&good).status, Status::Ok);
        let r = judge_interfaces(&ComProbe {
            fn_configure: false,
            ..good.clone()
        });
        assert_eq!(r.status, Status::Ng);
        assert!(r.detail.contains("ITfFnConfigure"));
        let r = judge_interfaces(&ComProbe {
            created: false,
            error: "クラスが登録されていません".into(),
            ..ComProbe::default()
        });
        assert_eq!(r.status, Status::Ng);
        assert!(r.detail.contains("クラスが登録されていません"));
    }

    #[test]
    fn tsf_log_stopped_events() {
        let now = tsflog::parse_utc("2026-11-21T12:00:00Z").unwrap();
        let stop = |ts: &str, pid: u32| {
            format!("{ts} pid={pid} [engine] stopped (crash or start failure)\n")
        };
        // 実レポート相当: 同一秒に複数 pid → 1 事象で OK
        let mut text = String::new();
        for pid in [100, 200, 300, 400] {
            text += &stop("2026-11-21T11:00:00Z", pid);
        }
        let r = judge_tsf_log(&tsflog::analyze(&text), now);
        assert_eq!(r.status, Status::Ok, "{r:?}");
        // 別の秒が 3 回 + 直近 → 要確認
        let text = stop("2026-11-21T09:00:00Z", 1)
            + &stop("2026-11-21T10:00:00Z", 1)
            + &stop("2026-11-21T11:00:00Z", 1);
        assert_eq!(
            judge_tsf_log(&tsflog::analyze(&text), now).status,
            Status::Warn
        );
        // 3 回でも最新が 24 時間より前 → OK
        assert_eq!(
            judge_tsf_log(&tsflog::analyze(&text), now + 3 * 86400).status,
            Status::Ok
        );
    }

    #[test]
    fn tsf_log_real_report_and_install_time() {
        let text = "\
2026-11-21T10:34:18Z pid=37280 [engine] stopped (crash or start failure) attempt=20 retry_in_ms=10000
2026-11-21T10:34:21Z pid=2988 [engine] stopped (crash or start failure) attempt=1 retry_in_ms=200
2026-11-21T10:34:22Z pid=4100 [engine] stopped (crash or start failure) attempt=3 retry_in_ms=800
2026-11-21T10:34:24Z pid=28992 [engine] stopped (crash or start failure) attempt=5 retry_in_ms=3200
2026-11-21T10:34:24Z pid=28992 [engine] entering minimal profile after restarts=5
2026-11-21T11:13:48Z pid=11484 [engine] stopped (crash or start failure) attempt=1 retry_in_ms=200
";
        let now = tsflog::parse_utc("2026-11-21T11:17:00Z").unwrap();
        // 従来: NG
        let r = judge_tsf_log(&tsflog::analyze(text), now);
        assert_eq!(r.status, Status::Ng);
        // インストール時刻が 11:13:40 → 全事象が影響期間内 → OK、件数は根拠に出る
        let inst = tsflog::parse_utc("2026-11-21T11:13:40Z").unwrap();
        let a = tsflog::analyze_with(text, Some(inst + tsflog::INSTALL_GRACE_SECS));
        let r = judge_tsf_log_at(&a, now, Some(inst), Some(&[]));
        assert_eq!(r.status, Status::Ok, "{r:?}");
        assert!(r.evidence.iter().any(|l| l.contains("事象: 5 回")), "{r:?}");
        // インストールが 10:30 なら 10:34 以降の事象は数える（NG・更新直後ではない）
        let inst = tsflog::parse_utc("2026-11-21T10:30:00Z").unwrap();
        let a = tsflog::analyze_with(text, Some(inst + tsflog::INSTALL_GRACE_SECS));
        let log = vec!["thread panicked".to_owned()];
        let r = judge_tsf_log_at(&a, now, Some(inst), Some(&log));
        assert_eq!(r.status, Status::Ng);
        assert!(!r.advice.as_deref().unwrap().contains("更新直後"));
        assert!(r.evidence.iter().any(|l| l == "thread panicked"));
        // 直前（5 分前）のインストールで NG が残る場合は案内が付く
        let inst = now - 300;
        let r = judge_tsf_log_at(&tsflog::analyze(text), now, Some(inst), Some(&[]));
        assert!(
            r.advice
                .as_deref()
                .unwrap()
                .contains("更新直後は一時的に出ることがあります")
        );
        assert!(r.evidence.iter().any(|l| l.contains("該当なし")));
    }

    #[test]
    fn tsf_log_judgement() {
        let r = judge_tsf_log(
            &tsflog::analyze("t pid=1 [tsf] Activate done langbar=true"),
            0,
        );
        assert_eq!(r.status, Status::Ok);
        let r = judge_tsf_log(
            &tsflog::analyze("t pid=1 [engine] entering minimal profile"),
            0,
        );
        assert_eq!(r.status, Status::Ng);
        let r = judge_tsf_log(
            &tsflog::analyze("t pid=1 [tsf] Activate done langbar=false"),
            0,
        );
        assert_eq!(r.status, Status::Warn);
        let r = judge_tsf_log(
            &tsflog::analyze("t pid=1 [menu] configure failed (no settings exe)"),
            0,
        );
        assert_eq!(r.status, Status::Ng);
    }
}
