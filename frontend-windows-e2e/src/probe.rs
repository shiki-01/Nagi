//! IME 側のプローブファイル（`NAGI_E2E_PROBE`。`frontend-windows-tsf/src/e2e_probe.rs`）の読み取り。
//! 1 行 1 JSON: `{"marked": <string|null>, "committed_delta": <string>}`。

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
struct Line {
    marked: Option<String>,
    event: Option<String>,
    detail: Option<String>,
    #[serde(default)]
    committed_delta: String,
}

/// プローブファイルの追い読み。最後に観測した未確定文字列と、確定の累計を持つ
#[derive(Debug)]
pub struct ProbeReader {
    path: PathBuf,
    offset: u64,
    marked: String,
    committed: String,
    lines: u64,
    events: std::collections::HashMap<String, u64>,
    last_has_preedit: Option<bool>,
    tail: std::collections::VecDeque<String>,
}

impl ProbeReader {
    /// `path` を末尾（現在の長さ）から読み始める
    pub fn new(path: &Path) -> Self {
        let mut reader = Self {
            path: path.to_path_buf(),
            offset: 0,
            marked: String::new(),
            committed: String::new(),
            lines: 0,
            events: std::collections::HashMap::new(),
            last_has_preedit: None,
            tail: std::collections::VecDeque::new(),
        };
        reader.reset();
        reader
    }

    /// 既に書かれた分を読み飛ばし、状態を空に戻す（シナリオの開始時）
    pub fn reset(&mut self) {
        self.offset = std::fs::metadata(&self.path).map_or(0, |m| m.len());
        self.marked.clear();
        self.committed.clear();
    }

    /// 新しい行を取り込む。改行まで揃っていない末尾は次回に回す
    pub fn poll(&mut self) {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return;
        }
        let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
            return;
        };
        for raw in buf[..end].split(|&b| b == b'\n') {
            if self.tail.len() >= 60 {
                self.tail.pop_front();
            }
            self.tail
                .push_back(String::from_utf8_lossy(raw).into_owned());
            if let Ok(line) = serde_json::from_slice::<Line>(raw) {
                self.lines += 1;
                if let Some(ev) = &line.event {
                    *self.events.entry(ev.clone()).or_insert(0) += 1;
                    if ev == "engine_result"
                        && let Some(d) = &line.detail
                    {
                        if d.contains("has_preedit=true") {
                            self.last_has_preedit = Some(true);
                        } else if d.contains("has_preedit=false") {
                            self.last_has_preedit = Some(false);
                        }
                    }
                }
                if let Some(m) = line.marked {
                    self.marked = m;
                }
                self.committed.push_str(&line.committed_delta);
            }
        }
        self.offset += (end + 1) as u64;
    }

    /// 最後に観測した未確定文字列
    pub fn marked(&self) -> &str {
        &self.marked
    }

    /// IME が報告した確定文字列の累計（診断用。検証にはテキスト欄の内容を使う）
    pub fn committed_total(&self) -> &str {
        &self.committed
    }

    /// イベント行（`event`）の累計件数
    pub fn event_count(&self, name: &str) -> u64 {
        self.events.get(name).copied().unwrap_or(0)
    }

    /// 最後の `engine_result` の `has_preedit`
    pub fn last_has_preedit(&self) -> Option<bool> {
        self.last_has_preedit
    }

    /// 直近に取り込んだ行（診断用。最大 60 行）
    pub fn tail(&self) -> String {
        self.tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    /// これまでに取り込んだ行数（プローブが動いているかの診断用）
    pub fn line_count(&self) -> u64 {
        self.lines
    }
}

/// テキスト欄の全文から、確定済みの部分を取り出す。
/// 未確定文字列が欄の内容にインライン表示されている（末尾が `marked`）ならそれを除く。
/// 表示されていない場合は全文を確定済みとして扱う
pub fn committed_text(full: &str, marked: &str) -> String {
    if marked.is_empty() {
        return full.to_owned();
    }
    match full.strip_suffix(marked) {
        Some(head) => head.to_owned(),
        None => full.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("nagi-e2e-probe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn follows_appended_lines_and_keeps_partial_tail() {
        let path = temp("follow");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"marked":"old","committed_delta":""}}"#).unwrap();
        let mut r = ProbeReader::new(&path);
        assert_eq!(r.marked(), "");
        write!(f, r#"{{"marked":"こん","committed_delta":""}}"#).unwrap();
        r.poll();
        assert_eq!(r.marked(), "", "改行が無い行は読まない");
        writeln!(f).unwrap();
        writeln!(f, r#"{{"marked":null,"committed_delta":"あ"}}"#).unwrap();
        writeln!(f, r#"{{"marked":"","committed_delta":"い"}}"#).unwrap();
        r.poll();
        assert_eq!(r.marked(), "");
        assert_eq!(r.committed_total(), "あい");
        assert_eq!(r.line_count(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_empty() {
        let mut r = ProbeReader::new(&temp("missing"));
        r.poll();
        assert_eq!(r.marked(), "");
    }

    #[test]
    fn strips_inline_marked_text() {
        assert_eq!(committed_text("はこんに", "こんに"), "は");
        assert_eq!(committed_text("は", "こんに"), "は");
        assert_eq!(committed_text("は", ""), "は");
    }
}
