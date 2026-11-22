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
    /// `[engine] exit code=... reason=crash|error` の回数（更新の影響期間と起動不可の pid を除く）
    pub exit_crashes: usize,
    /// 上記の最新の時刻
    pub exit_last: Option<String>,
    /// 上記の直近の数件（時刻・pid・reason・code・uptime_ms・last_method。stderr は含めない）
    pub exit_recent: Vec<String>,
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
    /// インストール・更新の時刻の前後 120 秒だったため数えなかったエンジン停止の事象の数
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
    analyze_with(text, &[])
}

/// インストール・更新の影響期間（インストール時刻の前後この秒数）
pub const INSTALL_GRACE_SECS: i64 = 120;

/// 複数 pid の停止を 1 事象にまとめる間隔（秒）
const SIMULTANEOUS_SECS: i64 = 10;
/// 同一 pid の再試行の列を 1 事象にまとめる間隔（秒）
const RETRY_SERIES_SECS: i64 = 60;

fn line_attempt(line: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|t| t.strip_prefix("attempt="))
        .and_then(|n| n.parse().ok())
}

fn line_token<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|t| t.strip_prefix(key))
}

/// 停止の行を事象にまとめる。次のどちらかなら同じ事象:
/// - 直前の停止から 10 秒以内（更新で全ホストのエンジンが一斉に止まる）
/// - 同一 pid で、60 秒以内に attempt の番号が増えている（再起動の試行の列）
#[derive(Default)]
struct StopClusters {
    events: Vec<StopEvent>,
}

struct StopEvent {
    last: Option<i64>,
    last_ts: String,
    /// pid → (最後の時刻, attempt)
    pids: std::collections::BTreeMap<u32, (Option<i64>, Option<u32>)>,
}

impl StopClusters {
    /// 新しい事象になったら `true`
    fn add(&mut self, ts: &str, pid: Option<u32>, attempt: Option<u32>) -> bool {
        let t = parse_utc(ts);
        let joined = self.events.iter_mut().rev().find(|e| {
            let near = match (t, e.last) {
                (Some(t), Some(l)) => (0..=SIMULTANEOUS_SECS).contains(&(t - l)),
                (None, None) => e.last_ts == ts,
                _ => false,
            };
            let series = pid.is_some_and(|p| match (e.pids.get(&p), t, attempt) {
                (Some((Some(pt), Some(pa))), Some(t), Some(a)) => {
                    (0..=RETRY_SERIES_SECS).contains(&(t - pt)) && a > *pa
                }
                _ => false,
            });
            near || series
        });
        let is_new = joined.is_none();
        let e = match joined {
            Some(e) => e,
            None => {
                self.events.push(StopEvent {
                    last: t,
                    last_ts: ts.to_owned(),
                    pids: Default::default(),
                });
                self.events.last_mut().expect("just pushed")
            }
        };
        if t > e.last {
            e.last = t;
        }
        e.last_ts = ts.to_owned();
        if let Some(p) = pid {
            e.pids.insert(p, (t, attempt));
        }
        is_new
    }
}

/// `install_times`（UNIX 秒。インストール・更新の時刻の候補）の前後 ±120 秒に起きたエンジンの
/// 起動失敗・停止・降格・異常終了の行を数えずに解析する
pub fn analyze_with(text: &str, install_times: &[i64]) -> LogAnalysis {
    let in_window = |line: &str| -> bool {
        line.split_whitespace()
            .next()
            .and_then(parse_utc)
            .is_some_and(|t| {
                install_times
                    .iter()
                    .any(|&i| (t - i).abs() <= INSTALL_GRACE_SECS)
            })
    };
    let mut a = LogAnalysis::default();
    let mut problems: Vec<String> = Vec::new();
    let mut counted = StopClusters::default();
    let mut ignored = StopClusters::default();
    let mut pids = std::collections::BTreeSet::new();
    let mut unlaunchable = std::collections::BTreeSet::new();
    // 起動不可の pid は別集計にするので、その pid の停止・降格は一般の事象に数えない
    let unlaunchable_all: std::collections::BTreeSet<u32> = text
        .lines()
        .filter(|l| unlaunchable_code(l).is_some())
        .filter_map(line_pid)
        .collect();
    let mut exit_recent: Vec<String> = Vec::new();
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
        let is_exit_line = line.contains("[engine] exit code=");
        let is_engine_line = line.contains("launch failed")
            || line.contains("entering minimal profile")
            || line.contains("stopped (crash or start failure)")
            || is_exit_line;
        if is_engine_line && pid.is_some_and(|p| unlaunchable_all.contains(&p)) {
            continue;
        }
        if is_engine_line && in_window(line) {
            if line.contains("stopped (crash or start failure)") {
                let ts = line.split_whitespace().next().unwrap_or("");
                if ignored.add(ts, pid, line_attempt(line)) {
                    a.ignored_by_install += 1;
                }
            }
            continue;
        }
        if is_exit_line {
            // reason=update / shutdown / parent-exited などは正常な終了。crash / error だけ数える
            if matches!(line_token(line, "reason="), Some("crash" | "error")) {
                let ts = line.split_whitespace().next().unwrap_or("");
                a.exit_crashes += 1;
                a.exit_last = Some(ts.to_owned());
                let field = |k: &str| line_token(line, k).unwrap_or("-");
                exit_recent.push(format!(
                    "{ts} pid={} reason={} code={} uptime_ms={} last_method={}",
                    pid.map_or_else(|| "-".to_owned(), |p| p.to_string()),
                    field("reason="),
                    field("code="),
                    field("uptime_ms="),
                    field("last_method=")
                ));
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
            // 同時刻の複数 pid・同一 pid の再試行の列は 1 事象。表示も 1 行にする
            if counted.add(ts, pid, line_attempt(line)) {
                a.stopped += 1;
                problem = true;
            }
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
    let skip = exit_recent.len().saturating_sub(MAX_EXIT_RECENT);
    a.exit_recent = exit_recent.into_iter().skip(skip).collect();
    a
}

const MAX_EXIT_RECENT: usize = 3;

/// `nagi_windows_tsf.dll.<yyyyMMddHHmmssfff>.old` のファイル名の時刻（ローカル時刻を、UTC として読んだ UNIX 秒）
pub fn old_file_local_secs(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("nagi_windows_tsf.dll.")?;
    let stamp = rest.strip_suffix(".old")?;
    if stamp.len() != 17 || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ts = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[8..10],
        &stamp[10..12],
        &stamp[12..14]
    );
    parse_utc(&ts)
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
        let a = analyze_with(&text, &[parse_utc("2026-11-21T11:20:00Z").unwrap()]);
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
        assert_eq!((all.stopped, all.minimal_entries), (2, 1));
        let inst = parse_utc("2026-11-21T10:34:30Z").unwrap();
        let a = analyze_with(text, &[inst]);
        assert_eq!(
            (a.stopped, a.minimal_entries, a.ignored_by_install),
            (1, 0, 1),
            "前後 120 秒の事象は 10 秒以内の連なりで 1 事象"
        );
        assert_eq!(a.evidence.len(), 1);
        let a = analyze_with(text, &[parse_utc("2026-11-21T11:13:48Z").unwrap()]);
        assert_eq!((a.stopped, a.ignored_by_install), (1, 1));
        // 影響期間は「前」にも広がる（更新の少し前に止まり始める）
        let a = analyze_with(text, &[parse_utc("2026-11-21T10:36:00Z").unwrap()]);
        assert_eq!(a.ignored_by_install, 1);
    }

    #[test]
    fn same_pid_retry_series_is_one_event() {
        let text = "\
2026-11-22T05:00:00Z pid=5 [engine] stopped (crash or start failure) attempt=1 retry_in_ms=200
2026-11-22T05:00:20Z pid=5 [engine] stopped (crash or start failure) attempt=2 retry_in_ms=400
2026-11-22T05:00:50Z pid=5 [engine] stopped (crash or start failure) attempt=3 retry_in_ms=800
2026-11-22T05:01:30Z pid=5 [engine] stopped (crash or start failure) attempt=4 retry_in_ms=1600
2026-11-22T05:10:00Z pid=5 [engine] stopped (crash or start failure) attempt=1 retry_in_ms=200
";
        let a = analyze(text);
        assert_eq!(a.stopped, 2, "{a:?}");
    }

    #[test]
    fn unlaunchable_pid_lines_are_not_general_stops() {
        let text = format!(
            "{REAL_SPAWN}\n\
2026-11-21T11:20:00Z pid=7720 [engine] stopped (crash or start failure) attempt=1 retry_in_ms=200\n\
2026-11-21T11:20:30Z pid=7720 [engine] entering minimal profile after restarts=5\n"
        );
        let a = analyze(&text);
        assert_eq!((a.stopped, a.minimal_entries, a.launch_failures), (0, 0, 0));
        assert_eq!(a.unlaunchable_pids, vec![7720]);
    }

    #[test]
    fn exit_lines_count_crash_and_error_only() {
        let text = "\
2026-11-22T06:00:00Z pid=1 [engine] exit code=0x0 reason=update uptime_ms=1000 phase=run last_method=process_key last_recv=1
2026-11-22T06:00:01Z pid=1 [engine] exit code=0x0 reason=shutdown uptime_ms=1000 phase=run last_method=- last_recv=1
2026-11-22T07:00:00Z pid=2 [engine] exit code=0xc0000005 reason=crash uptime_ms=5000 phase=run last_method=process_key last_recv=3 stderr=[secret]
2026-11-22T07:10:00Z pid=3 [engine] exit code=0x1 reason=error uptime_ms=10 phase=start last_method=- last_recv=0
";
        let a = analyze(text);
        assert_eq!(a.exit_crashes, 2);
        assert_eq!(a.exit_last.as_deref(), Some("2026-11-22T07:10:00Z"));
        assert!(a.exit_recent[0].contains("code=0xc0000005"));
        assert!(a.exit_recent[0].contains("last_method=process_key"));
        assert!(a.exit_recent.iter().all(|l| !l.contains("secret")));
        // 更新の影響期間の crash は数えない
        let a = analyze_with(
            text,
            &[
                parse_utc("2026-11-22T07:00:30Z").unwrap(),
                parse_utc("2026-11-22T07:10:30Z").unwrap(),
            ],
        );
        assert_eq!(a.exit_crashes, 0);
    }

    #[test]
    fn old_file_name_time() {
        assert_eq!(
            old_file_local_secs("nagi_windows_tsf.dll.20261122060500123.old"),
            parse_utc("2026-11-22T06:05:00Z")
        );
        assert_eq!(old_file_local_secs("nagi_windows_tsf.dll.old"), None);
        assert_eq!(old_file_local_secs("imserver.exe.old"), None);
        assert_eq!(
            old_file_local_secs("nagi_windows_tsf.dll.2026112206050012x.old"),
            None
        );
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
