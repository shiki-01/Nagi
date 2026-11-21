//! 単体テスト用のフェイク。

use super::ctx::Ctx;
use super::sys::{ComProbe, ProcMemory, Sleeper, System, WinFacts};
use crate::cli::Args;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 固定の値を返す `System`
#[derive(Default)]
pub struct FakeSystem {
    pub windows: bool,
    pub facts: Option<WinFacts>,
    pub com: Option<ComProbe>,
    pub memory: Option<ProcMemory>,
}

impl FakeSystem {
    pub fn windows(facts: WinFacts) -> Self {
        Self {
            windows: true,
            facts: Some(facts),
            ..Self::default()
        }
    }
}

impl System for FakeSystem {
    fn is_windows(&self) -> bool {
        self.windows
    }

    fn facts(&self, _install_dir: &Path) -> Option<WinFacts> {
        self.facts.clone()
    }

    fn com_probe(&self) -> Option<ComProbe> {
        self.com.clone()
    }

    fn process_memory(&self, _pid: u32) -> Option<ProcMemory> {
        self.memory
    }

    fn spawn_sleeper(&self) -> Option<Sleeper> {
        None
    }
}

/// フェイクの `System` で `Ctx` を作る（場所は存在しないパス）
pub fn ctx_with_system(system: FakeSystem) -> Ctx {
    let args = Args {
        imserver: Some(PathBuf::from("/nonexistent/imserver")),
        ..Args::default()
    };
    Ctx::resolve(&args, &|_| None, None, "linux", Arc::new(system))
}

/// imserver の場所を指定した `Ctx`（Linux 扱い・フェイクの `System`）
pub fn ctx_with_imserver(path: &str) -> Ctx {
    let args = Args {
        imserver: Some(PathBuf::from(path)),
        ..Args::default()
    };
    Ctx::resolve(
        &args,
        &|_| None,
        None,
        "linux",
        Arc::new(FakeSystem::default()),
    )
}
