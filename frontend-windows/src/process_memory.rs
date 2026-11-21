//! imserver（変換エンジンの子プロセス）の常駐メモリの診断。OS 非依存の部分だけ。
//!
//! 値の取得（`GetProcessMemoryInfo`）は unsafe が要るため、このクレート（`forbid(unsafe_code)`）ではなく
//! TSF 層（`nagi-windows-tsf` の `proc_mem`）が行う。ここは取得結果の型・ログ文言・間引きの間隔だけを持つ。
//! 目的はメモリ逼迫の閾値（`environment.rs`、H-99）を実測で見直すための材料集め。**閾値は変えない**。

use std::time::Duration;

/// 取得した常駐メモリ（バイト）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessMemory {
    /// ワーキングセット（物理メモリ上にある量。タスクマネージャの「メモリ」に近い）
    pub working_set: u64,
    /// ピーク時のワーキングセット
    pub peak_working_set: u64,
    /// コミット済みのプライベートメモリ（`PrivateUsage`。モデルの mmap を除く実体の目安）
    pub private_usage: u64,
}

/// 診断ログへ出す間隔（ホストプロセスごと。定期タイマーの tick から呼ばれても間引く）
pub const LOG_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// バイトを MB（1 MB = 1024 * 1024）に直す。小数は切り捨て
pub const fn to_mb(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

/// `[mem]` ログの本文。数値だけで、入力内容は含まない。`memory` が `None`（取得失敗）なら `unavailable`
pub fn describe_process_memory(pid: u32, memory: Option<&ProcessMemory>) -> String {
    match memory {
        Some(m) => format!(
            "imserver pid={pid} working_set_mb={} peak_working_set_mb={} private_mb={}",
            to_mb(m.working_set),
            to_mb(m.peak_working_set),
            to_mb(m.private_usage)
        ),
        None => format!("imserver pid={pid} unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn mb_truncates() {
        assert_eq!(to_mb(0), 0);
        assert_eq!(to_mb(MB - 1), 0);
        assert_eq!(to_mb(512 * MB + 5), 512);
    }

    #[test]
    fn describes_numbers_only() {
        let m = ProcessMemory {
            working_set: 600 * MB,
            peak_working_set: 700 * MB,
            private_usage: 450 * MB + 1,
        };
        assert_eq!(
            describe_process_memory(42, Some(&m)),
            "imserver pid=42 working_set_mb=600 peak_working_set_mb=700 private_mb=450"
        );
        assert_eq!(
            describe_process_memory(42, None),
            "imserver pid=42 unavailable"
        );
    }
}
