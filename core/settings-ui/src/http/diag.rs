//! 「動作確認」（診断）の HTTP 側。実行はワーカースレッド（HTTP のループを止めない）、状態は [`DiagModel`]、実行・保存は [`DiagService`]。
//!
//! 画面（JS）は `status` をポーリングして進捗・結果を描く（実行中だけ。0.5 秒間隔）。
//! 状態の JSON 化（[`status_json`]）は純粋な関数で、ここでテストする。

use crate::diag::service::DiagService;
use crate::diag::{ItemId, ItemResult, Kind, Phase as DiagPhase, Report, Status};
use crate::diag_model::{
    BadgeKind, DiagModel, DiagProgress, RunState, badge_kind, badge_label, item_detail,
    summary_line,
};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard};

/// 開始できなかった理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    /// すでに実行中
    Busy,
    /// スレッドを作れなかった
    Spawn,
}

/// 診断の実行を持つ（ワーカースレッド + 共有の状態）
pub struct DiagRunner {
    service: Arc<dyn DiagService>,
    model: Arc<Mutex<DiagModel>>,
}

fn lock(model: &Mutex<DiagModel>) -> MutexGuard<'_, DiagModel> {
    // ワーカーが panic しても画面が固まらないよう、毒化は無視して中身を使う
    model.lock().unwrap_or_else(|e| e.into_inner())
}

impl DiagRunner {
    /// 診断サービスから作る
    pub fn new(service: Arc<dyn DiagService>) -> Self {
        Self {
            service,
            model: Arc::new(Mutex::new(DiagModel::default())),
        }
    }

    /// 全項目（`None`）または 1 項目の実行を始める
    pub fn start(&self, item: Option<ItemId>) -> Result<(), StartError> {
        let cancel = {
            let mut model = lock(&self.model);
            match item {
                None => model.begin_all(),
                Some(id) => model.begin_one(id),
            }
            .ok_or(StartError::Busy)?
        };
        let service = Arc::clone(&self.service);
        let model = Arc::clone(&self.model);
        let spawned = std::thread::Builder::new()
            .name("nagi-diag".to_owned())
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(|| match item {
                    None => {
                        let result = service.run_all(cancel.0.clone(), &mut |p| {
                            let progress = DiagProgress {
                                index: p.index,
                                total: p.total,
                                id: p.id,
                                result: (p.phase == DiagPhase::Finished)
                                    .then(|| p.result.cloned())
                                    .flatten(),
                            };
                            lock(&model).on_progress(&progress);
                        });
                        lock(&model).finish_all(result);
                    }
                    Some(id) => {
                        let result = service.run_one(id, cancel.0.clone());
                        lock(&model).finish_one(id, result);
                    }
                }));
                if outcome.is_err() {
                    let message = "診断の実行中に内部エラーが起きました".to_owned();
                    let mut model = lock(&model);
                    match item {
                        None => model.finish_all(Err(message)),
                        Some(id) => model.finish_one(id, Err(message)),
                    }
                }
            });
        if spawned.is_err() {
            let message = "診断を開始できません".to_owned();
            let mut model = lock(&self.model);
            match item {
                None => model.finish_all(Err(message)),
                Some(id) => model.finish_one(id, Err(message)),
            }
            return Err(StartError::Spawn);
        }
        Ok(())
    }

    /// 中止を求める（実行中の項目が終わったら残りはスキップ）
    pub fn cancel(&self) {
        lock(&self.model).request_cancel();
    }

    /// 画面に渡す状態
    pub fn status(&self) -> Value {
        status_json(&lock(&self.model))
    }

    /// コピー・保存できるレポートの本文（実行中・未実行なら `None`）
    pub fn report_text(&self) -> Option<String> {
        let model = lock(&self.model);
        model
            .can_export()
            .then(|| model.report())?
            .map(Report::to_text)
    }

    /// レポートを保存する。保存先（利用者名を伏せた表示用のパス）を返す
    pub fn save(&self) -> Result<String, SaveError> {
        let report = {
            let model = lock(&self.model);
            if !model.can_export() {
                return Err(SaveError::NoReport);
            }
            model.report().cloned().ok_or(SaveError::NoReport)?
        };
        self.service.save(&report).map_err(SaveError::Failed)
    }
}

impl Drop for DiagRunner {
    fn drop(&mut self) {
        // 画面を閉じるとき、実行中なら止める合図を立てる（残りの項目はスキップされる）
        lock(&self.model).cancel_for_close();
    }
}

/// 保存できなかった理由
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// 保存できるレポートが無い（未実行・実行中）
    NoReport,
    /// 保存に失敗した（メッセージは利用者に見せてよい）
    Failed(String),
}

fn badge_name(kind: BadgeKind) -> &'static str {
    match kind {
        BadgeKind::Ok => "ok",
        BadgeKind::Warn => "warn",
        BadgeKind::Ng => "ng",
        BadgeKind::Neutral => "neutral",
        BadgeKind::Info => "info",
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Ok => "ok",
        Status::Warn => "warn",
        Status::Ng => "ng",
        Status::Skipped => "skipped",
        Status::Manual => "manual",
    }
}

/// 項目 1 つの JSON。状態は `badge`（色分け）と `label`（文字。色だけに頼らない）の両方を持つ
pub fn item_json(id: ItemId, result: Option<&ItemResult>, running: bool) -> Value {
    json!({
        "id": id.as_str(),
        "title": id.title(),
        "kind": match id.kind() { Kind::Auto => "auto", Kind::Guide => "guide" },
        "running": running,
        "badge": badge_name(badge_kind(result, running)),
        "label": badge_label(result, running),
        "status": result.map(|r| status_name(r.status)),
        "detail": result.and_then(item_detail),
        "advice": result.and_then(|r| r.advice.as_deref()).map(str::trim).filter(|a| !a.is_empty()),
        "evidence": result.map(|r| r.evidence.clone()).unwrap_or_default(),
        "elapsed_ms": result.map(|r| r.elapsed_ms),
    })
}

/// 画面全体の状態の JSON
pub fn status_json(model: &DiagModel) -> Value {
    let items: Vec<Value> = ItemId::ALL
        .iter()
        .map(|&id| item_json(id, model.result(id), model.is_current(id)))
        .collect();
    let (mode, progress) = match model.run_state() {
        RunState::Idle => (Value::Null, Value::Null),
        RunState::All {
            index,
            total,
            current,
        } => (
            json!("all"),
            json!({
                "index": index,
                "total": total,
                "current": current.map(|id| json!({"id": id.as_str(), "title": id.title()})),
            }),
        ),
        RunState::One(id) => (
            json!("one"),
            json!({
                "index": 0,
                "total": 1,
                "current": {"id": id.as_str(), "title": id.title()},
            }),
        ),
    };
    let summary = model.report().map(|r| {
        let s = r.summary();
        json!({
            "ok": s.ok, "warn": s.warn, "ng": s.ng, "skipped": s.skipped, "manual": s.manual,
            "line": summary_line(r),
        })
    });
    json!({
        "running": model.is_running(),
        "cancelling": model.is_cancelling(),
        "mode": mode,
        "progress": progress,
        "progress_line": model.progress_line(),
        "note": model.note().map(|n| json!({"text": n.text, "is_error": n.is_error})),
        "can_export": model.can_export(),
        "summary": summary,
        "items": items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_status_lists_every_item_as_unchecked_with_spelled_out_labels() {
        let v = status_json(&DiagModel::default());
        assert_eq!(v["running"], false);
        assert_eq!(v["mode"], Value::Null);
        assert_eq!(v["can_export"], false);
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), ItemId::ALL.len());
        assert_eq!(items[0]["id"], "env.arch");
        assert_eq!(items[0]["label"], "未確認");
        assert_eq!(items[0]["badge"], "neutral");
        assert_eq!(items[0]["status"], Value::Null);
        let guide = items
            .iter()
            .find(|i| i["id"] == "manual.multi_dpi")
            .unwrap();
        assert_eq!(guide["kind"], "guide");
    }

    #[test]
    fn results_carry_label_detail_advice_and_evidence() {
        let r = ItemResult::ng(ItemId::EngineStart, " 起動できません ", " 入れ直す ")
            .with_evidence(["line1".to_owned()]);
        let v = item_json(ItemId::EngineStart, Some(&r), false);
        assert_eq!(v["label"], "NG");
        assert_eq!(v["badge"], "ng");
        assert_eq!(v["status"], "ng");
        assert_eq!(v["detail"], "起動できません");
        assert_eq!(v["advice"], "入れ直す");
        assert_eq!(v["evidence"], json!(["line1"]));
        // 確認中は結果より優先
        let running = item_json(ItemId::EngineStart, Some(&r), true);
        assert_eq!(running["label"], "確認中");
        assert_eq!(running["badge"], "info");
        // 手動項目は手順（advice）を持ち、バッジは info
        let m = ItemResult::manual(ItemId::ManualMultiDpi, "目視", "1. 移す");
        let v = item_json(ItemId::ManualMultiDpi, Some(&m), false);
        assert_eq!(v["label"], "手動確認");
        assert_eq!(v["advice"], "1. 移す");
    }

    #[test]
    fn progress_and_summary_follow_the_model() {
        let mut model = DiagModel::default();
        model.begin_all().unwrap();
        model.on_progress(&DiagProgress {
            index: 2,
            total: ItemId::ALL.len(),
            id: ItemId::EnvVcRuntime,
            result: None,
        });
        let v = status_json(&model);
        assert_eq!(v["running"], true);
        assert_eq!(v["mode"], "all");
        assert_eq!(v["progress"]["index"], 2);
        assert_eq!(v["progress"]["current"]["id"], "env.vcruntime");
        assert!(v["progress_line"].as_str().unwrap().contains("3/30"));
        let report = Report {
            tool_version: "0".into(),
            generated_at: "t".into(),
            environment: "e".into(),
            items: ItemId::ALL
                .iter()
                .map(|id| ItemResult::ok(*id, "良好"))
                .collect(),
        };
        model.finish_all(Ok(report));
        let v = status_json(&model);
        assert_eq!(v["running"], false);
        assert_eq!(v["can_export"], true);
        assert_eq!(v["summary"]["ok"], 30);
        assert!(v["summary"]["line"].as_str().unwrap().contains("OK 30"));
        assert!(
            v["note"]["text"]
                .as_str()
                .unwrap()
                .contains("確認が終わりました")
        );
        model.begin_one(ItemId::EnvUser).unwrap();
        let v = status_json(&model);
        assert_eq!(v["mode"], "one");
        assert_eq!(v["progress"]["current"]["id"], "env.user");
        assert_eq!(v["can_export"], false, "実行中は出力できない");
    }
}
