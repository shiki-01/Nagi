//! アプリごとの入力モードの記憶の永続化（H-49）: `Runtime` 経由の保存・再読込・全削除。

use nagi_config::Config;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::view::InputMode;
use nagi_server::Runtime;
use nagi_server::Tunables;
use nagi_server::app_modes::FILE_NAME;
use std::path::PathBuf;

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nagi-app-modes-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn runtime() -> std::sync::Arc<Runtime> {
    Runtime::new(
        SYSTEM,
        false,
        Tunables::from_config(&Config::default()),
        Learner::new(LearnConfig::default()),
        None,
    )
}

#[test]
fn modes_survive_restart_and_clear_removes_file() {
    let dir = temp_dir("restart");
    let path = dir.join(FILE_NAME);

    let first = runtime();
    first.set_app_modes_path(Some(path.clone()));
    first.app_mode_record("com.example.code", InputMode::Alphanumeric);
    // 保存は flush_learning のタイミング（force で即時）
    first.flush_learning(true);
    assert!(path.exists());

    let second = runtime();
    second.set_app_modes_path(Some(path.clone()));
    assert_eq!(
        second.app_modes_snapshot(),
        vec![("com.example.code".to_string(), InputMode::Alphanumeric)]
    );

    // 学習データの全削除で記憶も消え、ファイルも消える
    second.learn_clear();
    assert!(second.app_modes_snapshot().is_empty());
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
