//! 手動の更新確認（`imserver --check-update`。`docs/decisions.md` H-100）。
//!
//! 自動確認（[`crate::update_check::check_with_throttle`]。起動時に 24 時間間引き）とは別の、
//! 利用者の明示操作による一回限りの動作。間引きを**無視**してマニフェストを取得し、結果を
//! 標準出力に 1 行の JSON（[`CheckReport`]）で出して終了する（JSON-RPC は起動しない）。
//!
//! - `update.auto_check = false` でも実行できる（手動は明示操作のため）。`update.check_url` は尊重する
//! - `--stage-updates` が付いていて新版があれば、取得・署名検証・保存（[`crate::update_stage::stage_update`]）
//!   まで行う。すでに同じ版が保存済みなら再取得しない
//! - 確認した時刻は `<user_dir>/update_check.json` に記録し、自動確認の間引きにも反映する（重複確認を避ける）
//! - 失敗（ネットワーク・署名不正など）は `status = "error"` と日本語の理由。入力内容は一切含まない

use crate::setup::{ServerOptions, parse_args};
use crate::update_check::{
    Manifest, default_check_url, fetch_manifest, parse_version, record_checked,
    update_public_key_hex,
};
use crate::update_stage::{self, StageOutcome};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 確認の結果の種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// 最新版を使っている
    UpToDate,
    /// 新しい版がある（取得はしていない）
    Available,
    /// 新しい版を取得・検証して保存済み（適用できる）
    Staged,
    /// 確認または取得に失敗した
    Error,
}

/// 標準出力に出す 1 行の JSON
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckReport {
    /// この imserver の版
    pub current: String,
    /// マニフェストの最新版（取得できなかったら `null`）
    pub latest: Option<String>,
    /// 結果の種別
    pub status: CheckStatus,
    /// リリースノートの URL
    pub notes_url: Option<String>,
    /// 失敗の理由（日本語）。成功なら `null`
    pub error: Option<String>,
}

impl CheckReport {
    fn error(current: &str, latest: Option<String>, notes_url: Option<String>, e: String) -> Self {
        CheckReport {
            current: current.to_string(),
            latest,
            status: CheckStatus::Error,
            notes_url,
            error: Some(e),
        }
    }

    /// 1 行の JSON（改行を含まない）
    pub fn to_json_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// マニフェストと現在の版から、取得前の状態（`UpToDate` / `Available`）を決める（ネットワーク不要）
pub fn classify(current: &str, manifest: &Manifest) -> CheckStatus {
    if parse_version(&manifest.version) > parse_version(current) {
        CheckStatus::Available
    } else {
        CheckStatus::UpToDate
    }
}

/// 引数に `--check-update` があれば、残りを起動オプションとして解釈して返す（無ければ `None`）。
/// 残りの引数は通常の起動と同じ（`--stage-updates` `--update-dir` `--user-dir` `--data-dir`
/// `--config` `--parent-pid` など）
pub fn parse_check_args<I: Iterator<Item = String>>(
    args: I,
) -> Result<Option<ServerOptions>, String> {
    let args: Vec<String> = args.collect();
    if !args.iter().any(|a| a == "--check-update") {
        return Ok(None);
    }
    parse_args(args.into_iter().filter(|a| a != "--check-update")).map(Some)
}

/// 本体。取得処理を差し替えられる形（テスト用）。`check_url` は設定ファイルから読む前の既定値の
/// 解決を呼び出し側（[`run_check_update`]）で済ませて渡す
pub fn run_check_with(
    options: &ServerOptions,
    current_version: &str,
    url: &str,
    state_path: Option<&std::path::Path>,
    fetch: impl FnOnce(&str) -> Result<Manifest, String>,
    download: impl FnOnce(&Manifest) -> Result<Vec<u8>, String>,
) -> CheckReport {
    let manifest = match fetch(url) {
        Ok(m) => m,
        Err(e) => return CheckReport::error(current_version, None, None, e),
    };
    // 確認できたので、自動確認の間引きにも反映する
    if let Some(path) = state_path {
        record_checked(path);
    }
    let stage_dir: Option<PathBuf> = if options.stage_updates {
        options.update_dir.clone().or_else(|| {
            options
                .user_dir
                .as_ref()
                .map(|d| d.join(update_stage::DIR_NAME))
        })
    } else {
        None
    };
    if let Some(dir) = &stage_dir {
        update_stage::discard_obsolete(dir, current_version);
    }
    let latest = Some(manifest.version.clone());
    let notes_url = manifest.notes_url.clone();
    let make = |status, error| CheckReport {
        current: current_version.to_string(),
        latest: latest.clone(),
        status,
        notes_url: notes_url.clone(),
        error,
    };
    match classify(current_version, &manifest) {
        CheckStatus::UpToDate => make(CheckStatus::UpToDate, None),
        _ => match &stage_dir {
            None => make(CheckStatus::Available, None),
            Some(dir) => match update_stage::stage_update(dir, &manifest, download) {
                Ok(StageOutcome::AlreadyStaged | StageOutcome::Staged) => {
                    make(CheckStatus::Staged, None)
                }
                Err(e) => make(
                    CheckStatus::Error,
                    Some(format!("新しい版の取得に失敗しました: {e}")),
                ),
            },
        },
    }
}

/// 設定ファイルを読んで `update.check_url` を解決する（読めなければ既定の URL）。
/// 返す 2 つ目は `update_check.json` のパス
fn resolve_url_and_state(options: &ServerOptions) -> (String, Option<PathBuf>) {
    let user_dir = options.user_dir.clone().or_else(nagi_config::config_dir);
    let config_path = options
        .config_path
        .clone()
        .or_else(|| user_dir.as_ref().map(|d| d.join("config.toml")));
    let url = config_path
        .and_then(|p| nagi_config::load_file(&p).ok())
        .and_then(|(c, _)| c.update.check_url)
        .unwrap_or_else(default_check_url);
    (url, user_dir.map(|d| d.join("update_check.json")))
}

/// 実際の設定・HTTP で確認する（`imserver --check-update` の本体）。`public_key_hex` は署名検証用
/// （本番は [`update_public_key_hex`]）
pub fn run_check_update_with_key(
    options: &ServerOptions,
    current_version: &str,
    public_key_hex: &str,
) -> CheckReport {
    let (url, state_path) = resolve_url_and_state(options);
    run_check_with(
        options,
        current_version,
        &url,
        state_path.as_deref(),
        fetch_manifest,
        |m| crate::update_check::download_verified(m, public_key_hex),
    )
}

/// この imserver の版・組み込みの公開鍵で確認する
pub fn run_check_update(options: &ServerOptions) -> CheckReport {
    run_check_update_with_key(options, env!("CARGO_PKG_VERSION"), update_public_key_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str) -> Manifest {
        Manifest {
            version: version.to_string(),
            notes_url: Some("https://example.com/notes".to_string()),
            download_url: None,
            sha256: None,
            signature: None,
        }
    }

    fn opts(stage: bool) -> ServerOptions {
        ServerOptions {
            stage_updates: stage,
            ..ServerOptions::default()
        }
    }

    #[test]
    fn classify_compares_versions() {
        assert_eq!(
            classify("0.1.14", &manifest("0.1.15")),
            CheckStatus::Available
        );
        assert_eq!(
            classify("0.1.14", &manifest("0.1.14")),
            CheckStatus::UpToDate
        );
        assert_eq!(
            classify("0.1.14", &manifest("0.1.9")),
            CheckStatus::UpToDate
        );
    }

    #[test]
    fn report_json_shape() {
        let r = CheckReport {
            current: "0.1.14".into(),
            latest: Some("0.1.15".into()),
            status: CheckStatus::Available,
            notes_url: None,
            error: None,
        };
        assert_eq!(
            r.to_json_line(),
            r#"{"current":"0.1.14","latest":"0.1.15","status":"available","notes_url":null,"error":null}"#
        );
        let e = CheckReport::error("0.1.14", None, None, "失敗".into());
        let v: serde_json::Value = serde_json::from_str(&e.to_json_line()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["error"], "失敗");
        assert!(v["latest"].is_null());
    }

    #[test]
    fn fetch_error_becomes_error_status_and_does_not_record() {
        let state =
            std::env::temp_dir().join(format!("nagi-manual-err-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&state);
        let r = run_check_with(
            &opts(false),
            "0.1.14",
            "http://x",
            Some(&state),
            |_| Err("network down".into()),
            |_| Err("unused".into()),
        );
        assert_eq!(r.status, CheckStatus::Error);
        assert_eq!(r.error.as_deref(), Some("network down"));
        assert!(!state.exists());
    }

    #[test]
    fn up_to_date_and_available_without_staging() {
        let r = run_check_with(
            &opts(false),
            "0.1.14",
            "u",
            None,
            |_| Ok(manifest("0.1.14")),
            |_| Err("unused".into()),
        );
        assert_eq!(r.status, CheckStatus::UpToDate);
        assert_eq!(r.latest.as_deref(), Some("0.1.14"));
        let r = run_check_with(
            &opts(false),
            "0.1.14",
            "u",
            None,
            |_| Ok(manifest("0.1.15")),
            |_| Err("unused".into()),
        );
        assert_eq!(r.status, CheckStatus::Available);
        assert_eq!(r.notes_url.as_deref(), Some("https://example.com/notes"));
    }

    #[test]
    fn staging_failure_is_error_with_latest_kept() {
        let dir = std::env::temp_dir().join(format!("nagi-manual-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut o = opts(true);
        o.update_dir = Some(dir.clone());
        // download_url 等が無いマニフェスト → stage_update がエラー
        let r = run_check_with(
            &o,
            "0.1.14",
            "u",
            None,
            |_| Ok(manifest("0.1.15")),
            |_| Err("unused".into()),
        );
        assert_eq!(r.status, CheckStatus::Error);
        assert_eq!(r.latest.as_deref(), Some("0.1.15"));
        assert!(r.error.unwrap().contains("新しい版の取得に失敗"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_check_args_strips_flag_and_keeps_options() {
        let v = |a: &[&str]| {
            a.iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .into_iter()
        };
        assert!(parse_check_args(v(&["--no-llm"])).unwrap().is_none());
        let o = parse_check_args(v(&[
            "--check-update",
            "--stage-updates",
            "--update-dir",
            "u",
            "--parent-pid",
            "7",
        ]))
        .unwrap()
        .unwrap();
        assert!(o.stage_updates);
        assert_eq!(o.update_dir, Some(PathBuf::from("u")));
        assert_eq!(o.parent_pid, Some(7));
        assert!(parse_check_args(v(&["--check-update", "--bogus"])).is_err());
    }
}
