//! 親プロセスの終了監視。フロント（TSF の DLL を読んだホストプロセス）が消えたのに imserver が
//! 残り続けないよう、親が終了したら `on_exit` を呼ぶ（`docs/decisions.md` H-96）。
//!
//! stdin の EOF でも imserver は終了するが、DLL がアンロードされて標準入力のハンドルだけ残る・
//! ハンドルが他の子へ継承される、といった場合に EOF が来ない。親のプロセスハンドルを待つ監視を重ねる。
//! Windows 以外では何もしない（`spawn` は `false` を返す）。

/// 親プロセス `parent_pid` の終了を専用スレッドで待ち、終了したら `on_exit` を呼ぶ。
/// 監視を始めたら `true`。次の場合は `false`（監視しない。呼び出し側は何もしなくてよい）:
/// Windows 以外 / 親を開けない（権限不足など。親が既に存在しない場合は `on_exit` を呼んで `true`）
pub fn spawn(parent_pid: u32, on_exit: impl FnOnce() + Send + 'static) -> bool {
    imp::spawn(parent_pid, on_exit)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, INFINITE, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    /// 自分が閉じるハンドル
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    // ハンドルはカーネルオブジェクトの参照で、スレッド間で渡してよい
    unsafe impl Send for Owned {}

    fn ticks(t: FILETIME) -> u64 {
        (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
    }

    /// プロセスの作成時刻（100ns 単位。取れなければ `None`）
    fn creation_time(process: HANDLE) -> Option<u64> {
        let (mut created, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe { GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user) }
            .ok()?;
        Some(ticks(created))
    }

    /// 親の作成時刻が子（自分）より後なら、PID が再利用された別のプロセス（本当の親は終了済み）
    pub(super) fn is_reused_pid(parent_created: Option<u64>, own_created: Option<u64>) -> bool {
        matches!((parent_created, own_created), (Some(p), Some(c)) if p > c)
    }

    pub(super) fn spawn(parent_pid: u32, on_exit: impl FnOnce() + Send + 'static) -> bool {
        let access = PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION;
        let parent = match unsafe { OpenProcess(access, false, parent_pid) } {
            Ok(handle) => Owned(handle),
            Err(e) => {
                // ERROR_INVALID_PARAMETER = その PID のプロセスが無い（起動前に親が終了した）
                if e.code() == ERROR_INVALID_PARAMETER.to_hresult() {
                    on_exit();
                    return true;
                }
                return false;
            }
        };
        let own = creation_time(unsafe { GetCurrentProcess() });
        if is_reused_pid(creation_time(parent.0), own) {
            on_exit();
            return true;
        }
        std::thread::Builder::new()
            .name("nagi-parent-watch".into())
            .spawn(move || {
                let parent = parent;
                if unsafe { WaitForSingleObject(parent.0, INFINITE) } == WAIT_OBJECT_0 {
                    on_exit();
                }
            })
            .is_ok()
    }

    #[cfg(test)]
    mod tests {
        use super::is_reused_pid;

        #[test]
        fn later_creation_means_reused_pid() {
            assert!(is_reused_pid(Some(200), Some(100)));
            assert!(!is_reused_pid(Some(100), Some(200)));
            assert!(!is_reused_pid(None, Some(100)));
            assert!(!is_reused_pid(Some(100), None));
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub(super) fn spawn(_parent_pid: u32, _on_exit: impl FnOnce() + Send + 'static) -> bool {
        false
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    #[test]
    fn is_a_no_op_off_windows() {
        assert!(!super::spawn(1, || panic!("呼ばれないはず")));
    }
}
