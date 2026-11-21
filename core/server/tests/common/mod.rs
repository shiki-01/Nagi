//! テスト間で共有する部品: 小さなインライン辞書と、偽の変換モデル

#![allow(dead_code, missing_docs)]

use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};
use nagi_infer::{ConversionModel, ConvertRequest, Hypothesis, InferError};
use nagi_server::Server;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 動詞,自立,*,*,五段・カ行促音便,基本形,行く
4 助動詞,*,*,*,特殊・デス,基本形,です
5 記号,句点,*,*,*,*,。
";

pub fn dict() -> Arc<Dictionary> {
    dict_with_extra("")
}

/// 基本の辞書に、`extra`（`読み\t左ID\t右ID\tコスト\t表記` の行）を足した辞書
pub fn dict_with_extra(extra: &str) -> Arc<Dictionary> {
    let n = 6;
    let mut cost = vec![100u32; n * n];
    for (rid, lid) in [
        (1, 2),
        (1, 4),
        (2, 3),
        (2, 1),
        (0, 1),
        (0, 3),
        (1, 5),
        (3, 5),
        (4, 5),
        (1, 0),
        (4, 0),
        (5, 0),
        (3, 0),
        (2, 0),
    ] {
        cost[rid * n + lid] = 0;
    }
    let mut lines = vec![n.to_string()];
    lines.extend(cost.iter().map(u32::to_string));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text(
        "inline",
        "\
こうえん\t1\t1\t3000\t公園
こうえん\t1\t1\t3500\t講演
こうえん\t1\t1\t4000\t後援
こうえん\t1\t1\t9000\tこうえん
に\t2\t2\t400\tに
いく\t3\t3\t3000\t行く
です\t4\t4\t300\tです
きょう\t1\t1\t3000\t今日
は\t2\t2\t500\tは
",
    )
    .unwrap();
    b.add_dictionary_text("extra", extra).unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    Arc::new(b.build().unwrap())
}

/// 読み → 表記 の表で答え、`score` は表にある表記を最良にする。呼び出し回数と遅延を制御できる
pub struct FakeModel {
    pub table: HashMap<String, String>,
    /// `beam_width > 1` のときに使う表: 読み → [(表記, スコア), ...]（スコア降順で渡すこと）。
    /// 無ければ `table` の 1 件だけを返す（既定の貪欲デコードと同じ）
    pub beam_table: HashMap<String, Vec<(String, f32)>>,
    pub delay: Duration,
    pub converts: Arc<AtomicUsize>,
    pub last_contexts: Arc<Mutex<Vec<String>>>,
}

impl FakeModel {
    pub fn new(pairs: &[(&str, &str)]) -> Self {
        FakeModel {
            table: pairs
                .iter()
                .map(|(r, s)| (r.to_string(), s.to_string()))
                .collect(),
            beam_table: HashMap::new(),
            delay: Duration::ZERO,
            converts: Arc::new(AtomicUsize::new(0)),
            last_contexts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// `beam_width > 1` で呼ばれたときの複数仮説を登録する
    pub fn with_beam(mut self, reading: &str, hypotheses: &[(&str, f32)]) -> Self {
        self.beam_table.insert(
            reading.to_string(),
            hypotheses
                .iter()
                .map(|(s, score)| (s.to_string(), *score))
                .collect(),
        );
        self
    }
}

impl ConversionModel for FakeModel {
    fn id(&self) -> &str {
        "fake"
    }

    fn convert(&mut self, req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError> {
        std::thread::sleep(self.delay);
        self.converts.fetch_add(1, Ordering::SeqCst);
        self.last_contexts
            .lock()
            .unwrap()
            .push(req.left_context.clone());
        if req.beam_width > 1
            && let Some(hyps) = self.beam_table.get(&req.reading)
        {
            return Ok(hyps
                .iter()
                .take(req.beam_width)
                .map(|(s, score)| Hypothesis {
                    surface: s.clone(),
                    score: *score,
                })
                .collect());
        }
        self.table
            .get(&req.reading)
            .map(|s| {
                vec![Hypothesis {
                    surface: s.clone(),
                    score: 0.0,
                }]
            })
            .ok_or_else(|| InferError::Other("未登録".into()))
    }

    fn score(
        &mut self,
        req: &ConvertRequest,
        candidates: &[String],
    ) -> Result<Vec<f32>, InferError> {
        // 左文脈が「大学の先生が」なら 講演 を、それ以外は 後援 を最良にする（辞書順とは逆）
        let favorite = if req.left_context.contains("先生") {
            "講演"
        } else {
            "後援"
        };
        Ok(candidates
            .iter()
            .map(|c| if c == favorite { -0.1 } else { -5.0 })
            .collect())
    }
}

/// 非同期の完了を待つ上限。成功したらすぐ返るので、遅い CI ランナーでも足りるよう十分長く取る
/// （上限まで待つのは失敗時だけ）
pub const WAIT_LIMIT: Duration = Duration::from_secs(15);

/// 条件が満たされるまで短い間隔でポーリングする。上限を超えたら `what` で失敗する
pub fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT_LIMIT;
    while !cond() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 通知の行（応答に付いて返ったものも、`poll_notifications` のものも）から `update_candidates` の
/// `params` だけを取り出す
pub fn update_candidates_params(lines: &[String]) -> Vec<Value> {
    lines
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|l| l["method"] == "update_candidates")
        .map(|l| l["params"].clone())
        .collect()
}

/// 最後の打鍵の同期応答（`sync_revision` はそのリビジョン）より新しい `update_candidates` が届くまで
/// ポーリングし、届いた中でリビジョンが最大のものの `params` を返す。`seen` は、打鍵の応答に付いて
/// 返った通知など、すでに手元にあるもの。`limit` までに届かなければ `None`。
/// 通知が必ず届く場面（`limit` = `WAIT_LIMIT`）と、届かないことがある場面（読みが変わらない打鍵など。
/// 短い `limit` で、届いていれば拾うだけにする）を呼び出し側が選ぶ
pub fn latest_candidates_update(
    s: &mut Server,
    sync_revision: u64,
    mut seen: Vec<Value>,
    limit: Duration,
) -> Option<Value> {
    let deadline = Instant::now() + limit;
    loop {
        seen.extend(update_candidates_params(&s.poll_notifications()));
        let newest = seen
            .iter()
            .max_by_key(|p| p["revision"].as_u64().unwrap_or(0));
        if let Some(n) = newest
            && n["revision"].as_u64().unwrap_or(0) > sync_revision
        {
            return Some(n.clone());
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
