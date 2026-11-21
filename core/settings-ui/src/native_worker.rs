//! ネイティブ設定ウィンドウのワーカースレッド（OS 非依存）。
//!
//! `imserver` の起動（最大 60 秒）・設定の書き込み・更新の確認（最大 60 秒）は UI スレッドで待てないので、
//! バックエンドは専用のスレッドが持つ。UI は [`Command`] を送り、完了は `notify`（Win32 では
//! `PostMessageW`）で知らされて [`WorkerHandle::try_recv`] で [`Reply`] を取り出す。
//! コマンドは送った順に 1 つずつ処理する（更新の確認中に送った設定変更は、確認の後に処理される）。

use crate::backend::{BackendError, SettingsBackend, UpdateCheck};
use crate::http::is_safe_notes_url;
use nagi_protocol::settings::{
    SettingChange, SettingValue, SettingsSnapshot, UpdateSettingsResult,
};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

/// バックエンドを作る関数（再試行のたびに呼ぶ。`imserver` の起動を含むので時間がかかる）
pub type Factory = Box<dyn Fn() -> Result<Box<dyn SettingsBackend>, BackendError> + Send>;

/// UI → ワーカー
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// （再）接続して設定を読む。既存のバックエンドは先に終了する
    Connect,
    /// 1 項目を書く（`value` が `None` なら既定に戻す）
    Update {
        /// 設定キー
        key: String,
        /// 新しい値
        value: Option<SettingValue>,
    },
    /// 学習データの全削除
    ClearLearning,
    /// 更新の確認
    CheckUpdate,
    /// 取得済みの更新の適用
    ApplyUpdate,
    /// 終了（バックエンドを閉じてスレッドを抜ける）
    Quit,
}

/// 読み込んだ設定
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// 項目と現在値
    pub snapshot: SettingsSnapshot,
    /// `config.toml` の場所
    pub config_path: Option<String>,
}

/// ワーカー → UI
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `Connect` の結果
    Loaded(Result<Loaded, BackendError>),
    /// `Update` の結果
    Updated {
        /// 書こうとしたキー
        key: String,
        /// 結果（項目ごとの検証エラーは `UpdateSettingsResult::errors`）
        result: Result<UpdateSettingsResult, BackendError>,
    },
    /// `ClearLearning` の結果
    LearningCleared(Result<(), BackendError>),
    /// `CheckUpdate` の結果（`notes_url` は `https` の安全な URL だけに絞り済み）
    UpdateChecked(Result<UpdateCheck, BackendError>),
    /// `ApplyUpdate` の結果
    UpdateApplied(Result<(), BackendError>),
}

/// ワーカースレッドの持ち主（UI スレッド側）
pub struct WorkerHandle {
    commands: Sender<Command>,
    replies: Receiver<Reply>,
    done: Receiver<()>,
    join: Option<JoinHandle<()>>,
}

type Backend = Option<Box<dyn SettingsBackend>>;

fn not_connected() -> BackendError {
    BackendError::transport("設定エンジンに接続していません")
}

fn connect(factory: &Factory) -> Result<(Box<dyn SettingsBackend>, Loaded), BackendError> {
    let mut backend = factory()?;
    let snapshot = backend.get_settings()?;
    // 場所が取れなくても画面は出す
    let config_path = backend.config_path().ok().flatten();
    Ok((
        backend,
        Loaded {
            snapshot,
            config_path,
        },
    ))
}

fn handle(command: Command, backend: &mut Backend, factory: &Factory) -> Option<Reply> {
    if command == Command::Connect {
        // 先に古いエンジンを終了する（Drop が shutdown する）
        *backend = None;
        return Some(match connect(factory) {
            Ok((b, loaded)) => {
                *backend = Some(b);
                Reply::Loaded(Ok(loaded))
            }
            Err(e) => Reply::Loaded(Err(e)),
        });
    }
    let b = backend.as_deref_mut();
    match command {
        Command::Connect | Command::Quit => None,
        Command::Update { key, value } => Some(Reply::Updated {
            result: match b {
                Some(b) => b.update_settings(vec![SettingChange {
                    key: key.clone(),
                    value,
                }]),
                None => Err(not_connected()),
            },
            key,
        }),
        Command::ClearLearning => Some(Reply::LearningCleared(
            b.map_or_else(|| Err(not_connected()), |b| b.clear_learning()),
        )),
        Command::CheckUpdate => Some(Reply::UpdateChecked(
            b.map_or_else(|| Err(not_connected()), |b| b.check_update())
                .map(|mut check| {
                    // 開いてよいのは https の安全な URL だけ（マニフェストは配布元の JSON）
                    check.notes_url = check.notes_url.take().filter(|u| is_safe_notes_url(u));
                    check
                }),
        )),
        Command::ApplyUpdate => Some(Reply::UpdateApplied(
            b.map_or_else(|| Err(not_connected()), |b| b.apply_update()),
        )),
    }
}

impl WorkerHandle {
    /// スレッドを起動する。`notify` は返信がキューに入るたびにワーカースレッドから呼ばれる
    /// （`PostMessageW` など、どのスレッドからでも安全なものだけを書く）
    pub fn start(factory: Factory, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (reply_tx, reply_rx) = mpsc::channel::<Reply>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let join = std::thread::spawn(move || {
            let mut backend: Backend = None;
            while let Ok(command) = command_rx.recv() {
                if command == Command::Quit {
                    break;
                }
                if let Some(reply) = handle(command, &mut backend, &factory) {
                    if reply_tx.send(reply).is_err() {
                        break;
                    }
                    notify();
                }
            }
            // imserver を終了する（最大で数秒かかる）
            drop(backend);
            let _ = done_tx.send(());
        });
        Self {
            commands: command_tx,
            replies: reply_rx,
            done: done_rx,
            join: Some(join),
        }
    }

    /// コマンドを送る（ワーカーが終わっていれば捨てる）
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// 溜まっている返信を 1 つ取り出す
    pub fn try_recv(&self) -> Option<Reply> {
        self.replies.try_recv().ok()
    }

    /// 終了を頼み、`timeout` まで待つ。間に合えば `true`。間に合わなければスレッドは放置する
    /// （子プロセスは `--parent-pid` で親の終了とともに止まる）
    pub fn shutdown(&mut self, timeout: Duration) -> bool {
        let _ = self.commands.send(Command::Quit);
        let finished = self.done.recv_timeout(timeout).is_ok();
        if finished && let Some(join) = self.join.take() {
            let _ = join.join();
        }
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::UpdateStatus;
    use nagi_protocol::settings::SettingError;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);

    struct Fake {
        log: Arc<Log>,
        notes: Option<String>,
    }

    fn empty_snapshot() -> SettingsSnapshot {
        SettingsSnapshot {
            items: vec![],
            error: None,
        }
    }

    impl SettingsBackend for Fake {
        fn get_settings(&mut self) -> Result<SettingsSnapshot, BackendError> {
            self.log.0.lock().unwrap().push("get".into());
            Ok(empty_snapshot())
        }

        fn update_settings(
            &mut self,
            changes: Vec<SettingChange>,
        ) -> Result<UpdateSettingsResult, BackendError> {
            if changes[0].key == "bad" {
                return Err(BackendError::rpc(-32602, "範囲外です"));
            }
            Ok(UpdateSettingsResult {
                applied: vec![],
                errors: vec![SettingError {
                    key: changes[0].key.clone(),
                    message: "x".into(),
                }],
                snapshot: empty_snapshot(),
            })
        }

        fn clear_learning(&mut self) -> Result<(), BackendError> {
            Ok(())
        }

        fn config_path(&mut self) -> Result<Option<String>, BackendError> {
            Ok(Some("C:/n/config.toml".into()))
        }

        fn check_update(&mut self) -> Result<UpdateCheck, BackendError> {
            Ok(UpdateCheck {
                current: "0.1.0".into(),
                latest: Some("0.2.0".into()),
                status: UpdateStatus::Available,
                notes_url: self.notes.clone(),
                error: None,
            })
        }

        fn apply_update(&mut self) -> Result<(), BackendError> {
            Err(BackendError::transport("適用できません"))
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.log.0.lock().unwrap().push("drop".into());
        }
    }

    fn start(notes: Option<&str>, fail_first: bool) -> (WorkerHandle, Arc<Log>, Arc<AtomicUsize>) {
        let log = Arc::new(Log::default());
        let notified = Arc::new(AtomicUsize::new(0));
        let attempts = Arc::new(AtomicUsize::new(0));
        let l = log.clone();
        let notes = notes.map(str::to_owned);
        let factory: Factory = Box::new(move || {
            if fail_first && attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(BackendError::transport("起動できません"));
            }
            Ok(Box::new(Fake {
                log: l.clone(),
                notes: notes.clone(),
            }))
        });
        let n = notified.clone();
        let handle = WorkerHandle::start(
            factory,
            Arc::new(move || {
                n.fetch_add(1, Ordering::SeqCst);
            }),
        );
        (handle, log, notified)
    }

    fn wait_reply(h: &WorkerHandle) -> Reply {
        for _ in 0..500 {
            if let Some(r) = h.try_recv() {
                return r;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("返信が来ません");
    }

    #[test]
    fn connect_loads_settings_and_config_path() {
        let (mut h, _log, notified) = start(None, false);
        h.send(Command::Connect);
        match wait_reply(&h) {
            Reply::Loaded(Ok(l)) => assert_eq!(l.config_path.as_deref(), Some("C:/n/config.toml")),
            other => panic!("{other:?}"),
        }
        assert_eq!(notified.load(Ordering::SeqCst), 1);
        assert!(h.shutdown(Duration::from_secs(5)));
    }

    #[test]
    fn commands_before_connect_fail_and_retry_works() {
        let (mut h, _log, _n) = start(None, true);
        h.send(Command::ClearLearning);
        assert!(matches!(wait_reply(&h), Reply::LearningCleared(Err(_))));
        h.send(Command::Connect);
        match wait_reply(&h) {
            Reply::Loaded(Err(e)) => assert_eq!(e.message, "起動できません"),
            other => panic!("{other:?}"),
        }
        h.send(Command::Connect);
        assert!(matches!(wait_reply(&h), Reply::Loaded(Ok(_))));
        h.send(Command::ClearLearning);
        assert!(matches!(wait_reply(&h), Reply::LearningCleared(Ok(()))));
        assert!(h.shutdown(Duration::from_secs(5)));
    }

    #[test]
    fn update_replies_carry_the_key_and_errors() {
        let (mut h, log, _n) = start(None, false);
        h.send(Command::Connect);
        let _ = wait_reply(&h);
        h.send(Command::Update {
            key: "a.b".into(),
            value: Some(SettingValue::Int(3)),
        });
        match wait_reply(&h) {
            Reply::Updated { key, result: Ok(r) } => {
                assert_eq!(key, "a.b");
                assert_eq!(r.errors[0].key, "a.b");
            }
            other => panic!("{other:?}"),
        }
        h.send(Command::Update {
            key: "bad".into(),
            value: None,
        });
        match wait_reply(&h) {
            Reply::Updated {
                key,
                result: Err(e),
            } => {
                assert_eq!(key, "bad");
                assert!(e.is_invalid_params());
            }
            other => panic!("{other:?}"),
        }
        assert!(h.shutdown(Duration::from_secs(5)));
        let log = log.0.lock().unwrap();
        assert_eq!(log.last().map(String::as_str), Some("drop"));
    }

    #[test]
    fn reconnect_drops_the_old_backend_first() {
        let (mut h, log, _n) = start(None, false);
        h.send(Command::Connect);
        let _ = wait_reply(&h);
        h.send(Command::Connect);
        let _ = wait_reply(&h);
        assert!(h.shutdown(Duration::from_secs(5)));
        let log = log.0.lock().unwrap().clone();
        assert_eq!(log, ["get", "drop", "get", "drop"]);
    }

    #[test]
    fn update_check_keeps_only_safe_notes_urls() {
        for (url, kept) in [
            ("https://example.com/notes", true),
            ("http://example.com/notes", false),
            ("javascript:alert(1)", false),
            ("https://example.com/a b", false),
        ] {
            let (mut h, _log, _n) = start(Some(url), false);
            h.send(Command::Connect);
            let _ = wait_reply(&h);
            h.send(Command::CheckUpdate);
            match wait_reply(&h) {
                Reply::UpdateChecked(Ok(c)) => assert_eq!(c.notes_url.is_some(), kept, "{url}"),
                other => panic!("{other:?}"),
            }
            h.send(Command::ApplyUpdate);
            assert!(matches!(wait_reply(&h), Reply::UpdateApplied(Err(_))));
            assert!(h.shutdown(Duration::from_secs(5)));
        }
    }
}
