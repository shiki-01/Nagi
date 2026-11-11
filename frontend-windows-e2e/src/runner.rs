//! シナリオの実行。OS 依存の操作は [`Host`] に閉じ込め、手順の解釈・待ち・結果の組み立てはここ
//! （Linux でもテストできる）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::scenario::{DEFAULT_TIMEOUT_MS, Expect, Scenario, Step, Suite};

/// ある時点の観測
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    /// 未確定文字列
    pub marked: String,
    /// 確定済みの文字列
    pub committed: String,
    /// テキスト欄の全文（診断用。`committed` と `marked` の分離の確認）
    pub raw: String,
}

impl Observation {
    /// `committed` + `marked`
    pub fn text(&self) -> String {
        format!("{}{}", self.committed, self.marked)
    }
}

/// OS 依存の操作
pub trait Host {
    /// シナリオの開始: テキスト欄を空にし、IME を日本語（ひらがな）モードにする
    fn begin_scenario(&mut self) -> Result<(), String>;
    /// ASCII 文字列を物理キーとして 1 文字ずつ送る
    fn type_text(&mut self, text: &str) -> Result<(), String>;
    /// 特殊キー（`Shift+Left` 等）を送る
    fn press(&mut self, spec: &str) -> Result<(), String>;
    /// 現在の観測（メッセージを処理してから読む）
    fn observe(&mut self) -> Result<Observation, String>;
    /// メッセージを処理しながら待つ
    fn idle(&mut self, ms: u64);
    /// スクリーンショットを PNG で保存する
    fn screenshot(&mut self, path: &Path) -> Result<(), String>;
    /// 最初の失敗時に出す診断（直近のプローブ行など）。既定は空
    fn diagnostics(&mut self) -> String {
        String::new()
    }
}

/// `results.json` の 1 要素
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResultEntry {
    /// スイート名
    pub suite: String,
    /// シナリオ名
    pub name: String,
    /// 成否
    pub ok: bool,
    /// 失敗の理由（成功なら空。注記があれば入る）
    pub message: String,
}

/// 成果物（スクリーンショット）の置き場
pub struct Artifacts {
    /// `screenshots/` ディレクトリ
    pub dir: PathBuf,
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `expect` の不一致の説明。全て満たすなら `None`
fn mismatch(expect: &Expect, obs: &Observation) -> Option<String> {
    let mut diffs = Vec::new();
    if let Some(m) = &expect.marked
        && *m != obs.marked
    {
        diffs.push(format!("marked: 期待 {m:?} / 実測 {:?}", obs.marked));
    }
    if let Some(c) = &expect.committed
        && *c != obs.committed
    {
        diffs.push(format!("committed: 期待 {c:?} / 実測 {:?}", obs.committed));
    }
    if let Some(t) = &expect.text
        && *t != obs.text()
    {
        diffs.push(format!("text: 期待 {t:?} / 実測 {:?}", obs.text()));
    }
    (!diffs.is_empty()).then(|| format!("{} ; (テキスト欄の全文 {:?})", diffs.join(" ; "), obs.raw))
}

fn run_expect(host: &mut dyn Host, expect: &Expect, notes: &mut Vec<String>) -> Result<(), String> {
    if expect.candidates_contain.is_some() {
        notes.push("candidates_contain は Windows では取得できないため検証していない".to_owned());
    }
    let timeout = Duration::from_millis(expect.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let start = Instant::now();
    loop {
        let obs = host.observe()?;
        let Some(diff) = mismatch(expect, &obs) else {
            return Ok(());
        };
        if start.elapsed() >= timeout {
            return Err(diff);
        }
        host.idle(20);
    }
}

fn run_step(
    host: &mut dyn Host,
    step: &Step,
    shot_prefix: &str,
    artifacts: &Artifacts,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    if let Some(t) = &step.type_text {
        host.type_text(t)
    } else if let Some(k) = &step.key {
        host.press(k)
    } else if let Some(e) = &step.expect {
        run_expect(host, e, notes)
    } else if let Some(name) = &step.screenshot {
        let path = artifacts
            .dir
            .join(format!("{shot_prefix}__{}.png", sanitize(name)));
        host.screenshot(&path)
    } else if step.tap.is_some() || step.flick.is_some() {
        Err("tap / flick は iOS 用の手順で、Windows では実行できません".to_owned())
    } else {
        Err("解釈できない手順です".to_owned())
    }
}

/// 1 シナリオを実行する。失敗時は自動でスクリーンショットを残す
pub fn run_scenario(
    host: &mut dyn Host,
    suite: &str,
    scenario: &Scenario,
    artifacts: &Artifacts,
) -> ResultEntry {
    let prefix = format!("{}__{}", sanitize(suite), sanitize(&scenario.name));
    let mut notes = Vec::new();
    let mut outcome = host
        .begin_scenario()
        .map_err(|e| format!("開始に失敗: {e}"));
    if outcome.is_ok() {
        for (i, step) in scenario.steps.iter().enumerate() {
            if let Err(e) = run_step(host, step, &prefix, artifacts, &mut notes) {
                outcome = Err(format!("手順 {} で失敗: {e}", i + 1));
                break;
            }
        }
    }
    let (ok, mut message) = match outcome {
        Ok(()) => (true, String::new()),
        Err(m) => {
            let path = artifacts.dir.join(format!("FAIL__{prefix}.png"));
            if let Err(e) = host.screenshot(&path) {
                notes.push(format!("失敗時のスクリーンショットを保存できません: {e}"));
            }
            (false, m)
        }
    };
    for n in notes {
        if !message.is_empty() {
            message.push_str(" ; ");
        }
        message.push_str(&n);
    }
    ResultEntry {
        suite: suite.to_owned(),
        name: scenario.name.clone(),
        ok,
        message,
    }
}

/// 全スイートを実行し、`PASS` / `FAIL` を標準出力へ出す。`filter` は `suite/name` の部分一致
pub fn run_all(
    host: &mut dyn Host,
    suites: &[Suite],
    platform: &str,
    filter: Option<&str>,
    artifacts: &Artifacts,
) -> Vec<ResultEntry> {
    let mut results = Vec::new();
    let mut diagnosed = false;
    for suite in suites.iter().filter(|s| s.runs_on(platform)) {
        for scenario in &suite.scenarios {
            let id = format!("{}/{}", suite.suite, scenario.name);
            if filter.is_some_and(|f| !id.contains(f)) {
                continue;
            }
            let entry = run_scenario(host, &suite.suite, scenario, artifacts);
            if entry.ok {
                println!("PASS {id}");
            } else {
                println!("FAIL {id}: {}", entry.message);
                if !diagnosed {
                    diagnosed = true;
                    let d = host.diagnostics();
                    if !d.is_empty() {
                        println!("--- 最初の失敗時の診断 ---\n{d}");
                    }
                }
            }
            results.push(entry);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 入力された文字をそのままひらがな代わりに積むだけの擬似ホスト
    #[derive(Default)]
    struct Fake {
        marked: String,
        committed: String,
        shots: Vec<PathBuf>,
        begun: u32,
    }

    impl Host for Fake {
        fn begin_scenario(&mut self) -> Result<(), String> {
            self.begun += 1;
            self.marked.clear();
            self.committed.clear();
            Ok(())
        }
        fn type_text(&mut self, text: &str) -> Result<(), String> {
            self.marked.push_str(text);
            Ok(())
        }
        fn press(&mut self, spec: &str) -> Result<(), String> {
            if spec == "Return" {
                self.committed.push_str(&std::mem::take(&mut self.marked));
            }
            Ok(())
        }
        fn observe(&mut self) -> Result<Observation, String> {
            Ok(Observation {
                marked: self.marked.clone(),
                committed: self.committed.clone(),
                raw: format!("{}{}", self.committed, self.marked),
            })
        }
        fn idle(&mut self, ms: u64) {
            std::thread::sleep(Duration::from_millis(ms));
        }
        fn screenshot(&mut self, path: &Path) -> Result<(), String> {
            self.shots.push(path.to_path_buf());
            Ok(())
        }
    }

    fn suite(json: &str) -> Suite {
        serde_json::from_str(json).unwrap()
    }

    fn artifacts() -> Artifacts {
        Artifacts {
            dir: PathBuf::from("shots"),
        }
    }

    #[test]
    fn passes_and_fails_with_screenshots() {
        let s = suite(
            r#"{"suite":"s","platforms":["windows"],"scenarios":[
              {"name":"ok","steps":[{"type":"ab"},{"expect":{"marked":"ab"}},{"key":"Return"},
                {"expect":{"marked":"","committed":"ab","text":"ab"}},{"screenshot":"x y"}]},
              {"name":"ng","steps":[{"type":"ab"},{"expect":{"marked":"zz","timeout_ms":50}}]},
              {"name":"ios","steps":[{"tap":["a"]}]}
            ]}"#,
        );
        let mut host = Fake::default();
        let r = run_all(&mut host, &[s], "windows", None, &artifacts());
        assert_eq!(r.len(), 3);
        assert!(r[0].ok && r[0].message.is_empty());
        assert!(!r[1].ok);
        assert!(r[1].message.contains("手順 2") && r[1].message.contains("\"zz\""));
        assert!(!r[2].ok);
        assert_eq!(host.begun, 3);
        let shots: Vec<_> = host
            .shots
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            shots,
            ["s__ok__x_y.png", "FAIL__s__ng.png", "FAIL__s__ios.png"]
        );
    }

    #[test]
    fn skips_other_platforms_and_filters() {
        let ios =
            suite(r#"{"suite":"a","platforms":["ios"],"scenarios":[{"name":"n","steps":[]}]}"#);
        let both = suite(
            r#"{"suite":"b","scenarios":[{"name":"n1","steps":[]},{"name":"n2","steps":[]}]}"#,
        );
        let mut host = Fake::default();
        let r = run_all(&mut host, &[ios, both], "windows", Some("n2"), &artifacts());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "n2");
    }

    #[test]
    fn candidates_contain_is_noted_not_failed() {
        let s = suite(
            r#"{"suite":"s","scenarios":[{"name":"c","steps":[{"expect":{"candidates_contain":["x"]}}]}]}"#,
        );
        let r = run_all(&mut Fake::default(), &[s], "windows", None, &artifacts());
        assert!(r[0].ok);
        assert!(r[0].message.contains("candidates_contain"));
    }
}
