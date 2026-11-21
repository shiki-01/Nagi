//! `[keymap.*]` だけの TOML 断片の読み取りと書き出し（純粋関数）。
//!
//! キーコンフィグ UI のインポート/エクスポートと、`validate_keymap` RPC が使う。構文の誤りは行番号つきの
//! [`Issue`] にし、その行だけを読み飛ばす（設定ファイル全体の「不正な行はその行のみ無視」と同じ方針）。
//! キー・Action の意味の検査はエンジン側（`nagi_engine::keymap::validate`）が行う。
//!
//! 受け付ける形式（`config.toml` の一部をそのまま切り出したもの）:
//!
//! ```toml
//! [keymap]
//! preset = "ctrl_letters"
//!
//! [keymap.composing]
//! "Ctrl+j" = "ToKatakana"
//!
//! [keymap.converting]
//! "Tab" = "NextCand"
//! ```
//!
//! 1 行 1 エントリ（`"キー" = "Action"`）に限る。値が複数行にまたがるものは構文の誤りとして扱う。

use crate::{Issue, Severity};

/// 断片の中で上書きできる文脈
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FragmentContext {
    /// `[keymap.composing]`
    Composing,
    /// `[keymap.converting]`
    Converting,
}

impl FragmentContext {
    /// テーブル名（`composing` / `converting`）
    pub fn table_name(self) -> &'static str {
        match self {
            FragmentContext::Composing => "composing",
            FragmentContext::Converting => "converting",
        }
    }
}

/// 断片の 1 エントリ（`"キー" = "Action"` の 1 行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentEntry {
    /// 断片内の行番号（1 始まり）
    pub line: u32,
    /// 文脈
    pub context: FragmentContext,
    /// キー仕様文字列（書かれたまま）
    pub key: String,
    /// Action 仕様文字列（書かれたまま）
    pub action: String,
}

/// 読み取った断片
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapFragment {
    /// `[keymap]` の `preset`（行番号, 値）
    pub preset: Option<(u32, String)>,
    /// 上書きのエントリ（書かれた順）
    pub entries: Vec<FragmentEntry>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    /// テーブルの外（先頭）
    None,
    /// `[keymap]`
    Keymap,
    /// `[keymap.composing]` / `[keymap.converting]`
    Context(FragmentContext),
    /// 対応しないテーブル（中のエントリは読み飛ばす）
    Unsupported,
}

/// `[keymap.*]` の TOML 断片を読む。構文の誤り・対応しないテーブル・未対応のキーは行番号つきの
/// エラー（`Severity::Error`。その行は読み飛ばす）として返す。意味の検査（キー・Action が解釈できるか、
/// 衝突など）はしない
pub fn parse_keymap_fragment(source: &str) -> (KeymapFragment, Vec<Issue>) {
    let source = source.trim_start_matches('\u{feff}');
    let mut fragment = KeymapFragment::default();
    let mut issues = Vec::new();
    let error = |line: u32, message: String| Issue {
        line,
        severity: Severity::Error,
        message,
    };
    let mut section = Section::None;
    for (i, raw) in source.lines().enumerate() {
        let line_no = i as u32 + 1;
        let text = raw.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        if let Some(header) = text.strip_prefix('[') {
            if header.starts_with('[') {
                issues.push(error(
                    line_no,
                    "配列のテーブル（[[...]]）は使えません".to_string(),
                ));
                section = Section::Unsupported;
                continue;
            }
            let Some((name, rest)) = header.split_once(']') else {
                issues.push(error(line_no, "テーブル名の ] がありません".to_string()));
                section = Section::Unsupported;
                continue;
            };
            let rest = rest.trim();
            if !rest.is_empty() && !rest.starts_with('#') {
                issues.push(error(
                    line_no,
                    format!("テーブル名の後ろに余分な文字があります: {rest}"),
                ));
                section = Section::Unsupported;
                continue;
            }
            let normalized: String = name.chars().filter(|c| !c.is_whitespace()).collect();
            section = match normalized.as_str() {
                "keymap" => Section::Keymap,
                "keymap.composing" => Section::Context(FragmentContext::Composing),
                "keymap.converting" => Section::Context(FragmentContext::Converting),
                other => {
                    issues.push(error(
                        line_no,
                        format!(
                            "対応しないテーブルです（[keymap] / [keymap.composing] / [keymap.converting] のみ）: [{other}]"
                        ),
                    ));
                    Section::Unsupported
                }
            };
            continue;
        }
        if section == Section::Unsupported {
            continue;
        }
        let (key, value) = match parse_entry(text) {
            Ok(kv) => kv,
            Err(reason) => {
                issues.push(error(line_no, format!("行を読めません: {reason}")));
                continue;
            }
        };
        match section {
            Section::None => issues.push(error(
                line_no,
                "テーブルの外にエントリがあります（先に [keymap.composing] などを書いてください）"
                    .to_string(),
            )),
            Section::Keymap => {
                if key == "preset" {
                    fragment.preset = Some((line_no, value));
                } else {
                    issues.push(error(
                        line_no,
                        format!("[keymap] で使えるのは preset だけです: {key}"),
                    ));
                }
            }
            Section::Context(context) => fragment.entries.push(FragmentEntry {
                line: line_no,
                context,
                key,
                action: value,
            }),
            Section::Unsupported => {}
        }
    }
    (fragment, issues)
}

/// `"キー" = "値"` の 1 行（行末のコメントを含んでよい）を TOML として読み、(キー, 文字列の値) を返す
fn parse_entry(text: &str) -> Result<(String, String), String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| e.message().to_string())?;
    let mut it = table.into_iter();
    let (key, value) = it.next().ok_or_else(|| "キーがありません".to_string())?;
    if it.next().is_some() {
        return Err("1 行に複数のエントリは書けません".to_string());
    }
    match value {
        toml::Value::String(s) => Ok((key, s)),
        _ => Err("値は文字列（\"Action 名\"）で書いてください".to_string()),
    }
}

/// 断片の書き出し（`parse_keymap_fragment` で読み直せる形）。`preset` が `Some` なら `[keymap]` に書き、
/// `entries`（文脈, キー, Action）は文脈ごとに、渡された順で書く。空の文脈のテーブルは書かない
pub fn render_keymap_fragment(
    preset: Option<&str>,
    entries: &[(FragmentContext, String, String)],
) -> String {
    let mut out = String::new();
    if let Some(preset) = preset {
        out.push_str("[keymap]\n");
        out.push_str(&format!("preset = {}\n", toml_string(preset)));
    }
    for context in [FragmentContext::Composing, FragmentContext::Converting] {
        let mut wrote_header = false;
        for (c, key, action) in entries {
            if *c != context {
                continue;
            }
            if !wrote_header {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("[keymap.{}]\n", context.table_name()));
                wrote_header = true;
            }
            out.push_str(&format!("{} = {}\n", toml_string(key), toml_string(action)));
        }
    }
    out
}

/// TOML の基本文字列（ダブルクォート）にする
fn toml_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_with_line_numbers() {
        let src = "\
# 先頭のコメント
[keymap]
preset = \"ctrl_letters\"

[keymap.composing]
\"Ctrl+j\" = \"ToKatakana\"  # 行末コメント
\"F9\" = \"ToZenkakuAlnum\"

[ keymap . converting ]
\"Tab\" = \"NextCand\"
";
        let (f, issues) = parse_keymap_fragment(src);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(f.preset, Some((3, "ctrl_letters".to_string())));
        assert_eq!(f.entries.len(), 3);
        assert_eq!(
            f.entries[0],
            FragmentEntry {
                line: 6,
                context: FragmentContext::Composing,
                key: "Ctrl+j".into(),
                action: "ToKatakana".into()
            }
        );
        assert_eq!(f.entries[1].line, 7);
        assert_eq!(f.entries[2].context, FragmentContext::Converting);
        assert_eq!(f.entries[2].line, 10);
    }

    #[test]
    fn reports_bad_lines_with_line_numbers_and_skips_them() {
        let src = "\
[keymap.composing]
\"Ctrl+j\" = \"ToKatakana\"
not toml at all
\"F9\" = 3
\"F8\" = \"ToHankakuKana\"
[other]
\"x\" = \"y\"
[keymap.converting]
\"Tab\" = \"NextCand\"
\"a\" = \"b\" \"c\" = \"d\"
";
        let (f, issues) = parse_keymap_fragment(src);
        let lines: Vec<u32> = issues.iter().map(|i| i.line).collect();
        assert_eq!(lines, vec![3, 4, 6, 10], "{issues:?}");
        assert!(issues.iter().all(|i| i.severity == Severity::Error));
        // 読めた行は残る。対応しないテーブルの中は読み飛ばす
        assert_eq!(f.entries.len(), 3);
        assert_eq!(f.entries[1].key, "F8");
        assert_eq!(f.entries[2].key, "Tab");
    }

    #[test]
    fn entry_outside_table_and_unknown_keymap_key_are_errors() {
        let (f, issues) = parse_keymap_fragment("\"a\" = \"b\"\n[keymap]\nfoo = \"x\"\n");
        assert!(f.entries.is_empty() && f.preset.is_none());
        assert_eq!(
            issues.iter().map(|i| i.line).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn render_round_trips() {
        let entries = vec![
            (
                FragmentContext::Converting,
                "Tab".to_string(),
                "NextCand".to_string(),
            ),
            (
                FragmentContext::Composing,
                "Ctrl+j".to_string(),
                "ToKatakana".to_string(),
            ),
            (
                FragmentContext::Composing,
                "\"".to_string(),
                "NoOp".to_string(),
            ),
        ];
        let text = render_keymap_fragment(Some("henkan_keys"), &entries);
        assert_eq!(
            text,
            "[keymap]\npreset = \"henkan_keys\"\n\n[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n\"\\\"\" = \"NoOp\"\n\n[keymap.converting]\n\"Tab\" = \"NextCand\"\n"
        );
        let (f, issues) = parse_keymap_fragment(&text);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(f.preset.map(|p| p.1), Some("henkan_keys".to_string()));
        let back: Vec<(FragmentContext, String, String)> = f
            .entries
            .into_iter()
            .map(|e| (e.context, e.key, e.action))
            .collect();
        // 文脈ごとにまとまるので、書き出しの順（composing → converting）になる
        assert_eq!(back[0].1, "Ctrl+j");
        assert_eq!(back[1].1, "\"");
        assert_eq!(back[2].1, "Tab");
    }

    #[test]
    fn render_empty_is_empty() {
        assert_eq!(render_keymap_fragment(None, &[]), "");
    }
}
