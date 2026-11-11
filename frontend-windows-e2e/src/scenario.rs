//! シナリオ定義（`tests/e2e/README.md` のスキーマ）の読み込み。

use std::path::Path;

use serde::Deserialize;

/// 1 つのスイート（`scenarios/*.json` の 1 ファイル）
#[derive(Debug, Clone, Deserialize)]
pub struct Suite {
    /// スイート名
    pub suite: String,
    /// 実行するプラットフォーム。省略時は全て
    #[serde(default)]
    pub platforms: Option<Vec<String>>,
    /// シナリオ
    pub scenarios: Vec<Scenario>,
}

impl Suite {
    /// `platform`（`windows` 等）で実行するスイートか
    pub fn runs_on(&self, platform: &str) -> bool {
        self.platforms
            .as_ref()
            .is_none_or(|p| p.iter().any(|x| x == platform))
    }
}

/// 1 つのシナリオ
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    /// シナリオ名
    pub name: String,
    /// 手順
    pub steps: Vec<Step>,
}

/// 手順。`type` / `key` / `tap` / `flick` / `expect` / `screenshot` のうち 1 つだけを持つ
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Step {
    /// 物理キーとして 1 文字ずつ送る ASCII 文字列
    #[serde(rename = "type")]
    pub type_text: Option<String>,
    /// 特殊キー（`Shift+Left` のように `+` で連結）
    pub key: Option<String>,
    /// ソフトキーボードのタップ（iOS。Windows では未対応）
    pub tap: Option<Vec<String>>,
    /// フリック（iOS。Windows では未対応）
    pub flick: Option<serde_json::Value>,
    /// 検証
    pub expect: Option<Expect>,
    /// スクリーンショット名
    pub screenshot: Option<String>,
}

/// `expect`。指定したものだけを比較する
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Expect {
    /// 未確定文字列
    pub marked: Option<String>,
    /// 確定済みの文字列
    pub committed: Option<String>,
    /// テキスト欄の表示全体（`committed` + `marked`）
    pub text: Option<String>,
    /// 候補一覧に含まれるべき文字列（Windows では取得できない）
    pub candidates_contain: Option<Vec<String>>,
    /// 待ち時間の上限（ミリ秒）
    pub timeout_ms: Option<u64>,
}

/// `expect` の既定の待ち時間（ミリ秒）
pub const DEFAULT_TIMEOUT_MS: u64 = 2000;

/// ディレクトリ直下の `*.json` を名前順に読む。読めない・解釈できないファイルはエラー
pub fn load_dir(dir: &Path) -> Result<Vec<Suite>, String> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("シナリオのディレクトリを読めません: {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("読めません: {}: {e}", p.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("解釈できません: {}: {e}", p.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "suite": "romaji-basic",
      "platforms": ["macos", "windows"],
      "scenarios": [{
        "name": "konnichiha_commit",
        "steps": [
          { "type": "konnichiha" },
          { "expect": { "marked": "こんにちは" } },
          { "key": "Return" },
          { "expect": { "marked": "", "committed": "こんにちは", "timeout_ms": 500 } },
          { "screenshot": "done" },
          { "tap": ["あ"] },
          { "flick": { "key": "か", "dir": "up" } }
        ]
      }]
    }"#;

    #[test]
    fn parses_the_readme_schema() {
        let suite: Suite = serde_json::from_str(SAMPLE).unwrap();
        assert!(suite.runs_on("windows"));
        assert!(!suite.runs_on("ios"));
        let steps = &suite.scenarios[0].steps;
        assert_eq!(steps[0].type_text.as_deref(), Some("konnichiha"));
        assert_eq!(steps[2].key.as_deref(), Some("Return"));
        let e = steps[3].expect.as_ref().unwrap();
        assert_eq!(e.marked.as_deref(), Some(""));
        assert_eq!(e.timeout_ms, Some(500));
        assert_eq!(steps[4].screenshot.as_deref(), Some("done"));
        assert!(steps[5].tap.is_some() && steps[6].flick.is_some());
    }

    #[test]
    fn missing_platforms_runs_everywhere() {
        let suite: Suite = serde_json::from_str(r#"{"suite":"s","scenarios":[]}"#).unwrap();
        assert!(suite.runs_on("windows") && suite.runs_on("ios"));
    }
}
