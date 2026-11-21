//! レポートの出力（日本語の文字列・JSON・ファイル保存・クリップボード）。
//!
//! レポートには入力した文字・辞書の内容・ユーザー名を含めない（パスは `Ctx::redact` で置き換え済み。
//! 診断が imserver に流す文字列は固定のテスト文字列で、結果にも本文は出さない）。

use super::types::{ItemResult, Kind, Report, Status};
use std::path::{Path, PathBuf};

/// UNIX 時刻（秒）→ UTC の `YYYY-MM-DDTHH:MM:SSZ`
pub fn format_utc(unix_secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix_secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// ファイル名に使う `YYYYMMDD-HHMMSS`（UTC）
pub fn file_stamp(unix_secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix_secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

fn civil(unix_secs: u64) -> (i64, i64, i64, u64, u64, u64) {
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    // 日数 → 西暦年月日（Howard Hinnant の civil_from_days。1970-01-01 起点）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, rem / 3_600, rem % 3_600 / 60, rem % 60)
}

/// 現在の UNIX 時刻（秒）
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn mark(status: Status) -> &'static str {
    match status {
        Status::Ok => "[OK]      ",
        Status::Warn => "[要確認]  ",
        Status::Ng => "[NG]      ",
        Status::Skipped => "[スキップ]",
        Status::Manual => "[手動確認]",
    }
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|l| format!("{prefix}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 1 項目の本文
fn item_block(item: &ItemResult) -> String {
    let mut s = format!(
        "{} {}（{}）\n",
        mark(item.status),
        item.title,
        item.id.as_str()
    );
    s.push_str(&indent(&item.detail, "    "));
    s.push('\n');
    if let Some(advice) = &item.advice {
        let label = if item.status == Status::Manual {
            "手順"
        } else {
            "対処"
        };
        s.push_str(&format!("  {label}:\n{}\n", indent(advice, "    ")));
    }
    if !item.evidence.is_empty() {
        s.push_str("  根拠:\n");
        for line in &item.evidence {
            s.push_str(&indent(line, "    - "));
            s.push('\n');
        }
    }
    s
}

impl Report {
    /// 日本語のレポート（クリップボード・ファイル共通）
    pub fn to_text(&self) -> String {
        let sum = self.summary();
        let mut s = String::new();
        s.push_str("Nagi 診断レポート\n");
        s.push_str(&format!("実行日時: {}\n", self.generated_at));
        s.push_str(&format!(
            "診断プログラム: nagi-settings {}\n",
            self.tool_version
        ));
        s.push_str(&format!("環境: {}\n", self.environment));
        s.push_str(&format!(
            "結果: NG {} / 要確認 {} / OK {} / スキップ {} / 手動確認 {}（総合: {}）\n\n",
            sum.ng,
            sum.warn,
            sum.ok,
            sum.skipped,
            sum.manual,
            self.overall().label()
        ));
        let attention: Vec<&ItemResult> = self
            .items
            .iter()
            .filter(|i| matches!(i.status, Status::Ng | Status::Warn))
            .collect();
        if attention.is_empty() {
            s.push_str("== 要対応 ==\n  自動判定の項目に問題は見つかりませんでした。\n\n");
        } else {
            s.push_str("== 要対応（NG・要確認）==\n");
            for item in attention {
                s.push_str(&format!(
                    "  {} {}: {}\n",
                    mark(item.status),
                    item.id.as_str(),
                    first_line(&item.detail)
                ));
            }
            s.push('\n');
        }
        s.push_str("== 自動判定の結果 ==\n");
        for item in self.items.iter().filter(|i| i.id.kind() == Kind::Auto) {
            s.push_str(&item_block(item));
            s.push('\n');
        }
        let guides: Vec<&ItemResult> = self
            .items
            .iter()
            .filter(|i| i.id.kind() == Kind::Guide)
            .collect();
        if !guides.is_empty() {
            s.push_str("== 人の操作が必要な項目（案内）==\n");
            for item in guides {
                s.push_str(&item_block(item));
                s.push('\n');
            }
        }
        s.push_str("※ このレポートには入力した文字・辞書の内容は含まれません。ユーザー名を含むパスは %USERPROFILE% などに置き換えています。\n");
        s
    }

    /// JSON（機械可読。報告・集計用）
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// 診断を再現する短い 1 行（進捗表示・ログ向け）
    pub fn one_line(&self) -> String {
        let s = self.summary();
        format!(
            "診断: NG {} / 要確認 {} / OK {} / スキップ {} / 手動 {}",
            s.ng, s.warn, s.ok, s.skipped, s.manual
        )
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// 保存したファイル
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedReport {
    /// テキスト版
    pub text_path: PathBuf,
    /// JSON 版
    pub json_path: PathBuf,
}

/// レポートをフォルダに保存する（`nagi-diag-<日時>.txt` と `.json`。UTF-8 の BOM 付きテキスト。
/// メモ帳でも文字化けしない）。フォルダが無ければ作る
pub fn save_to(report: &Report, dir: &Path, unix_secs: u64) -> Result<SavedReport, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("保存先のフォルダを作れません（{}）", e.kind()))?;
    let stem = format!("nagi-diag-{}", file_stamp(unix_secs));
    let text_path = dir.join(format!("{stem}.txt"));
    let json_path = dir.join(format!("{stem}.json"));
    let mut text = String::from('\u{feff}');
    text.push_str(&report.to_text());
    std::fs::write(&text_path, text).map_err(|e| format!("保存できません（{}）", e.kind()))?;
    std::fs::write(&json_path, report.to_json())
        .map_err(|e| format!("保存できません（{}）", e.kind()))?;
    Ok(SavedReport {
        text_path,
        json_path,
    })
}

/// テキストをクリップボードへコピーする（Windows のみ。PowerShell の `Set-Clipboard`）
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    imp::copy(text)
}

#[cfg(windows)]
mod imp {
    use super::super::sys::encode_command;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    pub fn copy(text: &str) -> Result<(), String> {
        let path = std::env::temp_dir().join(format!("nagi-diag-clip-{}.txt", std::process::id()));
        let mut data = String::from('\u{feff}');
        data.push_str(text);
        std::fs::write(&path, data)
            .map_err(|e| format!("一時ファイルを書けません（{}）", e.kind()))?;
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let exe = std::path::Path::new(&root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let script =
            "Get-Content -Raw -Encoding UTF8 -LiteralPath $env:NAGI_DIAG_CLIP | Set-Clipboard";
        let status = Command::new(exe)
            .args(["-NoProfile", "-NonInteractive", "-STA", "-EncodedCommand"])
            .arg(encode_command(script))
            .env("NAGI_DIAG_CLIP", &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
        let _ = std::fs::remove_file(&path);
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => Err("クリップボードにコピーできませんでした".to_owned()),
            Err(e) => Err(format!("PowerShell を起動できません（{}）", e.kind())),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn copy(_text: &str) -> Result<(), String> {
        Err("クリップボードへのコピーは Windows のみです".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::types::ItemId;

    fn sample() -> Report {
        Report {
            tool_version: "0.1.17".into(),
            generated_at: format_utc(1_795_222_923),
            environment: "Windows 11 / CPU ARM64".into(),
            items: vec![
                ItemResult::ok(ItemId::EnvArch, "合っています"),
                ItemResult::ng(ItemId::EngineStart, "起動できません\n2 行目", "入れ直す"),
                ItemResult::warn(ItemId::DataFiles, "モデルなし", "取得する")
                    .with_evidence(["ファイル: なし".to_owned()]),
                ItemResult::manual(ItemId::ManualSmartScreen, "手動", "1. 開く\n2. 記録"),
            ],
        }
    }

    #[test]
    fn utc_formatting() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(file_stamp(1_000_000_000), "20010909-014640");
        // うるう日
        assert_eq!(format_utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn text_report_has_summary_attention_and_sections() {
        let text = sample().to_text();
        assert!(
            text.contains("結果: NG 1 / 要確認 1 / OK 1 / スキップ 0 / 手動確認 1（総合: NG）")
        );
        assert!(text.contains("== 要対応（NG・要確認）=="));
        assert!(text.contains("engine.start: 起動できません"));
        assert!(text.contains("== 自動判定の結果 =="));
        assert!(text.contains("== 人の操作が必要な項目（案内）=="));
        assert!(text.contains("  対処:\n    入れ直す"));
        assert!(text.contains("  手順:\n    1. 開く\n    2. 記録"));
        assert!(text.contains("    - ファイル: なし"));
        assert!(text.contains("2 行目"));
    }

    #[test]
    fn clean_report_says_no_problems() {
        let r = Report {
            items: vec![ItemResult::ok(ItemId::EnvArch, "ok")],
            ..sample()
        };
        assert!(r.to_text().contains("問題は見つかりませんでした"));
    }

    #[test]
    fn json_round_trips() {
        let r = sample();
        let back: Report = serde_json::from_str(&r.to_json()).unwrap();
        assert_eq!(back, r);
        assert!(r.to_json().contains("\"env.arch\""));
    }

    #[test]
    fn save_writes_bom_text_and_json() {
        let dir = std::env::temp_dir().join(format!("nagi-diag-save-{}", std::process::id()));
        let saved = save_to(&sample(), &dir.join("sub"), 1_000_000_000).unwrap();
        assert!(saved.text_path.ends_with("nagi-diag-20010909-014640.txt"));
        let text = std::fs::read_to_string(&saved.text_path).unwrap();
        assert!(text.starts_with('\u{feff}'));
        assert!(text.contains("Nagi 診断レポート"));
        let json: Report =
            serde_json::from_str(&std::fs::read_to_string(&saved.json_path).unwrap()).unwrap();
        assert_eq!(json.items.len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clipboard_is_unsupported_off_windows() {
        if !cfg!(windows) {
            assert!(copy_to_clipboard("x").is_err());
        }
    }
}
