//! 「動作確認」ページの画面モデルと表示用の文字列整形（OS 非依存の純粋ロジック）。
//!
//! 診断そのもの（[`crate::diag`]）はワーカースレッドで動き、UI スレッドはここのモデルに進捗・結果を
//! 反映して、[`plan_rows`] が返す行の計画をそのまま Win32 のコントロールに写す。状態（OK / 要確認 / NG /
//! スキップ / 手動確認）は色に頼らず、見出しの文字（`[NG] 項目名`）で示す。

use crate::diag::{ItemId, ItemResult, Kind, Report, Status};
use crate::native_layout::RowKind;
use crate::native_model::ids;
use crate::native_worker::{CancelFlag, DiagProgress};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// `Page::Extra` に渡すページの識別子
pub const PAGE_ID: &str = "diag";
/// サイドバーの表示名（ページの見出しにもなる）
pub const PAGE_TITLE: &str = "動作確認";

/// ページ上部の説明
pub const INTRO: &str = "Nagi が正しく動く状態かを自動で確認します。確認の結果にあなたが入力した文章は含まれません（確認に使うのは固定のテスト文字列だけです）。すべて確認するには 1 分ほどかかることがあります。";

/// 項目のボタンの ID（`ids::DIAG_ITEM_BASE` + `ItemId::ALL` の添字）
pub fn item_button_id(id: ItemId) -> i32 {
    let index = ItemId::ALL.iter().position(|i| *i == id).unwrap_or(0);
    ids::DIAG_ITEM_BASE + index as i32
}

/// ボタンの ID から項目を引く
pub fn item_from_button_id(button: i32) -> Option<ItemId> {
    let index = usize::try_from(button.checked_sub(ids::DIAG_ITEM_BASE)?).ok()?;
    ItemId::ALL.get(index).copied()
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

/// 項目の見出し（状態を文字で示す）。例: `[NG] 変換エンジン（imserver）の起動と応答`
pub fn item_heading(id: ItemId, result: Option<&ItemResult>, running: bool) -> String {
    let state = if running {
        "確認中"
    } else {
        result.map_or("未確認", |r| r.status.label())
    };
    format!("[{state}] {}", id.title())
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

/// 項目の対処。手動確認は手順として出す
pub fn item_advice(result: &ItemResult) -> Option<String> {
    let advice = result.advice.as_deref()?.trim();
    if advice.is_empty() {
        return None;
    }
    Some(if result.status == Status::Manual {
        format!("手順:\n{advice}")
    } else {
        format!("対処: {advice}")
    })
}

/// 項目のボタンの、読み上げ用の名前
pub fn item_button_name(id: ItemId) -> String {
    format!("{}だけ確認", id.title())
}

/// ボタンの計画
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanBtn {
    /// コントロール ID
    pub id: i32,
    /// 表示する文字
    pub text: &'static str,
    /// 押せるか
    pub enabled: bool,
    /// 読み上げ用の名前（見える文字だけでは区別できないとき）
    pub accessible_name: Option<String>,
}

/// ページの行の計画（Win32 に写す前の形）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanRow {
    /// 節の見出し
    Section(String),
    /// 本文
    Text(String),
    /// 診断項目の結果行（状態のバッジ + 項目名）。画面上の文字（読み上げの名前）は
    /// [`PlanRow::heading`] の `[状態] 項目名`
    Result {
        /// バッジの色分け
        badge: BadgeKind,
        /// バッジの文字
        label: String,
        /// 項目名
        title: String,
    },
    /// 説明（グレーの小さめ）
    Desc(String),
    /// ボタンの行
    Buttons(Vec<PlanBtn>),
}

impl PlanRow {
    /// 行の種類（行間の空きの決定用）
    pub fn kind(&self) -> RowKind {
        match self {
            PlanRow::Section(_) => RowKind::Section,
            PlanRow::Text(_) | PlanRow::Result { .. } => RowKind::Text,
            PlanRow::Desc(_) => RowKind::Desc,
            PlanRow::Buttons(_) => RowKind::Buttons,
        }
    }
}

impl PlanRow {
    /// 結果行の文字列（`[状態] 項目名`。コントロールの文字 = 読み上げの名前）。結果行以外は `None`
    pub fn heading(&self) -> Option<String> {
        match self {
            PlanRow::Result { label, title, .. } => Some(format!("[{label}] {title}")),
            _ => None,
        }
    }
}

fn btn(id: i32, text: &'static str, enabled: bool) -> PlanBtn {
    PlanBtn {
        id,
        text,
        enabled,
        accessible_name: None,
    }
}

/// ページの行（見出しより下）。Tab 順 = 行の並び順:
/// 「すべて確認」（先頭 = 既定のフォーカス位置）→「中止」→ コピー → 保存 → 項目ごとのボタン
pub fn plan_rows(model: &DiagModel) -> Vec<PlanRow> {
    let running = model.is_running();
    let mut rows = vec![
        PlanRow::Desc(INTRO.to_owned()),
        PlanRow::Buttons(vec![
            btn(ids::DIAG_RUN_ALL, "すべて確認", !running),
            btn(ids::DIAG_CANCEL, "中止", running && !model.is_cancelling()),
        ]),
        PlanRow::Buttons(vec![
            btn(ids::DIAG_COPY, "レポートをコピー", model.can_export()),
            btn(ids::DIAG_SAVE, "レポートを保存", model.can_export()),
        ]),
    ];
    if let Some(line) = model.progress_line() {
        rows.push(PlanRow::Text(line));
    }
    if let Some(note) = model.note() {
        let text = if note.is_error {
            format!("エラー: {}", note.text)
        } else {
            note.text.clone()
        };
        rows.push(PlanRow::Text(text));
    }
    rows.push(PlanRow::Section("確認項目".to_owned()));
    for &id in ItemId::ALL {
        let result = model.result(id);
        let running_now = model.is_current(id);
        rows.push(PlanRow::Result {
            badge: badge_kind(result, running_now),
            label: badge_label(result, running_now).to_owned(),
            title: id.title().to_owned(),
        });
        if let Some(result) = result {
            if let Some(detail) = item_detail(result) {
                rows.push(PlanRow::Desc(detail));
            }
            if let Some(advice) = item_advice(result) {
                rows.push(PlanRow::Desc(advice));
            }
        }
        let text = match id.kind() {
            Kind::Auto => "この項目だけ確認",
            Kind::Guide => "この項目の手順を表示",
        };
        let mut b = btn(item_button_id(id), text, !running);
        b.accessible_name = Some(item_button_name(id));
        rows.push(PlanRow::Buttons(vec![b]));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_layout::{Gap, RowSpec, gap_between, layout_rows};

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
    fn status_is_always_spelled_out_in_the_heading() {
        let id = ItemId::EngineStart;
        for (status, label) in [
            (Status::Ok, "OK"),
            (Status::Warn, "要確認"),
            (Status::Ng, "NG"),
            (Status::Skipped, "スキップ"),
            (Status::Manual, "手動確認"),
        ] {
            let r = ItemResult::new(id, status, "x");
            assert_eq!(
                item_heading(id, Some(&r), false),
                format!("[{label}] {}", id.title())
            );
        }
        assert_eq!(
            item_heading(id, None, false),
            format!("[未確認] {}", id.title())
        );
        // 実行中は結果より優先して「確認中」
        let old = ItemResult::ok(id, "x");
        assert!(item_heading(id, Some(&old), true).starts_with("[確認中] "));
    }

    #[test]
    fn detail_and_advice_formatting() {
        let r = ItemResult::ng(
            ItemId::EngineStart,
            " 起動できません\n2 行目 ",
            " 入れ直す ",
        );
        assert_eq!(item_detail(&r).as_deref(), Some("起動できません\n2 行目"));
        assert_eq!(item_advice(&r).as_deref(), Some("対処: 入れ直す"));
        let ok = ItemResult::ok(ItemId::EnvArch, "良好");
        assert_eq!(item_advice(&ok), None);
        assert_eq!(item_detail(&ItemResult::ok(ItemId::EnvArch, "  ")), None);
        let m = ItemResult::manual(
            ItemId::ManualMultiDpi,
            "確認してください",
            "1. 開く\n2. 移す",
        );
        assert_eq!(item_advice(&m).as_deref(), Some("手順:\n1. 開く\n2. 移す"));
        assert_eq!(
            item_button_name(ItemId::EnvArch),
            format!("{}だけ確認", ItemId::EnvArch.title())
        );
    }

    #[test]
    fn button_ids_round_trip_and_stay_below_the_dynamic_range() {
        for &id in ItemId::ALL {
            let b = item_button_id(id);
            assert!(b > ids::DIAG_ITEM_BASE - 1 && b < ids::DYNAMIC_BASE);
            assert_eq!(item_from_button_id(b), Some(id));
            assert!(!ids::FIXED.contains(&b));
        }
        assert_eq!(item_from_button_id(ids::DIAG_ITEM_BASE - 1), None);
        assert_eq!(
            item_from_button_id(ids::DIAG_ITEM_BASE + ItemId::ALL.len() as i32),
            None
        );
        assert_eq!(item_from_button_id(i32::MIN), None);
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

    fn buttons(rows: &[PlanRow]) -> Vec<&PlanBtn> {
        rows.iter()
            .filter_map(|r| match r {
                PlanRow::Buttons(b) => Some(b),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn plan_has_the_default_action_first_and_unique_ids() {
        let m = DiagModel::default();
        let rows = plan_rows(&m);
        let btns = buttons(&rows);
        // Tab 順の先頭は「すべて確認」（押せる）
        assert_eq!(btns[0].id, ids::DIAG_RUN_ALL);
        assert!(btns[0].enabled);
        assert_eq!(btns.len(), 4 + ItemId::ALL.len());
        let mut seen: Vec<i32> = btns.iter().map(|b| b.id).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), btns.len());
        // 結果の前は中止・コピー・保存が押せない
        for b in &btns[1..4] {
            assert!(!b.enabled, "{}", b.text);
        }
        // 項目ごとのボタンには項目名つきの名前がある
        for b in &btns[4..] {
            assert!(b.accessible_name.as_deref().unwrap().contains("だけ確認"));
        }
        // 項目の見出しは全項目ぶん
        let headings = rows
            .iter()
            .filter(|r| r.heading().is_some_and(|t| t.starts_with("[未確認] ")))
            .count();
        assert_eq!(headings, ItemId::ALL.len());
    }

    #[test]
    fn plan_while_running_and_after() {
        let mut m = DiagModel::default();
        m.begin_all();
        let rows = plan_rows(&m);
        let btns = buttons(&rows);
        assert!(!btns[0].enabled, "実行中は「すべて確認」を押せない");
        assert!(btns[1].enabled, "中止は押せる");
        assert!(btns[4..].iter().all(|b| !b.enabled));
        m.request_cancel();
        assert!(
            !buttons(&plan_rows(&m))[1].enabled,
            "中止済みなら重ねて押せない"
        );
        m.finish_all(Ok(full_report(Status::Warn)));
        let rows = plan_rows(&m);
        let btns = buttons(&rows);
        assert!(btns[0].enabled && !btns[1].enabled && btns[2].enabled && btns[3].enabled);
        assert!(btns[4..].iter().all(|b| b.enabled));
        // 結果の詳細は説明の行として出る
        assert!(rows.contains(&PlanRow::Desc("詳細".into())));
        assert!(
            rows.iter()
                .any(|r| r.heading().is_some_and(|t| t.starts_with("[要確認] ")))
        );
    }

    #[test]
    fn manual_items_show_steps_and_a_steps_button() {
        let mut m = DiagModel::default();
        m.begin_all();
        let mut report = full_report(Status::Ok);
        for item in &mut report.items {
            if item.id == ItemId::ManualMultiDpi {
                *item = ItemResult::manual(ItemId::ManualMultiDpi, "目視", "1. 移す");
            }
        }
        m.finish_all(Ok(report));
        let rows = plan_rows(&m);
        assert!(rows.contains(&PlanRow::Desc("手順:\n1. 移す".into())));
        let manual_btn = buttons(&rows)
            .into_iter()
            .find(|b| b.id == item_button_id(ItemId::ManualMultiDpi))
            .unwrap();
        assert_eq!(manual_btn.text, "この項目の手順を表示");
    }

    #[test]
    fn errors_in_the_note_are_labelled_and_spacing_groups_each_item() {
        let mut m = DiagModel::default();
        m.set_note("保存できません", true);
        let rows = plan_rows(&m);
        assert!(rows.contains(&PlanRow::Text("エラー: 保存できません".into())));
        // 行間: 項目の見出し・詳細・ボタンは詰め、項目と項目の間は通常の空き
        let mut prev = None;
        let mut tight = 0;
        let mut separate = 0;
        for r in rows
            .iter()
            .skip_while(|r| !matches!(r, PlanRow::Section(_)))
        {
            let gap = gap_between(prev, r.kind());
            match (prev, r.kind()) {
                (Some(RowKind::Desc | RowKind::Text), RowKind::Buttons) => {
                    assert_eq!(gap, Gap::Tight);
                    tight += 1;
                }
                (Some(RowKind::Buttons), RowKind::Text) => {
                    assert_eq!(gap, Gap::Row);
                    separate += 1;
                }
                _ => {}
            }
            prev = Some(r.kind());
        }
        assert_eq!(tight, ItemId::ALL.len());
        assert_eq!(separate, ItemId::ALL.len() - 1);
        // 全行を実際に積んでも、内容の高さは DPI に応じて増えるだけ（4dp グリッドの空きで破綻しない）
        let heights: Vec<i32> = [96u32, 144, 192]
            .iter()
            .map(|dpi| {
                let specs: Vec<RowSpec> = rows
                    .iter()
                    .enumerate()
                    .map(|(n, r)| RowSpec {
                        gap: gap_between(n.checked_sub(1).map(|p| rows[p].kind()), r.kind()),
                        title_h: 16,
                        control_h: if matches!(r, PlanRow::Buttons(_)) {
                            28
                        } else {
                            0
                        },
                        ..RowSpec::default()
                    })
                    .collect();
                layout_rows(&specs, *dpi).content_height
            })
            .collect();
        assert!(heights[0] < heights[1] && heights[1] < heights[2]);
    }

    #[test]
    fn result_rows_carry_a_badge_and_a_spoken_heading() {
        let mut m = DiagModel::default();
        let rows = plan_rows(&m);
        let results: Vec<&PlanRow> = rows
            .iter()
            .filter(|r| matches!(r, PlanRow::Result { .. }))
            .collect();
        assert_eq!(results.len(), ItemId::ALL.len());
        // 未確認は灰色のバッジ、文字は必ず併記
        match results[0] {
            PlanRow::Result {
                badge,
                label,
                title,
            } => {
                assert_eq!(*badge, BadgeKind::Neutral);
                assert_eq!(label, "未確認");
                assert_eq!(results[0].heading().unwrap(), format!("[未確認] {title}"));
            }
            _ => unreachable!(),
        }
        assert_eq!(PlanRow::Text("x".into()).heading(), None);
        // 結果が入るとバッジの色が変わる
        m.finish_all(Ok(full_report(Status::Ng)));
        let rows = plan_rows(&m);
        assert!(rows.iter().any(
            |r| matches!(r, PlanRow::Result { badge: BadgeKind::Ng, label, .. } if label == "NG")
        ));
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
