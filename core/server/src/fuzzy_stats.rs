//! 曖昧入力の「もしかして」候補の採用状況（補正の種類ごとの件数）。
//!
//! 個人の癖に合わせて、よく採用される種類の補正は出やすく、ほとんど採用されない種類は出にくくする
//! ための統計。保存するのは種類ごとの `shown`（提示された回数）と `adopted`（選ばれて確定された回数）
//! の件数だけで、入力した読み・表記は一切保存しない（ファイルは `<user-dir>/fuzzy_stats.json`）。
//!
//! 打った読みと補正後の読みの組から種類を引くための対応表（[`FuzzyStats::note_kind`]）は、
//! 補正を提案した時点でメモリ上にだけ持つ短命の小さな表で、保存しない。
//!
//! 倍率の決め方: 採用率 `r = (adopted + 1) / (shown + 4)`（事前の採用率 0.25）を基準 0.25 と比べ、
//! `margin_scale = clamp(sqrt(0.25 / r), 0.6, 1.6)` を covered の読みの採否のマージンに掛ける
//! （よく採用されるほど小さく）。`shown` が [`MIN_SHOWN`] 未満の種類はデータ不足で 1.0。

use nagi_fuzzy::{EditKind, EditOp};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// 保存ファイル名（`<user-dir>` 直下）
pub const FILE_NAME: &str = "fuzzy_stats.json";

/// 保存ファイルのスキーマバージョン
const VERSION: u32 = 1;

/// 倍率を変え始める `shown` の下限（これ未満はデータ不足で 1.0）
pub const MIN_SHOWN: u32 = 10;

/// 倍率の基準にする採用率
const BASE_RATE: f32 = 0.25;

/// 採用率の事前分布（採用 1 件・不採用 3 件ぶんを仮に持つ）
const PRIOR_ADOPTED: f32 = 1.0;
const PRIOR_SHOWN: f32 = 4.0;

/// 倍率の下限・上限
const SCALE_MIN: f32 = 0.6;
const SCALE_MAX: f32 = 1.6;

/// 種類の対応表の最大件数（超えたら空にする。入力の長さと無関係に増え続けないための上限）
const KIND_CACHE_MAX: usize = 512;

/// 濁点・半濁点・小書きの付け忘れ / 付け過ぎだけ
pub const KIND_VOICING: &str = "voicing";
/// 隣のキーへの置換を含む
pub const KIND_SUBSTITUTE: &str = "substitute";
/// 隣り合う 2 文字の転置
pub const KIND_TRANSPOSE: &str = "transpose";
/// 余計な打鍵（挿入）
pub const KIND_INSERT: &str = "insert";
/// 打ち忘れ（脱落）
pub const KIND_DELETE: &str = "delete";
/// 置換を含まない複数種類の混在
pub const KIND_MIXED: &str = "mixed";

/// 編集操作の構成から種類のキーを決める。操作が無い（誤り辞書の対応など）なら `None`。
///
/// Voicing だけなら `voicing`、Substitute を含むなら `substitute`、残りは 1 種類だけならその種類、
/// 複数混ざるなら `mixed`
pub fn kind_key(ops: &[EditOp]) -> Option<&'static str> {
    if ops.is_empty() {
        return None;
    }
    if ops.iter().all(|op| op.kind == EditKind::Voicing) {
        return Some(KIND_VOICING);
    }
    if ops.iter().any(|op| op.kind == EditKind::Substitute) {
        return Some(KIND_SUBSTITUTE);
    }
    let first = ops[0].kind;
    if ops.iter().any(|op| op.kind != first) {
        return Some(KIND_MIXED);
    }
    Some(match first {
        EditKind::Transpose => KIND_TRANSPOSE,
        EditKind::Insert => KIND_INSERT,
        EditKind::Delete => KIND_DELETE,
        // 上で処理済み（到達しない）
        EditKind::Voicing => KIND_VOICING,
        EditKind::Substitute => KIND_SUBSTITUTE,
    })
}

/// 件数から倍率を求める。`shown` が [`MIN_SHOWN`] 未満なら 1.0
pub fn scale_from_counts(shown: u32, adopted: u32) -> f32 {
    if shown < MIN_SHOWN {
        return 1.0;
    }
    let rate = (adopted as f32 + PRIOR_ADOPTED) / (shown as f32 + PRIOR_SHOWN);
    (BASE_RATE / rate).sqrt().clamp(SCALE_MIN, SCALE_MAX)
}

/// 1 つの種類の件数
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// 提示された回数
    pub shown: u32,
    /// 選ばれて確定された回数
    pub adopted: u32,
}

/// 振り返り表示用の 1 行
#[derive(Debug, Clone, PartialEq)]
pub struct KindStat {
    /// 種類のキー
    pub kind: String,
    /// 提示された回数
    pub shown: u32,
    /// 選ばれて確定された回数
    pub adopted: u32,
    /// 今の倍率
    pub margin_scale: f32,
}

/// ファイルの形
#[derive(Debug, Serialize, Deserialize)]
struct FileData {
    version: u32,
    kinds: BTreeMap<String, Counts>,
}

/// 補正の種類ごとの採用状況
#[derive(Default)]
pub struct FuzzyStats {
    kinds: Mutex<BTreeMap<String, Counts>>,
    /// (打った読み, 補正後の読み) → 種類。保存しない短命の対応表
    kind_cache: Mutex<HashMap<(String, String), &'static str>>,
    /// 保存していない変更があるか
    dirty: AtomicBool,
}

impl FuzzyStats {
    /// 空の統計
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_kinds(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Counts>> {
        self.kinds.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 種類 `kind` の候補が提示された
    pub fn record_shown(&self, kind: &str) {
        let mut kinds = self.lock_kinds();
        let c = kinds.entry(kind.to_string()).or_default();
        c.shown = c.shown.saturating_add(1);
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 種類 `kind` の候補が選ばれて確定された
    pub fn record_adopted(&self, kind: &str) {
        let mut kinds = self.lock_kinds();
        let c = kinds.entry(kind.to_string()).or_default();
        c.adopted = c.adopted.saturating_add(1);
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 種類 `kind` のマージンの倍率（記録が無い・データ不足は 1.0）
    pub fn margin_scale(&self, kind: &str) -> f32 {
        self.lock_kinds()
            .get(kind)
            .map_or(1.0, |c| scale_from_counts(c.shown, c.adopted))
    }

    /// 全種類の件数と倍率（種類の名前順）
    pub fn snapshot(&self) -> Vec<KindStat> {
        self.lock_kinds()
            .iter()
            .map(|(kind, c)| KindStat {
                kind: kind.clone(),
                shown: c.shown,
                adopted: c.adopted,
                margin_scale: scale_from_counts(c.shown, c.adopted),
            })
            .collect()
    }

    /// 件数をすべて 0 にする（学習データの全削除）
    pub fn clear(&self) {
        let mut kinds = self.lock_kinds();
        if !kinds.is_empty() {
            kinds.clear();
        }
        self.kind_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        // ファイルも消すので、次の flush で消えるようにする
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// 補正の提案時に、(打った読み, 補正後の読み) の種類を覚える（確定時に種類を引くため）
    pub fn note_kind(&self, typed: &str, corrected: &str, kind: &'static str) {
        let mut cache = self.kind_cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= KIND_CACHE_MAX {
            cache.clear();
        }
        cache.insert((typed.to_string(), corrected.to_string()), kind);
    }

    /// [`Self::note_kind`] で覚えた種類。覚えていない（誤り辞書の対応など）なら `None`
    pub fn kind_of(&self, typed: &str, corrected: &str) -> Option<&'static str> {
        self.kind_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(typed.to_string(), corrected.to_string()))
            .copied()
    }

    /// 保存していない変更があるか
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// ファイルから読んで、件数を置き換える。無い・壊れている・未知のバージョンなら何もしない
    /// （空のまま始める）。読めたら `true`
    pub fn load_from(&self, path: &Path) -> bool {
        let Ok(data) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(file) = serde_json::from_str::<FileData>(&data) else {
            return false;
        };
        if file.version != VERSION {
            return false;
        }
        *self.lock_kinds() = file.kinds;
        self.dirty.store(false, Ordering::Relaxed);
        true
    }

    /// 変更があるときだけファイルに書く。全件 0 なら（学習の全削除後など）ファイルを消す
    pub fn flush(&self, path: &Path) -> std::io::Result<()> {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return Ok(());
        }
        let kinds = self.lock_kinds().clone();
        let result = if kinds.is_empty() {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        } else {
            save(path, &kinds)
        };
        if result.is_err() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result
    }

    /// 件数をファイルに書く（変更の有無に関わらず）
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let kinds = self.lock_kinds().clone();
        save(path, &kinds)
    }
}

/// ファイルに書く（一時ファイル経由で原子的に）
fn save(path: &Path, kinds: &BTreeMap<String, Counts>) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(&FileData {
        version: VERSION,
        kinds: kinds.clone(),
    })
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp = path.to_path_buf();
    temp.set_extension("tmp");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(kind: EditKind) -> EditOp {
        EditOp {
            kind,
            pos: 0,
            typed: None,
            intended: None,
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "nagi-fuzzy-stats-test-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn kind_key_follows_edit_composition() {
        assert_eq!(kind_key(&[]), None);
        assert_eq!(
            kind_key(&[op(EditKind::Voicing), op(EditKind::Voicing)]),
            Some("voicing")
        );
        assert_eq!(
            kind_key(&[op(EditKind::Voicing), op(EditKind::Substitute)]),
            Some("substitute")
        );
        assert_eq!(
            kind_key(&[op(EditKind::Substitute), op(EditKind::Delete)]),
            Some("substitute")
        );
        assert_eq!(kind_key(&[op(EditKind::Transpose)]), Some("transpose"));
        assert_eq!(kind_key(&[op(EditKind::Insert)]), Some("insert"));
        assert_eq!(kind_key(&[op(EditKind::Delete)]), Some("delete"));
        assert_eq!(
            kind_key(&[op(EditKind::Insert), op(EditKind::Delete)]),
            Some("mixed")
        );
    }

    #[test]
    fn scale_is_neutral_with_little_data() {
        assert_eq!(scale_from_counts(0, 0), 1.0);
        assert_eq!(scale_from_counts(MIN_SHOWN - 1, MIN_SHOWN - 1), 1.0);
    }

    #[test]
    fn scale_shrinks_for_often_adopted_and_grows_for_rarely_adopted() {
        // 採用率が高い → 小さく（下限 0.6）
        assert_eq!(scale_from_counts(100, 100), SCALE_MIN);
        // ほとんど採用されない → 大きく（上限 1.6）
        assert_eq!(scale_from_counts(1000, 0), SCALE_MAX);
        // 基準（r = 0.25）ちょうどなら 1.0: (a + 1) / (20 + 4) = 0.25 → a = 5
        assert!((scale_from_counts(20, 5) - 1.0).abs() < 1e-6);
        // 途中は単調
        assert!(scale_from_counts(20, 10) < scale_from_counts(20, 5));
        assert!(scale_from_counts(20, 5) < scale_from_counts(20, 0));
    }

    #[test]
    fn counters_and_margin_scale() {
        let s = FuzzyStats::new();
        assert_eq!(s.margin_scale("voicing"), 1.0);
        for _ in 0..MIN_SHOWN {
            s.record_shown("voicing");
            s.record_adopted("voicing");
        }
        assert!(s.margin_scale("voicing") < 1.0);
        assert_eq!(s.margin_scale("insert"), 1.0);
        let snap = s.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].kind, "voicing");
        assert_eq!((snap[0].shown, snap[0].adopted), (MIN_SHOWN, MIN_SHOWN));
    }

    #[test]
    fn save_and_load_round_trip_without_text() {
        let path = temp_path("roundtrip");
        let s = FuzzyStats::new();
        s.record_shown("substitute");
        s.record_shown("substitute");
        s.record_adopted("substitute");
        s.flush(&path).expect("保存できる");
        let text = std::fs::read_to_string(&path).expect("読める");
        assert!(text.contains("\"version\": 1"));
        let loaded = FuzzyStats::new();
        assert!(loaded.load_from(&path));
        assert_eq!(loaded.snapshot(), s.snapshot());
        assert!(!loaded.is_dirty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_ignores_missing_corrupt_and_unknown_version() {
        let s = FuzzyStats::new();
        let missing = temp_path("missing");
        assert!(!s.load_from(&missing));
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not json").expect("書ける");
        assert!(!s.load_from(&path));
        std::fs::write(
            &path,
            r#"{"version":99,"kinds":{"voicing":{"shown":5,"adopted":1}}}"#,
        )
        .expect("書ける");
        assert!(!s.load_from(&path));
        assert!(s.snapshot().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clear_zeroes_counters_and_removes_file_on_flush() {
        let path = temp_path("clear");
        let s = FuzzyStats::new();
        s.record_shown("voicing");
        s.flush(&path).expect("保存できる");
        assert!(path.exists());
        s.clear();
        assert!(s.snapshot().is_empty());
        s.flush(&path).expect("消せる");
        assert!(!path.exists());
    }

    #[test]
    fn kind_cache_round_trip_and_bound() {
        let s = FuzzyStats::new();
        s.note_kind("かmじ", "かんじ", KIND_SUBSTITUTE);
        assert_eq!(s.kind_of("かmじ", "かんじ"), Some("substitute"));
        assert_eq!(s.kind_of("かmじ", "かじ"), None);
        for i in 0..(KIND_CACHE_MAX + 5) {
            s.note_kind(&format!("a{i}"), "b", KIND_INSERT);
        }
        assert!(s.kind_cache.lock().unwrap().len() <= KIND_CACHE_MAX);
    }
}
