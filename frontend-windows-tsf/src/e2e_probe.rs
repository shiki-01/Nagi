//! E2E テスト専用の観測プローブ。**通常の利用では何もしない**。
//!
//! 環境変数 `NAGI_E2E_PROBE=<path>` が設定されているプロセスでだけ、composition の更新・確定のたびに
//! `{"marked":"…","committed_delta":"…"}` を 1 行（JSON Lines）で `<path>` へ追記する。
//! 実際の OS 入力経路（SendInput → TSF → IME → テキスト欄）を通す E2E テスト（`frontend-windows-e2e`、
//! `tests/e2e/README.md`）が、未確定文字列（marked）を読むための仕組みで、`ITfRange::GetText` を
//! テストホスト側から呼ぶと文書ロックの扱いが複雑になるため、IME 側が自分の書いた内容を出す。
//!
//! - **入力内容をログに残さない方針（CLAUDE.md）との関係**: 環境変数を明示的に設定したテスト用プロセスでのみ
//!   有効で、設定しなければファイルを開きもしない（未設定時のコストは `OnceLock` の読み出しと分岐だけ）。
//!   利用者の環境では設定されない。診断ログ（`tsf_log`）とは別物で、そちらへは入力内容を書かない
//! - `marked`: 反映後の未確定文字列。`null` は「この更新では変わらない」
//! - `committed_delta`: この更新で確定した文字列（確定が無ければ空）

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::tsf_log;

/// プローブの出力先を指定する環境変数
pub const ENV_VAR: &str = "NAGI_E2E_PROBE";

/// 追記先の上限（テスト用なので回転はせず、実質無制限にする）
const MAX_BYTES: u64 = 256 * 1024 * 1024;

fn path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os(ENV_VAR)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    })
    .as_ref()
}

static TEST_EVALS: AtomicU32 = AtomicU32::new(0);
static ENGINE_RESULTS: AtomicU32 = AtomicU32::new(0);

/// `OnTestKeyDown` の評価回数を数える（プローブ有効時のみ。無効なら 0）
pub fn bump_test_evals() -> u32 {
    if path().is_some() {
        TEST_EVALS.fetch_add(1, Ordering::Relaxed) + 1
    } else {
        0
    }
}

/// エンジンからの `process_key` 応答の件数を数える（プローブ有効時のみ。無効なら 0）
pub fn bump_engine_results() -> u32 {
    if path().is_some() {
        ENGINE_RESULTS.fetch_add(1, Ordering::Relaxed) + 1
    } else {
        0
    }
}

/// JSON の文字列リテラル（引用符つき）にする。非 ASCII はそのまま（UTF-8）
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// プローブの 1 行（改行つき）を作る
pub fn format_line(marked: Option<&str>, committed_delta: &str) -> String {
    let marked = marked.map_or_else(|| "null".to_owned(), json_string);
    format!(
        "{{\"marked\":{marked},\"committed_delta\":{}}}\n",
        json_string(committed_delta)
    )
}

/// 有効なときだけ 1 行追記する。`marked` が `None` は「未確定文字列は変わらない」
pub fn record(marked: Option<&str>, committed_delta: &str) {
    let Some(path) = path() else {
        return;
    };
    tsf_log::append_line(path, &format_line(marked, committed_delta), MAX_BYTES);
}

/// イベント行（キーイベントの到達・フォーカスの確認用）。`{"event":"…","detail":"…"}`。
/// `detail` はキーの仮想キーコードや真偽値だけで、入力文字は含めない。無効なら何もしない
pub fn record_event(event: &str, detail: &str) {
    let Some(path) = path() else {
        return;
    };
    let line = format!(
        "{{\"event\":{},\"detail\":{}}}\n",
        json_string(event),
        json_string(detail)
    );
    tsf_log::append_line(path, &line, MAX_BYTES);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_json_lines() {
        assert_eq!(
            format_line(Some("こん"), ""),
            "{\"marked\":\"こん\",\"committed_delta\":\"\"}\n"
        );
        assert_eq!(
            format_line(None, "a\"b\\c\n"),
            "{\"marked\":null,\"committed_delta\":\"a\\\"b\\\\c\\n\"}\n"
        );
        assert!(format_line(Some("\u{1}"), "").contains("\\u0001"));
    }
}
