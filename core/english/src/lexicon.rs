//! 英単語の語彙: 単語（小文字の ASCII）→ 頻度の段階（小さいほど一般的）の fst 索引。
//!
//! 値の下位 8 ビットが段階、その上のビットが表記の印（[`CASE_CAPITALIZED`]・[`CASE_UPPER`]。SCOWL で
//! 大文字の表記しか無い語。`Monday`・`NASA`）。印の無い古いファイルもそのまま読める（全部小文字として扱う）。
//!
//! ファイルは fst の `Map` のバイト列そのもの。非 Windows では mmap で読む（`nagi-dict` と同じ方針）ので、
//! 更新は別 inode への rename で行うこと（`nagi-datafetch` がそうする）。

use fst::{Map, MapBuilder};
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

/// 値の印: 先頭だけ大文字の表記しか無い語（`Monday`・`Tokyo`）
pub const CASE_CAPITALIZED: u64 = 1 << 8;
/// 値の印: 全部大文字の表記しか無い語（`NASA`）
pub const CASE_UPPER: u64 = 1 << 9;

/// 語の表記の形
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordCase {
    /// 小文字の表記がある
    Lower,
    /// 先頭だけ大文字
    Capitalized,
    /// 全部大文字
    Upper,
}

impl WordCase {
    fn from_value(v: u64) -> WordCase {
        if v & CASE_UPPER != 0 {
            WordCase::Upper
        } else if v & CASE_CAPITALIZED != 0 {
            WordCase::Capitalized
        } else {
            WordCase::Lower
        }
    }

    /// 小文字の語 `lower` にこの表記の形を当てる
    pub fn render(self, lower: &str) -> String {
        match self {
            WordCase::Lower => lower.to_string(),
            WordCase::Upper => lower.to_ascii_uppercase(),
            WordCase::Capitalized => {
                let mut c = lower.chars();
                match c.next() {
                    Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                    None => String::new(),
                }
            }
        }
    }
}

/// 語彙の読み書きのエラー
#[derive(Debug, Error)]
pub enum LexiconError {
    /// ファイル入出力
    #[error("語彙ファイルの入出力に失敗しました: {0}")]
    Io(#[from] std::io::Error),
    /// fst として不正
    #[error("語彙ファイルが不正です: {0}")]
    Fst(#[from] fst::Error),
}

/// 語彙ファイル全体のバイト列の持ち主（メモリ上の `Vec` か、mmap）
pub(crate) enum Backing {
    Owned(Vec<u8>),
    #[cfg(not(windows))]
    Mapped(memmap2::Mmap),
}

impl AsRef<[u8]> for Backing {
    fn as_ref(&self) -> &[u8] {
        match self {
            Backing::Owned(v) => v,
            #[cfg(not(windows))]
            Backing::Mapped(m) => m,
        }
    }
}

/// 読み込み済みの英単語語彙（不変。`Send + Sync`）
pub struct Lexicon {
    map: Map<Backing>,
}

impl std::fmt::Debug for Lexicon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lexicon")
            .field("words", &self.map.len())
            .finish()
    }
}

impl Lexicon {
    /// 語彙ファイルを読み込む。非 Windows では mmap し、失敗したら全量を読む
    pub fn load(path: &Path) -> Result<Lexicon, LexiconError> {
        #[cfg(not(windows))]
        if let Some(mapped) = map_file(path)
            && let Ok(map) = Map::new(Backing::Mapped(mapped))
        {
            return Ok(Lexicon { map });
        }
        Lexicon::from_vec(std::fs::read(path)?)
    }

    /// バイト列から読み込む
    pub fn from_vec(bytes: Vec<u8>) -> Result<Lexicon, LexiconError> {
        Ok(Lexicon {
            map: Map::new(Backing::Owned(bytes))?,
        })
    }

    /// 単語の頻度の段階（小さいほど一般的）。語彙に無ければ `None`。小文字の ASCII で引く
    pub fn tier(&self, word: &str) -> Option<u8> {
        self.map.get(word).map(|v| v as u8)
    }

    /// 語の表記の形（大文字の表記しか無い語か）。語彙に無ければ `None`
    pub fn case_of(&self, word: &str) -> Option<WordCase> {
        self.map.get(word).map(WordCase::from_value)
    }

    /// `prefix`（小文字の ASCII）で始まる語を、一般的な順（頻度の段階 → 短い順 → 辞書順）に最大
    /// `limit` 件返す。`prefix` 自体は含めない。`(語, 段階)`
    pub fn complete(&self, prefix: &str, limit: usize) -> Vec<(String, u8)> {
        use fst::raw::Output;
        if limit == 0 || prefix.is_empty() {
            return Vec::new();
        }
        let fst = self.fst();
        let mut node = fst.root();
        let mut out = Output::zero();
        for &b in prefix.as_bytes() {
            let Some(i) = node.find_input(b) else {
                return Vec::new();
            };
            let t = node.transition(i);
            out = out.cat(t.out);
            node = fst.node(t.addr);
        }
        // 部分木の全語を集めて上位を残す（1 文字の接頭辞でも語彙の 1 割程度で、数ミリ秒）
        let mut found: Vec<(Vec<u8>, u8)> = Vec::new();
        let mut word = prefix.as_bytes().to_vec();
        let mut stack = vec![(node.addr(), out, word.len(), None::<u8>)];
        while let Some((addr, out, depth, input)) = stack.pop() {
            word.truncate(depth);
            if let Some(b) = input {
                word.push(b);
            }
            let node = fst.node(addr);
            if node.is_final() && word.len() > prefix.len() {
                let tier = out.cat(node.final_output()).value() as u8;
                found.push((word.clone(), tier));
            }
            for t in node.transitions() {
                stack.push((t.addr, out.cat(t.out), word.len(), Some(t.inp)));
            }
        }
        found.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(a.0.len().cmp(&b.0.len()))
                .then(a.0.cmp(&b.0))
        });
        found.truncate(limit);
        found
            .into_iter()
            .map(|(w, tier)| (String::from_utf8_lossy(&w).into_owned(), tier))
            .collect()
    }

    /// fst 本体（綴りの候補の探索でたどる）
    pub(crate) fn fst(&self) -> &fst::raw::Fst<Backing> {
        self.map.as_fst()
    }

    /// 単語数
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// 語彙ファイルを mmap する。失敗したら `None`（呼び出し側が全量読み込みに戻す）
#[cfg(not(windows))]
#[allow(unsafe_code)]
fn map_file(path: &Path) -> Option<memmap2::Mmap> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: 読み取り専用で開く。マップ中に同じ inode が書き換えられると SIGBUS になりうるため、
    // 更新は別 inode への rename で行う運用とする（モジュール先頭の注意）
    unsafe { memmap2::Mmap::map(&file) }.ok()
}

/// 語彙の組み立て
#[derive(Debug, Default)]
pub struct LexiconBuilder {
    /// 小文字の語 → (段階, 表記の印。小文字の表記を見たら 0)
    words: BTreeMap<String, (u8, u64)>,
}

impl LexiconBuilder {
    /// 空の組み立て器
    pub fn new() -> Self {
        Self::default()
    }

    /// 単語を足す。小文字の ASCII 英字だけの語（2 文字以上）のみ受け付け、受け付けたら `true`。
    /// 同じ語が複数あれば、小さい（一般的な）段階を採る
    pub fn add(&mut self, word: &str, tier: u8) -> bool {
        if word.len() < 2 || !word.bytes().all(|b| b.is_ascii_lowercase()) {
            return false;
        }
        self.insert(word.to_string(), tier, 0);
        true
    }

    /// 大文字を含みうる語を足す（SCOWL の `english-upper` など）。先頭だけ大文字（`Monday`）・全部大文字
    /// （`NASA`）は表記の印を付け、小文字にして登録する。それ以外の混ざり方（`McDonald`）は受け付けない。
    /// 同じ語の小文字の表記を見たら印は消える（`polish` と `Polish`）
    pub fn add_cased(&mut self, word: &str, tier: u8) -> bool {
        if word.len() < 2 || !word.bytes().all(|b| b.is_ascii_alphabetic()) {
            return false;
        }
        let bytes = word.as_bytes();
        let flag = if bytes.iter().all(|b| b.is_ascii_lowercase()) {
            0
        } else if bytes.iter().all(|b| b.is_ascii_uppercase()) {
            CASE_UPPER
        } else if bytes[0].is_ascii_uppercase() && bytes[1..].iter().all(|b| b.is_ascii_lowercase())
        {
            CASE_CAPITALIZED
        } else {
            return false;
        };
        self.insert(word.to_ascii_lowercase(), tier, flag);
        true
    }

    fn insert(&mut self, lower: String, tier: u8, flag: u64) {
        match self.words.get_mut(&lower) {
            Some(slot) => {
                slot.0 = slot.0.min(tier);
                // 小文字の表記が一度でもあれば小文字。大文字どうしは全部大文字を優先しない（先に見た方）
                if flag == 0 {
                    slot.1 = 0;
                }
            }
            None => {
                self.words.insert(lower, (tier, flag));
            }
        }
    }

    /// 単語数
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// fst のバイト列にする
    pub fn build(&self) -> Result<Vec<u8>, LexiconError> {
        let mut builder = MapBuilder::memory();
        for (word, (tier, flag)) in &self.words {
            builder.insert(word, u64::from(*tier) | flag)?;
        }
        Ok(builder.into_inner()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_lookup() {
        let mut b = LexiconBuilder::new();
        assert!(b.add("typescript", 70));
        assert!(b.add("the", 10));
        assert!(b.add("the", 35)); // 小さい段階を採る
        assert!(!b.add("Don't", 10));
        assert!(!b.add("a", 10));
        let lex = Lexicon::from_vec(b.build().unwrap()).unwrap();
        assert_eq!(lex.len(), 2);
        assert_eq!(lex.tier("the"), Some(10));
        assert_eq!(lex.tier("typescript"), Some(70));
        assert_eq!(lex.tier("kyou"), None);
    }

    #[test]
    fn load_maps_the_file() {
        let mut b = LexiconBuilder::new();
        b.add("hello", 10);
        let dir = std::env::temp_dir().join(format!("nagi-english-lex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("words.fst");
        std::fs::write(&path, b.build().unwrap()).unwrap();
        let lex = Lexicon::load(&path).unwrap();
        assert_eq!(lex.tier("hello"), Some(10));
        assert!(Lexicon::load(&dir.join("none.fst")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
