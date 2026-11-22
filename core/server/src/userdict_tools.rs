//! ユーザー辞書の管理（一覧・登録・編集・削除・書き出し・取り込み）と学習の統計。
//! 設定画面の「ユーザー辞書」「学習と抑制語」ページ向けの RPC の実体で、サーバーの状態には触らない
//! （ディレクトリを渡して読み書きする純粋な関数。反映のための辞書の再読み込みは呼び出し側が行う）。
//!
//! # データの形
//!
//! ユーザー辞書は `<user-dir>/user_dict/` 内の `*.txt` / `*.tsv`（Mozc TSV。1 行 `読み<TAB>単語<TAB>品詞<TAB>コメント`）を
//! ファイル名順にすべて読む。手で置いたファイルも一覧に出る。このモジュールの書き込みは、
//!
//! - 登録（単語登録）は [`REGISTERED_FILE`]、取り込みの新規分は [`IMPORTED_FILE`] に追記する
//! - 編集・削除は、その語がある行だけを書き換える（他の行・コメント行・空行はそのまま残す）
//!
//! 書き込みは一時ファイルに書いて `rename` する（原子的）。一時ファイルは拡張子が `.nagi-tmp` なので、
//! 途中で落ちても辞書としては読まれない。
//!
//! # プライバシー
//!
//! エラーメッセージ・ログに読み・表記・コメントの内容を入れない（件数・行番号・理由だけ）。

use nagi_dict::{
    ImportError, ImportFormat, ImportOutcome, MOZC_POS, SNIPPET_POS, UserDictionary, UserEntry,
    import_user_dict, to_hiragana, valid_reading,
};
use nagi_protocol::methods::{ImportSkipReason, UserDictFormat};
use nagi_protocol::userdict::{
    AddUserDictEntryParams, AddUserDictEntryResult, ExportUserDictResult, GetLearningStatsResult,
    ListUserDictParams, ListUserDictResult, MergeSkippedLine, MergeUserDictParams,
    MergeUserDictResult, RemoveUserDictEntriesParams, RemoveUserDictEntriesResult,
    UpdateUserDictEntryParams, UserDictDuplicate, UserDictEntry, UserDictKey, UserDictListItem,
    UserDictSort,
};
use std::path::{Path, PathBuf};

/// 設定画面・単語登録で追加した語を書くファイル
pub const REGISTERED_FILE: &str = "registered.tsv";
/// 他の IME から取り込んだ語を書くファイル（`import_user_dict` と共通）
pub const IMPORTED_FILE: &str = "imported.tsv";
/// 取り込むファイルの大きさの上限（バイト）
pub const MAX_IMPORT_BYTES: usize = 5 * 1024 * 1024;
/// 一覧の 1 回の最大件数
pub const MAX_LIST_LIMIT: u32 = 500;
/// 一覧の既定の件数
pub const DEFAULT_LIST_LIMIT: u32 = 100;
/// 取り込みの結果に載せる、スキップした行の最大件数
pub const MAX_SKIPPED_LINES: usize = 50;
/// 1 回の削除で指定できる最大件数
pub const MAX_REMOVE_ITEMS: usize = 20_000;

const MAX_READING_CHARS: usize = 100;
const MAX_WORD_CHARS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 4000;
const MAX_COMMENT_CHARS: usize = 200;
const TMP_SUFFIX: &str = ".nagi-tmp";

/// 操作の失敗
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// 入力・状態が不正（利用者が直せる。`INVALID_PARAMS`）
    Invalid(String),
    /// 読み書きに失敗した（`ENGINE_FAILURE`）
    Io(String),
}

impl ToolError {
    fn invalid(message: impl Into<String>) -> Self {
        ToolError::Invalid(message.into())
    }

    fn io(message: impl Into<String>) -> Self {
        ToolError::Io(message.into())
    }
}

type Result<T> = std::result::Result<T, ToolError>;

/// このモジュールが扱う RPC メソッド名か（`Server` の振り分け用）
pub fn is_userdict_method(method: &str) -> bool {
    use nagi_protocol::userdict::names;
    [
        names::LIST_USER_DICT,
        names::ADD_USER_DICT_ENTRY,
        names::UPDATE_USER_DICT_ENTRY,
        names::REMOVE_USER_DICT_ENTRIES,
        names::EXPORT_USER_DICT,
        names::MERGE_USER_DICT,
        names::GET_LEARNING_STATS,
    ]
    .contains(&method)
}

// ---- ファイルの走査・書き込み ----

/// 辞書ファイルの 1 行
struct Line {
    /// 行の文字列（`\r` を含む。区切りの `\n` は含まない）
    raw: String,
    /// 語の行ならその内容
    entry: Option<UserEntry>,
}

/// 辞書ファイル 1 つ（行単位。書き換えるときも他の行はそのまま残す）
struct DictFile {
    name: String,
    path: PathBuf,
    lines: Vec<Line>,
    dirty: bool,
}

fn parse_line(file: &str, raw: &str) -> Option<UserEntry> {
    let mut dict = UserDictionary::default();
    let mut issues = Vec::new();
    dict.add_tsv(file, raw.trim_end_matches('\r'), &mut issues);
    dict.entries.pop()
}

impl DictFile {
    fn from_text(name: &str, path: PathBuf, text: &str) -> Self {
        DictFile {
            name: name.to_owned(),
            path,
            lines: text
                .split('\n')
                .map(|raw| Line {
                    entry: parse_line(name, raw),
                    raw: raw.to_owned(),
                })
                .collect(),
            dirty: false,
        }
    }

    fn text(&self) -> String {
        let raws: Vec<&str> = self.lines.iter().map(|l| l.raw.as_str()).collect();
        raws.join("\n")
    }

    /// 末尾に 1 行足す（末尾の改行の有無にかかわらず、結果は改行で終わる）
    fn append(&mut self, entry: &UserEntry) {
        let line = Line {
            raw: tsv_line(entry),
            entry: Some(entry.clone()),
        };
        match self.lines.last() {
            None => {
                self.lines.push(line);
                self.lines.push(blank());
            }
            Some(last) if last.raw.is_empty() => {
                let at = self.lines.len() - 1;
                self.lines.insert(at, line);
            }
            Some(_) => {
                self.lines.push(line);
                self.lines.push(blank());
            }
        }
        self.dirty = true;
    }

    /// `index` 行目を別の語に差し替える（元の行が CRLF なら CRLF のまま）
    fn replace(&mut self, index: usize, entry: &UserEntry) {
        let crlf = self.lines[index].raw.ends_with('\r');
        let mut raw = tsv_line(entry);
        if crlf {
            raw.push('\r');
        }
        self.lines[index] = Line {
            raw,
            entry: Some(entry.clone()),
        };
        self.dirty = true;
    }

    fn remove(&mut self, index: usize) {
        self.lines.remove(index);
        self.dirty = true;
    }
}

fn blank() -> Line {
    Line {
        raw: String::new(),
        entry: None,
    }
}

/// 1 語を Mozc TSV の 1 行にする（末尾の改行なし。スニペットの改行・タブ・`\` はエスケープする）
fn tsv_line(entry: &UserEntry) -> String {
    let text = UserDictionary {
        entries: vec![entry.clone()],
    }
    .to_mozc_tsv();
    text.trim_end_matches('\n').to_owned()
}

fn is_dict_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("tsv"))
}

/// ディレクトリ内の辞書ファイルをファイル名順に読む（`UserDictionary::load_dir` と同じ対象・順序）。
/// ディレクトリが無ければ空
fn scan(dir: &Path) -> Vec<DictFile> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| is_dict_file(p))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let bytes = std::fs::read(&path).ok()?;
            let text = nagi_dict::decode_text(&bytes);
            Some(DictFile::from_text(&name, path, &text))
        })
        .collect()
}

/// 名前のファイルを取り出す。無ければ空のファイルを作って返す（書くまでディスクには作らない）
fn file_mut<'a>(files: &'a mut Vec<DictFile>, dir: &Path, name: &str) -> &'a mut DictFile {
    if let Some(i) = files.iter().position(|f| f.name == name) {
        return &mut files[i];
    }
    files.push(DictFile {
        name: name.to_owned(),
        path: dir.join(name),
        lines: Vec::new(),
        dirty: false,
    });
    let last = files.len() - 1;
    &mut files[last]
}

/// 一時ファイルに書いて `rename` する
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(TMP_SUFFIX);
    let tmp = PathBuf::from(tmp);
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.sync_all()
    });
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(ToolError::io("ユーザー辞書に書き込めません"));
    }
    std::fs::rename(&tmp, path).map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        ToolError::io("ユーザー辞書を保存できません")
    })
}

/// 変更のあったファイルを書き出す。フォルダが無ければ作る
fn save(dir: &Path, files: &[DictFile]) -> Result<()> {
    if files.iter().any(|f| f.dirty) {
        std::fs::create_dir_all(dir)
            .map_err(|_| ToolError::io("ユーザー辞書のフォルダを作れません"))?;
    }
    for file in files.iter().filter(|f| f.dirty) {
        write_atomic(&file.path, &file.text())?;
    }
    Ok(())
}

fn entry_count(files: &[DictFile]) -> usize {
    files
        .iter()
        .map(|f| f.lines.iter().filter(|l| l.entry.is_some()).count())
        .sum()
}

/// (ファイル, 行) の位置
type Pos = (usize, usize);

fn find_all(files: &[DictFile], reading: &str, surface: &str) -> Vec<Pos> {
    let mut out = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        for (li, l) in f.lines.iter().enumerate() {
            if l.entry
                .as_ref()
                .is_some_and(|e| e.reading == reading && e.surface == surface)
            {
                out.push((fi, li));
            }
        }
    }
    out
}

// ---- 入力の検証 ----

fn too_long(what: &str, max: usize) -> ToolError {
    ToolError::invalid(format!("{what}は {max} 文字までにしてください"))
}

/// 登録する語を検証して正規化する（読みはひらがなに、品詞は未指定なら `名詞`）
pub fn normalize_entry(entry: &UserDictEntry) -> Result<UserEntry> {
    let reading = to_hiragana(entry.reading.trim());
    if reading.is_empty() {
        return Err(ToolError::invalid("読みを入力してください"));
    }
    if !valid_reading(&reading) {
        return Err(ToolError::invalid(
            "読みはひらがなで入力してください（長音・中黒・英数字・ハイフンも使えます）",
        ));
    }
    if reading.chars().count() > MAX_READING_CHARS {
        return Err(too_long("読み", MAX_READING_CHARS));
    }
    let pos = entry
        .pos
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("名詞")
        .to_owned();
    if pos != SNIPPET_POS && !MOZC_POS.contains(&pos.as_str()) {
        return Err(ToolError::invalid("品詞が正しくありません"));
    }
    let snippet = pos == SNIPPET_POS;
    let surface = if snippet {
        entry.surface.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        entry.surface.trim().to_owned()
    };
    if surface.trim().is_empty() {
        return Err(ToolError::invalid(if snippet {
            "定型文を入力してください"
        } else {
            "単語を入力してください"
        }));
    }
    if !snippet && surface.chars().any(char::is_control) {
        return Err(ToolError::invalid(
            "単語に改行・タブなどの制御文字は使えません（改行を含めるときは品詞を「スニペット」にしてください）",
        ));
    }
    let max = if snippet {
        MAX_SNIPPET_CHARS
    } else {
        MAX_WORD_CHARS
    };
    if surface.chars().count() > max {
        return Err(too_long(if snippet { "定型文" } else { "単語" }, max));
    }
    let comment = entry.comment.as_deref().unwrap_or("").trim().to_owned();
    if comment.chars().any(char::is_control) {
        return Err(ToolError::invalid("コメントに改行・タブは使えません"));
    }
    if comment.chars().count() > MAX_COMMENT_CHARS {
        return Err(too_long("コメント", MAX_COMMENT_CHARS));
    }
    Ok(UserEntry {
        reading,
        surface,
        pos,
        comment,
    })
}

fn limit_error(max_entries: usize, current: usize, adding: usize) -> ToolError {
    ToolError::invalid(format!(
        "ユーザー辞書の登録が上限（{max_entries} 件）を超えるため登録できません（登録済み {current} 件、追加 {adding} 件）"
    ))
}

// ---- 一覧 ----

/// 選べる品詞（Mozc の品詞名 + スニペット）
pub fn pos_options() -> Vec<String> {
    MOZC_POS
        .iter()
        .copied()
        .chain(std::iter::once(SNIPPET_POS))
        .map(str::to_owned)
        .collect()
}

fn fold(s: &str) -> String {
    to_hiragana(&s.to_lowercase())
}

/// 登録語の一覧（検索・並べ替え・ページング）
pub fn list(dir: &Path, params: &ListUserDictParams, max_entries: usize) -> ListUserDictResult {
    let files = scan(dir);
    let mut rows: Vec<UserDictListItem> = files
        .iter()
        .flat_map(|f| {
            f.lines.iter().filter_map(|l| {
                l.entry.as_ref().map(|e| UserDictListItem {
                    reading: e.reading.clone(),
                    surface: e.surface.clone(),
                    pos: e.pos.clone(),
                    comment: e.comment.clone(),
                    file: f.name.clone(),
                })
            })
        })
        .collect();
    let total = rows.len();
    let query = params
        .query
        .as_deref()
        .map(|q| fold(q.trim()))
        .filter(|q| !q.is_empty());
    if let Some(q) = &query {
        rows.retain(|r| {
            [&r.reading, &r.surface, &r.comment, &r.pos]
                .iter()
                .any(|s| fold(s).contains(q.as_str()))
        });
    }
    let matched = rows.len();
    match params.sort.unwrap_or(UserDictSort::Registered) {
        UserDictSort::Registered => {}
        UserDictSort::Reading => rows.sort_by(|a, b| a.reading.cmp(&b.reading)),
        UserDictSort::Surface => rows.sort_by(|a, b| a.surface.cmp(&b.surface)),
        UserDictSort::Pos => rows.sort_by(|a, b| a.pos.cmp(&b.pos)),
    }
    if params.descending.unwrap_or(false) {
        rows.reverse();
    }
    let offset = (params.offset.unwrap_or(0) as usize).min(matched);
    let limit = params
        .limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT) as usize;
    let items = rows.into_iter().skip(offset).take(limit).collect();
    ListUserDictResult {
        items,
        total: total as u32,
        matched: matched as u32,
        offset: offset as u32,
        max_entries: max_entries as u32,
        pos_options: pos_options(),
    }
}

// ---- 登録・編集・削除 ----

/// 1 語を登録する。同じ読みと単語が既にあるときは `on_duplicate` に従う（省略はエラー）
pub fn add(
    dir: &Path,
    params: &AddUserDictEntryParams,
    max_entries: usize,
) -> Result<AddUserDictEntryResult> {
    let entry = normalize_entry(&params.entry)?;
    let mut files = scan(dir);
    let found = find_all(&files, &entry.reading, &entry.surface);
    if let Some(&(fi, li)) = found.first() {
        return match params.on_duplicate {
            None => Err(ToolError::invalid("同じ読みと単語がすでに登録されています")),
            Some(UserDictDuplicate::Skip) => Ok(AddUserDictEntryResult {
                added: false,
                replaced: false,
            }),
            Some(UserDictDuplicate::Overwrite) => {
                files[fi].replace(li, &entry);
                // 同じ語が他にもあれば 1 つにまとめる（後ろの行から消して位置をずらさない）
                for &(f, l) in found.iter().skip(1).rev() {
                    files[f].remove(l);
                }
                save(dir, &files)?;
                Ok(AddUserDictEntryResult {
                    added: false,
                    replaced: true,
                })
            }
        };
    }
    let current = entry_count(&files);
    if current + 1 > max_entries {
        return Err(limit_error(max_entries, current, 1));
    }
    file_mut(&mut files, dir, REGISTERED_FILE).append(&entry);
    save(dir, &files)?;
    Ok(AddUserDictEntryResult {
        added: true,
        replaced: false,
    })
}

fn matches_key(file: &DictFile, entry: &UserEntry, key: &UserDictKey) -> bool {
    entry.reading == to_hiragana(key.reading.trim())
        && entry.surface == key.surface
        && key.file.as_deref().is_none_or(|f| f == file.name)
}

/// 1 語を編集する。読み・単語を変えて、別の既存の語と重なるときは拒否する
pub fn update(dir: &Path, params: &UpdateUserDictEntryParams) -> Result<()> {
    let new = normalize_entry(&params.entry)?;
    let mut files = scan(dir);
    let target = (0..files.len()).find_map(|fi| {
        files[fi]
            .lines
            .iter()
            .position(|l| {
                l.entry
                    .as_ref()
                    .is_some_and(|e| matches_key(&files[fi], e, &params.target))
            })
            .map(|li| (fi, li))
    });
    let Some((fi, li)) = target else {
        return Err(ToolError::invalid(
            "編集する語が見つかりません（別の場所で変更された可能性があります）",
        ));
    };
    if find_all(&files, &new.reading, &new.surface)
        .iter()
        .any(|&p| p != (fi, li))
    {
        return Err(ToolError::invalid("同じ読みと単語がすでに登録されています"));
    }
    files[fi].replace(li, &new);
    save(dir, &files)
}

/// 複数の語を削除する（見つかった分だけ。件数を返す）
pub fn remove(
    dir: &Path,
    params: &RemoveUserDictEntriesParams,
) -> Result<RemoveUserDictEntriesResult> {
    if params.items.len() > MAX_REMOVE_ITEMS {
        return Err(ToolError::invalid("一度に削除できる件数を超えています"));
    }
    let mut files = scan(dir);
    let mut removed = 0u32;
    for file in &mut files {
        let before = file.lines.len();
        let keep: Vec<Line> = std::mem::take(&mut file.lines)
            .into_iter()
            .filter(|l| {
                !l.entry.as_ref().is_some_and(|e| {
                    params.items.iter().any(|k| {
                        e.reading == to_hiragana(k.reading.trim())
                            && e.surface == k.surface
                            && k.file.as_deref().is_none_or(|f| f == file.name)
                    })
                })
            })
            .collect();
        let gone = before - keep.len();
        file.lines = keep;
        if gone > 0 {
            file.dirty = true;
            removed += gone as u32;
        }
    }
    if removed > 0 {
        save(dir, &files)?;
    }
    Ok(RemoveUserDictEntriesResult { removed })
}

// ---- 書き出し ----

/// すべての語を Mozc TSV にする（読み込み順）
pub fn export(dir: &Path) -> ExportUserDictResult {
    let (dict, _) = UserDictionary::load_dir(dir);
    ExportUserDictResult {
        text: dict.to_mozc_tsv(),
        count: dict.entries.len() as u32,
    }
}

// ---- 取り込み ----

fn to_format(f: ImportFormat) -> UserDictFormat {
    match f {
        ImportFormat::MozcTsv => UserDictFormat::Mozc,
        ImportFormat::MsIme => UserDictFormat::MsIme,
        ImportFormat::Atok => UserDictFormat::Atok,
        ImportFormat::MacPlist => UserDictFormat::MacPlist,
        ImportFormat::TwoColumn => UserDictFormat::TwoColumn,
    }
}

fn from_format(f: UserDictFormat) -> ImportFormat {
    match f {
        UserDictFormat::Mozc => ImportFormat::MozcTsv,
        UserDictFormat::MsIme => ImportFormat::MsIme,
        UserDictFormat::Atok => ImportFormat::Atok,
        UserDictFormat::MacPlist => ImportFormat::MacPlist,
        UserDictFormat::TwoColumn => ImportFormat::TwoColumn,
    }
}

fn import_error(e: &ImportError) -> ToolError {
    ToolError::invalid(e.to_string())
}

/// 他の IME の辞書（Mozc TSV を含む）を取り込む。新規の語は [`IMPORTED_FILE`] に追記し、
/// 既存と同じ読み・単語は `on_duplicate`（省略は `skip`）に従う
pub fn merge(
    dir: &Path,
    bytes: &[u8],
    params: &MergeUserDictParams,
    max_entries: usize,
) -> Result<MergeUserDictResult> {
    if bytes.is_empty() {
        return Err(ToolError::invalid("ファイルが空です"));
    }
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(ToolError::invalid(format!(
            "ファイルが大きすぎます（上限 {} MB）",
            MAX_IMPORT_BYTES / (1024 * 1024)
        )));
    }
    let policy = params.on_duplicate.unwrap_or(UserDictDuplicate::Skip);
    let hint = params.format.map(from_format);
    let mut files = scan(dir);
    let existing = UserDictionary {
        entries: files
            .iter()
            .flat_map(|f| f.lines.iter().filter_map(|l| l.entry.clone()))
            .collect(),
    };
    // 既存と重ねて読む: スキップの行番号（既存との重複を含む）はこちらが正
    let with_existing = import_user_dict(bytes, hint, &existing).map_err(|e| import_error(&e))?;
    let (to_apply, skipped_source): (Vec<UserEntry>, &ImportOutcome);
    let overwrite_outcome;
    match policy {
        UserDictDuplicate::Skip => {
            to_apply = with_existing.entries.clone();
            skipped_source = &with_existing;
        }
        UserDictDuplicate::Overwrite => {
            // 既存と重なる語も適用する（上書き）。スキップはファイル内の重複・不正な行だけ
            overwrite_outcome = import_user_dict(bytes, hint, &UserDictionary::default())
                .map_err(|e| import_error(&e))?;
            to_apply = overwrite_outcome.entries.clone();
            skipped_source = &overwrite_outcome;
        }
    }
    let mut fresh: Vec<UserEntry> = Vec::new();
    let mut replaced = 0u32;
    for entry in &to_apply {
        let found = find_all(&files, &entry.reading, &entry.surface);
        match found.first() {
            Some(&(fi, li)) => {
                files[fi].replace(li, entry);
                replaced += 1;
            }
            None => fresh.push(entry.clone()),
        }
    }
    let current = entry_count(&files);
    if !fresh.is_empty() && current + fresh.len() > max_entries {
        return Err(limit_error(max_entries, current, fresh.len()));
    }
    if !fresh.is_empty() {
        let file = file_mut(&mut files, dir, IMPORTED_FILE);
        for entry in &fresh {
            file.append(entry);
        }
    }
    save(dir, &files)?;

    let mut reasons: Vec<ImportSkipReason> = Vec::new();
    for s in &skipped_source.skipped {
        if let Some(r) = reasons.iter_mut().find(|r| r.reason == s.reason.code()) {
            r.count += 1;
        } else {
            reasons.push(ImportSkipReason {
                reason: s.reason.code().to_owned(),
                message: s.reason.message().to_owned(),
                count: 1,
                first_line: s.line as u32,
            });
        }
    }
    let skipped_lines = skipped_source
        .skipped
        .iter()
        .take(MAX_SKIPPED_LINES)
        .map(|s| MergeSkippedLine {
            line: s.line as u32,
            reason: s.reason.code().to_owned(),
            message: s.reason.message().to_owned(),
        })
        .collect();
    Ok(MergeUserDictResult {
        format: to_format(skipped_source.format),
        total: skipped_source.total as u32,
        imported: fresh.len() as u32,
        replaced,
        skipped: skipped_source.skipped.len() as u32,
        skipped_reasons: reasons,
        skipped_lines,
    })
}

// ---- 学習の統計 ----

/// 学習データの統計。`user_dir` は学習ファイル（`learning.tsv`・`learning-next.tsv`）の置き場で、
/// 最後に保存した時刻（更新日時の新しい方）を求める。語・入力の内容は読まない
pub fn learning_stats(
    enabled: bool,
    learned_entries: usize,
    user_dict_entries: usize,
    suppressed_entries: usize,
    user_dir: Option<&Path>,
) -> GetLearningStatsResult {
    let last_saved_unix = user_dir.and_then(|dir| {
        ["learning.tsv", "learning-next.tsv"]
            .iter()
            .filter_map(|name| std::fs::metadata(dir.join(name)).ok()?.modified().ok())
            .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .max()
            .and_then(|s| u32::try_from(s).ok())
    });
    GetLearningStatsResult {
        enabled,
        learned_entries: learned_entries as u32,
        user_dict_entries: user_dict_entries as u32,
        suppressed_entries: suppressed_entries as u32,
        last_saved_unix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nagi-userdict-tools-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(
        reading: &str,
        surface: &str,
        pos: Option<&str>,
        comment: Option<&str>,
    ) -> UserDictEntry {
        UserDictEntry {
            reading: reading.into(),
            surface: surface.into(),
            pos: pos.map(Into::into),
            comment: comment.map(Into::into),
        }
    }

    fn add_params(e: UserDictEntry, dup: Option<UserDictDuplicate>) -> AddUserDictEntryParams {
        AddUserDictEntryParams {
            entry: e,
            on_duplicate: dup,
        }
    }

    fn key(reading: &str, surface: &str) -> UserDictKey {
        UserDictKey {
            reading: reading.into(),
            surface: surface.into(),
            file: None,
        }
    }

    fn read(dir: &Path, name: &str) -> String {
        std::fs::read_to_string(dir.join(name)).unwrap()
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn add_validates_and_normalizes() {
        let dir = temp_dir();
        let ok = add(
            &dir,
            &add_params(entry("コウエン", " 公苑 ", None, Some(" 近所 ")), None),
            100,
        )
        .unwrap();
        assert!(ok.added && !ok.replaced);
        // 読みはひらがなに、品詞は名詞、前後の空白は落とす
        assert_eq!(read(&dir, REGISTERED_FILE), "こうえん\t公苑\t名詞\t近所\n");
        for (e, needle) in [
            (entry("", "x", None, None), "読み"),
            (entry("漢字", "x", None, None), "ひらがな"),
            (entry("あ", "", None, None), "単語"),
            (entry("あ", "a\tb", None, None), "制御文字"),
            (entry("あ", "x", Some("まちがい"), None), "品詞"),
            (entry("あ", "x", None, Some("a\nb")), "コメント"),
        ] {
            let err = add(&dir, &add_params(e, None), 100).unwrap_err();
            assert!(
                matches!(&err, ToolError::Invalid(m) if m.contains(needle)),
                "{err:?}"
            );
        }
        // 失敗では何も書き足さない
        assert_eq!(read(&dir, REGISTERED_FILE), "こうえん\t公苑\t名詞\t近所\n");
        // 一時ファイルが残らない
        assert_eq!(files_in(&dir), [REGISTERED_FILE]);
    }

    #[test]
    fn duplicate_policies() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("mine.txt"),
            "# 自分の辞書\nこうえん\t公苑\t名詞\t旧\n",
        )
        .unwrap();
        let e = entry("こうえん", "公苑", Some("地名"), Some("新"));
        let err = add(&dir, &add_params(e.clone(), None), 100).unwrap_err();
        assert!(matches!(err, ToolError::Invalid(_)));
        let r = add(
            &dir,
            &add_params(e.clone(), Some(UserDictDuplicate::Skip)),
            100,
        )
        .unwrap();
        assert!(!r.added && !r.replaced);
        assert_eq!(
            read(&dir, "mine.txt"),
            "# 自分の辞書\nこうえん\t公苑\t名詞\t旧\n"
        );
        let r = add(
            &dir,
            &add_params(e, Some(UserDictDuplicate::Overwrite)),
            100,
        )
        .unwrap();
        assert!(!r.added && r.replaced);
        // 手書きのコメント行はそのまま、語の行だけ差し替わる。新しいファイルは作らない
        assert_eq!(
            read(&dir, "mine.txt"),
            "# 自分の辞書\nこうえん\t公苑\t地名\t新\n"
        );
        assert_eq!(files_in(&dir), ["mine.txt"]);
    }

    #[test]
    fn upper_limit_is_enforced_on_add() {
        let dir = temp_dir();
        std::fs::write(dir.join("a.txt"), "あ\t亜\t名詞\nい\t伊\t名詞\n").unwrap();
        let err = add(&dir, &add_params(entry("う", "宇", None, None), None), 2).unwrap_err();
        assert!(
            matches!(&err, ToolError::Invalid(m) if m.contains("上限")),
            "{err:?}"
        );
        assert!(!dir.join(REGISTERED_FILE).exists());
        add(&dir, &add_params(entry("う", "宇", None, None), None), 3).unwrap();
    }

    #[test]
    fn append_handles_missing_trailing_newline_and_crlf() {
        let dir = temp_dir();
        std::fs::write(dir.join(REGISTERED_FILE), "あ\t亜\t名詞\t").unwrap();
        add(&dir, &add_params(entry("い", "伊", None, None), None), 100).unwrap();
        assert_eq!(
            read(&dir, REGISTERED_FILE),
            "あ\t亜\t名詞\t\nい\t伊\t名詞\t\n"
        );
        std::fs::write(dir.join("w.txt"), "う\t宇\t名詞\r\nえ\t絵\t名詞\r\n").unwrap();
        update(
            &dir,
            &UpdateUserDictEntryParams {
                target: key("う", "宇"),
                entry: entry("う", "羽", None, None),
            },
        )
        .unwrap();
        assert_eq!(read(&dir, "w.txt"), "う\t羽\t名詞\t\r\nえ\t絵\t名詞\r\n");
    }

    #[test]
    fn update_changes_key_and_rejects_collision_and_missing() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("a.txt"),
            "あ\t亜\t名詞\nい\t伊\t名詞\n! メモ\nう\t宇\t名詞\n",
        )
        .unwrap();
        let upd = |t: UserDictKey, e: UserDictEntry| {
            update(
                &dir,
                &UpdateUserDictEntryParams {
                    target: t,
                    entry: e,
                },
            )
        };
        // 他の語と重なる
        assert!(upd(key("あ", "亜"), entry("い", "伊", None, None)).is_err());
        // 見つからない
        assert!(upd(key("お", "尾"), entry("お", "緒", None, None)).is_err());
        // 自分自身と同じキーで品詞・コメントだけ変える
        upd(key("あ", "亜"), entry("あ", "亜", Some("人名"), Some("c"))).unwrap();
        // 読みも単語も変える
        upd(key("い", "伊"), entry("いい", "良い", None, None)).unwrap();
        assert_eq!(
            read(&dir, "a.txt"),
            "あ\t亜\t人名\tc\nいい\t良い\t名詞\t\n! メモ\nう\t宇\t名詞\n"
        );
    }

    #[test]
    fn remove_by_key_and_by_file_keeps_other_lines() {
        let dir = temp_dir();
        std::fs::write(dir.join("a.txt"), "# c\nあ\t亜\t名詞\nい\t伊\t名詞\n").unwrap();
        std::fs::write(dir.join("b.txt"), "あ\t亜\t名詞\nう\t宇\t名詞\n").unwrap();
        // ファイル指定: b だけ
        let r = remove(
            &dir,
            &RemoveUserDictEntriesParams {
                items: vec![UserDictKey {
                    file: Some("b.txt".into()),
                    ..key("あ", "亜")
                }],
            },
        )
        .unwrap();
        assert_eq!(r.removed, 1);
        assert_eq!(read(&dir, "a.txt"), "# c\nあ\t亜\t名詞\nい\t伊\t名詞\n");
        assert_eq!(read(&dir, "b.txt"), "う\t宇\t名詞\n");
        // ファイル指定なし: すべて。見つからない語は数えない
        let r = remove(
            &dir,
            &RemoveUserDictEntriesParams {
                items: vec![key("あ", "亜"), key("う", "宇"), key("お", "尾")],
            },
        )
        .unwrap();
        assert_eq!(r.removed, 2);
        assert_eq!(read(&dir, "a.txt"), "# c\nい\t伊\t名詞\n");
        assert_eq!(read(&dir, "b.txt"), "");
        // 何も消えなければ書き換えない
        let before = std::fs::metadata(dir.join("a.txt"))
            .unwrap()
            .modified()
            .unwrap();
        let r = remove(
            &dir,
            &RemoveUserDictEntriesParams {
                items: vec![key("お", "尾")],
            },
        )
        .unwrap();
        assert_eq!(r.removed, 0);
        assert_eq!(
            std::fs::metadata(dir.join("a.txt"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
    }

    #[test]
    fn list_filters_sorts_and_pages() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("a.txt"),
            "さ\t差\t名詞\t\nあ\t亜\t人名\tメモ\nい\tイー\t名詞\t\nか\tカ\t名詞\t\n",
        )
        .unwrap();
        let p = |q: Option<&str>, sort, desc, offset, limit| ListUserDictParams {
            query: q.map(Into::into),
            sort,
            descending: desc,
            offset,
            limit,
        };
        let all = list(&dir, &p(None, None, None, None, None), 10);
        assert_eq!((all.total, all.matched, all.max_entries), (4, 4, 10));
        assert_eq!(all.items[0].file, "a.txt");
        assert!(all.pos_options.contains(&"スニペット".to_string()));
        let sorted = list(
            &dir,
            &p(None, Some(UserDictSort::Reading), None, None, None),
            10,
        );
        let readings: Vec<&str> = sorted.items.iter().map(|i| i.reading.as_str()).collect();
        assert_eq!(readings, ["あ", "い", "か", "さ"]);
        let desc = list(
            &dir,
            &p(None, Some(UserDictSort::Reading), Some(true), None, None),
            10,
        );
        assert_eq!(desc.items[0].reading, "さ");
        // 検索: カタカナ・ひらがなを区別しない。コメント・品詞にも一致
        assert_eq!(
            list(&dir, &p(Some("カ"), None, None, None, None), 10).matched,
            1
        );
        assert_eq!(
            list(&dir, &p(Some("メモ"), None, None, None, None), 10).matched,
            1
        );
        assert_eq!(
            list(&dir, &p(Some("人名"), None, None, None, None), 10).matched,
            1
        );
        // ページング: total は検索に影響されない
        let page = list(
            &dir,
            &p(None, Some(UserDictSort::Reading), None, Some(3), Some(5)),
            10,
        );
        assert_eq!((page.items.len(), page.offset, page.total), (1, 3, 4));
        let beyond = list(&dir, &p(None, None, None, Some(99), None), 10);
        assert!(beyond.items.is_empty() && beyond.offset == 4);
    }

    #[test]
    fn export_round_trips_snippets() {
        let dir = temp_dir();
        add(
            &dir,
            &add_params(
                entry("ひなた", "拝啓\n敬具", Some("スニペット"), None),
                None,
            ),
            100,
        )
        .unwrap();
        // ファイル上は 1 行（改行は \n のエスケープ）
        assert_eq!(
            read(&dir, REGISTERED_FILE),
            "ひなた\t拝啓\\n敬具\tスニペット\t\n"
        );
        let out = export(&dir);
        assert_eq!(out.count, 1);
        assert_eq!(out.text, "ひなた\t拝啓\\n敬具\tスニペット\t\n");
        let listed = list(&dir, &ListUserDictParams::default(), 10);
        assert_eq!(listed.items[0].surface, "拝啓\n敬具");
    }

    fn merge_params(dup: Option<UserDictDuplicate>) -> MergeUserDictParams {
        MergeUserDictParams {
            data_base64: String::new(),
            format: None,
            on_duplicate: dup,
        }
    }

    #[test]
    fn merge_skip_and_overwrite_with_line_numbers() {
        let dir = temp_dir();
        std::fs::write(dir.join("mine.txt"), "こうえん\t公苑\t名詞\t旧\n").unwrap();
        let text =
            "こうえん\t公苑\t地名\t新\nひなた\t日向\t地名\n漢字\t不正\t名詞\nひなた\t日向\t地名\n";
        // 既定（skip）: 既存優先。行番号つきでスキップを返す
        let r = merge(&dir, text.as_bytes(), &merge_params(None), 100).unwrap();
        assert_eq!((r.total, r.imported, r.replaced, r.skipped), (4, 1, 0, 3));
        let lines: Vec<(u32, &str)> = r
            .skipped_lines
            .iter()
            .map(|l| (l.line, l.reason.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (1, "duplicate_existing"),
                (3, "invalid_reading"),
                (4, "duplicate_in_file")
            ]
        );
        assert_eq!(read(&dir, "mine.txt"), "こうえん\t公苑\t名詞\t旧\n");
        assert_eq!(read(&dir, IMPORTED_FILE), "ひなた\t日向\t地名\t\n");

        // 上書き: 既存の行を差し替える。新規は増えない。ファイル内の重複・不正はスキップのまま
        let r = merge(
            &dir,
            text.as_bytes(),
            &merge_params(Some(UserDictDuplicate::Overwrite)),
            100,
        )
        .unwrap();
        assert_eq!((r.total, r.imported, r.replaced, r.skipped), (4, 0, 2, 2));
        assert_eq!(read(&dir, "mine.txt"), "こうえん\t公苑\t地名\t新\n");
        assert_eq!(read(&dir, IMPORTED_FILE), "ひなた\t日向\t地名\t\n");
        assert_eq!(export(&dir).count, 2);
    }

    #[test]
    fn merge_rejects_empty_oversize_unrecognized_and_over_limit() {
        let dir = temp_dir();
        assert!(matches!(
            merge(&dir, b"", &merge_params(None), 10),
            Err(ToolError::Invalid(_))
        ));
        let big = vec![b'a'; MAX_IMPORT_BYTES + 1];
        assert!(matches!(
            merge(&dir, &big, &merge_params(None), 10),
            Err(ToolError::Invalid(m)) if m.contains("大きすぎ")
        ));
        assert!(matches!(
            merge(&dir, "ただの文章\n".as_bytes(), &merge_params(None), 10),
            Err(ToolError::Invalid(_))
        ));
        std::fs::write(dir.join("a.txt"), "あ\t亜\t名詞\n").unwrap();
        let err = merge(&dir, "い\t伊\t名詞\n".as_bytes(), &merge_params(None), 1).unwrap_err();
        assert!(matches!(&err, ToolError::Invalid(m) if m.contains("上限")));
        assert!(!dir.join(IMPORTED_FILE).exists());
    }

    #[test]
    fn learning_stats_reports_counts_and_latest_mtime_without_content() {
        let dir = temp_dir();
        let none = learning_stats(true, 3, 2, 1, Some(&dir));
        assert_eq!(
            (
                none.enabled,
                none.learned_entries,
                none.user_dict_entries,
                none.suppressed_entries
            ),
            (true, 3, 2, 1)
        );
        assert_eq!(none.last_saved_unix, None);
        std::fs::write(dir.join("learning.tsv"), "x").unwrap();
        let some = learning_stats(false, 0, 0, 0, Some(&dir));
        assert!(some.last_saved_unix.is_some_and(|t| t > 1_600_000_000));
        assert_eq!(learning_stats(true, 0, 0, 0, None).last_saved_unix, None);
    }

    #[test]
    fn error_messages_do_not_contain_input() {
        let dir = temp_dir();
        let e = entry("ひみつ", "秘密の語", Some("まちがい"), None);
        let err = add(&dir, &add_params(e.clone(), None), 100).unwrap_err();
        let ToolError::Invalid(m) = err else { panic!() };
        assert!(!m.contains("秘密") && !m.contains("ひみつ"));
        add(
            &dir,
            &add_params(entry("ひみつ", "秘密の語", None, None), None),
            100,
        )
        .unwrap();
        let err = add(
            &dir,
            &add_params(entry("ひみつ", "秘密の語", None, None), None),
            100,
        )
        .unwrap_err();
        let ToolError::Invalid(m) = err else { panic!() };
        assert!(!m.contains("秘密") && !m.contains("ひみつ"));
    }
}
