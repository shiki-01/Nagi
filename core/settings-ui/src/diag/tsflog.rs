//! `tsf.log`（`%LOCALAPPDATA%\Nagi\logs\tsf.log`）の解析。
//!
//! 書式は `YYYY-MM-DDTHH:MM:SSZ pid=N [tag] message`（`frontend-windows/src/diag.rs`）。入力内容は
//! 書かれない設計だが、念のため**既知のマーカーに合う行だけ**を証拠として取り出す（他の行は数えない・載せない）。

/// 解析結果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogAnalysis {
    /// 読んだ行数
    pub lines: usize,
    /// `[tsf] Activate done` があった
    pub activate_done: bool,
    /// `Activate done langbar=false`（言語バーの登録に失敗）
    pub langbar_failed: bool,
    /// `AddItem failed` / `ITfLangBarItemMgr unavailable`
    pub add_item_failed: bool,
    /// `launch failed` の回数
    pub launch_failures: usize,
    /// `entering minimal profile`（辞書のみに降格）の回数
    pub minimal_entries: usize,
    /// `stopped (crash or start failure)` の回数
    pub stopped: usize,
    /// `ActivateEx panicked`
    pub panicked: bool,
    /// `[menu] configure requested`（言語のオプションからの起動）
    pub configure_requested: bool,
    /// `[menu] configure failed`
    pub configure_failed: bool,
    /// `[ui] BeginUIElement show=false`（UILess）
    pub uiless: bool,
    /// `dropped non-JSON line`
    pub dropped_non_json: bool,
    /// 証拠の行（マーカーに合ったもののうち問題のあるもの。最大 8 行・各 200 文字）
    pub evidence: Vec<String>,
}

const MAX_EVIDENCE: usize = 8;
const MAX_LINE_CHARS: usize = 200;

fn clip(line: &str) -> String {
    let mut s: String = line.chars().take(MAX_LINE_CHARS).collect();
    if line.chars().count() > MAX_LINE_CHARS {
        s.push('…');
    }
    s
}

/// ログ全文を解析する
pub fn analyze(text: &str) -> LogAnalysis {
    let mut a = LogAnalysis::default();
    let mut problems: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        a.lines += 1;
        let mut problem = false;
        if line.contains("[tsf] Activate done") {
            a.activate_done = true;
            if line.contains("langbar=false") {
                a.langbar_failed = true;
                problem = true;
            }
        }
        if line.contains("AddItem failed") || line.contains("ITfLangBarItemMgr unavailable") {
            a.add_item_failed = true;
            problem = true;
        }
        if line.contains("launch failed") {
            a.launch_failures += 1;
            problem = true;
        }
        if line.contains("entering minimal profile") {
            a.minimal_entries += 1;
            problem = true;
        }
        if line.contains("stopped (crash or start failure)") {
            a.stopped += 1;
            problem = true;
        }
        if line.contains("ActivateEx panicked") {
            a.panicked = true;
            problem = true;
        }
        if line.contains("[menu] configure requested") {
            a.configure_requested = true;
        }
        if line.contains("[menu] configure failed") {
            a.configure_failed = true;
            problem = true;
        }
        if line.contains("[ui] BeginUIElement show=false") {
            a.uiless = true;
        }
        if line.contains("dropped non-JSON line") {
            a.dropped_non_json = true;
            problem = true;
        }
        if problem {
            problems.push(clip(line));
        }
    }
    // 新しいものを優先して残す
    let skip = problems.len().saturating_sub(MAX_EVIDENCE);
    a.evidence = problems.into_iter().skip(skip).collect();
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
2026-11-21T01:00:00Z pid=10 [tsf] Activate client_id=1
2026-11-21T01:00:01Z pid=10 [tsf] Activate done langbar=true compartments=true message_window=true
2026-11-21T01:00:02Z pid=10 [ui] BeginUIElement show=false
2026-11-21T01:00:03Z pid=10 [menu] configure requested
2026-11-21T01:00:04Z pid=11 [engine] launch failed: spawn failed
2026-11-21T01:00:05Z pid=11 [engine] entering minimal profile
2026-11-21T01:00:06Z pid=11 [key] secret typed text should never be here
";

    #[test]
    fn detects_markers_and_keeps_only_problem_lines_as_evidence() {
        let a = analyze(SAMPLE);
        assert_eq!(a.lines, 7);
        assert!(a.activate_done && !a.langbar_failed);
        assert!(a.uiless && a.configure_requested && !a.configure_failed);
        assert_eq!(a.launch_failures, 1);
        assert_eq!(a.minimal_entries, 1);
        assert_eq!(a.evidence.len(), 2);
        assert!(a.evidence.iter().all(|l| !l.contains("secret")));
    }

    #[test]
    fn langbar_failure_and_evidence_cap() {
        let mut text = String::from("t pid=1 [tsf] Activate done langbar=false\n");
        for i in 0..20 {
            text.push_str(&format!("t pid=1 [engine] launch failed: n={i}\n"));
        }
        let a = analyze(&text);
        assert!(a.langbar_failed);
        assert_eq!(a.launch_failures, 20);
        assert_eq!(a.evidence.len(), MAX_EVIDENCE);
        assert!(a.evidence.last().unwrap().contains("n=19"));
    }

    #[test]
    fn empty_log() {
        assert_eq!(analyze(""), LogAnalysis::default());
    }
}
