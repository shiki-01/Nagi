//! imserver の常駐メモリの取得（`GetProcessMemoryInfo`）と、診断ログ `[mem]` への記録。
//!
//! メモリ逼迫の閾値（`nagi_windows::environment`、H-99）を実測で見直すための材料集めで、閾値は変えない。
//! 文言・間引きの間隔は `nagi_windows::process_memory`。この層は取得とログへの書き出しだけを行う。
//! unsafe が要る取得はここに隔離する（`nagi-windows` は `forbid(unsafe_code)`）。

#![allow(unsafe_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use nagi_windows::diag::LogThrottle;
use nagi_windows::process_memory::{LOG_INTERVAL, ProcessMemory, describe_process_memory};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// 直近に起動した imserver の pid（0 は未起動）。`SpawnHook` が記録する
static ENGINE_PID: AtomicU32 = AtomicU32::new(0);

/// 起動した imserver の pid を記録する（再起動のたびに上書きされる）
pub fn record_engine_pid(pid: u32) {
    ENGINE_PID.store(pid, Ordering::Relaxed);
}

/// `pid` のプロセスの常駐メモリ。開けない・取得に失敗したら `None`
pub fn process_memory(pid: u32) -> Option<ProcessMemory> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // `PROCESS_MEMORY_COUNTERS_EX` は `PROCESS_MEMORY_COUNTERS` の末尾に `PrivateUsage` を足した構造体で、
    // `cb` に大きい方のサイズを渡すと OS が全体を書く
    let result = unsafe {
        GetProcessMemoryInfo(
            handle,
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    result.ok()?;
    Some(ProcessMemory {
        working_set: counters.WorkingSetSize as u64,
        peak_working_set: counters.PeakWorkingSetSize as u64,
        private_usage: counters.PrivateUsage as u64,
    })
}

/// 直近の imserver の常駐メモリを `[mem]` に 1 行記録する（間引きは `LOG_INTERVAL`。ホストプロセスごと）。
/// imserver が未起動なら何もしない。定期タイマーから呼んでよい
pub fn log_engine_memory_throttled() {
    static THROTTLE: Mutex<LogThrottle> = Mutex::new(LogThrottle::new());
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let pid = ENGINE_PID.load(Ordering::Relaxed);
    if pid == 0 {
        return;
    }
    let now: Duration = EPOCH.get_or_init(Instant::now).elapsed();
    let allowed = THROTTLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .allow(now, LOG_INTERVAL);
    if allowed {
        crate::tsf_log::log(
            "mem",
            &describe_process_memory(pid, process_memory(pid).as_ref()),
        );
    }
}
