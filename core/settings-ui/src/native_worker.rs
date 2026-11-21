//! ネイティブ設定ウィンドウのワーカースレッド（OS 非依存）。
//!
//! `imserver` の起動（最大 60 秒）・設定の書き込み・更新の確認（最大 60 秒）は UI スレッドで待てないので、
//! バックエンドは専用のスレッドが持つ。UI は [`Command`] を送り、完了は `notify`（Win32 では
//! `PostMessageW`）で知らされて [`WorkerHandle::try_recv`] で [`Reply`] を取り出す。
//! コマンドは送った順に 1 つずつ処理する（更新の確認中に送った設定変更は、確認の後に処理される）。

use crate::backend::{BackendError, SettingsBackend, UpdateCheck};
use crate::diag::service::DiagService;
use crate::diag::{ItemId, ItemResult, Phase as DiagPhase, Progress, Report};
use crate::http::is_safe_notes_url;
use nagi_protocol::settings::{
    SettingChange, SettingValue, SettingsSnapshot, UpdateSettingsResult,
};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

/// バックエンドを作る関数（再試行のたびに呼ぶ。`imserver` の起動を含むので時間がかかる）
pub type Factory = Box<dyn Fn() -> Result<Box<dyn SettingsBackend>, BackendError> + Send>;

/// 診断の中断の合図（UI が持っていて、立てるとワーカーの診断が残りの項目をスキップする）。
/// 比較は同じ合図かどうか（ポインタ）で行う
#[derive(Debug, Clone)]
pub struct CancelFlag(pub Arc<AtomicBool>);

impl PartialEq for CancelFlag {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

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
    /// 動作確認: 全項目を実行する（進捗は [`Reply::DiagProgress`]、最後に [`Reply::DiagAllDone`]）
    DiagRunAll {
        /// 中断の合図
        cancel: CancelFlag,
    },
    /// 動作確認: 1 項目だけ実行する
    DiagRunOne {
        /// 項目
        id: ItemId,
        /// 中断の合図
        cancel: CancelFlag,
    },
    /// 動作確認: レポートのテキストをクリップボードへ（PowerShell を使うので待つ）
    DiagCopy {
        /// レポートのテキスト
        text: String,
    },
    /// 動作確認: レポートをファイルへ保存する
    DiagSave {
        /// 保存するレポート
        report: Box<Report>,
    },
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

/// 動作確認の進捗（項目の開始・終了。UI へ渡すために所有する形にしたもの）
#[derive(Debug, Clone, PartialEq)]
pub struct DiagProgress {
    /// 何番目か（0 始まり）
    pub index: usize,
    /// 全項目数
    pub total: usize,
    /// 項目
    pub id: ItemId,
    /// 終わったときの結果（開始のときは `None`）
    pub result: Option<ItemResult>,
}

impl DiagProgress {
    fn from_progress(p: &Progress) -> Self {
        Self {
            index: p.index,
            total: p.total,
            id: p.id,
            result: (p.phase == DiagPhase::Finished)
                .then(|| p.result.cloned())
                .flatten(),
        }
    }
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
    /// `DiagRunAll` の途中経過
    DiagProgress(DiagProgress),
    /// `DiagRunAll` の結果
    DiagAllDone(Result<Report, String>),
    /// `DiagRunOne` の結果
    DiagOneDone {
        /// 項目
        id: ItemId,
        /// 結果
        result: Result<ItemResult, String>,
    },
    /// `DiagCopy` の結果
    DiagCopied(Result<(), String>),
    /// `DiagSave` の結果（成功は保存先の表示用パス）
    DiagSaved(Result<String, String>),
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
        // 動作確認はバックエンドを使わない（`run_diag`）
        Command::DiagRunAll { .. }
        | Command::DiagRunOne { .. }
        | Command::DiagCopy { .. }
        | Command::DiagSave { .. } => None,
    }
}

/// 動作確認のコマンドを実行する。途中経過・結果は `emit` で返す。診断は自前の imserver を
/// 作業ディレクトリで動かすので、設定用のバックエンドには触れない。ただしワーカーは 1 本なので、
/// 実行中は他のコマンド（設定の保存など）が待たされる
fn run_diag(command: Command, diag: &dyn DiagService, emit: &mut dyn FnMut(Reply)) {
    match command {
        Command::DiagRunAll { cancel } => {
            let result = diag.run_all(cancel.0, &mut |p| {
                emit(Reply::DiagProgress(DiagProgress::from_progress(p)));
            });
            emit(Reply::DiagAllDone(result));
        }
        Command::DiagRunOne { id, cancel } => {
            emit(Reply::DiagOneDone {
                id,
                result: diag.run_one(id, cancel.0),
            });
        }
        Command::DiagCopy { text } => emit(Reply::DiagCopied(diag.copy(&text))),
        Command::DiagSave { report } => emit(Reply::DiagSaved(diag.save(&report))),
        _ => {}
    }
}

impl WorkerHandle {
    /// スレッドを起動する。`notify` は返信がキューに入るたびにワーカースレッドから呼ばれる
    /// （`PostMessageW` など、どのスレッドからでも安全なものだけを書く）
    pub fn start(
        factory: Factory,
        diag: Arc<dyn DiagService>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (reply_tx, reply_rx) = mpsc::channel::<Reply>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let join = std::thread::spawn(move || {
            let mut backend: Backend = None;
            while let Ok(command) = command_rx.recv() {
                if command == Command::Quit {
                    break;
                }
                let mut closed = false;
                {
                    let mut emit = |reply: Reply| {
                        if reply_tx.send(reply).is_err() {
                            closed = true;
                        } else {
                            notify();
                        }
                    };
                    match command {
                        Command::DiagRunAll { .. }
                        | Command::DiagRunOne { .. }
                        | Command::DiagCopy { .. }
                        | Command::DiagSave { .. } => run_diag(command, &*diag, &mut emit),
                        other => {
                            if let Some(reply) = handle(other, &mut backend, &factory) {
                                emit(reply);
                            }
                        }
                    }
                }
                if closed {
                    break;
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
    use crate::diag::Status;
    use nagi_protocol::settings::SettingError;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 2 項目だけ実行する偽の診断サービス（`cancel` が立っていれば 2 つ目をスキップする）
    struct FakeDiag;

    impl DiagService for FakeDiag {
        fn run_all(
            &self,
            cancel: Arc<AtomicBool>,
            progress: &mut dyn FnMut(&Progress),
        ) -> Result<Report, String> {
            let ids = [ItemId::EnvArch, ItemId::EngineStart];
            let mut items = Vec::new();
            for (index, id) in ids.into_iter().enumerate() {
                let p = |phase, result| Progress {
                    index,
                    total: 2,
                    id,
                    phase,
                    result,
                };
                progress(&p(DiagPhase::Started, None));
                let r = if cancel.load(Ordering::Relaxed) {
                    ItemResult::skipped(id, "中断されました")
                } else {
                    ItemResult::ok(id, "良好")
                };
                progress(&p(DiagPhase::Finished, Some(&r)));
                items.push(r);
            }
            Ok(Report {
                tool_version: "0".into(),
                generated_at: "t".into(),
                environment: "e".into(),
                items,
            })
        }

        fn run_one(&self, id: ItemId, _cancel: Arc<AtomicBool>) -> Result<ItemResult, String> {
            Ok(ItemResult::ng(id, "だめ", "直す"))
        }

        fn copy(&self, text: &str) -> Result<(), String> {
            if text.is_empty() {
                Err("空です".into())
            } else {
                Ok(())
            }
        }

        fn save(&self, _report: &Report) -> Result<String, String> {
            Ok("%LOCALAPPDATA%\\Nagi\\logs\\x.txt".into())
        }
    }

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
            Arc::new(FakeDiag),
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

    #[test]
    fn diag_run_all_streams_progress_then_the_report() {
        let (mut h, _log, notified) = start(None, false);
        let cancel = CancelFlag(Arc::new(AtomicBool::new(false)));
        h.send(Command::DiagRunAll {
            cancel: cancel.clone(),
        });
        let mut events = Vec::new();
        let report = loop {
            match wait_reply(&h) {
                Reply::DiagProgress(p) => events.push((p.index, p.result.is_some())),
                Reply::DiagAllDone(r) => break r.unwrap(),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(events, [(0, false), (0, true), (1, false), (1, true)]);
        assert_eq!(report.items.len(), 2);
        // 進捗 4 回 + 完了 1 回の通知
        assert_eq!(notified.load(Ordering::SeqCst), 5);
        // 中断の合図はサービスに届く
        cancel.0.store(true, Ordering::Relaxed);
        h.send(Command::DiagRunAll { cancel });
        let report = loop {
            if let Reply::DiagAllDone(r) = wait_reply(&h) {
                break r.unwrap();
            }
        };
        assert!(report.items.iter().all(|i| i.status == Status::Skipped));
        assert!(h.shutdown(Duration::from_secs(5)));
    }

    #[test]
    fn diag_single_copy_and_save_replies_and_no_backend_needed() {
        // 未接続（Connect していない）でも診断は動く
        let (mut h, _log, _n) = start(None, false);
        h.send(Command::DiagRunOne {
            id: ItemId::EnvUser,
            cancel: CancelFlag(Arc::new(AtomicBool::new(false))),
        });
        match wait_reply(&h) {
            Reply::DiagOneDone { id, result: Ok(r) } => {
                assert_eq!(id, ItemId::EnvUser);
                assert_eq!(r.status, Status::Ng);
            }
            other => panic!("{other:?}"),
        }
        h.send(Command::DiagCopy { text: "x".into() });
        assert_eq!(wait_reply(&h), Reply::DiagCopied(Ok(())));
        h.send(Command::DiagCopy {
            text: String::new(),
        });
        assert_eq!(wait_reply(&h), Reply::DiagCopied(Err("空です".into())));
        h.send(Command::DiagSave {
            report: Box::new(Report {
                tool_version: "0".into(),
                generated_at: "t".into(),
                environment: "e".into(),
                items: vec![],
            }),
        });
        assert!(matches!(wait_reply(&h), Reply::DiagSaved(Ok(p)) if p.ends_with("x.txt")));
        assert!(h.shutdown(Duration::from_secs(5)));
    }
}
