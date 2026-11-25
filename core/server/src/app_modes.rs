//! アプリごとの入力モードの記憶（H-49）の永続化。
//!
//! 利用者がアプリでモードを切り替えた記録（アプリの識別子 → モード）を `<user-dir>/app_modes.tsv` に
//! 保存し、起動時に engine へ戻す。保存するのはアプリの識別子とモード名だけで、入力内容は一切含まない。
//!
//! ファイルの形: 1 行目 `# nagi app_modes v1`、以降 `アプリ識別子<TAB>モード名`（モード名は
//! [`InputMode`] の serde 名: `hiragana` / `alphanumeric` など）。不正な行・未知のモードは読み飛ばす。
//!
//! 設定バックアップ（[`crate::backup_tools`]）の対象には含めない。記憶は操作の癖で、学習データ・設定・
//! 辞書の 4 項目（`BackupItem`）のどれでもなく、項目を増やすと全か無かの適用と protocol の変更が要るため。
//!
//! 件数は [`MAX_APPS`] まで。超えたときは、今記録したアプリ以外のうち識別子の順で最初のものを落とす
//! （古さは追わない単純な上限）。

use crate::runtime::Runtime;
use nagi_engine::ModeObserver;
use nagi_protocol::view::InputMode;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// 保存ファイル名（`<user-dir>` 直下）
pub const FILE_NAME: &str = "app_modes.tsv";

/// ヘッダ行（形式の版を含む）
const HEADER: &str = "# nagi app_modes v1";

/// 記憶するアプリの最大件数
pub const MAX_APPS: usize = 500;

/// アプリごとのモードの記憶（メモリ上の正本と、保存の状態）
#[derive(Default)]
pub struct AppModeStore {
    modes: Mutex<BTreeMap<String, InputMode>>,
    /// 保存していない変更があるか
    dirty: AtomicBool,
}

impl AppModeStore {
    /// 空の記憶
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_modes(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, InputMode>> {
        self.modes.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// ファイルから読んで、記憶を置き換える。無い・ヘッダが違う場合は空のまま（何もしない）。
    /// 読めたら `true`
    pub fn load(&self, path: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let mut lines = text.lines();
        if lines.next().map(str::trim_end) != Some(HEADER) {
            return false;
        }
        let mut modes = BTreeMap::new();
        for line in lines {
            if let Some((app, mode)) = parse_line(line)
                && modes.len() < MAX_APPS
            {
                modes.insert(app, mode);
            }
        }
        *self.lock_modes() = modes;
        self.dirty.store(false, Ordering::Relaxed);
        true
    }

    /// アプリ `app` のモードを `mode` として記録する（変化があれば保存待ちにする）。
    /// 空の識別子・タブや改行を含む識別子は記録しない
    pub fn record(&self, app: &str, mode: InputMode) {
        let app = app.to_lowercase();
        if !valid_app(&app) {
            return;
        }
        let mut modes = self.lock_modes();
        if modes.get(&app) == Some(&mode) {
            return;
        }
        modes.insert(app.clone(), mode);
        if modes.len() > MAX_APPS
            && let Some(old) = modes.keys().find(|k| **k != app).cloned()
        {
            modes.remove(&old);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 記憶の全件（識別子の順）
    pub fn snapshot(&self) -> Vec<(String, InputMode)> {
        self.lock_modes()
            .iter()
            .map(|(app, mode)| (app.clone(), *mode))
            .collect()
    }

    /// 記憶を全て消す（次の flush でファイルも消える）
    pub fn clear(&self) {
        let mut modes = self.lock_modes();
        if !modes.is_empty() {
            modes.clear();
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 保存していない変更があるか
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// 変更があるときだけファイルに書く。全件空ならファイルを消す。原子的に書く（一時ファイル → rename）
    pub fn flush(&self, path: &Path) -> std::io::Result<()> {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return Ok(());
        }
        let modes = self.lock_modes().clone();
        let result = if modes.is_empty() {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        } else {
            save(path, &modes)
        };
        if result.is_err() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result
    }
}

/// 識別子として記録してよいか（空・タブ・改行・制御文字を含まない）
fn valid_app(app: &str) -> bool {
    !app.is_empty() && !app.chars().any(|c| c.is_control())
}

/// 1 行を読む。不正なら `None`
fn parse_line(line: &str) -> Option<(String, InputMode)> {
    let (app, name) = line.split_once('\t')?;
    if !valid_app(app) {
        return None;
    }
    let mode = mode_from_name(name.trim())?;
    Some((app.to_string(), mode))
}

/// モード名（serde 名）から [`InputMode`]。未知なら `None`
fn mode_from_name(name: &str) -> Option<InputMode> {
    serde_json::from_value(Value::String(name.to_string())).ok()
}

/// [`InputMode`] のモード名（serde 名）
fn mode_name(mode: InputMode) -> Option<String> {
    match serde_json::to_value(mode) {
        Ok(Value::String(s)) => Some(s),
        _ => None,
    }
}

/// ファイルに書く（一時ファイル経由で原子的に）
fn save(path: &Path, modes: &BTreeMap<String, InputMode>) -> std::io::Result<()> {
    let mut text = String::new();
    text.push_str(HEADER);
    text.push('\n');
    for (app, mode) in modes {
        let name = mode_name(*mode).ok_or_else(|| std::io::Error::other("モード名を作れない"))?;
        text.push_str(app);
        text.push('\t');
        text.push_str(&name);
        text.push('\n');
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp = path.to_path_buf();
    temp.set_extension("tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

/// engine のモードの記憶の通知先。記録は [`Runtime`] の記憶へ入れる（保存は `flush_learning` のタイミング）。
///
/// secure input の判定は engine 側で行い、secure input 中は通知が来ない。記録するのはアプリの識別子と
/// モードだけで、入力内容は含まない。学習の停止（`learning.enabled` や停止アプリ）とは独立させている
/// （モードの記憶は学習ではない）。
pub struct AppModeObserver {
    runtime: Arc<Runtime>,
}

impl AppModeObserver {
    /// 作る
    pub fn new(runtime: Arc<Runtime>) -> Self {
        AppModeObserver { runtime }
    }
}

impl ModeObserver for AppModeObserver {
    fn on_mode_remembered(&self, app: &str, mode: InputMode) {
        self.runtime.app_mode_record(app, mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "nagi-app-modes-test-{name}-{}.tsv",
            std::process::id()
        ))
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = temp_path("roundtrip");
        let s = AppModeStore::new();
        s.record("com.jetbrains.intellij", InputMode::Alphanumeric);
        s.record("com.apple.Notes", InputMode::Hiragana);
        s.record("com.example.Katakana", InputMode::Katakana);
        assert!(s.is_dirty());
        s.flush(&path).expect("保存できる");
        assert!(!s.is_dirty());
        let text = std::fs::read_to_string(&path).expect("読める");
        assert!(text.starts_with("# nagi app_modes v1\n"));
        assert!(text.contains("com.jetbrains.intellij\talphanumeric\n"));
        let loaded = AppModeStore::new();
        assert!(loaded.load(&path));
        assert_eq!(loaded.snapshot(), s.snapshot());
        assert!(!loaded.is_dirty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn record_keys_are_lowercased_and_unchanged_values_are_not_dirty() {
        let s = AppModeStore::new();
        s.record("Com.Apple.Notes", InputMode::Hiragana);
        assert_eq!(
            s.snapshot(),
            vec![("com.apple.notes".to_string(), InputMode::Hiragana)]
        );
        let path = temp_path("unchanged");
        s.flush(&path).expect("保存できる");
        s.record("com.apple.notes", InputMode::Hiragana);
        assert!(!s.is_dirty());
        s.record("", InputMode::Hiragana);
        s.record("bad\tid", InputMode::Hiragana);
        assert_eq!(s.snapshot().len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bad_lines_and_unknown_modes_are_skipped() {
        let path = temp_path("bad");
        let text = "# nagi app_modes v1\n\
            com.good\talphanumeric\n\
            no-tab-line\n\
            com.unknown\tnot_a_mode\n\
            \thiragana\n\
            com.spaces\t  katakana  \n\
            com.also.good\thankaku_alnum\n";
        std::fs::write(&path, text).expect("書ける");
        let s = AppModeStore::new();
        assert!(s.load(&path));
        assert_eq!(
            s.snapshot(),
            vec![
                ("com.also.good".to_string(), InputMode::HankakuAlnum),
                ("com.good".to_string(), InputMode::Alphanumeric),
                ("com.spaces".to_string(), InputMode::Katakana),
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_or_wrong_header_loads_empty() {
        let s = AppModeStore::new();
        assert!(!s.load(&temp_path("missing")));
        assert!(s.snapshot().is_empty());
        let path = temp_path("header");
        std::fs::write(&path, "com.good\thiragana\n").expect("書ける");
        assert!(!s.load(&path));
        assert!(s.snapshot().is_empty());
        std::fs::write(&path, "").expect("書ける");
        assert!(!s.load(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clear_then_flush_removes_file() {
        let path = temp_path("clear");
        let s = AppModeStore::new();
        s.record("com.apple.notes", InputMode::Hiragana);
        s.flush(&path).expect("保存できる");
        assert!(path.exists());
        s.clear();
        assert!(s.snapshot().is_empty());
        s.flush(&path).expect("消せる");
        assert!(!path.exists());
        // 空のまま flush しても、ファイルが無ければエラーにならない
        s.clear();
        s.flush(&path).expect("無くても成功");
    }

    #[test]
    fn over_limit_drops_an_old_entry_not_the_new_one() {
        let s = AppModeStore::new();
        for i in 0..MAX_APPS {
            s.record(&format!("app{i:04}"), InputMode::Hiragana);
        }
        s.record("zzz.new", InputMode::Alphanumeric);
        let snap = s.snapshot();
        assert_eq!(snap.len(), MAX_APPS);
        assert!(
            snap.iter()
                .any(|(a, m)| a == "zzz.new" && *m == InputMode::Alphanumeric)
        );
    }
}
