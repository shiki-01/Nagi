//! 英字入力で利用者が入力した英単語の学習（H-146）。
//!
//! 英字入力の予測候補・スペル訂正（`nagi_english::assist`）が、利用者のよく使う語（技術用語・固有名詞）を
//! 補完の先頭に出し、正しい語として訂正の対象から外すために使う。`<user-dir>/english_words.tsv` に保存する。
//!
//! ファイルの形: 1 行目 `# nagi english_words v1`、以降 `表記<TAB>回数`。表記の小文字が鍵で、同じ語を
//! 違う大文字小文字で入力したときは最後の表記を残す。不正な行は読み飛ばす。
//!
//! 学習は通常の学習と同じ条件で止める（`learning.enabled`・停止リストのアプリ・secure input）。
//! 「学習データの全削除」（`clear_learning`）で消える。設定バックアップの対象には含めない（項目の拡張が
//! 要るため。app_modes.tsv と同じ扱い）。
//!
//! 件数は [`MAX_WORDS`] まで。超えたときは回数の最も少ない語（同じなら鍵の順で後ろ）を落とす。

use nagi_english::LearnedWords;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// 保存ファイル名（`<user-dir>` 直下）
pub const FILE_NAME: &str = "english_words.tsv";

/// ヘッダ行（形式の版を含む）
const HEADER: &str = "# nagi english_words v1";

/// 記憶する語の最大件数
pub const MAX_WORDS: usize = 5000;

/// 回数の上限（数え続けて桁あふれしないように）
const MAX_COUNT: u32 = 1_000_000;

/// 入力のされ方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitSource {
    /// 打ったまま確定した（1 回と数える）
    Typed,
    /// 候補から選んだ（正しい語とみなせる回数まで上げる）
    Suggestion,
    /// 自動修正を取り消して打ったままに戻した（正しい語とみなせる回数まで上げる）
    Reverted,
}

/// 学習した英単語（小文字の鍵 → (表記, 回数)）
#[derive(Default)]
pub struct EnglishWordStore {
    words: Mutex<BTreeMap<String, (String, u32)>>,
    dirty: AtomicBool,
}

impl EnglishWordStore {
    /// 空
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, (String, u32)>> {
        self.words.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// ファイルから読んで置き換える。無い・ヘッダが違うときは何もしない。読めたら `true`
    pub fn load(&self, path: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let mut lines = text.lines();
        if lines.next().map(str::trim_end) != Some(HEADER) {
            return false;
        }
        let mut words = BTreeMap::new();
        for line in lines {
            let Some((display, count)) = line.split_once('\t') else {
                continue;
            };
            let Ok(count) = count.trim().parse::<u32>() else {
                continue;
            };
            if !nagi_english::assist::is_word(display) || count == 0 || words.len() >= MAX_WORDS {
                continue;
            }
            words.insert(
                display.to_ascii_lowercase(),
                (display.to_string(), count.min(MAX_COUNT)),
            );
        }
        *self.lock() = words;
        self.dirty.store(false, Ordering::Relaxed);
        true
    }

    /// 語 `word` の入力を記録する。英単語として扱えない語（数字・記号を含むなど）は記録しない
    pub fn record(&self, word: &str, source: CommitSource) {
        if !nagi_english::assist::is_word(word) {
            return;
        }
        let key = word.to_ascii_lowercase();
        let mut words = self.lock();
        let entry = words.entry(key.clone()).or_insert((word.to_string(), 0));
        entry.0 = word.to_string();
        entry.1 = match source {
            CommitSource::Typed => entry.1.saturating_add(1),
            CommitSource::Suggestion | CommitSource::Reverted => entry
                .1
                .saturating_add(1)
                .max(nagi_english::assist::LEARNED_KNOWN_COUNT),
        }
        .min(MAX_COUNT);
        if words.len() > MAX_WORDS
            && let Some(old) = words
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by(|a, b| a.1.1.cmp(&b.1.1).then(b.0.cmp(a.0)))
                .map(|(k, _)| k.clone())
        {
            words.remove(&old);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 語を忘れる（候補の長押しの「学習から削除」など）
    pub fn forget(&self, word: &str) {
        if self.lock().remove(&word.to_ascii_lowercase()).is_some() {
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// 件数
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// 全て消す（次の flush でファイルも消える）
    pub fn clear(&self) {
        self.lock().clear();
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 保存していない変更があるか
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// 変更があるときだけ書く。空ならファイルを消す。一時ファイル → rename で原子的に書く
    pub fn flush(&self, path: &Path) -> std::io::Result<()> {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return Ok(());
        }
        let words = self.lock().clone();
        let result = if words.is_empty() {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        } else {
            save(path, &words)
        };
        if result.is_err() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result
    }
}

impl LearnedWords for EnglishWordStore {
    fn get(&self, lower: &str) -> Option<(String, u32)> {
        self.lock().get(lower).cloned()
    }

    fn with_prefix(&self, prefix: &str, limit: usize) -> Vec<(String, u32)> {
        let words = self.lock();
        let mut found: Vec<(String, u32)> = words
            .range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .filter(|(k, _)| k.as_str() != prefix)
            .map(|(_, v)| v.clone())
            .collect();
        found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.len().cmp(&b.0.len())));
        found.truncate(limit);
        found
    }
}

fn save(path: &Path, words: &BTreeMap<String, (String, u32)>) -> std::io::Result<()> {
    let mut text = String::new();
    text.push_str(HEADER);
    text.push('\n');
    for (display, count) in words.values() {
        text.push_str(display);
        text.push('\t');
        text.push_str(&count.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nagi-english-words-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn record_counts_and_keeps_latest_display() {
        let s = EnglishWordStore::new();
        s.record("kubectl", CommitSource::Typed);
        assert_eq!(s.get("kubectl"), Some(("kubectl".into(), 1)));
        s.record("iPhone", CommitSource::Suggestion);
        assert_eq!(s.get("iphone"), Some(("iPhone".into(), 2)));
        s.record("Kubectl", CommitSource::Typed);
        assert_eq!(s.get("kubectl"), Some(("Kubectl".into(), 2)));
        s.record("a1", CommitSource::Typed);
        s.record("", CommitSource::Typed);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn prefix_is_ordered_by_count() {
        let s = EnglishWordStore::new();
        for _ in 0..3 {
            s.record("kubernetes", CommitSource::Typed);
        }
        s.record("kubectl", CommitSource::Typed);
        s.record("kub", CommitSource::Typed);
        let found = s.with_prefix("kub", 5);
        assert_eq!(found, vec![("kubernetes".into(), 3), ("kubectl".into(), 1)]);
    }

    #[test]
    fn flush_load_and_clear_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join(FILE_NAME);
        let s = EnglishWordStore::new();
        s.record("serde", CommitSource::Reverted);
        s.flush(&path).unwrap();
        assert!(!s.is_dirty());
        let t = EnglishWordStore::new();
        assert!(t.load(&path));
        assert_eq!(t.get("serde"), Some(("serde".into(), 2)));
        t.clear();
        t.flush(&path).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn evicts_least_used_over_limit() {
        let s = EnglishWordStore::new();
        s.record("keep", CommitSource::Suggestion);
        for i in 0..MAX_WORDS {
            let w: String = format!("w{i}")
                .chars()
                .map(|c| {
                    if c.is_ascii_digit() {
                        (b'a' + (c as u8 - b'0')) as char
                    } else {
                        c
                    }
                })
                .collect();
            s.record(&w, CommitSource::Typed);
        }
        assert_eq!(s.len(), MAX_WORDS);
        assert!(s.get("keep").is_some());
    }

    #[test]
    fn forget_removes() {
        let s = EnglishWordStore::new();
        s.record("tokio", CommitSource::Typed);
        s.forget("Tokio");
        assert!(s.is_empty());
    }
}
