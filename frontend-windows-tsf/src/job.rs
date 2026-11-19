//! imserver を Job Object（`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`）に入れる。
//!
//! ジョブのハンドルはプロセス全体で 1 つ、プロセスの終了まで閉じない。ホストプロセスが終了・クラッシュすると
//! OS がハンドルを閉じ、ジョブ内の imserver をすべて終了させる（H-96。孤児の防止）。
//! Win8 以降はジョブのネストが可能なので、ホスト自身がジョブに入っていても割り当てられる
//! （入れない環境ではログに残して続行する。`--parent-pid` の監視が代わりに働く）。

#![allow(unsafe_code)]

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::sync::OnceLock;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::core::PCWSTR;

/// ジョブのハンドル。閉じない（プロセスの終了で OS が閉じる）ことが目的なので `Drop` は持たない
struct Job(HANDLE);

// ジョブのハンドルはカーネルオブジェクトの参照で、スレッド間で共有してよい
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

static JOB: OnceLock<Option<Job>> = OnceLock::new();

fn create() -> Option<Job> {
    let handle = match unsafe { CreateJobObjectW(None, PCWSTR::null()) } {
        Ok(h) => h,
        Err(e) => {
            crate::tsf_log::log(
                "engine",
                &format!(
                    "job create failed code={}",
                    nagi_windows::diag::hresult_text(e.code().0)
                ),
            );
            return None;
        }
    };
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let result = unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if let Err(e) = result {
        crate::tsf_log::log(
            "engine",
            &format!(
                "job set failed code={}",
                nagi_windows::diag::hresult_text(e.code().0)
            ),
        );
        return None;
    }
    Some(Job(handle))
}

/// 起動した子プロセスをジョブへ入れる（`ProcessEngineConfiguration::after_spawn` に渡す）。
/// 失敗は `[engine] job assign failed code=…` に残すだけで、エンジンの起動は続ける
pub fn assign(child: &Child) {
    let Some(job) = JOB.get_or_init(create).as_ref() else {
        return;
    };
    let process = HANDLE(child.as_raw_handle());
    match unsafe { AssignProcessToJobObject(job.0, process) } {
        Ok(()) => crate::tsf_log::log("engine", &format!("job assigned pid={}", child.id())),
        Err(e) => crate::tsf_log::log(
            "engine",
            &format!(
                "job assign failed code={}",
                nagi_windows::diag::hresult_text(e.code().0)
            ),
        ),
    }
}
