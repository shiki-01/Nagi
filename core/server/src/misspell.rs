//! 誤り辞書（読みの段階の書き間違い → 正しい読み）。
//!
//! 曖昧入力（[`crate::fuzzy`]）が、編集距離の探索や採否の判定を通さず、決定的に「もしかして」候補を
//! 出すための対応表。同梱のリスト（`config/misspellings.tsv`）と、利用者が追記するファイル
//! （`<user-dir>/misspellings.tsv`）の 2 層からなり、同じ誤った読みは利用者側の対応が先に並ぶ。
//!
//! 形式は UTF-8 の TSV で、`誤った読み<TAB>正しい読み[<TAB>備考]`。`#` と `!` で始まる行と空行は
//! 無視する。読みはカタカナをひらがなに直して扱う。

use nagi_dict::to_hiragana;
use nagi_fuzzy::{MAX_READING_CHARS, MIN_READING_CHARS};
use std::collections::HashMap;
use std::path::Path;

/// 同梱のリスト（`config/misspellings.tsv`）
const BUILTIN_TSV: &str = include_str!("../../../config/misspellings.tsv");

/// 同梱のリストの表示名（問題の通知に使う）
pub const BUILTIN_NAME: &str = "misspellings.tsv（同梱）";

/// 利用者の追記ファイル名（`<user-dir>` 直下）
pub const USER_FILE_NAME: &str = "misspellings.tsv";

/// 読み込み時の問題（その行は無視される）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MisspellIssue {
    /// ファイル名
    pub file: String,
    /// 1 始まりの行番号（行に結び付かない問題は 0）
    pub line: usize,
    /// 内容
    pub message: String,
}

/// 誤り辞書。誤った読み → 正しい読み（行の順。重複なし）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Misspellings {
    map: HashMap<String, Vec<String>>,
}

/// ひらがな・長音符だけで構成され、空でないか
fn is_kana_reading(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| matches!(c, '\u{3041}'..='\u{3096}') || c == 'ー')
}

impl Misspellings {
    /// 空の誤り辞書
    pub fn empty() -> Self {
        Self::default()
    }

    /// TSV のテキストを解析する。不正な行は `issues` に記録して読み飛ばす
    pub fn parse(file: &str, text: &str, issues: &mut Vec<MisspellIssue>) -> Self {
        let mut out = Misspellings::default();
        for (i, raw) in text.split('\n').enumerate() {
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let mut problem = |message: String| {
                issues.push(MisspellIssue {
                    file: file.to_string(),
                    line: i + 1,
                    message,
                });
            };
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 2 {
                problem("列が 2 つ未満です（誤った読みと正しい読みが必要です）".to_string());
                continue;
            }
            let wrong = to_hiragana(cols[0].trim());
            let right = to_hiragana(cols[1].trim());
            if !is_kana_reading(&wrong) || !is_kana_reading(&right) {
                problem("読みはひらがな（と長音符）だけで書く必要があります".to_string());
                continue;
            }
            if wrong == right {
                problem("誤った読みと正しい読みが同じです".to_string());
                continue;
            }
            let chars = wrong.chars().count();
            if !(MIN_READING_CHARS..=MAX_READING_CHARS).contains(&chars) {
                problem(format!(
                    "誤った読みは {MIN_READING_CHARS} 〜 {MAX_READING_CHARS} 文字である必要があります"
                ));
                continue;
            }
            out.add(wrong, right);
        }
        out
    }

    /// 対応を末尾に足す（同じ対応がすでにあれば何もしない）
    fn add(&mut self, wrong: String, right: String) {
        let list = self.map.entry(wrong).or_default();
        if !list.contains(&right) {
            list.push(right);
        }
    }

    /// 同梱のリスト。同梱ファイルに問題があれば `issues` に入る（テストで 0 件を保証する）
    pub fn builtin(issues: &mut Vec<MisspellIssue>) -> Self {
        Self::parse(BUILTIN_NAME, BUILTIN_TSV, issues)
    }

    /// 利用者の追記 `user_path` を同梱のリストの前に重ねて読む。ファイルが無ければ同梱のみ。
    /// 読めない・不正な行は問題として返す
    pub fn load(user_path: Option<&Path>) -> (Self, Vec<MisspellIssue>) {
        let mut issues = Vec::new();
        let mut merged = Misspellings::default();
        if let Some(path) = user_path.filter(|p| p.is_file()) {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(USER_FILE_NAME)
                .to_string();
            match std::fs::read(path) {
                Ok(bytes) => {
                    let text = nagi_dict::decode_text(&bytes);
                    merged = Self::parse(&name, &text, &mut issues);
                }
                Err(e) => issues.push(MisspellIssue {
                    file: name,
                    line: 0,
                    message: format!("読み込めません: {e}"),
                }),
            }
        }
        let builtin = Self::builtin(&mut issues);
        merged.extend(builtin);
        (merged, issues)
    }

    /// `other` の対応を、自分の対応の後ろに足す（同じ誤った読みは自分の側が先）
    fn extend(&mut self, other: Misspellings) {
        for (wrong, rights) in other.map {
            for right in rights {
                self.add(wrong.clone(), right);
            }
        }
    }

    /// 誤った読み `reading` に対する正しい読み（先頭ほど優先）。無ければ空
    pub fn lookup(&self, reading: &str) -> &[String] {
        self.map.get(reading).map_or(&[], Vec::as_slice)
    }

    /// 誤った読みの数
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 全対応（順不同）
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Misspellings, Vec<MisspellIssue>) {
        let mut issues = Vec::new();
        (Misspellings::parse("t.tsv", text, &mut issues), issues)
    }

    #[test]
    fn comments_blank_lines_and_notes_are_handled() {
        let (m, issues) =
            parse("# コメント\n! ヘッダ\n\nふいんき\tふんいき\t備考\nあらたしい\tあたらしい\n\r\n");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(m.len(), 2);
        assert_eq!(m.lookup("ふいんき"), ["ふんいき"]);
        assert_eq!(m.lookup("あらたしい"), ["あたらしい"]);
        assert!(m.lookup("ない").is_empty());
    }

    #[test]
    fn katakana_is_normalized_to_hiragana() {
        let (m, issues) = parse("フインキ\tフンイキ\nしゅみれーしょん\tシミュレーション\n");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(m.lookup("ふいんき"), ["ふんいき"]);
        assert_eq!(m.lookup("しゅみれーしょん"), ["しみゅれーしょん"]);
    }

    #[test]
    fn invalid_lines_are_skipped_with_line_numbers() {
        let long = "ながい".repeat(9);
        let text = format!(
            "ふいんき\tふんいき\nひとつだけ\nあいう\tabc\nあいう\tあいう\nああ\tあいう\nあ\tいう\n\
             ほげほげ\t漢字\n{long}\tあ\nちょと\tちょっと\n"
        );
        let (m, issues) = parse(&text);
        let lines: Vec<usize> = issues.iter().map(|i| i.line).collect();
        assert_eq!(lines, [2, 3, 4, 5, 6, 7, 8], "{issues:?}");
        assert!(issues.iter().all(|i| i.file == "t.tsv"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn length_bounds_apply_to_the_wrong_reading() {
        let ok24 = "あ".repeat(24);
        let ng25 = "あ".repeat(25);
        let (m, issues) = parse(&format!("あいう\tえお\n{ok24}\tい\n{ng25}\tい\nあい\tう\n"));
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn multiple_corrections_keep_order_without_duplicates() {
        let (m, issues) = parse("かんし\tかんじ\nかんし\tかんち\nかんし\tかんじ\n");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(m.lookup("かんし"), ["かんじ", "かんち"]);
    }

    #[test]
    fn load_puts_user_entries_before_builtin() {
        let dir = std::env::temp_dir().join(format!("nagi-misspell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(USER_FILE_NAME);
        std::fs::write(&path, "あらたしい\tゆーざー\nゆーざーだけ\tゆーざー\n").unwrap();
        let (m, issues) = Misspellings::load(Some(&path));
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(m.lookup("あらたしい"), ["ゆーざー", "あたらしい"]);
        assert_eq!(m.lookup("ゆーざーだけ"), ["ゆーざー"]);
        // ファイルが無ければ同梱のみ
        let (builtin_only, _) = Misspellings::load(Some(&dir.join("none.tsv")));
        assert_eq!(builtin_only.lookup("あらたしい"), ["あたらしい"]);
        let (none, _) = Misspellings::load(None);
        assert_eq!(none, builtin_only);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builtin_list_is_valid_and_has_unique_wrong_readings() {
        let mut issues = Vec::new();
        let m = Misspellings::builtin(&mut issues);
        assert!(issues.is_empty(), "{issues:?}");
        assert!((40..=80).contains(&m.len()), "件数: {}", m.len());
        // 重複する誤った読みが無い（行数と一意な誤った読みの数が一致する）
        let rows = BUILTIN_TSV
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#') && !l.starts_with('!'))
            .count();
        assert_eq!(rows, m.len(), "誤った読みが重複している");
    }
}
