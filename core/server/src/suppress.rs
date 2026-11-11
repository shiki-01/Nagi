//! 抑制語（「この読みでこの表記は候補に出さない」という利用者の登録）。
//!
//! 誤変換を、学習の重みづけではなく確実に候補から消すための受け皿。`(読み, 表記)` の組を
//! 利用者のファイル（`<user-dir>/suppressed.tsv`）に持ち、変換パイプラインが候補の統合直後に
//! 該当するものを除く。読みと表記の両方が一致したときだけ除くので、同じ表記でも別の読みには効かない。
//!
//! 形式は UTF-8 の TSV で、`読み<TAB>表記`。`#` と `!` で始まる行と空行は無視する。
//! 読みはカタカナをひらがなに直して扱う。3 列目以降は無視する。
//! 保存（[`Suppressions::save`]）はコメントを保持せず、読み・表記の順に並べ直して書き出す。
//!
//! プライバシー: 抑制リストは利用者が入力した語を含むので、ログや通知には内容を出さない
//! （読み込みの問題は行番号と理由だけを返す）。また、利用者が明示した設定なので、
//! 学習データの全削除（`clear_learning`）の対象には含めない。

use nagi_dict::to_hiragana;
use std::collections::BTreeSet;
use std::path::Path;

/// 利用者の抑制語ファイル名（`<user-dir>` 直下）
pub const USER_FILE_NAME: &str = "suppressed.tsv";

/// 読み込み時の問題（その行は無視される）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressIssue {
    /// ファイル名
    pub file: String,
    /// 1 始まりの行番号（行に結び付かない問題は 0）
    pub line: usize,
    /// 内容（登録内容は含めない）
    pub message: String,
}

/// 抑制語の集合。`(ひらがなの読み, 表記)` の組（読み・表記の順に並ぶ。重複なし）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Suppressions {
    set: BTreeSet<(String, String)>,
}

/// ひらがな・長音符だけで構成され、空でないか
fn is_kana_reading(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| matches!(c, '\u{3041}'..='\u{3096}') || c == 'ー')
}

/// 読みをひらがなに直し、読み・表記の妥当性を確かめる。不正なら理由（登録内容を含まない）を返す
fn normalize(reading: &str, surface: &str) -> Result<(String, String), &'static str> {
    let reading = to_hiragana(reading.trim());
    if !is_kana_reading(&reading) {
        return Err("読みはひらがな（と長音符）だけで書く必要があります");
    }
    if surface.is_empty() || surface.chars().any(char::is_control) {
        return Err("表記が空、または制御文字を含んでいます");
    }
    if surface == reading {
        return Err("読みと同じ表記は抑制できません（候補が空になるため）");
    }
    Ok((reading, surface.to_string()))
}

impl Suppressions {
    /// 空の集合
    pub fn empty() -> Self {
        Self::default()
    }

    /// TSV のテキストを解析する。不正な行は `issues` に記録して読み飛ばす
    pub fn parse(file: &str, text: &str, issues: &mut Vec<SuppressIssue>) -> Self {
        let mut out = Suppressions::default();
        for (i, raw) in text.split('\n').enumerate() {
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let mut problem = |message: &str| {
                issues.push(SuppressIssue {
                    file: file.to_string(),
                    line: i + 1,
                    message: message.to_string(),
                });
            };
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 2 {
                problem("列が 2 つ未満です（読みと表記が必要です）");
                continue;
            }
            match normalize(cols[0], cols[1]) {
                Ok((reading, surface)) => {
                    out.set.insert((reading, surface));
                }
                Err(message) => problem(message),
            }
        }
        out
    }

    /// ファイルから読む。ファイルが無ければ空（問題なし）。読めない場合は問題として返す
    pub fn load(path: &Path) -> (Self, Vec<SuppressIssue>) {
        let mut issues = Vec::new();
        if !path.is_file() {
            return (Self::empty(), issues);
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(USER_FILE_NAME)
            .to_string();
        match std::fs::read(path) {
            Ok(bytes) => {
                let text = nagi_dict::decode_text(&bytes);
                (Self::parse(&name, &text, &mut issues), issues)
            }
            Err(e) => {
                issues.push(SuppressIssue {
                    file: name,
                    line: 0,
                    message: format!("読み込めません: {e}"),
                });
                (Self::empty(), issues)
            }
        }
    }

    /// ファイルに書く（一時ファイル経由で原子的に）
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut text =
            String::from("# 抑制語: 読み<TAB>表記（この読みでこの表記は候補に出しません）\n");
        for (reading, surface) in &self.set {
            text.push_str(reading);
            text.push('\t');
            text.push_str(surface);
            text.push('\n');
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut temp = path.to_path_buf();
        temp.set_extension("tmp");
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, path)
    }

    /// `(読み, 表記)` が登録済みか。`reading` はひらがな済みであること（変換の経路は常にそう）
    pub fn contains(&self, reading: &str, surface: &str) -> bool {
        !self.set.is_empty()
            && self
                .set
                .contains(&(reading.to_string(), surface.to_string()))
    }

    /// 読みを問わず、その表記がどれかの読みで登録されているか（読みが分からない予測変換用）
    pub fn contains_surface(&self, surface: &str) -> bool {
        self.set.iter().any(|(_, s)| s == surface)
    }

    /// 登録する。新規なら `Ok(true)`、すでにあれば `Ok(false)`、不正なら理由を返す
    pub fn add(&mut self, reading: &str, surface: &str) -> Result<bool, &'static str> {
        let (reading, surface) = normalize(reading, surface)?;
        Ok(self.set.insert((reading, surface)))
    }

    /// 解除する。登録があって除けたら `true`
    pub fn remove(&mut self, reading: &str, surface: &str) -> bool {
        let reading = to_hiragana(reading.trim());
        self.set.remove(&(reading, surface.to_string()))
    }

    /// 全件（読み・表記の順）
    pub fn list(&self) -> Vec<(String, String)> {
        self.set.iter().cloned().collect()
    }

    /// 件数
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Suppressions, Vec<SuppressIssue>) {
        let mut issues = Vec::new();
        (Suppressions::parse("t.tsv", text, &mut issues), issues)
    }

    #[test]
    fn comments_blank_lines_and_extra_columns_are_handled() {
        let (s, issues) = parse("# コメント\n! ヘッダ\n\nきしゃ\t汽車\t備考\r\nかんじ\t幹事\n");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(s.len(), 2);
        assert!(s.contains("きしゃ", "汽車"));
        assert!(s.contains("かんじ", "幹事"));
    }

    #[test]
    fn reading_is_normalized_to_hiragana() {
        let (s, issues) = parse("キシャ\t汽車\n");
        assert!(issues.is_empty(), "{issues:?}");
        assert!(s.contains("きしゃ", "汽車"));
        assert!(!s.contains("きしゃ", "記者"), "別の表記には効かない");
        assert!(!s.contains("きしゃー", "汽車"), "別の読みには効かない");
    }

    #[test]
    fn invalid_lines_are_skipped_with_line_numbers() {
        let (s, issues) =
            parse("きしゃ\t汽車\nひとつだけ\nabc\t汽車\nきしゃ\t\nきしゃ\tきしゃ\nきしゃ\t記者\n");
        let lines: Vec<usize> = issues.iter().map(|i| i.line).collect();
        assert_eq!(lines, [2, 3, 4, 5], "{issues:?}");
        assert!(issues.iter().all(|i| i.file == "t.tsv"));
        // 問題の文言に登録内容を含めない
        assert!(issues.iter().all(|i| !i.message.contains("汽車")));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn add_remove_list() {
        let mut s = Suppressions::empty();
        assert_eq!(s.add("キシャ", "汽車"), Ok(true));
        assert_eq!(s.add("きしゃ", "汽車"), Ok(false), "重複は追加しない");
        assert!(s.add("きしゃ", "きしゃ").is_err());
        assert!(s.add("kisha", "汽車").is_err());
        assert_eq!(s.list(), [("きしゃ".to_string(), "汽車".to_string())]);
        assert!(s.contains_surface("汽車"));
        assert!(!s.contains_surface("記者"));
        assert!(s.remove("キシャ", "汽車"));
        assert!(!s.remove("きしゃ", "汽車"));
        assert!(s.is_empty());
    }

    #[test]
    fn save_and_load_round_trip_atomically() {
        let dir = std::env::temp_dir().join(format!("nagi-suppress-{}", std::process::id()));
        let path = dir.join(USER_FILE_NAME);
        let mut s = Suppressions::empty();
        s.add("きしゃ", "汽車").unwrap();
        s.add("かんじ", "幹事").unwrap();
        s.save(&path).unwrap();
        assert!(
            !path.with_extension("tmp").exists(),
            "一時ファイルは残らない"
        );
        let (loaded, issues) = Suppressions::load(&path);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(loaded, s);
        // ファイルが無ければ空
        let (none, issues) = Suppressions::load(&dir.join("none.tsv"));
        assert!(none.is_empty() && issues.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
