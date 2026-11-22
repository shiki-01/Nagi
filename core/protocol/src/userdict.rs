//! ユーザー辞書・学習の管理 RPC の型（設定画面の「ユーザー辞書」「学習と抑制語」ページ向け）。
//!
//! - `list_user_dict` / `add_user_dict_entry` / `update_user_dict_entry` / `remove_user_dict_entries`
//! - `export_user_dict`（Mozc TSV）/ `merge_user_dict`（取り込み。重複の扱いを選べる）
//! - `get_learning_stats`（学習データの件数・最終保存。入力内容は含まない）
//!
//! 既存の `import_user_dict`（`methods` の型）は変えず、新しい型だけを足す（加算的な変更）。
//! 抑制語は既存の `suppress_candidate` / `unsuppress_candidate` / `list_suppressed` を使う。

use crate::methods::{ImportSkipReason, UserDictFormat};
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// メソッド名の定数
pub mod names {
    /// `list_user_dict` メソッド（登録語の一覧。検索・並べ替え・ページング）
    pub const LIST_USER_DICT: &str = "list_user_dict";
    /// `add_user_dict_entry` メソッド（1 語の登録）
    pub const ADD_USER_DICT_ENTRY: &str = "add_user_dict_entry";
    /// `update_user_dict_entry` メソッド（1 語の編集）
    pub const UPDATE_USER_DICT_ENTRY: &str = "update_user_dict_entry";
    /// `remove_user_dict_entries` メソッド（複数語の削除）
    pub const REMOVE_USER_DICT_ENTRIES: &str = "remove_user_dict_entries";
    /// `export_user_dict` メソッド（Mozc TSV の書き出し）
    pub const EXPORT_USER_DICT: &str = "export_user_dict";
    /// `merge_user_dict` メソッド（他の IME の辞書の取り込み。重複の扱いを選べる）
    pub const MERGE_USER_DICT: &str = "merge_user_dict";
    /// `get_learning_stats` メソッド（学習データの統計）
    pub const GET_LEARNING_STATS: &str = "get_learning_stats";
}

/// ユーザー辞書の 1 語（Mozc TSV の 1 行）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDictEntry {
    /// 読み（ひらがな。カタカナはひらがなに直して保存する）
    pub reading: String,
    /// 単語（スニペットでは展開後の定型文。改行を含められる）
    pub surface: String,
    /// 品詞（Mozc の品詞名または `スニペット`。省略すると `名詞`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<String>,
    /// コメント（省略可）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// 一覧の 1 行（登録語 + どのファイルにあるか）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDictListItem {
    /// 読み
    pub reading: String,
    /// 単語
    pub surface: String,
    /// 品詞
    pub pos: String,
    /// コメント
    pub comment: String,
    /// 登録されているファイル名（ユーザー辞書フォルダ内）
    pub file: String,
}

/// 語を指す（編集・削除の対象）。同じ読み・表記が複数のファイルにあるときは `file` で区別する
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDictKey {
    /// 読み
    pub reading: String,
    /// 単語
    pub surface: String,
    /// ファイル名。省略すると、同じ読み・表記のすべて
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// 一覧の並べ替えの基準
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDictSort {
    /// 登録順（ファイル名順・ファイル内の行順）
    Registered,
    /// 読み
    Reading,
    /// 単語
    Surface,
    /// 品詞
    Pos,
}

/// 重複（同じ読みと単語）の扱い
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDictDuplicate {
    /// 既存を残して新しい方を取り込まない
    Skip,
    /// 既存の品詞・コメントを新しい内容で上書きする
    Overwrite,
}

/// `list_user_dict` の要求パラメータ（すべて省略可）
#[typeshare]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListUserDictParams {
    /// 検索語（読み・単語・コメント・品詞に部分一致。大文字小文字・カタカナ/ひらがなを区別しない）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// 並べ替え（既定: 登録順）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<UserDictSort>,
    /// 降順にするか（既定: 昇順）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descending: Option<bool>,
    /// 何件目から返すか（既定 0）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// 最大件数（既定 100、上限 500）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// `list_user_dict` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListUserDictResult {
    /// 該当した語（`offset` から `limit` 件）
    pub items: Vec<UserDictListItem>,
    /// 登録語の総数
    pub total: u32,
    /// 検索に一致した件数（検索語が無ければ `total` と同じ）
    pub matched: u32,
    /// 返した先頭の位置
    pub offset: u32,
    /// 登録できる上限（`userdict.max_entries`）
    pub max_entries: u32,
    /// 選べる品詞（Mozc の品詞名 + `スニペット`）
    pub pos_options: Vec<String>,
}

/// `add_user_dict_entry` の要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddUserDictEntryParams {
    /// 登録する語
    pub entry: UserDictEntry,
    /// 同じ読みと単語が既にあるときの扱い。省略するとエラー
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_duplicate: Option<UserDictDuplicate>,
}

/// `add_user_dict_entry` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddUserDictEntryResult {
    /// 新しく登録したか
    pub added: bool,
    /// 既存を上書きしたか
    pub replaced: bool,
}

/// `update_user_dict_entry` の要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateUserDictEntryParams {
    /// 編集する語
    pub target: UserDictKey,
    /// 新しい内容（読み・単語も変えられる）
    pub entry: UserDictEntry,
}

/// `remove_user_dict_entries` の要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveUserDictEntriesParams {
    /// 削除する語
    pub items: Vec<UserDictKey>,
}

/// `remove_user_dict_entries` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveUserDictEntriesResult {
    /// 削除した件数
    pub removed: u32,
}

/// `export_user_dict` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportUserDictResult {
    /// Mozc TSV（UTF-8。1 行 `読み<TAB>単語<TAB>品詞<TAB>コメント`）
    pub text: String,
    /// 書き出した語数
    pub count: u32,
}

/// `merge_user_dict` の要求パラメータ（`import_user_dict` + 重複の扱い）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeUserDictParams {
    /// ファイルの中身（バイト列そのものを base64 で。文字コードの判定はコアが行う）
    pub data_base64: String,
    /// 形式のヒント（省略すると自動判定）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<UserDictFormat>,
    /// 既存と同じ読み・単語の扱い（省略すると `skip`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_duplicate: Option<UserDictDuplicate>,
}

/// 取り込みをスキップした行（行番号つき）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeSkippedLine {
    /// 行番号（1 始まり。plist では辞書の通し番号）
    pub line: u32,
    /// 理由の識別子（`ImportSkipReason::reason` と同じ）
    pub reason: String,
    /// 表示用の説明（日本語）
    pub message: String,
}

/// `merge_user_dict` の応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeUserDictResult {
    /// 判定（または指定）された形式
    pub format: UserDictFormat,
    /// 読み取った項目の総数
    pub total: u32,
    /// 新たに取り込んだ件数
    pub imported: u32,
    /// 既存を上書きした件数
    pub replaced: u32,
    /// スキップした件数
    pub skipped: u32,
    /// スキップの理由ごとの内訳
    pub skipped_reasons: Vec<ImportSkipReason>,
    /// スキップした行（先頭から最大 50 件。行番号つき）
    pub skipped_lines: Vec<MergeSkippedLine>,
}

/// `get_learning_stats` の応答。学習した語・入力の内容は含めない（件数と時刻だけ）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetLearningStatsResult {
    /// 学習が有効か（設定として。secure input 中などの一時停止は含まない）
    pub enabled: bool,
    /// 学習した語（読みと表記の組）の件数
    pub learned_entries: u32,
    /// ユーザー辞書の登録数（語 + スニペット）
    pub user_dict_entries: u32,
    /// 抑制語の件数
    pub suppressed_entries: u32,
    /// 学習ファイルを最後に保存した時刻（UNIX 秒。まだ保存されていなければ省略）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_saved_unix: Option<u32>,
}
