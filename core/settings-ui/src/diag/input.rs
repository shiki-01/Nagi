//! 入力まわりの診断（imserver に JSON-RPC で数ケース流して期待結果と比較する）: 曖昧入力（H-88・H-89）と再変換。
//!
//! 流すテキストは診断用の固定文字列で、利用者の入力は含まない。診断用のエンジンは
//! 作業ディレクトリの `config.toml`（`[input.fuzzy] enabled = true`）で起動する。

use super::ctx::Ctx;
use super::engine::{Engines, Rpc, call};
use super::types::{ItemId, ItemResult};
use crate::backend::BackendError;
use nagi_protocol::key::keysym;
use nagi_protocol::methods::{ProcessKeyResult, method};
use nagi_protocol::view::{CandidateSource, CandidateView};
use serde_json::{Value, json};
use std::time::Duration;

/// 期待する結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// 補正後の読みが（全体または一部として）これと一致する「もしかして」候補が出る
    Reading(&'static str),
    /// 「もしかして」候補が出ない（普通の語・文での誤表示がない）
    NoFuzzy,
}

/// 試す 1 ケース
#[derive(Debug, Clone, Copy)]
pub struct FuzzyCase {
    /// 表示名
    pub label: &'static str,
    /// 打つローマ字
    pub typed: &'static str,
    /// 期待
    pub expect: Expect,
    /// 外れたら NG にするか（`false` は要確認止まり。辞書・強さに依存するケース）
    pub required: bool,
    /// 参考（結果は根拠に残すが、合否に影響させない）
    pub reference: bool,
}

/// `docs/real-hardware-checklist.md` §9.1・§9.2 の代表ケース
pub const FUZZY_CASES: &[FuzzyCase] = &[
    FuzzyCase {
        // 「在り難う」「あり難う」が辞書にあり、読みとして成立するため、辞書のみ（LLM なし）の診断用エンジンでは
        // 出ない（H-88 の「成立する読み」の扱い）。LLM の再採点が要る可能性があるので、外れても要確認止まり
        label: "濁点の付け忘れ（ありかとう）",
        typed: "arikatou",
        expect: Expect::Reading("ありがとう"),
        required: false,
        reference: true,
    },
    FuzzyCase {
        label: "誤り辞書（ありがとうごさいます）",
        typed: "arigatougosaimasu",
        expect: Expect::Reading("ありがとうございます"),
        required: true,
        reference: false,
    },
    FuzzyCase {
        label: "隣のキーの打ち間違い（kamji）",
        typed: "kamji",
        expect: Expect::Reading("かんじ"),
        required: false,
        reference: false,
    },
    FuzzyCase {
        label: "濁点の付け忘れ（おねかいします）",
        typed: "onekaisimasu",
        expect: Expect::Reading("おねがいします"),
        required: false,
        reference: false,
    },
    FuzzyCase {
        label: "普通の語（がっこう）で誤表示しない",
        typed: "gakkou",
        expect: Expect::NoFuzzy,
        required: false,
        reference: false,
    },
    FuzzyCase {
        label: "普通の文（きょうはいいてんきです）で誤表示しない",
        typed: "kyouhaiitenkidesu",
        expect: Expect::NoFuzzy,
        required: false,
        reference: false,
    },
];

/// 1 ケースの結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseOutcome {
    /// 表示名
    pub label: &'static str,
    /// 期待どおりか
    pub passed: bool,
    /// 外れたら NG にするケースか
    pub required: bool,
    /// 参考（合否に影響させない）
    pub reference: bool,
    /// 期待が「出ること」か（`false` は誤表示がないことの検査）
    pub positive: bool,
    /// 補足（出た「もしかして」の読み・エラー。入力内容は含まない）
    pub note: String,
}

fn key_params(session: &str, keysym: u32, text: &str) -> Value {
    json!({
        "session_id": session, "keysym": keysym, "keycode": 0, "text": text,
        "modifiers": {"shift": false}, "kind": "down"
    })
}

fn press(
    rpc: &mut dyn Rpc,
    session: &str,
    keysym: u32,
    text: &str,
    timeout: Duration,
) -> Result<ProcessKeyResult, BackendError> {
    call(
        rpc,
        method::PROCESS_KEY,
        key_params(session, keysym, text),
        timeout,
    )
}

fn fuzzy_items(result: &ProcessKeyResult) -> Vec<CandidateView> {
    result
        .candidates
        .as_ref()
        .map(|c| {
            c.items
                .iter()
                .filter(|i| i.sources.contains(&CandidateSource::Fuzzy))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// 補正後の読み `got` が期待 `want` に合うか（文節に分かれて一部だけのこともあるので、包含も許す）
fn reading_matches(got: &str, want: &str) -> bool {
    !got.is_empty() && (got == want || want.contains(got) || got.contains(want))
}

/// 1 ケースを流す。通信の失敗は `Err`（以降のケースも流せない）
pub fn run_case(
    rpc: &mut dyn Rpc,
    session: &str,
    case: &FuzzyCase,
    timeout: Duration,
) -> Result<CaseOutcome, BackendError> {
    let mut fuzzy: Vec<CandidateView> = Vec::new();
    for c in case.typed.chars() {
        let r = press(rpc, session, c as u32, &c.to_string(), timeout)?;
        fuzzy.extend(fuzzy_items(&r));
    }
    // Space で変換し、注目文節を右へ動かしながら候補を集める（誤りが後ろの文節にあっても拾う）
    let r = press(rpc, session, keysym::SPACE, " ", timeout)?;
    fuzzy.extend(fuzzy_items(&r));
    for _ in 0..3 {
        let r = press(rpc, session, keysym::RIGHT, "", timeout)?;
        fuzzy.extend(fuzzy_items(&r));
    }
    // 状態を戻す（次のケースに持ち越さない）
    let _ = rpc.call_value(method::RESET, json!({"session_id": session}), timeout);
    let readings: Vec<String> = fuzzy
        .iter()
        .filter_map(|i| i.reading.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let shown = if readings.is_empty() {
        "なし".to_owned()
    } else {
        readings.join("・")
    };
    Ok(match case.expect {
        Expect::Reading(want) => {
            let hit = readings.iter().any(|g| reading_matches(g, want));
            CaseOutcome {
                label: case.label,
                passed: hit,
                required: case.required,
                reference: case.reference,
                positive: true,
                note: format!("期待: 「もしかして」で {want} / 実際の補正読み: {shown}"),
            }
        }
        Expect::NoFuzzy => CaseOutcome {
            label: case.label,
            passed: fuzzy.is_empty(),
            required: false,
            reference: false,
            positive: false,
            note: format!("期待: 「もしかして」なし / 実際の補正読み: {shown}"),
        },
    })
}

/// 全ケースを判定する
pub fn judge_fuzzy(outcomes: &[CaseOutcome]) -> ItemResult {
    let id = ItemId::InputFuzzy;
    let lines: Vec<String> = outcomes
        .iter()
        .map(|o| {
            format!(
                "{}: {}（{}）",
                if o.reference {
                    "参考"
                } else if o.passed {
                    "合格"
                } else {
                    "不一致"
                },
                o.label,
                o.note
            )
        })
        .collect();
    let positives: Vec<&CaseOutcome> = outcomes
        .iter()
        .filter(|o| o.positive && !o.reference)
        .collect();
    let required_miss: Vec<&str> = positives
        .iter()
        .filter(|o| !o.passed && o.required)
        .map(|o| o.label)
        .collect();
    let optional_miss: Vec<&str> = positives
        .iter()
        .filter(|o| !o.passed && !o.required)
        .map(|o| o.label)
        .collect();
    let false_alarm: Vec<&str> = outcomes
        .iter()
        .filter(|o| !o.positive && !o.passed && !o.reference)
        .map(|o| o.label)
        .collect();
    if !positives.is_empty() && positives.iter().all(|o| !o.passed) {
        return ItemResult::ng(
            id,
            "「もしかして」候補がどのケースでも出ません",
            "曖昧入力が働いていません。辞書（data.files）が入っているか、imserver の版が曖昧入力に対応しているか（コア 0.1.10 以降）を確認してください。",
        )
        .with_evidence(lines);
    }
    if !required_miss.is_empty() {
        return ItemResult::ng(
            id,
            format!(
                "必須のケースで「もしかして」が出ません: {}",
                required_miss.join(" / ")
            ),
            "誤り辞書・濁点補正が働いていません。報告してください（結果の一覧を貼ってください）。",
        )
        .with_evidence(lines);
    }
    if !optional_miss.is_empty() || !false_alarm.is_empty() {
        let mut parts = Vec::new();
        if !optional_miss.is_empty() {
            parts.push(format!("出なかったケース: {}", optional_miss.join(" / ")));
        }
        if !false_alarm.is_empty() {
            parts.push(format!("誤表示のケース: {}", false_alarm.join(" / ")));
        }
        return ItemResult::warn(
            id,
            parts.join(" / "),
            "辞書・強さ（strength）に依存するケースです。結果の一覧を報告してください（20 文で 0〜2 件の誤表示は許容範囲）。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(
        id,
        format!(
            "{} ケースすべてで期待どおりでした",
            outcomes.iter().filter(|o| !o.reference).count()
        ),
    )
    .with_evidence(lines)
}

/// 曖昧入力項目
pub fn check_fuzzy(ctx: &Ctx, engines: &mut Engines) -> ItemResult {
    let id = ItemId::InputFuzzy;
    if !dict_present(ctx) {
        return ItemResult::skipped(
            id,
            "辞書がないため判定できません（data.files を参照。辞書がなければ曖昧入力は動きません）",
        );
    }
    let session = match engines.probe(ctx) {
        Ok(s) => s,
        Err(_) => {
            return ItemResult::skipped(
                id,
                "変換エンジンを起動できないため判定できません（engine.start を参照）",
            );
        }
    };
    let mut outcomes = Vec::new();
    for (i, case) in FUZZY_CASES.iter().enumerate() {
        if ctx.cancelled() {
            break;
        }
        match run_case(
            &mut session.rpc,
            &format!("diag-fuzzy-{i}"),
            case,
            ctx.timeouts.request,
        ) {
            Ok(o) => outcomes.push(o),
            Err(e) => {
                return ItemResult::ng(
                    id,
                    format!(
                        "ケース「{}」の途中でエンジンが応答しなくなりました（{}）",
                        case.label, e.message
                    ),
                    "エンジンが落ちた可能性があります。imserver-*.log の末尾を報告してください。",
                )
                .with_evidence(session.stderr_tail());
            }
        }
    }
    judge_fuzzy(&outcomes)
}

fn dict_present(ctx: &Ctx) -> bool {
    ctx.data_dir
        .as_ref()
        .map(|d| d.join("dict").join("nagi-dict.bin"))
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|m| m.len() > 0)
}

/// 再変換に使う固定の確定済みテキスト
pub const RECONVERT_TEXT: &str = "日本語";

/// 再変換の結果を判定する。`commit` は直後に Enter を押したときの確定文字列
pub fn judge_reconvert(
    result: &ProcessKeyResult,
    commit: Option<&str>,
    alive_after: bool,
) -> ItemResult {
    let id = ItemId::InputReconvert;
    let Some(preedit) = &result.preedit else {
        return ItemResult::ng(
            id,
            "再変換の要求に対してプリエディットが返りませんでした",
            "エンジンの reconvert が働いていません。報告してください。",
        );
    };
    let mut lines: Vec<String> = preedit
        .segments
        .iter()
        .map(|s| {
            format!(
                "文節: 種別 {:?} / 読みの文字数 {} / 表記の文字数 {}",
                s.kind,
                s.reading.chars().count(),
                s.surface.chars().count()
            )
        })
        .collect();
    lines.push(format!(
        "直後の確定: {}",
        match commit {
            Some(c) if c == RECONVERT_TEXT => "元のテキストと同じ（失われた文節なし）",
            Some(_) => "元のテキストと違う",
            None => "なし",
        }
    ));
    let japanese = preedit.segments.iter().any(|s| {
        matches!(s.kind, nagi_protocol::view::SegmentKind::Japanese) && s.reading != s.surface
    });
    if !japanese {
        return ItemResult::ng(
            id,
            "読みを逆引きできませんでした（Literal の 1 文節になりました）",
            "辞書が入っているか（data.files）を確認してください。入っていれば報告してください（逆引き索引の不具合）。",
        )
        .with_evidence(lines);
    }
    if !alive_after {
        return ItemResult::ng(
            id,
            "再変換のあと、エンジンが応答しなくなりました",
            "エンジンが落ちた可能性があります。imserver-*.log の末尾を報告してください。",
        )
        .with_evidence(lines);
    }
    match commit {
        Some(c) if c == RECONVERT_TEXT => ItemResult::ok(
            id,
            "確定済みテキストを再変換でき、そのまま確定すると元のテキストに戻りました（エンジン側）。アプリ側の再変換は手動項目で確認してください",
        )
        .with_evidence(lines),
        _ => ItemResult::warn(
            id,
            "再変換はできましたが、直後に確定したテキストが元と一致しません",
            "文節が失われている可能性があります。報告してください。",
        )
        .with_evidence(lines),
    }
}

/// 再変換項目
pub fn check_reconvert(ctx: &Ctx, engines: &mut Engines) -> ItemResult {
    let id = ItemId::InputReconvert;
    if !dict_present(ctx) {
        return ItemResult::skipped(
            id,
            "辞書がないため判定できません（辞書がないと読みを逆引きできません。data.files を参照）",
        );
    }
    let session = match engines.probe(ctx) {
        Ok(s) => s,
        Err(_) => {
            return ItemResult::skipped(
                id,
                "変換エンジンを起動できないため判定できません（engine.start を参照）",
            );
        }
    };
    let sid = "diag-reconvert";
    // 初回は逆引き索引の構築を待つことがある
    let result: Result<ProcessKeyResult, BackendError> = call(
        &mut session.rpc,
        method::RECONVERT,
        json!({"session_id": sid, "text": RECONVERT_TEXT}),
        ctx.timeouts.reconvert,
    );
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            return ItemResult::ng(
                id,
                format!("reconvert が失敗しました（{}）", e.message),
                "逆引き索引の構築が時間切れの可能性があります。もう一度実行し、同じなら報告してください。",
            )
            .with_evidence(session.stderr_tail());
        }
    };
    let commit = press(
        &mut session.rpc,
        sid,
        keysym::RETURN,
        "",
        ctx.timeouts.request,
    )
    .ok()
    .and_then(|r| r.commit);
    let alive = session
        .rpc
        .call_value(method::PING, json!({}), ctx.timeouts.request)
        .is_ok();
    judge_reconvert(&result, commit.as_deref(), alive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::types::Status;
    use nagi_protocol::view::{CandidatesView, PreeditView, SegmentKind, SegmentView};
    use std::collections::HashMap;

    /// 打ったローマ字ごとに決まった「もしかして」読みを返すフェイク
    struct FakeEngine {
        typed: HashMap<String, String>,
        fuzzy_by_typed: HashMap<&'static str, &'static str>,
        fail_after: Option<usize>,
        calls: usize,
    }

    impl FakeEngine {
        fn new(fuzzy: &[(&'static str, &'static str)]) -> Self {
            Self {
                typed: HashMap::new(),
                fuzzy_by_typed: fuzzy.iter().copied().collect(),
                fail_after: None,
                calls: 0,
            }
        }
    }

    impl Rpc for FakeEngine {
        fn call_value(
            &mut self,
            m: &str,
            params: Value,
            _t: Duration,
        ) -> Result<Value, BackendError> {
            self.calls += 1;
            if self.fail_after.is_some_and(|n| self.calls > n) {
                return Err(BackendError::transport("設定エンジンの応答がありません"));
            }
            let sid = params["session_id"].as_str().unwrap_or("").to_owned();
            match m {
                "process_key" => {
                    let sym = params["keysym"].as_u64().unwrap_or(0) as u32;
                    let mut result = ProcessKeyResult::default();
                    if sym == keysym::SPACE {
                        let typed = self.typed.get(&sid).cloned().unwrap_or_default();
                        let mut items = vec![CandidateView {
                            surface: "候補".into(),
                            sources: vec![CandidateSource::SystemDict],
                            covers: None,
                            reading: None,
                        }];
                        if let Some(r) = self.fuzzy_by_typed.get(typed.as_str()) {
                            items.push(CandidateView {
                                surface: "補正".into(),
                                sources: vec![CandidateSource::Fuzzy],
                                covers: None,
                                reading: Some((*r).to_owned()),
                            });
                        }
                        result.candidates = Some(CandidatesView {
                            items,
                            visible: true,
                            ..CandidatesView::default()
                        });
                    } else if sym < 0x80 {
                        self.typed
                            .entry(sid)
                            .or_default()
                            .push(char::from_u32(sym).unwrap());
                    }
                    Ok(serde_json::to_value(result).unwrap())
                }
                _ => Ok(json!({})),
            }
        }
    }

    fn run_all_cases(engine: &mut FakeEngine) -> Vec<CaseOutcome> {
        FUZZY_CASES
            .iter()
            .enumerate()
            .map(|(i, c)| run_case(engine, &format!("s{i}"), c, Duration::from_secs(1)).unwrap())
            .collect()
    }

    #[test]
    fn all_cases_pass_when_engine_behaves() {
        let mut e = FakeEngine::new(&[
            ("arikatou", "ありがとう"),
            ("arigatougosaimasu", "ありがとうございます"),
            ("kamji", "かんじ"),
            ("onekaisimasu", "おねがい"),
        ]);
        let outcomes = run_all_cases(&mut e);
        let r = judge_fuzzy(&outcomes);
        assert_eq!(r.status, Status::Ok, "{r:?}");
    }

    #[test]
    fn reference_case_does_not_affect_result() {
        // 実機レポート: ありかとう だけ出ない（辞書に「在り難う」がある）
        let mut e = FakeEngine::new(&[
            ("arigatougosaimasu", "ありがとうございます"),
            ("kamji", "かんじ"),
            ("onekaisimasu", "おねがいします"),
        ]);
        let r = judge_fuzzy(&run_all_cases(&mut e));
        assert_eq!(r.status, Status::Ok, "{r:?}");
        assert!(
            r.evidence
                .iter()
                .any(|l| l.starts_with("参考: 濁点の付け忘れ（ありかとう）"))
        );
    }

    #[test]
    fn nothing_shown_anywhere_is_ng() {
        let mut e = FakeEngine::new(&[]);
        let r = judge_fuzzy(&run_all_cases(&mut e));
        assert_eq!(r.status, Status::Ng);
        assert!(r.detail.contains("どのケースでも"));
    }

    #[test]
    fn required_miss_is_ng_optional_miss_and_false_alarm_are_warn() {
        let mut e = FakeEngine::new(&[("kamji", "かんじ")]);
        let r = judge_fuzzy(&run_all_cases(&mut e));
        assert_eq!(r.status, Status::Ng, "誤り辞書（必須）が外れている: {r:?}");
        assert!(r.detail.contains("誤り辞書"));

        let mut e = FakeEngine::new(&[
            ("arikatou", "ありがとう"),
            ("arigatougosaimasu", "ありがとうございます"),
        ]);
        let r = judge_fuzzy(&run_all_cases(&mut e));
        assert_eq!(r.status, Status::Warn);
        assert!(r.detail.contains("出なかったケース"));

        let mut e = FakeEngine::new(&[
            ("arikatou", "ありがとう"),
            ("arigatougosaimasu", "ありがとうございます"),
            ("kamji", "かんじ"),
            ("onekaisimasu", "おねがいします"),
            ("gakkou", "がっこう"),
        ]);
        let r = judge_fuzzy(&run_all_cases(&mut e));
        assert_eq!(r.status, Status::Warn);
        assert!(r.detail.contains("誤表示"));
    }

    #[test]
    fn partial_reading_of_the_expected_counts() {
        assert!(reading_matches("おねがい", "おねがいします"));
        assert!(reading_matches(
            "ありがとうございます",
            "ありがとうございます"
        ));
        assert!(!reading_matches("", "あ"));
        assert!(!reading_matches("かんじ", "ありがとう"));
    }

    #[test]
    fn rpc_failure_is_propagated() {
        let mut e = FakeEngine::new(&[]);
        e.fail_after = Some(2);
        assert!(run_case(&mut e, "s", &FUZZY_CASES[0], Duration::from_secs(1)).is_err());
    }

    fn preedit(kind: SegmentKind, reading: &str, surface: &str) -> ProcessKeyResult {
        ProcessKeyResult {
            consumed: true,
            preedit: Some(PreeditView {
                text: surface.into(),
                segments: vec![SegmentView {
                    surface: surface.into(),
                    reading: reading.into(),
                    kind,
                    range: nagi_protocol::view::RangeU16 { start: 0, end: 3 },
                }],
                ..PreeditView::default()
            }),
            ..ProcessKeyResult::default()
        }
    }

    #[test]
    fn reconvert_judgement() {
        let good = preedit(SegmentKind::Japanese, "にほんご", "日本語");
        assert_eq!(
            judge_reconvert(&good, Some("日本語"), true).status,
            Status::Ok
        );
        assert_eq!(
            judge_reconvert(&good, Some("別"), true).status,
            Status::Warn
        );
        assert_eq!(judge_reconvert(&good, None, true).status, Status::Warn);
        assert_eq!(
            judge_reconvert(&good, Some("日本語"), false).status,
            Status::Ng
        );
        let literal = preedit(SegmentKind::Literal, "日本語", "日本語");
        let r = judge_reconvert(&literal, Some("日本語"), true);
        assert_eq!(r.status, Status::Ng);
        assert!(r.detail.contains("逆引き"));
        let r = judge_reconvert(&ProcessKeyResult::default(), None, true);
        assert_eq!(r.status, Status::Ng);
        // 入力した本文（日本語）が根拠の行に出ない
        let r = judge_reconvert(&good, Some("日本語"), true);
        assert!(
            r.evidence
                .iter()
                .all(|l| !l.contains("日本語") || l.contains("元のテキスト"))
        );
    }

    #[test]
    fn skipped_without_dictionary() {
        let ctx = crate::diag::testutil::ctx_with_imserver("/x/imserver");
        let mut engines = Engines::default();
        assert_eq!(check_fuzzy(&ctx, &mut engines).status, Status::Skipped);
        assert_eq!(check_reconvert(&ctx, &mut engines).status, Status::Skipped);
    }
}
