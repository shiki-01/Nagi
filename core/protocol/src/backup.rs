//! 設定・キー割当・辞書・学習の一括エクスポート/インポート（バックアップ）の RPC の型
//! （設定画面の「バックアップと移行」ページ向け）。
//!
//! - `export_backup`（書き出し。含める項目を選ぶ）
//! - `preview_backup`（読み込み前の検証と、現在との差分の要約。何も書かない）
//! - `apply_backup`（適用。直前に自動バックアップを作り、全か無かで適用する）
//! - `list_backups` / `restore_backup`（自動バックアップの一覧と、そこからの復元）
//!
//! バックアップは 1 つの JSON ファイル（[`BackupFile`]）で、中身はテキストだけ（UTF-8）。
//! 既存の型は変えず、新しい型だけを足す（加算的な変更）。入力内容を含むので、ログには出さない。

use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// メソッド名の定数
pub mod names {
    /// `export_backup` メソッド
    pub const EXPORT_BACKUP: &str = "export_backup";
    /// `preview_backup` メソッド
    pub const PREVIEW_BACKUP: &str = "preview_backup";
    /// `apply_backup` メソッド
    pub const APPLY_BACKUP: &str = "apply_backup";
    /// `list_backups` メソッド
    pub const LIST_BACKUPS: &str = "list_backups";
    /// `restore_backup` メソッド
    pub const RESTORE_BACKUP: &str = "restore_backup";
}

/// バックアップに含められる項目
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupItem {
    /// 設定（`config.toml`。キー割当 `[keymap.*]` を含む）
    Settings,
    /// ユーザー辞書（`user_dict/` の `*.txt` / `*.tsv`）
    UserDict,
    /// 抑制語（`suppressed.tsv`）
    Suppressed,
    /// 学習データ（`learning.tsv`・`learning-next.tsv`。入力した内容の履歴を含む）
    Learning,
}

/// バックアップファイルの 1 つのファイル分の内容
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEntry {
    /// 項目（`settings` / `user_dict` / `suppressed` / `learning`。未知の値は無視される）
    pub kind: String,
    /// ファイル名（`config.toml`・`registered.tsv` など。保存先の決定には使わない）
    pub name: String,
    /// ファイルの中身（UTF-8 のテキスト）
    pub text: String,
}

/// バックアップファイル（`nagi-backup-<日時>.json` の中身）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupFile {
    /// スキーマの版（現在 1）
    pub version: u32,
    /// 作成日時（UTC。`2026-11-22T10:20:30Z`）
    #[serde(default)]
    pub created_at: String,
    /// 作成した Nagi の版
    #[serde(default)]
    pub app_version: String,
    /// 作成した OS（`windows` / `macos` / `linux` / `ios` など）
    #[serde(default)]
    pub platform: String,
    /// 含めた項目（中身が空の項目も示す。復元で「空にする」ために必要）。古いファイルでは省略される
    #[serde(default)]
    pub included: Vec<String>,
    /// ファイルごとの内容
    pub entries: Vec<BackupEntry>,
}

/// `export_backup` の要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportBackupParams {
    /// 含める項目
    pub include: Vec<BackupItem>,
}

/// `export_backup` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportBackupResult {
    /// バックアップの本体
    pub backup: BackupFile,
    /// 保存するときの既定のファイル名（`nagi-backup-<日時>.json`）
    pub file_name: String,
    /// 書き出しの注意（書き出しから除いた設定など）。入力内容は含まない
    pub warnings: Vec<String>,
}

/// 問題の 1 件（検証エラー）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupIssue {
    /// 行番号（1 始まり。行に結び付かないなら 0）
    pub line: u32,
    /// 内容（日本語。入力内容の語は含まない）
    pub message: String,
}

/// 設定の差分の 1 件（`config.toml` の 1 キー）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettingDiff {
    /// ドット区切りのキー（`conversion.live_conversion`）
    pub key: String,
    /// 表示名（設定画面に項目があれば。無ければキーと同じ）
    pub label: String,
    /// 現在の値（明示指定が無ければ `None`）
    pub before: Option<String>,
    /// 適用後の値（明示指定が無くなるなら `None`）
    pub after: Option<String>,
}

/// `preview_backup` の項目ごとの要約
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupItemPreview {
    /// 項目
    pub kind: BackupItem,
    /// バックアップに含まれているか
    pub present: bool,
    /// 適用できるか（`present` で、検証エラーが無い）
    pub applicable: bool,
    /// バックアップ内の件数（設定は明示指定のキー数、辞書は語数、抑制語は件数、学習は行数）
    pub count: u32,
    /// 現在の件数（同じ数え方）
    pub current: u32,
    /// 適用で増える件数（辞書・抑制語は新規。設定・学習は使わない）
    pub added: u32,
    /// すでにある件数（辞書・抑制語の重複。設定は同じ値のキー数）
    pub same: u32,
    /// 値が変わるキー数（設定のみ）
    pub changed: u32,
    /// 適用で明示指定がなくなるキー数（設定のみ。バックアップに無い現在の指定）
    pub removed: u32,
    /// 設定の差分（最大 100 件）
    pub changes: Vec<BackupSettingDiff>,
    /// 検証エラー（これがあると適用できない）
    pub errors: Vec<BackupIssue>,
    /// 注意（適用はできる）
    pub warnings: Vec<String>,
}

/// `preview_backup` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewBackupResult {
    /// スキーマの版
    pub version: u32,
    /// 作成日時
    pub created_at: String,
    /// 作成した Nagi の版
    pub app_version: String,
    /// 作成した OS
    pub platform: String,
    /// この OS と同じか
    pub same_platform: bool,
    /// 項目ごとの要約（4 項目すべて）
    pub items: Vec<BackupItemPreview>,
    /// 全体の注意（版・OS の違い、未知の項目の無視など）
    pub warnings: Vec<String>,
}

/// `preview_backup` の要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewBackupParams {
    /// バックアップの本体
    pub backup: BackupFile,
}

/// `apply_backup` の要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyBackupParams {
    /// バックアップの本体
    pub backup: BackupFile,
    /// 適用する項目
    pub apply: Vec<BackupItem>,
}

/// 適用した項目の結果
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupApplied {
    /// 項目
    pub kind: BackupItem,
    /// 適用した件数（設定は明示指定のキー数、辞書・抑制語は新しく加わった件数、学習は行数）
    pub count: u32,
}

/// `apply_backup` / `restore_backup` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyBackupResult {
    /// 適用した項目
    pub applied: Vec<BackupApplied>,
    /// 適用前に保存した自動バックアップのファイル名（`<user-dir>/backups/` 内）
    pub auto_backup: String,
    /// 注意
    pub warnings: Vec<String>,
}

/// 自動バックアップの 1 件
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupListItem {
    /// ファイル名
    pub name: String,
    /// 作成日時
    pub created_at: String,
    /// 作成した Nagi の版
    pub app_version: String,
    /// 含まれる項目
    pub kinds: Vec<BackupItem>,
    /// ファイルの大きさ（バイト）
    pub size: u32,
}

/// `list_backups` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListBackupsResult {
    /// 新しい順
    pub backups: Vec<BackupListItem>,
    /// 保存先のフォルダ（表示用）
    pub dir: Option<String>,
    /// 残す世代数
    pub keep: u32,
}

/// `restore_backup` の要求パラメータ
#[typeshare]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreBackupParams {
    /// 復元する自動バックアップのファイル名（省略は最新）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
