//! 実行環境（電源・メモリ・負荷）の報告: OS の値 → `set_environment` の params（OS 非依存）。
//!
//! macOS 版 `EnvironmentReporter.swift` に相当する。OS への問い合わせは [`EnvironmentProbe`] で注入し
//! （Windows の実装は `GetSystemPowerStatus` / `GlobalMemoryStatusEx` / `CallNtPowerInformation` を
//! 呼ぶ次の層が担う）、ここは値の解釈と「変化したときだけ送る」判定だけを持つ。
//! プロファイルの切り替えを決めるのはコア（設計書 §4.7）。省略した項目は「前回の値を維持」になる。

use std::cell::Cell;
use std::path::PathBuf;

use nagi_protocol::{MemoryPressureState, SetEnvironmentParams, ThermalState};
use serde::Deserialize;

/// `SYSTEM_POWER_STATUS.ACLineStatus`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcLineStatus {
    /// 電池駆動（0）
    Offline,
    /// AC 接続（1）
    Online,
    /// 不明（255）
    Unknown,
}

impl AcLineStatus {
    /// `ACLineStatus` の生の値から変換する
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Offline,
            1 => Self::Online,
            _ => Self::Unknown,
        }
    }
}

/// 電源状態（`GetSystemPowerStatus`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerSample {
    /// AC 接続の状態
    pub ac_line: AcLineStatus,
    /// 電池が無いと分かっているか（`BatteryFlag` の 128）。デスクトップ PC は AC 扱いにする
    pub no_battery: bool,
    /// バッテリー節約機能（`SystemStatusFlag` が 1）が有効か。`None` は不明
    pub battery_saver: Option<bool>,
}

/// 物理メモリ（`GlobalMemoryStatusEx`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySample {
    /// 物理メモリの総量（MB）
    pub total_mb: u64,
    /// 利用可能な物理メモリ（MB。スタンバイを含む `ullAvailPhys`）
    pub available_mb: u64,
}

/// プロセッサの周波数（`CallNtPowerInformation(ProcessorInformation)`）。サーマル制限の推定に使う
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorSample {
    /// 現在の周波数（MHz）
    pub current_mhz: u32,
    /// 上限の周波数（MHz）
    pub max_mhz: u32,
}

/// OS への問い合わせ。取得できない項目は `None`（その項目は報告せず、コアは前回の値を維持する）
pub trait EnvironmentProbe {
    /// 電源状態
    fn power(&self) -> Option<PowerSample>;
    /// 物理メモリ
    fn memory(&self) -> Option<MemorySample>;
    /// プロセッサの周波数
    fn processor(&self) -> Option<ProcessorSample>;
    /// 現在のローカル時刻の UTC からのずれ（分。夏時間を含む今の値。例: 日本 540）
    fn utc_offset_minutes(&self) -> Option<i32>;
    /// サーマル状態の直接指定（検証用の上書き専用。既定は `None` = 周波数から推定する）。
    /// 周波数からの推定はバッテリー節約中は `None` になるため、上書きは推定を経由せず直接渡す
    fn thermal_override(&self) -> Option<ThermalState> {
        None
    }
    /// メモリ逼迫の状態の直接指定（検証用の上書き専用。既定は `None` = 物理メモリから判定する）。
    /// 指定があればヒステリシスを通さず、そのまま使う
    fn memory_pressure_override(&self) -> Option<MemoryPressureState> {
        None
    }
    /// 検出値が検証用ファイルで上書きされているか（ログの `source=` 用）
    fn is_overridden(&self) -> bool {
        false
    }
}

/// 電池駆動か。不明なら `None`
pub fn on_battery(power: &PowerSample) -> Option<bool> {
    if power.no_battery {
        return Some(false);
    }
    match power.ac_line {
        AcLineStatus::Offline => Some(true),
        AcLineStatus::Online => Some(false),
        AcLineStatus::Unknown => None,
    }
}

/// `Warning` の絶対量の下限（MB）。利用可能がこれ未満なら総メモリによらず `Warning`
const WARNING_MB: u64 = 600;
/// `Warning` の割合（%）。大きな総メモリでは [`WARNING_PCT_CAP_MB`] で頭打ちにする
const WARNING_PCT: u64 = 4;
/// `Warning` の割合から求める閾値の上限（MB）
const WARNING_PCT_CAP_MB: u64 = 1000;
/// `Critical` の絶対量の下限（MB）
const CRITICAL_MB: u64 = 250;
/// `Critical` の割合（%）
const CRITICAL_PCT: u64 = 2;
/// `Critical` の割合から求める閾値の上限（MB）
const CRITICAL_PCT_CAP_MB: u64 = 500;
/// 改善（逼迫が軽くなる向きの遷移）に要る余裕。閾値の 125%（`5 / 4`）を上回ること
const RECOVERY_MARGIN: (u64, u64) = (5, 4);

/// 総メモリ `total_mb` での、`Warning` と `Critical` になる利用可能量の閾値（MB。未満で該当）。
///
/// 閾値は `max(絶対量, min(総メモリ × 割合, 上限))`。小さな総メモリでは絶対量が、大きな総メモリでは
/// 割合（上限で頭打ち）が効く。割合だけだと総メモリが大きいほど閾値が際限なく大きくなり、
/// 十分な空きがあっても逼迫と判定してしまうため、割合から求める値に上限を置く
fn pressure_thresholds_mb(total_mb: u64) -> (u64, u64) {
    let warning = WARNING_MB.max((total_mb * WARNING_PCT / 100).min(WARNING_PCT_CAP_MB));
    let critical = CRITICAL_MB.max((total_mb * CRITICAL_PCT / 100).min(CRITICAL_PCT_CAP_MB));
    (warning, critical)
}

/// 閾値を `scale`（分子, 分母）倍して分類する本体
fn classify(mem: &MemorySample, scale: (u64, u64)) -> MemoryPressureState {
    let (warning, critical) = pressure_thresholds_mb(mem.total_mb);
    // 利用可能 × 分母 < 閾値 × 分子 の整数比較（小数を避ける）
    let below = |threshold: u64| mem.available_mb.saturating_mul(scale.1) < threshold * scale.0;
    if below(critical) {
        MemoryPressureState::Critical
    } else if below(warning) {
        MemoryPressureState::Warning
    } else {
        MemoryPressureState::Normal
    }
}

/// メモリ逼迫の判定（その時点の値だけで決める。状態を持つ判定は [`MemoryHysteresis`]）。
/// Windows には OS のイベントが無いので、利用可能量から推定する。
///
/// 利用可能が `Critical`: 250MB 未満、または総メモリの 2%（上限 500MB）未満 /
/// `Warning`: 600MB 未満、または総メモリの 4%（上限 1000MB）未満。
///
/// **閾値は実測に基づく値ではない暫定値**。根拠は 2 点: (1) LLM（zenz small）の読み込みに要るのは
/// 数百 MB で、1GB 超の空きがあれば `balanced` で動かせる、(2) 実機（搭載 16GB・使用中 92%・
/// 利用可能 1.3GB〈約 8%〉・うちキャッシュ済み 1.1GB）で従来の閾値（10% または 800MB 未満で `Warning`）だと
/// `balanced` が `light` に落ちた。`ullAvailPhys` はスタンバイ（回収可能なキャッシュ）を含むので、
/// 割合だけで逼迫とみなさないよう上限を置いた。要調整
pub fn memory_pressure(mem: &MemorySample) -> MemoryPressureState {
    if mem.total_mb == 0 {
        return MemoryPressureState::Normal;
    }
    classify(mem, (1, 1))
}

fn pressure_rank(state: MemoryPressureState) -> u8 {
    match state {
        MemoryPressureState::Normal => 0,
        MemoryPressureState::Warning => 1,
        MemoryPressureState::Critical => 2,
    }
}

/// メモリ逼迫の判定にヒステリシスを付ける（境目で `Normal` と `Warning` を行き来しないため）。
///
/// 悪化は即時。改善は、閾値の 125% を上回ってから 1 段ずつではなく「125% の閾値で分類した状態」まで
/// 戻す（例: `Warning` から `Normal` へは利用可能が 600MB × 1.25 = 750MB 以上かつ
/// 割合の閾値 × 1.25 以上のとき）
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryHysteresis {
    state: Option<MemoryPressureState>,
}

impl MemoryHysteresis {
    /// 新しい判定（初回は現在値をそのまま採用する）
    pub fn new() -> Self {
        Self::default()
    }

    /// 現在の標本から状態を更新して返す
    pub fn update(&mut self, mem: &MemorySample) -> MemoryPressureState {
        let raw = memory_pressure(mem);
        let next = match self.state {
            Some(prev) if pressure_rank(raw) < pressure_rank(prev) && mem.total_mb > 0 => {
                // 改善: 閾値を 125% にした分類まで（それより悪い側に留まる）。悪化はしない
                let strict = classify(mem, RECOVERY_MARGIN);
                let held = if pressure_rank(strict) < pressure_rank(prev) {
                    strict
                } else {
                    prev
                };
                if pressure_rank(raw) > pressure_rank(held) {
                    raw
                } else {
                    held
                }
            }
            _ => raw,
        };
        self.state = Some(next);
        next
    }

    /// 検証用の上書きなど、外から状態を決めた（ヒステリシスを通さずその値を採用する）
    pub fn force(&mut self, state: MemoryPressureState) -> MemoryPressureState {
        self.state = Some(state);
        state
    }
}

/// サーマル状態の推定。Windows にはサーマル状態の公開 API が無いため、現在の周波数 / 上限の比で見る
/// （85% 以上 `Nominal`、60% 以上 `Fair`、40% 以上 `Serious`、未満 `Critical`。閾値は実測なし）。
/// 周波数はバッテリー節約機能でも下がるので、節約が有効または不明なときは推定せず `None`
/// （誤って軽いプロファイルへ落とさないため）
pub fn thermal(cpu: &ProcessorSample, battery_saver: Option<bool>) -> Option<ThermalState> {
    if battery_saver != Some(false) || cpu.max_mhz == 0 {
        return None;
    }
    let ratio_x100 = u64::from(cpu.current_mhz) * 100 / u64::from(cpu.max_mhz);
    Some(match ratio_x100 {
        85.. => ThermalState::Nominal,
        60..=84 => ThermalState::Fair,
        40..=59 => ThermalState::Serious,
        _ => ThermalState::Critical,
    })
}

/// 現在の環境を `set_environment` の params にする。`memory_budget_mb` は Windows では報告しない
/// （macOS と同じ。上限を課されるのは iOS 拡張のようなプロセス）。UTC からのずれ（日付・時刻の候補用）は
/// 夏時間の切り替えやタイムゾーンの変更で変わるので、他の項目と同じく変化したときに再送される
pub fn snapshot(probe: &dyn EnvironmentProbe) -> SetEnvironmentParams {
    snapshot_with(probe, &mut MemoryHysteresis::new())
}

/// [`snapshot`] に、メモリ逼迫のヒステリシス状態を渡す版（[`EnvironmentReporter`] が使う）。
/// 検証用の上書きがあるときはヒステリシスを通さず指定値を使う
pub fn snapshot_with(
    probe: &dyn EnvironmentProbe,
    memory: &mut MemoryHysteresis,
) -> SetEnvironmentParams {
    let memory_pressure = match probe.memory_pressure_override() {
        Some(state) => Some(memory.force(state)),
        None => probe.memory().map(|m| memory.update(&m)),
    };
    let power = probe.power();
    let battery_saver = power.and_then(|p| p.battery_saver);
    SetEnvironmentParams {
        on_battery: power.as_ref().and_then(on_battery),
        low_power_mode: battery_saver,
        thermal: probe.thermal_override().or_else(|| {
            probe
                .processor()
                .and_then(|cpu| thermal(&cpu, battery_saver))
        }),
        memory_pressure,
        memory_budget_mb: None,
        utc_offset_minutes: probe.utc_offset_minutes(),
    }
}

/// 環境の変化を検出して、送るべき params を返す。
/// 呼び出し側が電源変更通知（`WM_POWERBROADCAST`）や一定間隔のタイマーで [`poll`](Self::poll) を呼び、
/// `Some` が返ったら `EngineSupervisor::set_environment` で送る。
/// エンジンの起動・再起動のたびに [`reset`](Self::reset) を呼ぶ（新しいエンジンは前回の値を知らない）
pub struct EnvironmentReporter<P: EnvironmentProbe> {
    probe: P,
    last_sent: Option<SetEnvironmentParams>,
    memory: MemoryHysteresis,
}

impl<P: EnvironmentProbe> EnvironmentReporter<P> {
    /// 新しい報告器
    pub fn new(probe: P) -> Self {
        Self {
            probe,
            last_sent: None,
            memory: MemoryHysteresis::new(),
        }
    }

    /// 前回から変化していれば新しい値を返す（返した値を「送信済み」として記録する）。
    /// 何も取得できなかった（全項目 `None`）ときは送らない
    pub fn poll(&mut self) -> Option<SetEnvironmentParams> {
        self.poll_with_source().map(|(params, _)| params)
    }

    /// [`poll`](Self::poll) に加えて、その値が検証用の上書きによるものか（`true`）を返す（診断ログ用）
    pub fn poll_with_source(&mut self) -> Option<(SetEnvironmentParams, bool)> {
        let overridden = self.probe.is_overridden();
        let current = snapshot_with(&self.probe, &mut self.memory);
        if current == SetEnvironmentParams::default() || self.last_sent.as_ref() == Some(&current) {
            return None;
        }
        self.last_sent = Some(current.clone());
        Some((current, overridden))
    }

    /// 送信済みの記録を捨てる。次の [`poll`](Self::poll) は現在値を必ず返す
    pub fn reset(&mut self) {
        self.last_sent = None;
    }
}

/// 検証専用の環境上書きファイル（`%LOCALAPPDATA%\Nagi\debug-env.json`）の内容。
///
/// **実機検証のためだけの仕組み**（`verify.ps1 env`）。通常の利用ではこのファイルは存在せず、
/// 存在するときだけ OS の検出値を上書きする。省略した項目は実値を使う。
/// 形式: `{"on_battery":true,"battery_saver":false,"memory":"critical","thermal":"serious"}`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct DebugEnvOverride {
    /// 電池駆動か
    pub on_battery: Option<bool>,
    /// バッテリー節約が有効か
    pub battery_saver: Option<bool>,
    /// メモリ逼迫の状態（`normal` / `warning` / `critical`）
    pub memory: Option<MemoryPressureState>,
    /// サーマル状態（`nominal` / `fair` / `serious` / `critical`）
    pub thermal: Option<ThermalState>,
}

impl DebugEnvOverride {
    /// 上書きする項目が 1 つでもあるか（`{}` は実値のまま）
    pub fn is_active(&self) -> bool {
        self.on_battery.is_some()
            || self.battery_saver.is_some()
            || self.memory.is_some()
            || self.thermal.is_some()
    }
}

/// 上書きファイルの解釈。不正なら理由（行・列だけ。ファイルの内容は含めない）を返す
pub fn parse_debug_env(text: &str) -> Result<DebugEnvOverride, String> {
    serde_json::from_str(text).map_err(|e| {
        format!(
            "invalid debug-env.json (ignored, using real values): line {} column {}",
            e.line(),
            e.column()
        )
    })
}

/// 上書き用に偽装するメモリ量（総量 16GB。`memory_pressure` の閾値で Normal / Warning / Critical になる値。
/// 判定は [`EnvironmentProbe::memory_pressure_override`] で直接指定されるので、これは表示・互換用）
fn memory_sample_for(state: MemoryPressureState) -> MemorySample {
    let available_mb = match state {
        MemoryPressureState::Normal => 8000,
        MemoryPressureState::Warning => 500,
        MemoryPressureState::Critical => 200,
    };
    MemorySample {
        total_mb: 16000,
        available_mb,
    }
}

/// 実際の [`EnvironmentProbe`] を包み、上書きファイルがあるときだけ検出値を差し替える（**検証専用**）。
///
/// ファイルは問い合わせのたびに読む（消せば次の poll で実値に戻り、変化として送られる）。
/// 不正な JSON は無視して実値にし、`warn` を状態が変わるたびに 1 回だけ呼ぶ
/// （直るか消えるまで繰り返さない）。パスは注入する（OS 非依存・テスト可能）
pub struct OverridingProbe<P> {
    inner: P,
    path: PathBuf,
    warn: Box<dyn Fn(&str) + Send>,
    warned: Cell<bool>,
}

impl<P: EnvironmentProbe> OverridingProbe<P> {
    /// `path` は上書きファイル、`warn` は不正なファイルの警告の出力先
    pub fn new(inner: P, path: PathBuf, warn: Box<dyn Fn(&str) + Send>) -> Self {
        Self {
            inner,
            path,
            warn,
            warned: Cell::new(false),
        }
    }

    /// 有効な上書き。ファイルが無い・不正・空 (`{}`) なら `None`
    fn active(&self) -> Option<DebugEnvOverride> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    self.warned.set(false);
                }
                return None;
            }
        };
        match parse_debug_env(&text) {
            Ok(o) => {
                self.warned.set(false);
                o.is_active().then_some(o)
            }
            Err(message) => {
                if !self.warned.replace(true) {
                    (self.warn)(&message);
                }
                None
            }
        }
    }
}

impl<P: EnvironmentProbe> EnvironmentProbe for OverridingProbe<P> {
    fn power(&self) -> Option<PowerSample> {
        let real = self.inner.power();
        let Some(o) = self.active() else {
            return real;
        };
        if o.on_battery.is_none() && o.battery_saver.is_none() {
            return real;
        }
        let mut power = real.unwrap_or(PowerSample {
            ac_line: AcLineStatus::Unknown,
            no_battery: false,
            battery_saver: None,
        });
        if let Some(battery) = o.on_battery {
            power.ac_line = if battery {
                AcLineStatus::Offline
            } else {
                AcLineStatus::Online
            };
            power.no_battery = false;
        }
        if let Some(saver) = o.battery_saver {
            power.battery_saver = Some(saver);
        }
        Some(power)
    }

    fn memory_pressure_override(&self) -> Option<MemoryPressureState> {
        self.active().and_then(|o| o.memory)
    }

    fn memory(&self) -> Option<MemorySample> {
        match self.active().and_then(|o| o.memory) {
            Some(state) => Some(memory_sample_for(state)),
            None => self.inner.memory(),
        }
    }

    fn processor(&self) -> Option<ProcessorSample> {
        self.inner.processor()
    }

    fn utc_offset_minutes(&self) -> Option<i32> {
        self.inner.utc_offset_minutes()
    }

    fn thermal_override(&self) -> Option<ThermalState> {
        self.active().and_then(|o| o.thermal)
    }

    fn is_overridden(&self) -> bool {
        self.active().is_some()
    }
}

/// スリープ復帰の検出経路（診断ログ `restart requested (resume: …)` の区別用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeSource {
    /// OS の電源通知（`PBT_APMRESUMEAUTOMATIC` 等）
    OsNotification,
    /// 定期タイマーの間隔の飛び
    TickGap,
}

impl ResumeSource {
    /// ログ上の名前
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OsNotification => "os-notification",
            Self::TickGap => "tick-gap",
        }
    }
}

/// スリープからの復帰の検出（OS の通知の取りこぼしに備えた、定期タイマーの間隔の飛びとの併用）。
///
/// タイマーはスリープ中に止まるため、前回の tick からの経過が間隔の 2 倍を超えていれば復帰とみなす。
/// OS の通知（`PBT_APMRESUMEAUTOMATIC` 等）と両方で検出しても、`cooldown` の間は 1 回だけ報告する。
/// 時刻は引数で渡す（壁時計。スリープ中も進む `SystemTime` を想定）
#[derive(Debug, Clone)]
pub struct ResumeDetector {
    interval: std::time::Duration,
    cooldown: std::time::Duration,
    last_tick: Option<std::time::SystemTime>,
    last_resume: Option<std::time::SystemTime>,
}

impl ResumeDetector {
    /// `interval` は tick の間隔、`cooldown` は同じ復帰を重ねて報告しない時間
    pub fn new(interval: std::time::Duration, cooldown: std::time::Duration) -> Self {
        Self {
            interval,
            cooldown,
            last_tick: None,
            last_resume: None,
        }
    }

    fn report(&mut self, now: std::time::SystemTime) -> bool {
        if let Some(last) = self.last_resume
            && now.duration_since(last).unwrap_or_default() < self.cooldown
        {
            return false;
        }
        self.last_resume = Some(now);
        true
    }

    /// 定期タイマーの tick。復帰とみなして初めて報告するとき `true`
    pub fn tick(&mut self, now: std::time::SystemTime) -> bool {
        let gap = self
            .last_tick
            .map(|t| now.duration_since(t).unwrap_or_default());
        self.last_tick = Some(now);
        match gap {
            Some(g) if g > self.interval * 2 => self.report(now),
            _ => false,
        }
    }

    /// OS の復帰通知。初めて報告するとき `true`
    pub fn notified(&mut self, now: std::time::SystemTime) -> bool {
        self.report(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const AC: PowerSample = PowerSample {
        ac_line: AcLineStatus::Online,
        no_battery: false,
        battery_saver: Some(false),
    };

    #[test]
    fn ac_line_raw_values() {
        assert_eq!(AcLineStatus::from_raw(0), AcLineStatus::Offline);
        assert_eq!(AcLineStatus::from_raw(1), AcLineStatus::Online);
        assert_eq!(AcLineStatus::from_raw(255), AcLineStatus::Unknown);
    }

    #[test]
    fn battery_detection() {
        assert_eq!(on_battery(&AC), Some(false));
        let bat = PowerSample {
            ac_line: AcLineStatus::Offline,
            ..AC
        };
        assert_eq!(on_battery(&bat), Some(true));
        let unknown = PowerSample {
            ac_line: AcLineStatus::Unknown,
            ..AC
        };
        assert_eq!(on_battery(&unknown), None);
        // 電池が無い PC は AC 扱い（ACLineStatus が不明でも）
        let desktop = PowerSample {
            no_battery: true,
            ..unknown
        };
        assert_eq!(on_battery(&desktop), Some(false));
    }

    fn mem(total: u64, available: u64) -> MemorySample {
        MemorySample {
            total_mb: total,
            available_mb: available,
        }
    }

    #[test]
    fn memory_pressure_thresholds() {
        use MemoryPressureState::{Critical, Normal, Warning};
        assert_eq!(memory_pressure(&mem(16000, 8000)), Normal);
        // 実機の観測: 搭載 16GB・利用可能 1.3GB（約 8%）は Normal
        assert_eq!(memory_pressure(&mem(15600, 1300)), Normal);
        assert_eq!(memory_pressure(&mem(16000, 1300)), Normal);
        // 500MB は Warning、200MB は Critical
        assert_eq!(memory_pressure(&mem(16000, 500)), Warning);
        assert_eq!(memory_pressure(&mem(16000, 200)), Critical);
        // 割合（16GB の 4% = 640MB / 2% = 320MB）
        assert_eq!(memory_pressure(&mem(16000, 639)), Warning);
        assert_eq!(memory_pressure(&mem(16000, 640)), Normal);
        assert_eq!(memory_pressure(&mem(16000, 319)), Critical);
        assert_eq!(memory_pressure(&mem(16000, 320)), Warning);
        // 絶対量: 小さな総メモリでは 250MB 未満が Critical、600MB 未満が Warning
        assert_eq!(memory_pressure(&mem(2000, 249)), Critical);
        assert_eq!(memory_pressure(&mem(2000, 250)), Warning);
        assert_eq!(memory_pressure(&mem(4000, 599)), Warning);
        assert_eq!(memory_pressure(&mem(4000, 600)), Normal);
        assert_eq!(memory_pressure(&mem(0, 0)), Normal);
    }

    #[test]
    fn memory_pressure_percentage_is_capped_for_large_totals() {
        use MemoryPressureState::{Critical, Normal, Warning};
        // 64GB: 4% は 2.5GB だが上限 1000MB で頭打ち。2GB の空きは Normal
        assert_eq!(pressure_thresholds_mb(64000), (1000, 500));
        assert_eq!(memory_pressure(&mem(64000, 2000)), Normal);
        assert_eq!(memory_pressure(&mem(64000, 999)), Warning);
        assert_eq!(memory_pressure(&mem(64000, 499)), Critical);
        assert_eq!(pressure_thresholds_mb(4000), (600, 250));
        assert_eq!(pressure_thresholds_mb(16000), (640, 320));
    }

    #[test]
    fn hysteresis_worsens_immediately_and_recovers_with_margin() {
        use MemoryPressureState::{Critical, Normal, Warning};
        let mut h = MemoryHysteresis::new();
        assert_eq!(h.update(&mem(8000, 4000)), Normal);
        // 悪化は即時
        assert_eq!(h.update(&mem(8000, 500)), Warning);
        // 8GB の閾値は Warning 600MB / Critical 250MB。Warning から Normal へは 750MB 以上
        assert_eq!(h.update(&mem(8000, 620)), Warning);
        assert_eq!(h.update(&mem(8000, 749)), Warning);
        assert_eq!(h.update(&mem(8000, 750)), Normal);
        // Critical は即時
        assert_eq!(h.update(&mem(8000, 200)), Critical);
        // Critical から Warning へは 250 × 1.25 = 312.5 を上回ってから
        assert_eq!(h.update(&mem(8000, 300)), Critical);
        assert_eq!(h.update(&mem(8000, 313)), Warning);
        // Critical から一気に回復したら Normal まで
        assert_eq!(h.update(&mem(8000, 200)), Critical);
        assert_eq!(h.update(&mem(8000, 4000)), Normal);
    }

    #[test]
    fn hysteresis_uses_percentage_margin_for_large_totals() {
        use MemoryPressureState::{Normal, Warning};
        let mut h = MemoryHysteresis::new();
        assert_eq!(h.update(&mem(16000, 500)), Warning);
        // 16GB の Warning 閾値は 640MB。戻るには 800MB 以上
        assert_eq!(h.update(&mem(16000, 790)), Warning);
        assert_eq!(h.update(&mem(16000, 800)), Normal);
    }

    #[test]
    fn hysteresis_reduces_flapping_around_the_threshold() {
        let samples: Vec<u64> = [590, 610, 590, 610, 595, 615, 580, 620, 590, 610]
            .into_iter()
            .collect();
        let count =
            |states: Vec<MemoryPressureState>| states.windows(2).filter(|w| w[0] != w[1]).count();
        let raw: Vec<_> = samples
            .iter()
            .map(|&a| memory_pressure(&mem(8000, a)))
            .collect();
        let mut h = MemoryHysteresis::new();
        let smoothed: Vec<_> = samples.iter().map(|&a| h.update(&mem(8000, a))).collect();
        assert!(count(raw.clone()) >= 8, "素の判定は境目で揺れる");
        assert_eq!(count(smoothed), 0, "ヒステリシスなら Warning に留まる");
    }

    #[test]
    fn reporter_applies_hysteresis_but_not_to_overrides() {
        let probe = FakeProbe::default();
        probe.memory.set(Some(mem(8000, 4000)));
        let mut r = EnvironmentReporter::new(probe);
        assert_eq!(
            r.poll().unwrap().memory_pressure,
            Some(MemoryPressureState::Normal)
        );
        r.probe.memory.set(Some(mem(8000, 500)));
        assert_eq!(
            r.poll().unwrap().memory_pressure,
            Some(MemoryPressureState::Warning)
        );
        // 610MB は素の判定では Normal だが、ヒステリシスで Warning のまま（送り直さない）
        r.probe.memory.set(Some(mem(8000, 610)));
        assert!(r.poll().is_none());
        r.probe.memory.set(Some(mem(8000, 800)));
        assert_eq!(
            r.poll().unwrap().memory_pressure,
            Some(MemoryPressureState::Normal)
        );
    }

    #[test]
    fn override_memory_bypasses_hysteresis() {
        let f = TempFile::new("hyst");
        let (probe, _) = overriding(&f);
        let mut h = MemoryHysteresis::new();
        f.write(r#"{"memory":"warning"}"#);
        assert_eq!(
            snapshot_with(&probe, &mut h).memory_pressure,
            Some(MemoryPressureState::Warning)
        );
        // Warning から normal の上書きは、閾値の 125% を待たず即座に反映される
        f.write(r#"{"memory":"normal"}"#);
        assert_eq!(
            snapshot_with(&probe, &mut h).memory_pressure,
            Some(MemoryPressureState::Normal)
        );
        f.write(r#"{"memory":"critical"}"#);
        assert_eq!(
            snapshot_with(&probe, &mut h).memory_pressure,
            Some(MemoryPressureState::Critical)
        );
        f.write(r#"{"memory":"warning"}"#);
        assert_eq!(
            snapshot_with(&probe, &mut h).memory_pressure,
            Some(MemoryPressureState::Warning)
        );
    }

    #[test]
    fn thermal_from_frequency_ratio() {
        let cpu = |cur| ProcessorSample {
            current_mhz: cur,
            max_mhz: 3000,
        };
        let t = |cur| thermal(&cpu(cur), Some(false));
        assert_eq!(t(3000), Some(ThermalState::Nominal));
        assert_eq!(t(2550), Some(ThermalState::Nominal));
        assert_eq!(t(2549), Some(ThermalState::Fair));
        assert_eq!(t(1800), Some(ThermalState::Fair));
        assert_eq!(t(1700), Some(ThermalState::Serious));
        assert_eq!(t(1000), Some(ThermalState::Critical));
    }

    #[test]
    fn thermal_not_estimated_under_battery_saver_or_unknown() {
        let cpu = ProcessorSample {
            current_mhz: 800,
            max_mhz: 3000,
        };
        assert_eq!(thermal(&cpu, Some(true)), None);
        assert_eq!(thermal(&cpu, None), None);
        let zero = ProcessorSample {
            current_mhz: 1,
            max_mhz: 0,
        };
        assert_eq!(thermal(&zero, Some(false)), None);
    }

    #[derive(Default)]
    struct FakeProbe {
        power: Cell<Option<PowerSample>>,
        memory: Cell<Option<MemorySample>>,
        cpu: Cell<Option<ProcessorSample>>,
        offset: Cell<Option<i32>>,
    }

    impl EnvironmentProbe for FakeProbe {
        fn power(&self) -> Option<PowerSample> {
            self.power.get()
        }
        fn memory(&self) -> Option<MemorySample> {
            self.memory.get()
        }
        fn processor(&self) -> Option<ProcessorSample> {
            self.cpu.get()
        }
        fn utc_offset_minutes(&self) -> Option<i32> {
            self.offset.get()
        }
    }

    #[test]
    fn snapshot_maps_all_fields() {
        let probe = FakeProbe::default();
        probe.power.set(Some(PowerSample {
            ac_line: AcLineStatus::Offline,
            no_battery: false,
            battery_saver: Some(false),
        }));
        probe.memory.set(Some(MemorySample {
            total_mb: 8000,
            available_mb: 100,
        }));
        probe.cpu.set(Some(ProcessorSample {
            current_mhz: 1000,
            max_mhz: 3000,
        }));
        probe.offset.set(Some(540));
        assert_eq!(
            snapshot(&probe),
            SetEnvironmentParams {
                on_battery: Some(true),
                low_power_mode: Some(false),
                thermal: Some(ThermalState::Critical),
                memory_pressure: Some(MemoryPressureState::Critical),
                memory_budget_mb: None,
                utc_offset_minutes: Some(540),
            }
        );
    }

    #[test]
    fn snapshot_omits_unavailable_items() {
        let probe = FakeProbe::default();
        probe.memory.set(Some(MemorySample {
            total_mb: 8000,
            available_mb: 4000,
        }));
        let s = snapshot(&probe);
        assert_eq!(s.on_battery, None);
        assert_eq!(s.low_power_mode, None);
        assert_eq!(s.thermal, None);
        assert_eq!(s.memory_pressure, Some(MemoryPressureState::Normal));
    }

    #[test]
    fn reporter_sends_only_on_change_and_after_reset() {
        let probe = FakeProbe::default();
        probe.power.set(Some(AC));
        let mut r = EnvironmentReporter::new(probe);
        assert!(r.poll().is_some());
        assert!(r.poll().is_none());
        r.probe.power.set(Some(PowerSample {
            ac_line: AcLineStatus::Offline,
            ..AC
        }));
        assert_eq!(r.poll().unwrap().on_battery, Some(true));
        assert!(r.poll().is_none());
        // エンジン再起動後は同じ値でも送り直す
        r.reset();
        assert!(r.poll().is_some());
    }

    #[test]
    fn reporter_skips_when_nothing_available() {
        let mut r = EnvironmentReporter::new(FakeProbe::default());
        assert!(r.poll().is_none());
    }

    // ---- 検証用の上書き ----

    struct TempFile(PathBuf);
    impl TempFile {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("nagi-debug-env-{tag}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.join("debug-env.json"))
        }
        fn write(&self, text: &str) {
            std::fs::write(&self.0, text).unwrap();
        }
        fn remove(&self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    fn real_probe() -> FakeProbe {
        let probe = FakeProbe::default();
        probe.power.set(Some(AC));
        probe.memory.set(Some(MemorySample {
            total_mb: 8000,
            available_mb: 4000,
        }));
        probe.cpu.set(Some(ProcessorSample {
            current_mhz: 3000,
            max_mhz: 3000,
        }));
        probe
    }

    fn overriding(
        file: &TempFile,
    ) -> (
        OverridingProbe<FakeProbe>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&warnings);
        let probe = OverridingProbe::new(
            real_probe(),
            file.0.clone(),
            Box::new(move |m| sink.lock().unwrap().push(m.to_string())),
        );
        (probe, warnings)
    }

    #[test]
    fn debug_env_parses_partial_and_rejects_garbage() {
        let o = parse_debug_env(r#"{"on_battery":true,"memory":"critical"}"#).unwrap();
        assert_eq!(o.on_battery, Some(true));
        assert_eq!(o.memory, Some(MemoryPressureState::Critical));
        assert_eq!(o.thermal, None);
        assert!(o.is_active());
        assert!(!parse_debug_env("{}").unwrap().is_active());
        assert!(parse_debug_env("not json").is_err());
        assert!(parse_debug_env(r#"{"memory":"huge"}"#).is_err());
    }

    #[test]
    fn override_absent_file_uses_real_values() {
        let f = TempFile::new("absent");
        let (probe, warnings) = overriding(&f);
        assert!(!probe.is_overridden());
        assert_eq!(snapshot(&probe).on_battery, Some(false));
        assert!(warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn override_replaces_only_given_items() {
        let f = TempFile::new("partial");
        f.write(r#"{"on_battery":true,"memory":"critical","thermal":"serious"}"#);
        let (probe, _) = overriding(&f);
        assert!(probe.is_overridden());
        let s = snapshot(&probe);
        assert_eq!(s.on_battery, Some(true));
        // 指定の無い battery_saver は実値
        assert_eq!(s.low_power_mode, Some(false));
        assert_eq!(s.memory_pressure, Some(MemoryPressureState::Critical));
        assert_eq!(s.thermal, Some(ThermalState::Serious));
    }

    #[test]
    fn override_thermal_works_even_under_battery_saver() {
        let f = TempFile::new("saver");
        f.write(r#"{"battery_saver":true,"thermal":"critical"}"#);
        let (probe, _) = overriding(&f);
        let s = snapshot(&probe);
        assert_eq!(s.low_power_mode, Some(true));
        assert_eq!(s.thermal, Some(ThermalState::Critical));
    }

    #[test]
    fn override_memory_states_map_to_thresholds() {
        for (name, expected) in [
            ("normal", MemoryPressureState::Normal),
            ("warning", MemoryPressureState::Warning),
            ("critical", MemoryPressureState::Critical),
        ] {
            let f = TempFile::new(name);
            f.write(&format!(r#"{{"memory":"{name}"}}"#));
            let (probe, _) = overriding(&f);
            assert_eq!(snapshot(&probe).memory_pressure, Some(expected));
        }
    }

    #[test]
    fn invalid_json_falls_back_and_warns_once() {
        let f = TempFile::new("invalid");
        f.write("{oops");
        let (probe, warnings) = overriding(&f);
        for _ in 0..5 {
            assert_eq!(snapshot(&probe).on_battery, Some(false));
            assert!(!probe.is_overridden());
        }
        assert_eq!(warnings.lock().unwrap().len(), 1);
        // 直ると上書きが効き、また壊すと再び 1 回だけ警告する
        f.write(r#"{"on_battery":true}"#);
        assert_eq!(snapshot(&probe).on_battery, Some(true));
        f.write("{oops");
        let _ = snapshot(&probe);
        let _ = snapshot(&probe);
        assert_eq!(warnings.lock().unwrap().len(), 2);
    }

    #[test]
    fn removing_the_file_restores_real_values_as_a_change() {
        let f = TempFile::new("remove");
        f.write(r#"{"on_battery":true}"#);
        let (probe, _) = overriding(&f);
        let mut r = EnvironmentReporter::new(probe);
        let (first, overridden) = r.poll_with_source().unwrap();
        assert_eq!(first.on_battery, Some(true));
        assert!(overridden);
        assert!(r.poll().is_none());
        f.remove();
        let (back, overridden) = r.poll_with_source().unwrap();
        assert_eq!(back.on_battery, Some(false));
        assert!(!overridden);
    }

    #[test]
    fn resume_detected_once_by_gap_or_notification() {
        use std::time::{Duration, SystemTime};
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut d = ResumeDetector::new(Duration::from_secs(30), Duration::from_secs(60));
        assert!(!d.tick(at(0)));
        assert!(!d.tick(at(30)));
        // スリープで 10 分飛んだ
        assert!(d.tick(at(630)));
        // 直後の OS 通知は重ねて報告しない
        assert!(!d.notified(at(631)));
        assert!(!d.tick(at(660)));
        // 十分あとの通知は新しい復帰
        assert!(d.notified(at(900)));
    }
}
