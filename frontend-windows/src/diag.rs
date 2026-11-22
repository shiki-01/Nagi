//! 診断ログ（`%LOCALAPPDATA%\Nagi\logs\tsf.log`）の整形・ローテーション判断・イベントの文言。
//!
//! ファイル I/O は持たない純関数だけ（書き込みは TSF 層の薄いラッパー）。
//! **入力内容（キー・テキスト・preedit）は一切書かない**。書くのは起動・Activate・エンジンの起動の成否・
//! 失敗理由・言語バーの登録結果などのイベントのみ。エンジンからの `notify` の本文も書かない
//! （レベルとコードだけ）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use nagi_protocol::{MemoryPressureState, SetEnvironmentParams, ThermalState};

use crate::client::{ClientDiagnostic, ClientError, ExitReport};
use crate::environment::ResumeSource;
use crate::supervisor::{EngineEvent, ExitPhase, FailureRecord};

/// ログファイルの上限（超えたら 1 世代前へローテーションする）
pub const MAX_LOG_BYTES: u64 = 256 * 1024;

/// 1 行のメッセージの最大文字数（異常に長い文言でファイルを膨らませない）
const MAX_MESSAGE_CHARS: usize = 300;

/// ログファイルのパス（`<LOCALAPPDATA>\Nagi\logs\tsf.log`）
pub fn log_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Nagi").join("logs").join("tsf.log")
}

/// 検証専用の環境上書きファイルのパス（`<LOCALAPPDATA>\Nagi\debug-env.json`。`environment::OverridingProbe`）
pub fn debug_env_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Nagi").join("debug-env.json")
}

/// 1 世代前のログファイルのパス（`tsf.log.1`）
pub fn rotated_path(log: &Path) -> PathBuf {
    let mut name = log.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    log.with_file_name(name)
}

/// `line_len` バイトを追記すると上限を超えるか（超えるなら先にローテーションする）。
/// 空のファイルには、1 行が上限より長くてもローテーションしない
pub fn needs_rotation(current_len: u64, line_len: u64, max: u64) -> bool {
    current_len > 0 && current_len.saturating_add(line_len) > max
}

/// UNIX 時刻（秒）→ UTC の `YYYY-MM-DDTHH:MM:SSZ`
pub fn format_timestamp(unix_secs: u64) -> String {
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    // 日数 → 西暦年月日（Howard Hinnant の civil_from_days。1970-01-01 起点）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// 1 行に整形する（末尾の改行を含む）。改行・制御文字は空白にし、長すぎる文言は切る
pub fn format_line(unix_secs: u64, pid: u32, tag: &str, message: &str) -> String {
    let mut text: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_MESSAGE_CHARS)
        .collect();
    if message.chars().count() > MAX_MESSAGE_CHARS {
        text.push('…');
    }
    format!("{} pid={pid} [{tag}] {text}\n", format_timestamp(unix_secs))
}

/// HRESULT の表記（`0x80004005`）
pub fn hresult_text(code: i32) -> String {
    format!("0x{:08X}", code as u32)
}

/// エラーの理由（入力内容を含みうる応答の本文やサーバーのメッセージは書かない）
pub fn describe_error(error: &ClientError) -> String {
    match error {
        ClientError::NotConnected => "not connected".into(),
        ClientError::ConnectionClosed => "connection closed (engine exited)".into(),
        ClientError::Timeout => "timeout".into(),
        ClientError::Rpc { code, .. } => format!("rpc error code={code}"),
        ClientError::InvalidResponse(_) => "invalid response".into(),
        // 起動時は `spawn: <OS のエラー>` / パイプの取得失敗。パスと OS のエラーだけで入力は含まない
        ClientError::SendFailed(reason) => format!("send failed: {reason}"),
    }
}

/// キー応答の失敗の種類（ログの `kind=`。メッセージ本文は書かない）
pub fn key_error_kind(error: &ClientError) -> &'static str {
    match error {
        ClientError::Timeout => "timeout",
        ClientError::NotConnected => "not_connected",
        ClientError::ConnectionClosed => "disconnected",
        ClientError::Rpc { .. } => "rpc",
        ClientError::InvalidResponse(_) => "invalid_response",
        ClientError::SendFailed(_) => "send_failed",
    }
}

/// キー応答の失敗 1 件のログ本文。入力内容・キーの種類は含めない
pub fn format_key_error(error: &ClientError, composing: bool, elapsed: Duration) -> String {
    let detail = match error {
        // 種類だけでは足りないものに限り、`describe_error`（本文は含まない）の理由を足す
        ClientError::Rpc { .. } | ClientError::SendFailed(_) => {
            format!(" reason={}", describe_error(error))
        }
        _ => String::new(),
    };
    format!(
        "engine error kind={} composing={composing} elapsed_ms={}{detail}",
        key_error_kind(error),
        elapsed.as_millis()
    )
}

/// 一定間隔あたり最大 1 件に間引く（キー応答の失敗が続いてもログを埋めない）
#[derive(Debug, Default)]
pub struct LogThrottle {
    last: Option<Duration>,
}

impl LogThrottle {
    /// 何も出していない状態（`static` に置けるよう `const`）
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// `now`（任意の起点からの経過時間）で出してよければ true にして記録する
    pub fn allow(&mut self, now: Duration, interval: Duration) -> bool {
        match self.last {
            Some(last) if now.saturating_sub(last) < interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// 起動時の失敗の種類（ログの読み手が原因を絞れるよう、`LaunchFailed` の理由に添える）
fn launch_failure_kind(error: &ClientError) -> &'static str {
    match error {
        ClientError::SendFailed(reason) if reason.starts_with("spawn") => "spawn failed",
        ClientError::SendFailed(_) => "pipe/send failed",
        ClientError::Timeout => "initialize timeout",
        ClientError::Rpc { .. } | ClientError::InvalidResponse(_) => "initialize failed",
        ClientError::ConnectionClosed => "engine exited before initialize",
        ClientError::NotConnected => "not connected",
    }
}

/// 終了コードの表記（`101(0x00000065)`。取得できなければ `unknown`）
pub fn exit_code_text(exit: Option<&ExitReport>) -> String {
    match exit.and_then(|e| e.code) {
        Some(code) => format!("{code}({})", hresult_text(code)),
        None => "unknown".into(),
    }
}

fn phase_text(phase: ExitPhase) -> &'static str {
    match phase {
        ExitPhase::BeforeInitialize => "before-initialize",
        ExitPhase::AfterInitialize => "after-initialize",
    }
}

/// エンジンが止まった理由の分類（ログの `reason=`。区別できる範囲で。入力内容は使わない）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// 親（ホスト）の終了を検知して自分で終了した（コード 0 かつ stderr に親の終了の行がある）
    ParentExited,
    /// 更新の適用で `imserver.exe` が置き換わった・消えた後に止まった
    Update,
    /// 自分で終了した（終了コード 0。`shutdown`・stdin の EOF など）
    Shutdown,
    /// 異常終了（NTSTATUS の例外コード 0xC0000005 等、または panic の 101）
    Crash,
    /// 0 以外のコードでの終了（強制終了の 1 など。クラッシュとは断定できない）
    Error,
    /// 終了コードが取れず判別できない
    Unknown,
}

impl StopReason {
    /// ログの表記
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::ParentExited => "parent-exited",
            StopReason::Update => "update",
            StopReason::Shutdown => "shutdown",
            StopReason::Crash => "crash",
            StopReason::Error => "error",
            StopReason::Unknown => "unknown",
        }
    }
}

/// 終了の記録から停止の理由を分類する。優先順位: 実行ファイルの置き換え > 親の終了 > コード。
/// 注: 親（ホスト）が先に終了した場合は、この記録を書くホスト自体が居ないので通常は観測できない
/// （親の監視が誤作動してホストが生きているときだけ `ParentExited` になる）
pub fn classify_exit(exit: Option<&ExitReport>) -> StopReason {
    let Some(exit) = exit else {
        return StopReason::Unknown;
    };
    if exit.executable_replaced {
        return StopReason::Update;
    }
    match exit.code {
        None => StopReason::Unknown,
        Some(0) if exit.stderr_markers.iter().any(|l| l.contains("親プロセス")) => {
            StopReason::ParentExited
        }
        Some(0) => StopReason::Shutdown,
        Some(code) if code as u32 >= 0xC000_0000 || code == 101 => StopReason::Crash,
        Some(_) => StopReason::Error,
    }
}

/// 停止の行（`Crashed`）の直後に出す、終了の詳細の 1 行（`[engine] exit code=0x... reason=... ...`）。
/// メソッド名は定数だけ、stderr は panic の位置などの目印の行だけ（入力内容は含まない）
pub fn describe_exit_line(record: &FailureRecord) -> String {
    let exit = record.exit.as_ref();
    let code = exit
        .and_then(|e| e.code)
        .map_or_else(|| "unknown".to_owned(), hresult_text);
    let mut text = format!(
        "exit code={code} reason={} uptime_ms={} phase={}",
        classify_exit(exit).as_str(),
        exit.map_or_else(
            || "unknown".to_owned(),
            |e| e.lifetime.as_millis().to_string()
        ),
        phase_text(record.phase),
    );
    let method = |m: Option<&String>| m.map_or("-", String::as_str).to_owned();
    text.push_str(&format!(
        " last_method={} last_recv={}",
        method(exit.and_then(|e| e.last_sent.as_ref())),
        method(exit.and_then(|e| e.last_received.as_ref())),
    ));
    if let Some(e) = exit
        && !e.stderr_markers.is_empty()
    {
        text.push_str(&format!(" stderr=[{}]", e.stderr_markers.join(" | ")));
    }
    text
}

/// 失敗 1 回分の詳細（`ProcessExited` の本文）
fn describe_failure(f: &FailureRecord) -> String {
    let mut text = format!(
        "exited attempt={} minimal={} phase={} exit_code={}",
        f.attempt,
        f.minimal,
        phase_text(f.phase),
        exit_code_text(f.exit.as_ref())
    );
    if let Some(exit) = &f.exit {
        text.push_str(&format!(" uptime_ms={}", exit.lifetime.as_millis()));
    }
    if let Some(error) = &f.error {
        text.push_str(&format!(" cause={}", launch_failure_kind(error)));
    }
    text
}

/// minimal へ切り替えたときの、通常起動の失敗の一覧（1 行。直近の数件）
fn describe_failure_summary(failures: &[FailureRecord]) -> String {
    const SHOWN: usize = 6;
    let skipped = failures.len().saturating_sub(SHOWN);
    let items: Vec<String> = failures
        .iter()
        .skip(skipped)
        .map(|f| {
            let up = f
                .exit
                .as_ref()
                .map_or("?".into(), |e| e.lifetime.as_millis().to_string());
            format!(
                "#{} {} code={} up_ms={up}",
                f.attempt,
                match f.phase {
                    ExitPhase::BeforeInitialize => "pre-init",
                    ExitPhase::AfterInitialize => "post-init",
                },
                exit_code_text(f.exit.as_ref())
            )
        })
        .collect();
    format!(
        "normal (LLM) launch failures={} [{}]{}",
        failures.len(),
        items.join("; "),
        if skipped > 0 { " (older omitted)" } else { "" }
    )
}

/// 環境を送ったときのログ文言（`[env]`）。**列挙名と真偽だけ**で、入力内容は含まない。
/// `overridden` は検証用の上書き（`debug-env.json`）が効いているか
pub fn describe_environment_sent(params: &SetEnvironmentParams, overridden: bool) -> String {
    let ac = match params.on_battery {
        Some(true) => "battery",
        Some(false) => "ac",
        None => "unknown",
    };
    let saver = match params.low_power_mode {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    };
    let memory = match params.memory_pressure {
        Some(MemoryPressureState::Normal) => "normal",
        Some(MemoryPressureState::Warning) => "warning",
        Some(MemoryPressureState::Critical) => "critical",
        None => "unknown",
    };
    let thermal = match params.thermal {
        Some(ThermalState::Nominal) => "nominal",
        Some(ThermalState::Fair) => "fair",
        Some(ThermalState::Serious) => "serious",
        Some(ThermalState::Critical) => "critical",
        None => "unknown",
    };
    format!(
        "sent ac={ac} battery_saver={saver} memory={memory} thermal={thermal} source={}",
        if overridden { "override" } else { "real" }
    )
}

/// スリープ復帰でエンジンの再起動を要求したときのログ文言（検出経路つき）
pub fn describe_restart_requested(source: ResumeSource) -> String {
    format!("restart requested (resume: {})", source.as_str())
}

/// 入力欄が機密（パスワード等）かどうかが変わったときのログ文言（`[ctx]`）。
/// `app` は実行ファイル名（例 `chrome.exe`）。周辺テキストなど入力内容は含めない
pub fn describe_secure_input(secure: bool, app: Option<&str>) -> String {
    format!("secure_input={secure} app={}", app.unwrap_or("unknown"))
}

/// ホストプロセス（この DLL を読んだアプリ）の実行ファイル名の行（`exe=chrome.exe`。タグは `host`）。
/// 起動できないホストの切り分け用。パスは含めずファイル名だけ（空白は `_` にして 1 語にする）
pub fn describe_host(exe_name: Option<&str>) -> String {
    let name = exe_name
        .map(|n| n.rsplit(['\\', '/']).next().unwrap_or(n))
        .filter(|n| !n.is_empty())
        .map_or_else(
            || "unknown".to_owned(),
            |n| n.replace(char::is_whitespace, "_"),
        );
    format!("exe={name}")
}

/// エンジンのイベントのログ文言。記録しないイベント（preedit・候補の更新）は `None`
pub fn describe_event(event: &EngineEvent) -> Option<(&'static str, String)> {
    Some(match event {
        EngineEvent::Started { result, is_restart } => (
            "engine",
            format!(
                "started restart={is_restart} server={} {} profile={} protocol={}",
                result.server_name, result.server_version, result.profile, result.protocol_version
            ),
        ),
        EngineEvent::Crashed { attempt, delay } => (
            "engine",
            format!(
                "stopped (crash or start failure) attempt={attempt} retry_in_ms={}",
                delay.as_millis()
            ),
        ),
        EngineEvent::EnteredMinimalMode { restarts } => (
            "engine",
            format!("entering minimal profile after restarts={restarts}"),
        ),
        EngineEvent::LaunchFailed(error) => (
            "engine",
            format!(
                "launch failed: {} ({})",
                launch_failure_kind(error),
                describe_error(error)
            ),
        ),
        EngineEvent::SpawnRetry {
            code,
            attempt,
            delay,
        } => (
            "engine",
            format!(
                "spawn failed (os error {code}) attempt={attempt} retry_in_ms={}",
                delay.as_millis()
            ),
        ),
        EngineEvent::SpawnBlocked { code, recheck_in } => (
            "engine",
            format!(
                "spawn blocked (os error {code}); giving up in this process (recheck once in {}s)",
                recheck_in.as_secs()
            ),
        ),
        EngineEvent::Restored { .. } => ("engine", "session restored".into()),
        EngineEvent::RestoreFailed { error, .. } => (
            "engine",
            format!("session restore failed: {}", describe_error(error)),
        ),
        EngineEvent::Notify(params) => (
            "engine",
            format!(
                "notify level={:?} code={}",
                params.level,
                params.code.as_deref().unwrap_or("-")
            ),
        ),
        EngineEvent::ProcessExited(f) => ("engine", describe_failure(f)),
        EngineEvent::MinimalSwitchReport { failures } => {
            ("engine", describe_failure_summary(failures))
        }
        EngineEvent::Diagnostic(ClientDiagnostic::MalformedLine { len, first, count }) => (
            "engine",
            format!(
                "dropped non-JSON line from engine stdout len={len} first={first} total={count}"
            ),
        ),
        EngineEvent::UpdatePreedit(_) | EngineEvent::UpdateCandidates(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn host_line_has_file_name_only() {
        assert_eq!(
            describe_host(Some("msedgewebview2.exe")),
            "exe=msedgewebview2.exe"
        );
        assert_eq!(
            describe_host(Some("C:\\Program Files\\a b.exe")),
            "exe=a_b.exe"
        );
        assert_eq!(describe_host(None), "exe=unknown");
        assert_eq!(describe_host(Some("")), "exe=unknown");
        assert_eq!(
            format_line(5, 7720, "host", &describe_host(Some("x.exe"))),
            "1970-01-01T00:00:05Z pid=7720 [host] exe=x.exe\n"
        );
    }

    #[test]
    fn spawn_events_are_logged_once_per_kind() {
        let (tag, text) = describe_event(&EngineEvent::SpawnBlocked {
            code: 15623,
            recheck_in: Duration::from_secs(300),
        })
        .unwrap();
        assert_eq!(tag, "engine");
        assert!(text.starts_with("spawn blocked (os error 15623); giving up in this process"));
        let (_, retry) = describe_event(&EngineEvent::SpawnRetry {
            code: 5,
            attempt: 2,
            delay: Duration::from_millis(400),
        })
        .unwrap();
        assert_eq!(retry, "spawn failed (os error 5) attempt=2 retry_in_ms=400");
    }

    #[test]
    fn timestamp_known_values() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_timestamp(1_761_221_696), "2025-10-23T12:14:56Z");
        // 閏年でない年の 3/1
        assert_eq!(format_timestamp(1_677_628_800), "2023-03-01T00:00:00Z");
    }

    #[test]
    fn key_error_line_has_kind_state_and_elapsed_only() {
        let text = format_key_error(&ClientError::Timeout, true, Duration::from_millis(251));
        assert_eq!(
            text,
            "engine error kind=timeout composing=true elapsed_ms=251"
        );
        let text = format_key_error(
            &ClientError::ConnectionClosed,
            false,
            Duration::from_millis(3),
        );
        assert_eq!(
            text,
            "engine error kind=disconnected composing=false elapsed_ms=3"
        );
        // サーバーのメッセージ（入力を含みうる）は書かない
        let text = format_key_error(
            &ClientError::Rpc {
                code: -32000,
                message: "secret".into(),
            },
            true,
            Duration::ZERO,
        );
        assert!(text.contains("kind=rpc") && text.contains("code=-32000"));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn log_throttle_allows_one_per_interval() {
        let sec = Duration::from_secs(1);
        let mut t = LogThrottle::default();
        assert!(t.allow(Duration::from_millis(10), sec));
        assert!(!t.allow(Duration::from_millis(500), sec));
        assert!(!t.allow(Duration::from_millis(1009), sec));
        assert!(t.allow(Duration::from_millis(1010), sec));
        assert!(!t.allow(Duration::from_millis(1500), sec));
    }

    #[test]
    fn line_is_single_line_and_bounded() {
        let line = format_line(0, 42, "tsf", "a\nb\r\nc");
        assert_eq!(line, "1970-01-01T00:00:00Z pid=42 [tsf] a b  c\n");
        let long = format_line(0, 1, "tsf", &"あ".repeat(1000));
        assert_eq!(long.matches('\n').count(), 1);
        assert!(long.chars().count() < 400);
        assert!(long.trim_end().ends_with('…'));
    }

    #[test]
    fn environment_log_has_only_enum_names() {
        let params = SetEnvironmentParams {
            on_battery: Some(true),
            low_power_mode: Some(false),
            thermal: Some(ThermalState::Serious),
            memory_pressure: Some(MemoryPressureState::Warning),
            memory_budget_mb: None,
            utc_offset_minutes: Some(540),
        };
        assert_eq!(
            describe_environment_sent(&params, true),
            "sent ac=battery battery_saver=false memory=warning thermal=serious source=override"
        );
        assert_eq!(
            describe_environment_sent(&SetEnvironmentParams::default(), false),
            "sent ac=unknown battery_saver=unknown memory=unknown thermal=unknown source=real"
        );
    }

    #[test]
    fn resume_and_secure_messages() {
        assert_eq!(
            describe_restart_requested(ResumeSource::OsNotification),
            "restart requested (resume: os-notification)"
        );
        assert_eq!(
            describe_restart_requested(ResumeSource::TickGap),
            "restart requested (resume: tick-gap)"
        );
        assert_eq!(
            describe_secure_input(true, Some("powershell.exe")),
            "secure_input=true app=powershell.exe"
        );
        assert_eq!(
            describe_secure_input(false, None),
            "secure_input=false app=unknown"
        );
    }

    #[test]
    fn rotation_decision() {
        assert!(!needs_rotation(0, 10_000_000, MAX_LOG_BYTES));
        assert!(!needs_rotation(100, 100, MAX_LOG_BYTES));
        assert!(!needs_rotation(MAX_LOG_BYTES - 10, 10, MAX_LOG_BYTES));
        assert!(needs_rotation(MAX_LOG_BYTES - 10, 11, MAX_LOG_BYTES));
        assert!(!needs_rotation(0, u64::MAX, 10));
        assert!(needs_rotation(u64::MAX, u64::MAX, 10));
    }

    #[test]
    fn paths() {
        let log = log_path(Path::new("base"));
        assert_eq!(
            log,
            Path::new("base").join("Nagi").join("logs").join("tsf.log")
        );
        assert_eq!(rotated_path(&log).file_name().unwrap(), "tsf.log.1");
        assert_eq!(rotated_path(&log).parent(), log.parent());
    }

    #[test]
    fn hresult_is_unsigned_hex() {
        assert_eq!(hresult_text(0x8000_4005_u32 as i32), "0x80004005");
        assert_eq!(hresult_text(0), "0x00000000");
    }

    #[test]
    fn launch_failures_are_classified() {
        let spawn = EngineEvent::LaunchFailed(ClientError::SendFailed("spawn: not found".into()));
        let (_, text) = describe_event(&spawn).unwrap();
        assert!(text.contains("spawn failed"), "{text}");
        let timeout = EngineEvent::LaunchFailed(ClientError::Timeout);
        assert!(
            describe_event(&timeout)
                .unwrap()
                .1
                .contains("initialize timeout")
        );
        let rpc = EngineEvent::LaunchFailed(ClientError::Rpc {
            code: -32001,
            message: "secret".into(),
        });
        let text = describe_event(&rpc).unwrap().1;
        assert!(
            text.contains("code=-32001") && !text.contains("secret"),
            "{text}"
        );
    }

    fn record(attempt: u32, phase: ExitPhase, code: Option<i32>) -> FailureRecord {
        FailureRecord {
            attempt,
            minimal: false,
            phase,
            exit: code.map(|code| ExitReport {
                code: Some(code),
                lifetime: Duration::from_millis(120),
                ..ExitReport::default()
            }),
            error: None,
        }
    }

    #[test]
    fn process_exit_is_described_with_code_phase_and_uptime() {
        let before = record(1, ExitPhase::BeforeInitialize, Some(101));
        let text = describe_event(&EngineEvent::ProcessExited(before))
            .unwrap()
            .1;
        assert!(
            text.contains("phase=before-initialize")
                && text.contains("exit_code=101(0x00000065)")
                && text.contains("uptime_ms=120")
                && text.contains("minimal=false"),
            "{text}"
        );
        let crash = record(2, ExitPhase::AfterInitialize, Some(0xC000_0005_u32 as i32));
        let text = describe_event(&EngineEvent::ProcessExited(crash))
            .unwrap()
            .1;
        assert!(
            text.contains("after-initialize") && text.contains("0xC0000005"),
            "{text}"
        );
        // 終了コードが取れない（こちらから終了させた・起動できなかった）
        let mut none = record(3, ExitPhase::BeforeInitialize, None);
        none.error = Some(ClientError::Timeout);
        let text = describe_event(&EngineEvent::ProcessExited(none)).unwrap().1;
        assert!(
            text.contains("exit_code=unknown") && text.contains("cause=initialize timeout"),
            "{text}"
        );
    }

    #[test]
    fn exit_line_and_reason() {
        let mut crash = record(1, ExitPhase::AfterInitialize, Some(0xC000_0005_u32 as i32));
        if let Some(e) = crash.exit.as_mut() {
            e.last_sent = Some("process_key".into());
            e.stderr_markers = vec!["thread 'x' panicked at a.rs:1:2:".into()];
        }
        let text = describe_exit_line(&crash);
        assert!(
            text.starts_with(
                "exit code=0xC0000005 reason=crash uptime_ms=120 phase=after-initialize"
            ) && text.contains("last_method=process_key last_recv=-")
                && text.contains("stderr=[thread 'x' panicked at a.rs:1:2:]"),
            "{text}"
        );
        let reason = |f: &dyn Fn(&mut ExitReport)| {
            let mut e = ExitReport::default();
            f(&mut e);
            classify_exit(Some(&e))
        };
        assert_eq!(reason(&|e| e.code = Some(0)), StopReason::Shutdown);
        assert_eq!(reason(&|e| e.code = Some(101)), StopReason::Crash);
        assert_eq!(reason(&|e| e.code = Some(1)), StopReason::Error);
        assert_eq!(reason(&|e| e.code = None), StopReason::Unknown);
        assert_eq!(
            reason(&|e| {
                e.code = Some(1);
                e.executable_replaced = true;
            }),
            StopReason::Update
        );
        assert_eq!(
            reason(&|e| {
                e.code = Some(0);
                e.stderr_markers =
                    vec!["imserver: 親プロセス（pid 1）が終了したので終了します".into()];
            }),
            StopReason::ParentExited
        );
        assert_eq!(classify_exit(None), StopReason::Unknown);
        let none = record(2, ExitPhase::BeforeInitialize, None);
        let text = describe_exit_line(&none);
        assert!(
            text.contains("code=unknown reason=unknown uptime_ms=unknown"),
            "{text}"
        );
    }

    #[test]
    fn minimal_switch_summary_lists_failures() {
        let failures: Vec<FailureRecord> = (1..=5)
            .map(|i| record(i, ExitPhase::BeforeInitialize, Some(101)))
            .collect();
        let text = describe_event(&EngineEvent::MinimalSwitchReport { failures })
            .unwrap()
            .1;
        assert!(text.contains("failures=5") && text.contains("#1 pre-init code=101"));
        assert!(text.contains("#5"), "{text}");
        assert!(!text.contains("omitted"));
        let many: Vec<FailureRecord> = (1..=9)
            .map(|i| record(i, ExitPhase::AfterInitialize, None))
            .collect();
        let text = describe_failure_summary(&many);
        assert!(text.contains("failures=9") && text.contains("omitted") && !text.contains("#1 "));
        assert!(text.contains("post-init code=unknown"));
    }

    #[test]
    fn malformed_line_diagnostic_has_no_content() {
        let text = describe_event(&EngineEvent::Diagnostic(ClientDiagnostic::MalformedLine {
            len: 11,
            first: "alpha",
            count: 2,
        }))
        .unwrap()
        .1;
        assert_eq!(
            text,
            "dropped non-JSON line from engine stdout len=11 first=alpha total=2"
        );
    }

    #[test]
    fn content_bearing_events_are_not_logged() {
        let preedit = EngineEvent::UpdatePreedit(nagi_protocol::UpdatePreeditParams {
            session_id: "s".into(),
            preedit: Default::default(),
            revision: 1,
        });
        assert!(describe_event(&preedit).is_none());
        let notify = EngineEvent::Notify(nagi_protocol::NotifyParams {
            level: nagi_protocol::NotifyLevel::Warning,
            message: "本文は書かない".into(),
            code: Some("config".into()),
        });
        let text = describe_event(&notify).unwrap().1;
        assert!(text.contains("Warning") && text.contains("config"));
        assert!(!text.contains("本文"));
        let crashed = EngineEvent::Crashed {
            attempt: 2,
            delay: Duration::from_millis(400),
        };
        assert!(
            describe_event(&crashed)
                .unwrap()
                .1
                .contains("retry_in_ms=400")
        );
    }
}
