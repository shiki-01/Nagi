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
    /// `stopped (crash or start failure)` の事象の回数（同一秒の複数 pid は 1 事象に束ねる）
    pub stopped: usize,
    /// 最新の事象の時刻（ログの先頭トークン。`YYYY-MM-DDTHH:MM:SSZ`）
    pub stopped_last: Option<String>,
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
    /// インストール・更新の時刻（+120 秒）以前だったため数えなかったエンジン停止の事象の数
    pub ignored_by_install: usize,
    /// ログに現れたホストプロセス（pid）の数
    pub hosts: usize,
    /// 再試行しても直らない起動失敗（`spawn failed` かつ os error 15623 / 5 / 1260 など）の回数
    pub unlaunchable_count: usize,
    /// 上記が出た pid（昇順・重複なし）
    pub unlaunchable_pids: Vec<u32>,
    /// `pid=<n> [host] exe=<name>` の記録から解決した pid → 実行ファイル名
    pub host_exes: std::collections::BTreeMap<u32, String>,
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

/// 再試行しても直らない `spawn failed` の os error（15623 = 署名ポリシー違反、5 = アクセス拒否、1260 = ポリシーでブロック）
pub const UNLAUNCHABLE_OS_ERRORS: [u32; 3] = [15623, 5, 1260];

fn line_pid(line: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|t| t.strip_prefix("pid="))
        .and_then(|n| n.parse().ok())
}

/// 行の `spawn failed` が再試行しても直らない種類なら、その os error 番号
fn unlaunchable_code(line: &str) -> Option<u32> {
    if !line.contains("launch failed") || !line.contains("spawn failed") {
        return None;
    }
    let rest = line.split("os error ").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let code: u32 = digits.parse().ok()?;
    UNLAUNCHABLE_OS_ERRORS.contains(&code).then_some(code)
}

/// 時刻（UNIX 秒）がインストール時刻として妥当か。zip 展開の既定時刻（1980-01-01）や未来の値は不可
pub fn plausible_install_time(t: i64, now_unix: i64) -> bool {
    const MIN_PLAUSIBLE: i64 = 1_577_836_800; // 2020-01-01
    t >= MIN_PLAUSIBLE && t <= now_unix + 3600
}

/// ログ全文を解析する
pub fn analyze(text: &str) -> LogAnalysis {
    analyze_with(text, None)
}

/// インストール・更新の影響期間（インストール時刻の後この秒数まで）
pub const INSTALL_GRACE_SECS: i64 = 120;

/// `ignore_until`（UNIX 秒）以前のエンジン起動失敗・停止・降格の行を数えずに解析する
pub fn analyze_with(text: &str, ignore_until: Option<i64>) -> LogAnalysis {
    let mut a = LogAnalysis::default();
    let mut problems: Vec<String> = Vec::new();
    let mut last_stop_ts: Option<&str> = None;
    let mut last_ignored_ts: Option<&str> = None;
    let mut pids = std::collections::BTreeSet::new();
    let mut unlaunchable = std::collections::BTreeSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        a.lines += 1;
        let mut problem = false;
        let pid = line_pid(line);
        if let Some(p) = pid {
            pids.insert(p);
            if let Some(exe) = line.split("[host] exe=").nth(1) {
                let exe = exe.trim();
                if !exe.is_empty() {
                    a.host_exes.insert(p, clip(exe));
                }
            }
        }
        if let (Some(p), Some(_)) = (pid, unlaunchable_code(line)) {
            // 更新直後の一時的な停止とは別物なので、インストール時刻による除外の対象にしない
            a.unlaunchable_count += 1;
            if unlaunchable.insert(p) {
                problems.push(clip(line));
            }
            continue;
        }
        let is_engine_line = line.contains("launch failed")
            || line.contains("entering minimal profile")
            || line.contains("stopped (crash or start failure)");
        if is_engine_line
            && let Some(limit) = ignore_until
            && line
                .split_whitespace()
                .next()
                .and_then(parse_utc)
                .is_some_and(|t| t <= limit)
        {
            if line.contains("stopped (crash or start failure)") {
                let ts = line.split_whitespace().next().unwrap_or("");
                if last_ignored_ts != Some(ts) {
                    a.ignored_by_install += 1;
                }
                last_ignored_ts = Some(ts);
            }
            continue;
        }
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
            let ts = line.split_whitespace().next().unwrap_or("");
            // 同一秒の複数 pid は 1 事象（更新・差し替えで全ホストのエンジンが一斉に止まる）。表示も 1 行にする
            if last_stop_ts != Some(ts) {
                a.stopped += 1;
                problem = true;
            }
            last_stop_ts = Some(ts);
            a.stopped_last = Some(ts.to_owned());
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
    a.hosts = pids.len();
    a.unlaunchable_pids = unlaunchable.into_iter().collect();
    a.evidence = problems.into_iter().skip(skip).collect();
    a
}

const ENGINE_LOG_MARKERS: [&str; 9] = [
    "panic",
    "error",
    "failed",
    "異常終了",
    "loadlibrary",
    "モデル",
    "cannot load",
    "fatal",
    "exit code",
];

/// imserver のログから、エラー・起動失敗らしい行を末尾側から最大 `max` 行取り出す。
/// 既知のマーカーに合う行だけを返す（それ以外は入力内容を含みうるので出さない）
pub fn engine_log_evidence(text: &str, max: usize) -> Vec<String> {
    let hits: Vec<String> = text
        .lines()
        .filter(|l| {
            let low = l.to_lowercase();
            ENGINE_LOG_MARKERS.iter().any(|m| low.contains(m))
        })
        .map(clip)
        .collect();
    let skip = hits.len().saturating_sub(max);
    hits.into_iter().skip(skip).collect()
}

/// `YYYY-MM-DDTHH:MM:SSZ` を UNIX 秒にする
pub fn parse_utc(ts: &str) -> Option<i64> {
    let b = ts.strip_suffix('Z')?;
    let (d, t) = b.split_once('T')?;
    let mut dp = d.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (dp.next()??, dp.next()??, dp.next()??);
    let mut tp = t.split(':').map(|x| x.parse::<i64>().ok());
    let (hh, mm, ss) = (tp.next()??, tp.next()??, tp.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // days_from_civil
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
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

    #[test]
    fn stopped_same_second_is_one_event() {
        let a = analyze(
            "2026-11-21T01:00:00Z pid=1 [engine] stopped (crash or start failure)\n\
2026-11-21T01:00:00Z pid=2 [engine] stopped (crash or start failure)\n\
2026-11-21T01:00:00Z pid=3 [engine] stopped (crash or start failure)\n\
2026-11-21T02:00:00Z pid=1 [engine] stopped (crash or start failure)\n",
        );
        assert_eq!(a.stopped, 2);
        assert_eq!(a.evidence.len(), 2);
        assert_eq!(a.stopped_last.as_deref(), Some("2026-11-21T02:00:00Z"));
    }

    const REAL_SPAWN: &str = "2026-11-21T11:20:00Z pid=7720 [engine] launch failed: spawn failed (send failed: spawn: システム バイナリのエラーが検出されました。 (os error 15623))";

    #[test]
    fn unlaunchable_spawn_is_separated_and_grouped_by_pid() {
        let text = format!(
            "{REAL_SPAWN}\n{REAL_SPAWN}\n\
2026-11-21T11:20:01Z pid=7720 [host] exe=sandbox.exe\n\
2026-11-21T11:20:02Z pid=100 [tsf] Activate done langbar=true\n\
2026-11-21T11:20:03Z pid=200 [engine] launch failed: spawn failed (send failed: spawn: x (os error 2))\n"
        );
        let a = analyze(&text);
        assert_eq!(a.unlaunchable_count, 2);
        assert_eq!(a.unlaunchable_pids, vec![7720]);
        assert_eq!(a.hosts, 3);
        assert_eq!(a.launch_failures, 1, "os error 2 は従来どおり");
        assert_eq!(
            a.host_exes.get(&7720).map(String::as_str),
            Some("sandbox.exe")
        );
        // インストール時刻の除外対象にならない
        let a = analyze_with(&text, Some(i64::MAX));
        assert_eq!(a.unlaunchable_pids, vec![7720]);
        assert_eq!(
            unlaunchable_code("pid=1 launch failed: spawn failed (os error 5)"),
            Some(5)
        );
        assert_eq!(
            unlaunchable_code("pid=1 launch failed: spawn failed (os error 1260)"),
            Some(1260)
        );
    }

    #[test]
    fn plausible_install_time_rejects_1980_future() {
        let now = 1_795_219_200;
        assert!(
            !plausible_install_time(315_532_800, now),
            "1980-01-01 は zip の既定時刻"
        );
        assert!(!plausible_install_time(0, now));
        assert!(!plausible_install_time(now + 86_400, now));
        assert!(plausible_install_time(now - 300, now));
        // 1980 起点だと 10 億秒台の経過になってしまう値
        assert!(now - 315_532_800 > 1_000_000_000);
    }

    #[test]
    fn parse_utc_epoch() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2026-11-21T00:00:00Z"), Some(1_795_219_200));
        assert_eq!(parse_utc("bad"), None);
    }

    #[test]
    fn install_time_filters_engine_events() {
        let text = "\
2026-11-21T10:34:18Z pid=37280 [engine] stopped (crash or start failure) attempt=20 retry_in_ms=10000
2026-11-21T10:34:21Z pid=2988 [engine] stopped (crash or start failure) attempt=1 retry_in_ms=200
2026-11-21T10:34:24Z pid=28992 [engine] entering minimal profile after restarts=5
2026-11-21T11:13:48Z pid=11484 [engine] stopped (crash or start failure) attempt=1 retry_in_ms=200
";
        let all = analyze(text);
        assert_eq!((all.stopped, all.minimal_entries), (3, 1));
        let inst = parse_utc("2026-11-21T10:34:30Z").unwrap();
        let a = analyze_with(text, Some(inst + INSTALL_GRACE_SECS));
        assert_eq!(
            (a.stopped, a.minimal_entries, a.ignored_by_install),
            (1, 0, 2)
        );
        assert_eq!(a.evidence.len(), 1);
        let a = analyze_with(text, Some(parse_utc("2026-11-21T11:13:48Z").unwrap()));
        assert_eq!((a.stopped, a.ignored_by_install), (0, 3));
    }

    #[test]
    fn engine_log_evidence_picks_marker_lines_only() {
        let text = "hello 私の入力文\nthread panicked at x\nLoadLibrary failed 126\nok\n";
        let e = engine_log_evidence(text, 10);
        assert_eq!(e.len(), 2);
        assert_eq!(engine_log_evidence(text, 1), vec![e[1].clone()]);
        assert!(engine_log_evidence("nothing", 10).is_empty());
    }
}
