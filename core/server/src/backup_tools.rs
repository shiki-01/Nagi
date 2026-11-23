//! 設定・キー割当・辞書・学習の一括エクスポート/インポート（`export_backup` / `preview_backup` /
//! `apply_backup` / `list_backups` / `restore_backup` の実体）。設定画面の「バックアップと移行」向け。
//!
//! # バックアップの形
//!
//! 1 つの JSON ファイル（[`nagi_protocol::backup::BackupFile`]）。項目は 4 つで、ファイルの中身を
//! そのままテキストで持つ（UTF-8。圧縮・バイナリなし）。
//!
//! | 項目 | ファイル | 適用の意味（`apply_backup`） |
//! | --- | --- | --- |
//! | `settings` | `config.toml`（`[keymap.*]` を含む） | 置き換え（検証に通らなければ適用しない） |
//! | `user_dict` | `user_dict/*.txt|*.tsv` | 統合（既存を優先。新しい語だけ `imported.tsv` に追記） |
//! | `suppressed` | `suppressed.tsv` | 統合（和集合） |
//! | `learning` | `learning.tsv`・`learning-next.tsv` | 置き換え |
//!
//! `restore_backup`（自動バックアップからの復元）は、元の状態に**正確に**戻すため、どの項目も置き換えで
//! 適用する（辞書は、バックアップに無いファイルを消す）。
//!
//! # 全か無か
//!
//! 適用の直前に、適用する項目の現在の状態を `<user-dir>/backups/nagi-backup-<日時>.json` に保存し
//! （最新 [`KEEP_GENERATIONS`] 世代を残す）、メモリ上にも生のバイト列で退避する。各ファイルは一時ファイルへ
//! 書いてから `rename` する（原子的）。途中で 1 つでも失敗したら、退避から全項目を元に戻す。
//!
//! # プライバシー・安全
//!
//! バックアップには入力内容（辞書の読み・表記、学習の入力履歴）が含まれる。このモジュールはログを出さず、
//! エラーメッセージにも内容を入れない（件数・行番号・理由だけ）。バックアップ内のファイル名は保存先の決定に
//! 使わない（辞書は固定のファイルへ追記するか、復元では名前を検証してから使う）。

use crate::runtime::Runtime;
use crate::suppress::Suppressions;
use crate::userdict_tools::{self as tools, ToolError};
use nagi_dict::{ImportError, ImportFormat, UserDictionary, decode_text, import_user_dict};
use nagi_learn::{LearnConfig, LearnError, Learner, NextRow, NextWordLearner, Row};
use nagi_protocol::backup::{
    ApplyBackupResult, BackupApplied, BackupEntry, BackupFile, BackupIssue, BackupItem,
    BackupItemPreview, BackupListItem, BackupSettingDiff, ExportBackupParams, ExportBackupResult,
    ListBackupsResult, PreviewBackupResult,
};
use nagi_protocol::methods::UserDictFormat;
use nagi_protocol::userdict::{MergeUserDictParams, UserDictDuplicate};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// バックアップファイルのスキーマの版
pub const SCHEMA_VERSION: u32 = 1;
/// 自動バックアップを残す世代数
pub const KEEP_GENERATIONS: usize = 5;
/// 自動バックアップの置き場（`<user-dir>` 直下）
pub const BACKUP_DIR: &str = "backups";
/// 設定の差分の最大件数（応答の大きさを抑える）
const MAX_DIFFS: usize = 100;
/// 差分に出す値の最大文字数
const MAX_DIFF_VALUE_CHARS: usize = 120;
/// 学習データ（`learning.tsv`）のファイル名
const LEARNING_FILE: &str = "learning.tsv";
/// 次語学習のファイル名
const LEARNING_NEXT_FILE: &str = "learning-next.tsv";
/// 設定のファイル名（バックアップ内の表示名）
const CONFIG_NAME: &str = "config.toml";
/// 抑制語のファイル名（バックアップ内の表示名）
const SUPPRESSED_NAME: &str = "suppressed.tsv";
/// 辞書ファイルの一時ファイルの拡張子（`userdict_tools` と同じ）
const TMP_SUFFIX: &str = ".nagi-tmp";

/// 項目の並び順（表示・適用の順）
pub const ALL_ITEMS: [BackupItem; 4] = [
    BackupItem::Settings,
    BackupItem::UserDict,
    BackupItem::Suppressed,
    BackupItem::Learning,
];

type Result<T> = std::result::Result<T, ToolError>;

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::Invalid(message.into())
}

fn io(message: impl Into<String>) -> ToolError {
    ToolError::Io(message.into())
}

/// このモジュールが扱う RPC メソッド名か（`Server` の振り分け用）
pub fn is_backup_method(method: &str) -> bool {
    use nagi_protocol::backup::names;
    [
        names::EXPORT_BACKUP,
        names::PREVIEW_BACKUP,
        names::APPLY_BACKUP,
        names::LIST_BACKUPS,
        names::RESTORE_BACKUP,
    ]
    .contains(&method)
}

/// 項目の識別子（バックアップ内の `kind`）
pub fn item_name(item: BackupItem) -> &'static str {
    match item {
        BackupItem::Settings => "settings",
        BackupItem::UserDict => "user_dict",
        BackupItem::Suppressed => "suppressed",
        BackupItem::Learning => "learning",
    }
}

fn item_from(name: &str) -> Option<BackupItem> {
    ALL_ITEMS.into_iter().find(|i| item_name(*i) == name)
}

fn item_label(item: BackupItem) -> &'static str {
    match item {
        BackupItem::Settings => "設定",
        BackupItem::UserDict => "ユーザー辞書",
        BackupItem::Suppressed => "抑制語",
        BackupItem::Learning => "学習データ",
    }
}

/// 重複を除き、固定の順に並べる
fn normalize_items(items: &[BackupItem]) -> Vec<BackupItem> {
    ALL_ITEMS
        .into_iter()
        .filter(|i| items.contains(i))
        .collect()
}

/// 操作の環境（保存先・実行中のサーバーの状態）。サーバーが組み立てて渡す
pub struct Env<'a> {
    /// `config.toml`
    pub config_path: Option<&'a Path>,
    /// ユーザー辞書のフォルダ
    pub user_dict_dir: Option<&'a Path>,
    /// 抑制語のファイル
    pub suppressed_path: Option<&'a Path>,
    /// ユーザーデータのフォルダ（学習ファイルと `backups/` の置き場）
    pub user_dir: Option<&'a Path>,
    /// 学習データを持つ実行時の状態（無ければ学習は扱えない）
    pub runtime: Option<&'a Runtime>,
    /// `userdict.max_entries`
    pub userdict_max_entries: usize,
    /// この Nagi の版
    pub app_version: &'a str,
    /// この OS
    pub platform: &'a str,
    /// 現在時刻（unix 秒）
    pub now_unix: u64,
}

impl Env<'_> {
    fn backups_dir(&self) -> Option<PathBuf> {
        self.user_dir.map(|d| d.join(BACKUP_DIR))
    }
}

// ---- 時刻 ----

/// unix 秒 → (年, 月, 日, 時, 分, 秒)（UTC）
fn civil(unix: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (unix / 86_400) as i64;
    let secs = (unix % 86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day, secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// `2026-11-22T10:20:30Z`
pub fn iso8601(unix: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// `20261122-102030`
fn file_stamp(unix: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// 自動バックアップのファイル名か（`nagi-backup-YYYYMMDD-HHMMSS.json`）。保存先の決定に使うので厳密に見る
pub fn is_backup_file_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("nagi-backup-") else {
        return false;
    };
    let Some(stamp) = rest.strip_suffix(".json") else {
        return false;
    };
    let bytes = stamp.as_bytes();
    bytes.len() == 15
        && bytes[8] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 8 || b.is_ascii_digit())
}

// ---- ファイルの読み書き ----

fn read_opt(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(io("ファイルを読み込めません")),
    }
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_start_matches('\u{feff}')
        .to_string()
}

/// 一時ファイルへ書いて `rename` する（原子的）
fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(TMP_SUFFIX);
    let tmp = PathBuf::from(tmp);
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn is_dict_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("tsv"))
}

/// 辞書フォルダの `*.txt` / `*.tsv`（名前順。生のバイト列）
fn read_dict_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| is_dict_file(p))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let bytes = std::fs::read(&path).map_err(|_| io("辞書ファイルを読み込めません"))?;
        out.push((name.to_string(), bytes));
    }
    Ok(out)
}

/// 辞書フォルダを `files` の内容にそろえる（無いファイルは消す）
fn replace_dict_files(dir: &Path, files: &[(String, Vec<u8>)]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, bytes) in files {
        write_bytes_atomic(&dir.join(name), bytes)?;
    }
    if let Ok(read) = std::fs::read_dir(dir) {
        for path in read.filter_map(std::result::Result::ok).map(|e| e.path()) {
            let keep = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| files.iter().any(|(f, _)| f == n));
            if is_dict_file(&path) && !keep {
                std::fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

/// 辞書ファイル名として安全か（復元で保存先に使うので厳密に見る）
fn safe_dict_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && name.chars().count() <= 100
        && !name.starts_with('.')
        && (lower.ends_with(".txt") || lower.ends_with(".tsv"))
        && !name.ends_with(TMP_SUFFIX)
        && name.chars().all(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}

// ---- 設定（config.toml）----

fn looks_like_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    value.contains('/')
        || value.contains('\\')
        || lower.ends_with(".gguf")
        || value.as_bytes().get(1) == Some(&b':')
}

/// 別の環境で使えない設定（パスを持つもの）を取り除く。取り除いたら注意の文を返す
fn strip_machine_paths(text: &str) -> (String, Vec<String>) {
    let Ok(mut doc) = text
        .trim_start_matches('\u{feff}')
        .parse::<toml_edit::DocumentMut>()
    else {
        return (
            text.to_string(),
            vec!["config.toml に構文の誤りがあります（そのまま書き出しました）".to_string()],
        );
    };
    let mut warnings = Vec::new();
    if let Some(table) = doc.get_mut("conversion").and_then(|t| t.as_table_mut())
        && table
            .get("model")
            .and_then(|v| v.as_str())
            .is_some_and(looks_like_path)
    {
        table.remove("model");
        warnings.push(
            "conversion.model にこの PC のパスが指定されていたため、書き出しから除きました\
             （別の PC では使えないため。移行先で選び直してください）"
                .to_string(),
        );
    }
    (doc.to_string(), warnings)
}

/// 設定の値を 1 キーずつ平らにする（`conversion.live_conversion` → `false`）。値は空白を詰めて文字列にする
fn flatten(text: &str) -> Option<BTreeMap<String, String>> {
    let doc: toml_edit::DocumentMut = text.trim_start_matches('\u{feff}').parse().ok()?;
    let mut out = BTreeMap::new();
    walk(doc.as_item(), "", &mut out);
    Some(out)
}

fn walk(item: &toml_edit::Item, prefix: &str, out: &mut BTreeMap<String, String>) {
    match item {
        toml_edit::Item::Table(table) => {
            for (key, child) in table.iter() {
                let path = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                walk(child, &path, out);
            }
        }
        toml_edit::Item::Value(value) => {
            let mut value = value.clone();
            match &mut value {
                toml_edit::Value::Array(a) => a.fmt(),
                toml_edit::Value::InlineTable(t) => t.fmt(),
                _ => {}
            }
            value.decor_mut().clear();
            let rendered = value
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            out.insert(prefix.to_string(), rendered);
        }
        toml_edit::Item::ArrayOfTables(_) => {
            let rendered = item
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            out.insert(prefix.to_string(), rendered);
        }
        toml_edit::Item::None => {}
    }
}

fn shorten(value: &str) -> String {
    if value.chars().count() > MAX_DIFF_VALUE_CHARS {
        let head: String = value.chars().take(MAX_DIFF_VALUE_CHARS).collect();
        format!("{head}…")
    } else {
        value.to_string()
    }
}

fn setting_labels() -> BTreeMap<&'static str, &'static str> {
    nagi_config::settings::definitions()
        .into_iter()
        .map(|d| (d.key, d.label))
        .collect()
}

// ---- バックアップの読み取り ----

fn present(backup: &BackupFile, item: BackupItem) -> bool {
    let name = item_name(item);
    backup.included.iter().any(|k| k == name) || backup.entries.iter().any(|e| e.kind == name)
}

fn entries_of(backup: &BackupFile, item: BackupItem) -> impl Iterator<Item = &BackupEntry> {
    let name = item_name(item);
    backup.entries.iter().filter(move |e| e.kind == name)
}

/// 版の検査。使えない版はエラー、新しい版は注意
fn check_version(backup: &BackupFile, warnings: &mut Vec<String>) -> Result<()> {
    if backup.version == 0 {
        return Err(invalid("Nagi のバックアップではありません（版が不正です）"));
    }
    if backup.version > SCHEMA_VERSION {
        warnings.push(format!(
            "新しい版（スキーマ {}）のバックアップです。この版で扱える項目だけを読み込み、それ以外は無視します",
            backup.version
        ));
    }
    Ok(())
}

fn unknown_kind_warning(backup: &BackupFile) -> Option<String> {
    let unknown = backup
        .entries
        .iter()
        .filter(|e| item_from(&e.kind).is_none())
        .count();
    (unknown > 0)
        .then(|| format!("この版では扱えない項目が {unknown} 件含まれています（無視します）"))
}

// ---- 現在の状態 ----

/// 適用前の状態（生のバイト列。復元に使う）
#[derive(Default)]
struct Snapshot {
    items: Vec<BackupItem>,
    config: Option<Vec<u8>>,
    dict: Vec<(String, Vec<u8>)>,
    suppressed: Option<Vec<u8>>,
    learning_text: Option<Vec<u8>>,
    learning_next_text: Option<Vec<u8>>,
    learning_rows: Option<(Vec<Row>, Vec<NextRow>)>,
}

fn capture(env: &Env, items: &[BackupItem]) -> Result<Snapshot> {
    let mut snap = Snapshot {
        items: items.to_vec(),
        ..Snapshot::default()
    };
    for item in items {
        match item {
            BackupItem::Settings => {
                let path = env
                    .config_path
                    .ok_or_else(|| io("設定ファイルの場所が決まっていません"))?;
                snap.config = read_opt(path)?;
            }
            BackupItem::UserDict => {
                let dir = env
                    .user_dict_dir
                    .ok_or_else(|| io("ユーザー辞書の保存先がありません"))?;
                snap.dict = read_dict_files(dir)?;
            }
            BackupItem::Suppressed => {
                let path = env
                    .suppressed_path
                    .ok_or_else(|| io("抑制語の保存先がありません"))?;
                snap.suppressed = read_opt(path)?;
            }
            BackupItem::Learning => {
                let (Some(runtime), Some(dir)) = (env.runtime, env.user_dir) else {
                    return Err(io("学習データの保存先がありません"));
                };
                runtime.flush_learning(true);
                snap.learning_text = read_opt(&dir.join(LEARNING_FILE))?;
                snap.learning_next_text = read_opt(&dir.join(LEARNING_NEXT_FILE))?;
                snap.learning_rows = Some(runtime.learn_snapshot());
            }
        }
    }
    Ok(snap)
}

/// 状態からバックアップを作る（`sanitize` なら別の環境で使えない設定を除く）
fn backup_from(env: &Env, snap: &Snapshot, sanitize: bool) -> (BackupFile, Vec<String>) {
    let mut warnings = Vec::new();
    let mut entries = Vec::new();
    for item in &snap.items {
        match item {
            BackupItem::Settings => {
                if let Some(bytes) = &snap.config {
                    let mut text = text_of(bytes);
                    if sanitize {
                        let (stripped, w) = strip_machine_paths(&text);
                        text = stripped;
                        warnings.extend(w);
                    }
                    entries.push(entry(*item, CONFIG_NAME, text));
                }
            }
            BackupItem::UserDict => {
                for (name, bytes) in &snap.dict {
                    entries.push(entry(*item, name, decode_text(bytes)));
                }
            }
            BackupItem::Suppressed => {
                if let Some(bytes) = &snap.suppressed {
                    entries.push(entry(*item, SUPPRESSED_NAME, text_of(bytes)));
                }
            }
            BackupItem::Learning => {
                if let Some(bytes) = &snap.learning_text {
                    entries.push(entry(*item, LEARNING_FILE, text_of(bytes)));
                }
                if let Some(bytes) = &snap.learning_next_text {
                    entries.push(entry(*item, LEARNING_NEXT_FILE, text_of(bytes)));
                }
            }
        }
    }
    let backup = BackupFile {
        version: SCHEMA_VERSION,
        created_at: iso8601(env.now_unix),
        app_version: env.app_version.to_string(),
        platform: env.platform.to_string(),
        included: snap
            .items
            .iter()
            .map(|i| item_name(*i).to_string())
            .collect(),
        entries,
    };
    (backup, warnings)
}

fn entry(item: BackupItem, name: &str, text: String) -> BackupEntry {
    BackupEntry {
        kind: item_name(item).to_string(),
        name: name.to_string(),
        text,
    }
}

// ---- 書き出し ----

/// `export_backup`
pub fn export(env: &Env, params: &ExportBackupParams) -> Result<ExportBackupResult> {
    let items = normalize_items(&params.include);
    if items.is_empty() {
        return Err(invalid("書き出す項目を選んでください"));
    }
    let snap = capture(env, &items)?;
    let (backup, mut warnings) = backup_from(env, &snap, true);
    for item in &items {
        if *item == BackupItem::Learning {
            warnings.push(
                "学習データには入力した内容の履歴が含まれます。取り扱いに注意してください"
                    .to_string(),
            );
        }
    }
    Ok(ExportBackupResult {
        file_name: format!("nagi-backup-{}.json", file_stamp(env.now_unix)),
        backup,
        warnings,
    })
}

// ---- 検証・プレビュー ----

fn empty_preview(item: BackupItem, present: bool) -> BackupItemPreview {
    BackupItemPreview {
        kind: item,
        present,
        applicable: false,
        count: 0,
        current: 0,
        added: 0,
        same: 0,
        changed: 0,
        removed: 0,
        changes: Vec::new(),
        errors: Vec::new(),
        warnings: Vec::new(),
    }
}

/// 項目の検証・差分の計算の結果（適用の計画にも使う）
enum Plan {
    Settings(String),
    /// 統合: 読み取った Mozc TSV（バイト列）と、新しく加わる件数
    DictMerge {
        bytes: Vec<u8>,
        new: usize,
    },
    /// 置き換え: ファイル名と中身
    DictReplace(Vec<(String, Vec<u8>)>),
    Suppressed(Suppressions),
    Learning(Vec<Row>, Vec<NextRow>),
}

/// 項目 1 つを検証し、要約と適用の計画を作る。`mode` が `Replace` なら（復元）内容の意味の検査を省く
fn analyze(
    env: &Env,
    backup: &BackupFile,
    item: BackupItem,
    mode: Mode,
) -> (BackupItemPreview, Option<Plan>) {
    let is_present = present(backup, item);
    let mut p = empty_preview(item, is_present);
    if !is_present {
        return (p, None);
    }
    let plan = match item {
        BackupItem::Settings => analyze_settings(env, backup, mode, &mut p),
        BackupItem::UserDict => analyze_dict(env, backup, mode, &mut p),
        BackupItem::Suppressed => analyze_suppressed(env, backup, mode, &mut p),
        BackupItem::Learning => analyze_learning(env, backup, &mut p),
    };
    p.applicable = plan.is_some() && p.errors.is_empty();
    let plan = if p.applicable { plan } else { None };
    (p, plan)
}

fn first_entry<'a>(
    backup: &'a BackupFile,
    item: BackupItem,
    p: &mut BackupItemPreview,
) -> Option<&'a BackupEntry> {
    let mut it = entries_of(backup, item);
    let first = it.next();
    if it.next().is_some() {
        p.warnings
            .push("同じ項目が複数含まれています（最初の 1 つだけを使います）".to_string());
    }
    first
}

fn analyze_settings(
    env: &Env,
    backup: &BackupFile,
    mode: Mode,
    p: &mut BackupItemPreview,
) -> Option<Plan> {
    let text =
        first_entry(backup, BackupItem::Settings, p).map_or(String::new(), |e| e.text.clone());
    if env.config_path.is_none() {
        p.errors.push(BackupIssue {
            line: 0,
            message: "設定ファイルの場所が決まっていないため適用できません".to_string(),
        });
    }
    if mode == Mode::Merge {
        match nagi_config::parse(&text) {
            Ok((_, issues)) => {
                for i in issues {
                    p.warnings.push(format!("{} 行目: {}", i.line, i.message));
                }
            }
            Err(issues) => {
                for i in issues {
                    p.errors.push(BackupIssue {
                        line: i.line,
                        message: i.message,
                    });
                }
            }
        }
    }
    let current_text = env
        .config_path
        .and_then(|path| read_opt(path).ok().flatten())
        .map(|b| text_of(&b))
        .unwrap_or_default();
    let theirs = flatten(&text);
    let ours = flatten(&current_text);
    if theirs.is_none() {
        if p.errors.is_empty() {
            p.errors.push(BackupIssue {
                line: 0,
                message: "設定の構文を読み取れません".to_string(),
            });
        }
        return None;
    }
    let theirs = theirs.unwrap_or_default();
    let ours = ours.unwrap_or_else(|| {
        p.warnings
            .push("現在の config.toml に誤りがあるため、差分は空の設定との比較です".to_string());
        BTreeMap::new()
    });
    p.count = theirs.len() as u32;
    p.current = ours.len() as u32;
    let labels = setting_labels();
    let label = |key: &str| {
        if let Some(rest) = key.strip_prefix("keymap.")
            && let Some((context, k)) = rest.split_once('.')
        {
            return format!("キー割当（{context}）{}", k.trim_matches('"'));
        }
        labels
            .get(key)
            .map_or_else(|| key.to_string(), |l| (*l).to_string())
    };
    let mut diffs = Vec::new();
    for (key, after) in &theirs {
        match ours.get(key) {
            None => {
                p.added += 1;
                diffs.push(BackupSettingDiff {
                    key: key.clone(),
                    label: label(key),
                    before: None,
                    after: Some(shorten(after)),
                });
            }
            Some(before) if before != after => {
                p.changed += 1;
                diffs.push(BackupSettingDiff {
                    key: key.clone(),
                    label: label(key),
                    before: Some(shorten(before)),
                    after: Some(shorten(after)),
                });
            }
            Some(_) => p.same += 1,
        }
    }
    for (key, before) in &ours {
        if !theirs.contains_key(key) {
            p.removed += 1;
            diffs.push(BackupSettingDiff {
                key: key.clone(),
                label: label(key),
                before: Some(shorten(before)),
                after: None,
            });
        }
    }
    diffs.truncate(MAX_DIFFS);
    p.changes = diffs;
    // 他の環境で使えない設定への注意
    let other_os = backup.platform != env.platform;
    if other_os && theirs.keys().any(|k| k.starts_with("windows.")) && env.platform != "windows" {
        p.warnings.push(
            "[windows] の項目は Windows でだけ使われます（この OS では無視されます）".to_string(),
        );
    }
    if other_os && theirs.keys().any(|k| k.starts_with("apps.")) {
        p.warnings.push(
            "[apps] のアプリ名・アプリ ID は OS ごとに異なります（別の OS では一致しないことがあります）"
                .to_string(),
        );
    }
    if theirs
        .get("conversion.model")
        .is_some_and(|v| looks_like_path(v.trim_matches('"')))
    {
        p.warnings.push(
            "conversion.model にパスが指定されています（この PC に無いと変換モデルを読み込めません）"
                .to_string(),
        );
    }
    Some(Plan::Settings(text))
}

fn analyze_dict(
    env: &Env,
    backup: &BackupFile,
    mode: Mode,
    p: &mut BackupItemPreview,
) -> Option<Plan> {
    let files: Vec<&BackupEntry> = entries_of(backup, BackupItem::UserDict).collect();
    let existing = env
        .user_dict_dir
        .map(|d| UserDictionary::load_dir(d).0)
        .unwrap_or_default();
    p.current = existing.entries.len() as u32;
    if env.user_dict_dir.is_none() {
        p.errors.push(BackupIssue {
            line: 0,
            message: "ユーザー辞書の保存先がありません".to_string(),
        });
        return None;
    }
    if mode == Mode::Replace {
        let mut out = Vec::new();
        for f in &files {
            if !safe_dict_file_name(&f.name) {
                p.errors.push(BackupIssue {
                    line: 0,
                    message: "辞書ファイルの名前が不正です".to_string(),
                });
                return None;
            }
            out.push((f.name.clone(), f.text.clone().into_bytes()));
        }
        let mut dict = UserDictionary::default();
        let mut issues = Vec::new();
        for (name, bytes) in &out {
            dict.add_tsv(name, &decode_text(bytes), &mut issues);
        }
        p.count = dict.entries.len() as u32;
        return Some(Plan::DictReplace(out));
    }
    let mut bytes = Vec::new();
    for f in &files {
        bytes.extend_from_slice(f.text.as_bytes());
        if !f.text.ends_with('\n') {
            bytes.push(b'\n');
        }
    }
    if bytes.len() > tools::MAX_IMPORT_BYTES {
        p.errors.push(BackupIssue {
            line: 0,
            message: format!(
                "ユーザー辞書が大きすぎます（上限 {} MB）",
                tools::MAX_IMPORT_BYTES / (1024 * 1024)
            ),
        });
        return None;
    }
    match import_user_dict(&bytes, Some(ImportFormat::MozcTsv), &existing) {
        Ok(outcome) => {
            let dup = outcome
                .skipped
                .iter()
                .filter(|s| {
                    matches!(
                        s.reason,
                        nagi_dict::SkipReason::DuplicateExisting
                            | nagi_dict::SkipReason::DuplicateInFile
                    )
                })
                .count();
            let bad = outcome.skipped.len() - dup;
            p.count = outcome.total as u32;
            p.added = outcome.entries.len() as u32;
            p.same = dup as u32;
            if bad > 0 {
                p.warnings
                    .push(format!("読み取れない {bad} 件は取り込みません"));
            }
            if existing.entries.len() + outcome.entries.len() > env.userdict_max_entries {
                p.errors.push(BackupIssue {
                    line: 0,
                    message: format!(
                        "登録できる語数の上限（{} 語）を超えます",
                        env.userdict_max_entries
                    ),
                });
                return None;
            }
            Some(Plan::DictMerge {
                bytes,
                new: outcome.entries.len(),
            })
        }
        Err(ImportError::Empty) => Some(Plan::DictMerge {
            bytes: Vec::new(),
            new: 0,
        }),
        Err(e) => {
            p.errors.push(BackupIssue {
                line: 0,
                message: e.to_string(),
            });
            None
        }
    }
}

fn analyze_suppressed(
    env: &Env,
    backup: &BackupFile,
    mode: Mode,
    p: &mut BackupItemPreview,
) -> Option<Plan> {
    let text =
        first_entry(backup, BackupItem::Suppressed, p).map_or(String::new(), |e| e.text.clone());
    let Some(path) = env.suppressed_path else {
        p.errors.push(BackupIssue {
            line: 0,
            message: "抑制語の保存先がありません".to_string(),
        });
        return None;
    };
    let mut issues = Vec::new();
    let theirs = Suppressions::parse(SUPPRESSED_NAME, &text, &mut issues);
    for i in &issues {
        p.warnings
            .push(format!("{} 行目: {}（取り込みません）", i.line, i.message));
    }
    let ours = Suppressions::load(path).0;
    p.count = theirs.len() as u32;
    p.current = ours.len() as u32;
    let new = theirs
        .list()
        .into_iter()
        .filter(|(r, s)| !ours.contains(r, s))
        .count();
    p.added = new as u32;
    p.same = (theirs.len() - new) as u32;
    if mode == Mode::Replace {
        return Some(Plan::Suppressed(theirs));
    }
    let mut merged = ours;
    for (r, s) in theirs.list() {
        let _ = merged.add(&r, &s);
    }
    Some(Plan::Suppressed(merged))
}

/// 学習ファイルのテキストを行にする（一時ファイル経由で `Learner::load` に読ませる）
fn parse_learning(
    env: &Env,
    learning: Option<&str>,
    next: Option<&str>,
) -> std::result::Result<(Vec<Row>, Vec<NextRow>, usize), String> {
    let base = env
        .user_dir
        .map_or_else(std::env::temp_dir, Path::to_path_buf)
        .join(format!(
            ".backup-tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
    std::fs::create_dir_all(&base).map_err(|_| "一時フォルダを作れません".to_string())?;
    let result = (|| {
        let mut skipped = 0;
        let rows = match learning.filter(|t| !t.trim().is_empty()) {
            Some(text) => {
                let path = base.join(LEARNING_FILE);
                std::fs::write(&path, text).map_err(|_| "一時ファイルに書けません".to_string())?;
                let (l, report) = Learner::load(&path, LearnConfig::default())
                    .map_err(|e| learn_error_message(&e))?;
                skipped += report.skipped_lines;
                l.rows()
            }
            None => Vec::new(),
        };
        let next_rows = match next.filter(|t| !t.trim().is_empty()) {
            Some(text) => {
                let path = base.join(LEARNING_NEXT_FILE);
                std::fs::write(&path, text).map_err(|_| "一時ファイルに書けません".to_string())?;
                let (n, report) = NextWordLearner::load(&path, LearnConfig::default())
                    .map_err(|e| learn_error_message(&e))?;
                skipped += report.skipped_lines;
                n.rows()
            }
            None => Vec::new(),
        };
        Ok((rows, next_rows, skipped))
    })();
    let _ = std::fs::remove_dir_all(&base);
    result
}

fn learn_error_message(e: &LearnError) -> String {
    match e {
        LearnError::UnsupportedSchema { .. } => {
            "学習データの形式（スキーマ）に対応していません".to_string()
        }
        LearnError::Io(_) => "学習データを読み取れません".to_string(),
    }
}

fn analyze_learning(env: &Env, backup: &BackupFile, p: &mut BackupItemPreview) -> Option<Plan> {
    if env.runtime.is_none() || env.user_dir.is_none() {
        p.errors.push(BackupIssue {
            line: 0,
            message: "学習データの保存先がないため適用できません".to_string(),
        });
        return None;
    }
    p.current = env.runtime.map_or(0, |r| r.learned_entries() as u32);
    let find = |name: &str| {
        entries_of(backup, BackupItem::Learning)
            .find(|e| e.name == name)
            .map(|e| e.text.as_str())
    };
    match parse_learning(env, find(LEARNING_FILE), find(LEARNING_NEXT_FILE)) {
        Ok((rows, next, skipped)) => {
            p.count = rows.len() as u32;
            if skipped > 0 {
                p.warnings
                    .push(format!("読み取れない {skipped} 行は取り込みません"));
            }
            p.warnings.push(
                "適用すると、現在の学習データはバックアップの内容に置き換わります".to_string(),
            );
            Some(Plan::Learning(rows, next))
        }
        Err(message) => {
            p.errors.push(BackupIssue { line: 0, message });
            None
        }
    }
}

/// `preview_backup`（何も書かない）
pub fn preview(env: &Env, backup: &BackupFile) -> Result<PreviewBackupResult> {
    let mut warnings = Vec::new();
    check_version(backup, &mut warnings)?;
    if backup.platform != env.platform {
        warnings.push(format!(
            "別の OS（{}）で作られたバックアップです。この OS（{}）で使われない設定は無視されます",
            backup.platform, env.platform
        ));
    }
    if backup.app_version != env.app_version {
        warnings.push(format!(
            "別の版（{}）で作られたバックアップです（この版: {}）",
            backup.app_version, env.app_version
        ));
    }
    warnings.extend(unknown_kind_warning(backup));
    let items: Vec<BackupItemPreview> = ALL_ITEMS
        .into_iter()
        .map(|item| analyze(env, backup, item, Mode::Merge).0)
        .collect();
    if items.iter().all(|i| !i.present) {
        return Err(invalid("このファイルには読み込める項目がありません"));
    }
    Ok(PreviewBackupResult {
        version: backup.version,
        created_at: backup.created_at.clone(),
        app_version: backup.app_version.clone(),
        platform: backup.platform.clone(),
        same_platform: backup.platform == env.platform,
        items,
        warnings,
    })
}

// ---- 適用 ----

/// 適用の意味
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `apply_backup`: 設定・学習は置き換え、辞書・抑制語は統合
    Merge,
    /// `restore_backup`: すべて置き換え（元の状態に正確に戻す）
    Replace,
}

fn rollback(env: &Env, snap: &Snapshot) -> bool {
    let mut ok = true;
    for item in &snap.items {
        match item {
            BackupItem::Settings => {
                if let Some(path) = env.config_path {
                    ok &= restore_file(path, snap.config.as_deref());
                }
            }
            BackupItem::UserDict => {
                if let Some(dir) = env.user_dict_dir {
                    ok &= replace_dict_files(dir, &snap.dict).is_ok();
                }
            }
            BackupItem::Suppressed => {
                if let Some(path) = env.suppressed_path {
                    ok &= restore_file(path, snap.suppressed.as_deref());
                }
            }
            BackupItem::Learning => {
                if let (Some(runtime), Some((rows, next))) = (env.runtime, &snap.learning_rows) {
                    ok &= runtime.learn_replace(rows.clone(), Some(next.clone()));
                }
            }
        }
    }
    ok
}

/// `bytes` があれば書き、無ければ（元から無かった）消す
fn restore_file(path: &Path, bytes: Option<&[u8]>) -> bool {
    match bytes {
        Some(b) => write_bytes_atomic(path, b).is_ok(),
        None => match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        },
    }
}

fn commit(env: &Env, plans: &[(BackupItem, Plan)]) -> std::result::Result<Vec<BackupApplied>, ()> {
    let mut applied = Vec::new();
    for (item, plan) in plans {
        let count = match plan {
            Plan::Settings(text) => {
                let path = env.config_path.ok_or(())?;
                nagi_config::settings::write_atomic(path, text).map_err(|_| ())?;
                flatten(text).map_or(0, |m| m.len())
            }
            Plan::DictMerge { bytes, new } => {
                if *new > 0 {
                    let dir = env.user_dict_dir.ok_or(())?;
                    let params = MergeUserDictParams {
                        data_base64: String::new(),
                        format: Some(UserDictFormat::Mozc),
                        on_duplicate: Some(UserDictDuplicate::Skip),
                    };
                    let r = tools::merge(dir, bytes, &params, env.userdict_max_entries)
                        .map_err(|_| ())?;
                    r.imported as usize
                } else {
                    0
                }
            }
            Plan::DictReplace(files) => {
                let dir = env.user_dict_dir.ok_or(())?;
                replace_dict_files(dir, files).map_err(|_| ())?;
                files.len()
            }
            Plan::Suppressed(set) => {
                let path = env.suppressed_path.ok_or(())?;
                set.save(path).map_err(|_| ())?;
                set.len()
            }
            Plan::Learning(rows, next) => {
                let runtime = env.runtime.ok_or(())?;
                let n = rows.len();
                if !runtime.learn_replace(rows.clone(), Some(next.clone())) {
                    return Err(());
                }
                n
            }
        };
        applied.push(BackupApplied {
            kind: *item,
            count: count as u32,
        });
    }
    Ok(applied)
}

/// 自動バックアップを書き、古い世代を消す。ファイル名を返す
fn write_auto_backup(env: &Env, snap: &Snapshot) -> Result<String> {
    let dir = env
        .backups_dir()
        .ok_or_else(|| io("バックアップの保存先がありません"))?;
    let (backup, _) = backup_from(env, snap, false);
    let bytes = serde_json::to_vec(&backup).map_err(|_| io("バックアップを作れません"))?;
    std::fs::create_dir_all(&dir).map_err(|_| io("バックアップのフォルダを作れません"))?;
    let mut stamp = env.now_unix;
    let name = loop {
        let name = format!("nagi-backup-{}.json", file_stamp(stamp));
        if !dir.join(&name).exists() {
            break name;
        }
        stamp += 1;
    };
    write_bytes_atomic(&dir.join(&name), &bytes)
        .map_err(|_| io("適用前のバックアップを保存できないため、何も変更していません"))?;
    prune_backups(&dir);
    Ok(name)
}

/// 新しい順に [`KEEP_GENERATIONS`] 世代だけ残す
fn prune_backups(dir: &Path) {
    let mut names = backup_names(dir);
    names.sort();
    names.reverse();
    for old in names.into_iter().skip(KEEP_GENERATIONS) {
        let _ = std::fs::remove_file(dir.join(old));
    }
}

fn backup_names(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(std::result::Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_backup_file_name(n))
        .collect()
}

/// 適用する。直前に自動バックアップを作り、失敗したら全項目を元に戻す
pub fn apply(
    env: &Env,
    backup: &BackupFile,
    items: &[BackupItem],
    mode: Mode,
) -> Result<ApplyBackupResult> {
    let mut warnings = Vec::new();
    check_version(backup, &mut warnings)?;
    let items = normalize_items(items);
    if items.is_empty() {
        return Err(invalid("適用する項目を選んでください"));
    }
    let mut plans = Vec::new();
    for item in &items {
        let (p, plan) = analyze(env, backup, *item, mode);
        if !p.present {
            return Err(invalid(format!(
                "{}はこのバックアップに含まれていません",
                item_label(*item)
            )));
        }
        let Some(plan) = plan else {
            let reason = p
                .errors
                .first()
                .map(|e| {
                    if e.line > 0 {
                        format!("{} 行目: {}", e.line, e.message)
                    } else {
                        e.message.clone()
                    }
                })
                .unwrap_or_default();
            return Err(invalid(format!(
                "{}を適用できません（何も変更していません）: {reason}",
                item_label(*item)
            )));
        };
        warnings.extend(
            p.warnings
                .into_iter()
                .filter(|w| !w.starts_with("適用すると")),
        );
        plans.push((*item, plan));
    }
    let snap = capture(env, &items)?;
    let auto_backup = write_auto_backup(env, &snap)?;
    match commit(env, &plans) {
        Ok(applied) => Ok(ApplyBackupResult {
            applied,
            auto_backup,
            warnings,
        }),
        Err(()) => {
            let restored = rollback(env, &snap);
            Err(io(if restored {
                "適用に失敗したため、元に戻しました（何も変更していません）".to_string()
            } else {
                format!(
                    "適用に失敗し、元に戻すことにも失敗しました。自動バックアップ（{auto_backup}）から「元に戻す」を実行してください"
                )
            }))
        }
    }
}

// ---- 一覧・復元 ----

fn read_backup_file(path: &Path) -> Result<BackupFile> {
    let bytes = std::fs::read(path).map_err(|_| invalid("バックアップを読み込めません"))?;
    serde_json::from_slice(&bytes).map_err(|_| invalid("バックアップの形式が正しくありません"))
}

/// `list_backups`
pub fn list(env: &Env) -> ListBackupsResult {
    let dir = env.backups_dir();
    let mut backups = Vec::new();
    if let Some(dir) = &dir {
        let mut names = backup_names(dir);
        names.sort();
        names.reverse();
        for name in names {
            let path = dir.join(&name);
            let Ok(file) = read_backup_file(&path) else {
                continue;
            };
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            backups.push(BackupListItem {
                name,
                created_at: file.created_at.clone(),
                app_version: file.app_version.clone(),
                kinds: ALL_ITEMS
                    .into_iter()
                    .filter(|i| present(&file, *i))
                    .collect(),
                size: u32::try_from(size).unwrap_or(u32::MAX),
            });
        }
    }
    ListBackupsResult {
        backups,
        dir: dir.map(|d| d.to_string_lossy().into_owned()),
        keep: KEEP_GENERATIONS as u32,
    }
}

/// `restore_backup`（`name` を省略すると最新の自動バックアップ）
pub fn restore(env: &Env, name: Option<&str>) -> Result<ApplyBackupResult> {
    let dir = env
        .backups_dir()
        .ok_or_else(|| io("バックアップの保存先がありません"))?;
    let name = match name {
        Some(n) if is_backup_file_name(n) => n.to_string(),
        Some(_) => return Err(invalid("バックアップの名前が正しくありません")),
        None => {
            let mut names = backup_names(&dir);
            names.sort();
            names
                .pop()
                .ok_or_else(|| invalid("元に戻せる自動バックアップがありません"))?
        }
    };
    let file = read_backup_file(&dir.join(&name))?;
    let items: Vec<BackupItem> = ALL_ITEMS
        .into_iter()
        .filter(|i| present(&file, *i))
        .collect();
    if items.is_empty() {
        return Err(invalid("このバックアップには戻せる項目がありません"));
    }
    apply(env, &file, &items, Mode::Replace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_formats() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_795_342_830), "2026-11-22T10:20:30Z");
        assert_eq!(file_stamp(1_795_342_830), "20261122-102030");
        // うるう年の 2 月 29 日
        assert_eq!(iso8601(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn backup_file_names_are_strict() {
        assert!(is_backup_file_name("nagi-backup-20261122-102030.json"));
        for bad in [
            "nagi-backup-20261122-102030.json.bak",
            "../nagi-backup-20261122-102030.json",
            "nagi-backup-2026112-1020300.json",
            "nagi-backup-abcdefgh-102030.json",
            "config.toml",
            "nagi-backup-20261122-102030-2.json",
        ] {
            assert!(!is_backup_file_name(bad), "{bad}");
        }
    }

    #[test]
    fn dict_file_names_are_safe() {
        assert!(safe_dict_file_name("registered.tsv"));
        assert!(safe_dict_file_name("私の辞書.txt"));
        for bad in [
            "",
            "../x.tsv",
            "a/b.tsv",
            "C:x.tsv",
            ".hidden.tsv",
            "x.exe",
            "x.tsv.nagi-tmp",
        ] {
            assert!(!safe_dict_file_name(bad), "{bad}");
        }
    }

    #[test]
    fn model_paths_are_stripped_but_ids_are_kept() {
        let (out, w) = strip_machine_paths(
            "[conversion]\nmodel = \"C:\\\\models\\\\z.gguf\"\nuse_llm = true\n",
        );
        assert!(!out.contains("model"), "{out}");
        assert!(out.contains("use_llm"));
        assert_eq!(w.len(), 1);
        let (out, w) = strip_machine_paths("[conversion]\nmodel = \"zenz-v3.1-small\"\n");
        assert!(out.contains("zenz-v3.1-small"));
        assert!(w.is_empty());
    }

    #[test]
    fn flatten_ignores_comments_and_spacing() {
        let a = flatten("[a]\nx = 150 # c\ny = [ 1,   2 ]\n").unwrap();
        let b = flatten("[a]\ny=[1, 2]\nx=150\n").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.get("a.x").map(String::as_str), Some("150"));
    }
}
