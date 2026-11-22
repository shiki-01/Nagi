//! 「動作確認」ページの状態（OS 非依存の純粋ロジック）。
//!
//! 診断そのもの（[`crate::diag`]）は別スレッドで動き、ここのモデルに進捗・結果を反映する。
//! HTTP 側（`http/diag.rs`）がこれを JSON にして画面（JS）へ返す。状態（OK / 要確認 / NG / スキップ / 手動確認）は
//! 色に頼らず、バッジの文字でも示す。

use crate::diag::{ItemId, ItemResult, Report, Status};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 診断の中断の合図（UI が持っていて、立てるとワーカーの診断が残りの項目をスキップする）。
/// 比較は同じ合図かどうか（ポインタ）で行う
#[derive(Debug, Clone)]
pub struct CancelFlag(pub Arc<AtomicBool>);

impl PartialEq for CancelFlag {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
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

/// 実行の状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// 何も実行していない
    Idle,
    /// 全項目を実行中
    All {
        /// 実行中（または直前に終えた）項目の番号（0 始まり）
        index: usize,
        /// 全項目数
        total: usize,
        /// いま実行している項目
        current: Option<ItemId>,
    },
    /// 1 項目だけ実行中
    One(ItemId),
}

/// 状態表示の 1 行（ページ内）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// 文言
    pub text: String,
    /// 失敗の知らせか
    pub is_error: bool,
}

/// 「動作確認」ページのモデル
#[derive(Debug)]
pub struct DiagModel {
    run: RunState,
    cancelling: bool,
    /// `ItemId::ALL` と同じ並びの最新の結果
    results: Vec<Option<ItemResult>>,
    /// 全項目を確認したときのレポート（コピー・保存の対象。1 項目だけの再確認は反映する）
    report: Option<Report>,
    note: Option<Note>,
    cancel: CancelFlag,
}

impl Default for DiagModel {
    fn default() -> Self {
        Self {
            run: RunState::Idle,
            cancelling: false,
            results: vec![None; ItemId::ALL.len()],
            report: None,
            note: None,
            cancel: new_flag(),
        }
    }
}

fn new_flag() -> CancelFlag {
    CancelFlag(Arc::new(AtomicBool::new(false)))
}

fn index_of(id: ItemId) -> Option<usize> {
    ItemId::ALL.iter().position(|i| *i == id)
}

impl DiagModel {
    /// 実行中か
    pub fn is_running(&self) -> bool {
        self.run != RunState::Idle
    }

    /// 実行の状態（Web 画面の進捗表示用）
    pub fn run_state(&self) -> RunState {
        self.run
    }

    /// 中止を要求済みか
    pub fn is_cancelling(&self) -> bool {
        self.cancelling
    }

    /// コピー・保存できるレポートがあるか（実行中は不可）
    pub fn can_export(&self) -> bool {
        !self.is_running() && self.report.is_some()
    }

    /// コピー・保存の対象
    pub fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }

    /// 項目の最新の結果
    pub fn result(&self, id: ItemId) -> Option<&ItemResult> {
        self.results.get(index_of(id)?)?.as_ref()
    }

    /// ページ内の状態表示
    pub fn note(&self) -> Option<&Note> {
        self.note.as_ref()
    }

    /// 状態表示を置く
    pub fn set_note(&mut self, text: impl Into<String>, is_error: bool) {
        self.note = Some(Note {
            text: text.into(),
            is_error,
        });
    }

    /// 全項目の実行を始める。実行中なら `None`。前回の結果は消す
    pub fn begin_all(&mut self) -> Option<CancelFlag> {
        if self.is_running() {
            return None;
        }
        self.results = vec![None; ItemId::ALL.len()];
        self.report = None;
        self.note = None;
        Some(self.begin(RunState::All {
            index: 0,
            total: ItemId::ALL.len(),
            current: None,
        }))
    }

    /// 1 項目の実行を始める。実行中なら `None`
    pub fn begin_one(&mut self, id: ItemId) -> Option<CancelFlag> {
        if self.is_running() {
            return None;
        }
        self.note = None;
        Some(self.begin(RunState::One(id)))
    }

    fn begin(&mut self, run: RunState) -> CancelFlag {
        self.run = run;
        self.cancelling = false;
        self.cancel = new_flag();
        self.cancel.clone()
    }

    /// 中止を求める（いま実行中の項目が終わったら、残りはスキップされる）。実行中でなければ何もしない
    pub fn request_cancel(&mut self) {
        if self.is_running() {
            self.cancelling = true;
            self.cancel.0.store(true, Ordering::Relaxed);
        }
    }

    /// ウィンドウを閉じるとき: 実行中の診断を止める合図を立てる
    pub fn cancel_for_close(&self) {
        self.cancel.0.store(true, Ordering::Relaxed);
    }

    /// 全項目の実行の途中経過を反映する
    pub fn on_progress(&mut self, p: &DiagProgress) {
        if !matches!(self.run, RunState::All { .. }) {
            return;
        }
        match &p.result {
            None => {
                self.run = RunState::All {
                    index: p.index,
                    total: p.total,
                    current: Some(p.id),
                };
            }
            Some(result) => {
                if let Some(slot) = index_of(p.id).and_then(|i| self.results.get_mut(i)) {
                    *slot = Some(result.clone());
                }
                self.run = RunState::All {
                    index: p.index,
                    total: p.total,
                    current: None,
                };
            }
        }
    }

    /// 全項目の実行が終わった
    pub fn finish_all(&mut self, result: Result<Report, String>) {
        let cancelled = self.cancelling;
        self.run = RunState::Idle;
        self.cancelling = false;
        match result {
            Ok(report) => {
                self.results = ItemId::ALL
                    .iter()
                    .map(|id| report.get(*id).cloned())
                    .collect();
                let line = summary_line(&report);
                self.note = Some(Note {
                    text: if cancelled {
                        format!("中止しました。{line}")
                    } else {
                        line
                    },
                    is_error: false,
                });
                self.report = Some(report);
            }
            Err(message) => {
                self.note = Some(Note {
                    text: format!("確認を実行できませんでした: {message}"),
                    is_error: true,
                });
            }
        }
    }

    /// 1 項目の実行が終わった
    pub fn finish_one(&mut self, id: ItemId, result: Result<ItemResult, String>) {
        self.run = RunState::Idle;
        self.cancelling = false;
        match result {
            Ok(result) => {
                if let Some(report) = &mut self.report
                    && let Some(slot) = report.items.iter_mut().find(|i| i.id == id)
                {
                    *slot = result.clone();
                }
                if let Some(slot) = index_of(id).and_then(|i| self.results.get_mut(i)) {
                    *slot = Some(result);
                }
                self.note = None;
            }
            Err(message) => {
                self.note = Some(Note {
                    text: format!("確認を実行できませんでした: {message}"),
                    is_error: true,
                });
            }
        }
    }

    /// いま実行している項目か
    pub fn is_current(&self, id: ItemId) -> bool {
        match self.run {
            RunState::All { current, .. } => current == Some(id),
            RunState::One(one) => one == id,
            RunState::Idle => false,
        }
    }

    /// 進捗の文言（実行中だけ）
    pub fn progress_line(&self) -> Option<String> {
        if self.cancelling {
            return Some("中止しています…（実行中の項目が終わるまでお待ちください）".to_owned());
        }
        match self.run {
            RunState::Idle => None,
            RunState::All {
                index,
                total,
                current,
            } => Some(match current {
                Some(id) => format!("確認しています… {}/{}: {}", index + 1, total, id.title()),
                None => format!("確認しています… {}/{}", (index + 1).min(total), total),
            }),
            RunState::One(id) => Some(format!("確認しています… {}", id.title())),
        }
    }
}

/// 結果の件数の文言
pub fn summary_line(report: &Report) -> String {
    let s = report.summary();
    format!(
        "確認が終わりました: NG {} / 要確認 {} / OK {} / スキップ {} / 手動確認 {}",
        s.ng, s.warn, s.ok, s.skipped, s.manual
    )
}

/// 結果行のバッジの色分け（ステータスの表示にも使う）。色に頼らず、文字（ラベル）も必ず併記する
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    /// 期待どおり
    Ok,
    /// 要確認
    Warn,
    /// 期待と違う
    Ng,
    /// 未確認・スキップ
    Neutral,
    /// 確認中・手動確認・お知らせ
    Info,
}

/// 項目の結果のバッジの色分け。`running` は確認中
pub fn badge_kind(result: Option<&ItemResult>, running: bool) -> BadgeKind {
    if running {
        return BadgeKind::Info;
    }
    match result.map(|r| r.status) {
        None | Some(Status::Skipped) => BadgeKind::Neutral,
        Some(Status::Ok) => BadgeKind::Ok,
        Some(Status::Warn) => BadgeKind::Warn,
        Some(Status::Ng) => BadgeKind::Ng,
        Some(Status::Manual) => BadgeKind::Info,
    }
}

/// 項目の結果のバッジの文字（見出しの `[...]` の中身と同じ）
pub fn badge_label(result: Option<&ItemResult>, running: bool) -> &'static str {
    if running {
        "確認中"
    } else {
        result.map_or("未確認", |r| r.status.label())
    }
}

/// 項目の詳細（無ければ `None`）
pub fn item_detail(result: &ItemResult) -> Option<String> {
    let detail = result.detail.trim();
    (!detail.is_empty()).then(|| detail.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_of(results: Vec<ItemResult>) -> Report {
        Report {
            tool_version: "0".into(),
            generated_at: "t".into(),
            environment: "e".into(),
            items: results,
        }
    }

    fn full_report(status: Status) -> Report {
        report_of(
            ItemId::ALL
                .iter()
                .map(|id| ItemResult::new(*id, status, "詳細"))
                .collect(),
        )
    }

    fn progress(index: usize, id: ItemId, done: Option<ItemResult>) -> DiagProgress {
        DiagProgress {
            index,
            total: ItemId::ALL.len(),
            id,
            result: done,
        }
    }

    #[test]
    fn run_all_lifecycle_and_progress_text() {
        let mut m = DiagModel::default();
        assert!(!m.is_running() && !m.can_export() && m.progress_line().is_none());
        let flag = m.begin_all().expect("開始できる");
        assert!(m.is_running());
        assert!(m.begin_all().is_none(), "実行中は重ねて始めない");
        assert!(m.begin_one(ItemId::EnvArch).is_none());
        assert_eq!(m.progress_line().as_deref(), Some("確認しています… 1/30"));
        m.on_progress(&progress(0, ItemId::EnvArch, None));
        assert!(m.is_current(ItemId::EnvArch));
        assert_eq!(
            m.progress_line(),
            Some(format!("確認しています… 1/30: {}", ItemId::EnvArch.title()))
        );
        let r = ItemResult::ok(ItemId::EnvArch, "良好");
        m.on_progress(&progress(0, ItemId::EnvArch, Some(r.clone())));
        assert!(!m.is_current(ItemId::EnvArch));
        assert_eq!(m.result(ItemId::EnvArch), Some(&r));
        // 中止
        m.request_cancel();
        assert!(m.is_cancelling() && flag.0.load(Ordering::Relaxed));
        assert!(m.progress_line().unwrap().starts_with("中止しています"));
        m.finish_all(Ok(full_report(Status::Skipped)));
        assert!(!m.is_running() && !m.is_cancelling() && m.can_export());
        assert!(
            m.note()
                .unwrap()
                .text
                .starts_with("中止しました。確認が終わりました")
        );
        // 次の実行は新しい合図を使う
        let next = m.begin_all().unwrap();
        assert!(!next.0.load(Ordering::Relaxed));
        assert!(m.report().is_none() && m.result(ItemId::EnvArch).is_none());
    }

    #[test]
    fn failure_to_run_is_reported_and_unlocks() {
        let mut m = DiagModel::default();
        m.begin_all();
        m.finish_all(Err("場所を決められません".into()));
        assert!(!m.is_running() && !m.can_export());
        let n = m.note().unwrap();
        assert!(n.is_error && n.text.contains("場所を決められません"));
        m.begin_one(ItemId::EnvUser);
        m.finish_one(ItemId::EnvUser, Err("x".into()));
        assert!(!m.is_running() && m.note().unwrap().is_error);
    }

    #[test]
    fn single_rerun_updates_the_report() {
        let mut m = DiagModel::default();
        m.begin_all();
        m.finish_all(Ok(full_report(Status::Ng)));
        assert_eq!(
            m.note().unwrap().text,
            "確認が終わりました: NG 30 / 要確認 0 / OK 0 / スキップ 0 / 手動確認 0"
        );
        m.begin_one(ItemId::EnvUser).unwrap();
        assert!(!m.can_export(), "実行中はコピー・保存できない");
        assert_eq!(
            m.progress_line(),
            Some(format!("確認しています… {}", ItemId::EnvUser.title()))
        );
        m.finish_one(
            ItemId::EnvUser,
            Ok(ItemResult::ok(ItemId::EnvUser, "直った")),
        );
        assert!(m.can_export());
        assert_eq!(m.result(ItemId::EnvUser).unwrap().status, Status::Ok);
        let report = m.report().unwrap();
        assert_eq!(report.get(ItemId::EnvUser).unwrap().detail, "直った");
        assert_eq!(report.summary().ng, 29);
        assert!(m.note().is_none());
    }

    #[test]
    fn cancel_does_nothing_when_idle() {
        let mut m = DiagModel::default();
        m.request_cancel();
        assert!(!m.is_cancelling());
    }

    #[test]
    fn badge_kind_maps_every_status() {
        let r = |s| Some(ItemResult::new(ItemId::EngineStart, s, "d"));
        assert_eq!(badge_kind(r(Status::Ok).as_ref(), false), BadgeKind::Ok);
        assert_eq!(badge_kind(r(Status::Warn).as_ref(), false), BadgeKind::Warn);
        assert_eq!(badge_kind(r(Status::Ng).as_ref(), false), BadgeKind::Ng);
        assert_eq!(
            badge_kind(r(Status::Skipped).as_ref(), false),
            BadgeKind::Neutral
        );
        assert_eq!(
            badge_kind(r(Status::Manual).as_ref(), false),
            BadgeKind::Info
        );
        assert_eq!(badge_kind(None, false), BadgeKind::Neutral);
        // 確認中は結果より優先
        assert_eq!(badge_kind(r(Status::Ng).as_ref(), true), BadgeKind::Info);
        assert_eq!(badge_label(None, true), "確認中");
        assert_eq!(badge_label(r(Status::Ng).as_ref(), false), "NG");
    }
}
