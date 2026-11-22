//! キー割当の取得・検証・書き出し（`get_keymap` / `validate_keymap` の実体）。
//!
//! 純粋な関数だけで、サーバーの状態には触らない。TOML 断片の構文は `nagi_config::keymap_fragment`、
//! キー・Action の意味の検査はエンジン（`nagi_engine::validate_bindings`）が行い、ここで 1 つにつなぐ。

use nagi_config::Severity;
use nagi_config::keymap_edit::{KeymapEdit, edit_keymap};
use nagi_config::keymap_fragment::{
    FragmentContext, parse_keymap_fragment, render_keymap_fragment,
};
use nagi_engine::{BindingInput, KeyContext, KeyMap, Preset};
use nagi_protocol::methods::{
    GetKeymapResult, KeymapContext, KeymapIssue, KeymapIssueKind, KeymapResetScope,
    ValidateKeymapResult,
};
use std::path::Path;

/// `[keymap.*]` の TOML 断片を検証する。構文の誤り・解釈できないキー / Action は `errors`（その行は
/// 無視される）、二重割当・重要キーの上書き・文字入力との衝突・OS 予約キー・未知のプリセットは `warnings`
/// （拒否せず確認のための情報）。どの問題にも断片内の行番号が付く
pub fn validate_fragment(source: &str) -> ValidateKeymapResult {
    let (fragment, syntax) = parse_keymap_fragment(source);
    let mut errors: Vec<KeymapIssue> = syntax
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| KeymapIssue {
            kind: KeymapIssueKind::Syntax,
            line: i.line,
            context: None,
            key: None,
            message: i.message,
        })
        .collect();
    let inputs: Vec<BindingInput> = fragment
        .entries
        .iter()
        .map(|e| BindingInput {
            context: match e.context {
                FragmentContext::Composing => KeyContext::Composing,
                FragmentContext::Converting => KeyContext::Converting,
            },
            line: e.line,
            key: e.key.clone(),
            action: e.action.clone(),
        })
        .collect();
    let preset = fragment
        .preset
        .as_ref()
        .map(|(line, name)| (*line, name.as_str()));
    let (semantic_errors, warnings) = nagi_engine::validate_bindings(preset, &inputs);
    errors.extend(semantic_errors);
    errors.sort_by_key(|i| i.line);
    let mut warnings = warnings;
    warnings.sort_by_key(|i| i.line);
    ValidateKeymapResult { errors, warnings }
}

/// `get_keymap` の応答を作る。`preset` が `Some` ならそのプリセットに切り替えたときの一覧
/// （`current` の利用者の上書きは常にプリセットに優先して残る）。未知の名前は標準として扱う
pub fn keymap_result(current: &KeyMap, preset: Option<&str>) -> GetKeymapResult {
    let map = match preset {
        Some(name) => current.with_other_preset(Preset::from_name(name).unwrap_or_default()),
        None => current.clone(),
    };
    GetKeymapResult {
        preset: map.preset().name().to_string(),
        presets: nagi_engine::preset_infos(),
        actions: nagi_engine::assignable_actions(),
        bindings: map.bindings(),
    }
}

/// 編集の失敗。`invalid` は呼び出し側の入力の誤り（`INVALID_PARAMS`）、そうでなければ保存先の問題
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditError {
    /// 入力の誤りか
    pub invalid: bool,
    /// 日本語の説明
    pub message: String,
}

impl EditError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            invalid: true,
            message: message.into(),
        }
    }

    fn failure(message: impl Into<String>) -> Self {
        Self {
            invalid: false,
            message: message.into(),
        }
    }
}

/// `config.toml` への編集の依頼
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditRequest {
    /// 1 キーの設定・解除
    Set {
        /// 文脈
        context: KeymapContext,
        /// キー仕様文字列
        key: String,
        /// Action 名（`None` = 解除）
        action: Option<String>,
    },
    /// 文脈ごと・全体のリセット
    Reset {
        /// 範囲
        scope: KeymapResetScope,
        /// `Context` のときの文脈
        context: Option<KeymapContext>,
    },
    /// TOML 断片の取り込み
    Import {
        /// 断片
        toml: String,
        /// 既存の上書きを残すか
        merge: bool,
    },
}

/// 上書きできる文脈だけを取り出す。それ以外は理由つきで断る
fn editable_context(context: KeymapContext) -> Result<FragmentContext, EditError> {
    match context {
        KeymapContext::Composing => Ok(FragmentContext::Composing),
        KeymapContext::Converting => Ok(FragmentContext::Converting),
        KeymapContext::Common => Err(EditError::invalid(
            "全文脈共通の割当（入力モードの切り替えなど）は変更できません",
        )),
        KeymapContext::Direct => Err(EditError::invalid(
            "未入力のときの割当（入力の開始・入力モードの切り替えなど）は変更できません",
        )),
    }
}

fn same_key(a: &str, b: &str) -> bool {
    match (nagi_engine::canonical_key(a), nagi_engine::canonical_key(b)) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

fn read_source(path: &Path) -> Result<Option<String>, EditError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(EditError::failure(format!(
            "設定ファイルを読み込めません（{}）",
            e.kind()
        ))),
    }
}

/// 検証の `errors` を 1 つの文にする（行番号つき）
fn describe_errors(errors: &[KeymapIssue]) -> String {
    errors
        .iter()
        .map(|i| {
            if i.line > 0 {
                format!("{} 行目: {}", i.line, i.message)
            } else {
                i.message.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

/// `request` を検証し、`config.toml`（`path`）に原子的に書き込む。成功したら保存した内容への警告を返す。
/// 呼び出し側は、その後に設定を読み直して反映する。検証に通らなければ何も書かない
pub fn apply_edit(
    path: Option<&Path>,
    request: &EditRequest,
) -> Result<Vec<KeymapIssue>, EditError> {
    let Some(path) = path else {
        return Err(EditError::failure(
            "設定ファイルの場所が決まっていないため保存できません",
        ));
    };
    let (edit, warnings) = match request {
        EditRequest::Set {
            context,
            key,
            action,
        } => {
            let ctx = editable_context(*context)?;
            let key = key.trim();
            let mut warnings = Vec::new();
            if let Some(action) = action {
                let action = action.trim();
                if key.is_empty() || action.is_empty() {
                    return Err(EditError::invalid("キーと操作を指定してください"));
                }
                // 1 行の断片にして、一覧の検証と同じ規則で調べる（解釈できなければ保存しない）
                let fragment =
                    render_keymap_fragment(None, &[(ctx, key.to_string(), action.to_string())]);
                let result = validate_fragment(&fragment);
                if !result.errors.is_empty() {
                    return Err(EditError::invalid(describe_errors(&result.errors)));
                }
                warnings = result.warnings;
            } else if key.is_empty() {
                return Err(EditError::invalid("キーを指定してください"));
            }
            (
                KeymapEdit::Set {
                    context: ctx,
                    key: key.to_string(),
                    action: action.as_ref().map(|a| a.trim().to_string()),
                },
                warnings,
            )
        }
        EditRequest::Reset { scope, context } => {
            let edit = match scope {
                KeymapResetScope::All => KeymapEdit::ClearAll,
                KeymapResetScope::Context => {
                    let Some(context) = context else {
                        return Err(EditError::invalid("リセットする文脈を指定してください"));
                    };
                    KeymapEdit::ClearContext(editable_context(*context)?)
                }
            };
            (edit, Vec::new())
        }
        EditRequest::Import { toml, merge } => {
            let result = validate_fragment(toml);
            if !result.errors.is_empty() {
                return Err(EditError::invalid(format!(
                    "読み込めない行があります: {}",
                    describe_errors(&result.errors)
                )));
            }
            let (fragment, _) = parse_keymap_fragment(toml);
            let entries = fragment
                .entries
                .iter()
                .map(|e| {
                    (
                        e.context,
                        e.key.trim().to_string(),
                        e.action.trim().to_string(),
                    )
                })
                .collect();
            (
                KeymapEdit::Replace {
                    preset: fragment.preset.map(|(_, name)| name),
                    entries,
                    merge: *merge,
                },
                result.warnings,
            )
        }
    };
    let source = read_source(path)?;
    let next = edit_keymap(source.as_deref(), &edit, &same_key).map_err(EditError::failure)?;
    if source.as_deref() != Some(next.as_str()) {
        nagi_config::settings::write_atomic(path, &next).map_err(|e| {
            EditError::failure(format!("設定ファイルに書き込めません（{}）", e.kind()))
        })?;
    }
    Ok(warnings)
}

/// `config.toml` の利用者の上書きを `[keymap.*]` の TOML 断片にする（キーの昇順。`keymap.preset` が標準以外なら
/// それも書く）。ファイルが無ければ空。読めない・構文が壊れていれば理由
pub fn export_toml(path: Option<&Path>) -> Result<String, EditError> {
    let Some(path) = path else {
        return Ok(String::new());
    };
    let Some(source) = read_source(path)? else {
        return Ok(String::new());
    };
    let (config, _) = nagi_config::parse(&source).map_err(|issues| {
        EditError::failure(format!(
            "config.toml を読めません: {}",
            issues
                .first()
                .map_or("構文エラー".to_string(), |i| i.message.clone())
        ))
    })?;
    let mut entries = Vec::new();
    for (context, map) in [
        (FragmentContext::Composing, &config.keymap.composing),
        (FragmentContext::Converting, &config.keymap.converting),
    ] {
        let mut keys: Vec<(&String, &String)> = map.iter().collect();
        keys.sort();
        for (key, action) in keys {
            entries.push((context, key.clone(), action.clone()));
        }
    }
    let preset = match config.keymap.preset.as_str() {
        "standard" | "default" => None,
        name => Some(name),
    };
    Ok(render_keymap_fragment(preset, &entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::methods::{KeymapContext, KeymapSource};

    #[test]
    fn preset_tables_agree_between_config_and_engine() {
        let engine: Vec<&str> = Preset::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(engine, nagi_config::KEYMAP_PRESETS);
        for name in nagi_config::KEYMAP_PRESETS {
            assert!(nagi_config::is_known_keymap_preset(name));
        }
        assert!(nagi_config::is_known_keymap_preset("default"));
        assert_eq!(Preset::from_name("default"), Some(Preset::Standard));
        assert!(!nagi_config::is_known_keymap_preset("bogus"));
    }

    #[test]
    fn validate_fragment_collects_syntax_semantic_and_warnings_with_lines() {
        let src = "\
[keymap]
preset = \"bogus\"

[keymap.composing]
\"Ctrl+j\" = \"ToKatakana\"
\"Control+j\" = \"ToHiragana\"
\"Enter\" = \"NoOp\"
\"NotAKey\" = \"NoOp\"
garbage line
\"F9\" = \"NotAnAction\"

[keymap.converting]
\"Alt+F4\" = \"Cancel\"
";
        let r = validate_fragment(src);
        let errors: Vec<(KeymapIssueKind, u32)> =
            r.errors.iter().map(|i| (i.kind, i.line)).collect();
        assert_eq!(
            errors,
            vec![
                (KeymapIssueKind::InvalidKey, 8),
                (KeymapIssueKind::Syntax, 9),
                (KeymapIssueKind::InvalidAction, 10),
            ]
        );
        let warnings: Vec<(KeymapIssueKind, u32)> =
            r.warnings.iter().map(|i| (i.kind, i.line)).collect();
        assert_eq!(
            warnings,
            vec![
                (KeymapIssueKind::UnknownPreset, 2),
                (KeymapIssueKind::DuplicateBinding, 6),
                (KeymapIssueKind::OverridesEssentialKey, 7),
                (KeymapIssueKind::OsReserved, 13),
            ]
        );
    }

    #[test]
    fn every_key_spec_the_settings_page_can_capture_is_readable_by_the_core() {
        // 設定画面（keymap.js）が押されたキーから作る書き方。コアが解釈でき、正規形に直せること
        for spec in [
            "Ctrl+Shift+j",
            "Ctrl+j",
            "Shift+a",
            "F1",
            "Shift+F12",
            "Ctrl+Shift+Right",
            "Ctrl+Space",
            "Shift+Space",
            "Ctrl+BackSpace",
            "PageDown",
            "Home",
            "Enter",
            "Henkan",
            "Muhenkan",
            "Kana",
            "Eisu",
            "Zenkaku",
            "Hiragana",
            "Katakana",
            "Ctrl+Shift+;",
            "Ctrl+Escape",
            "Ctrl+Tab",
            "Meta+j",
            "Ctrl+Alt+Delete",
            "Shift+!",
            "Ctrl+\\",
        ] {
            let canon = nagi_engine::canonical_key(spec);
            assert!(canon.is_some(), "{spec}");
            assert_eq!(
                nagi_engine::canonical_key(&canon.unwrap()).as_deref(),
                nagi_engine::canonical_key(spec).as_deref()
            );
        }
        // 別の書き方は同じキーとして扱う
        assert_eq!(
            nagi_engine::canonical_key("Control+Shift+J"),
            nagi_engine::canonical_key("Shift+Ctrl+j")
        );
        assert!(nagi_engine::canonical_key("Ctrl++").is_none());
    }

    #[test]
    fn clean_fragment_has_no_issues() {
        let r = validate_fragment(
            "[keymap]\npreset = \"ctrl_letters\"\n[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n",
        );
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{r:?}");
        assert_eq!(
            validate_fragment(""),
            ValidateKeymapResult {
                errors: vec![],
                warnings: vec![]
            }
        );
    }

    #[test]
    fn keymap_result_previews_other_preset_keeping_user_overrides() {
        let (map, _) = KeyMap::with_preset(
            "standard",
            &[(KeyContext::Composing, "Ctrl+p".into(), "NoOp".into())],
        );
        let now = keymap_result(&map, None);
        assert_eq!(now.preset, "standard");
        assert_eq!(now.presets.len(), 4);
        let preview = keymap_result(&map, Some("ctrl_letters"));
        assert_eq!(preview.preset, "ctrl_letters");
        let get = |r: &GetKeymapResult, ctx, key: &str| {
            r.bindings
                .iter()
                .find(|b| b.context == ctx && b.key == key)
                .map(|b| (b.action.clone(), b.source))
        };
        // 利用者の上書きは新しいプリセットにも優先する
        assert_eq!(
            get(&preview, KeymapContext::Composing, "Ctrl+p"),
            Some(("NoOp".to_string(), KeymapSource::User))
        );
        // プリセットだけの割当
        assert_eq!(
            get(&preview, KeymapContext::Converting, "Ctrl+p"),
            Some(("ToZenkakuAlnum".to_string(), KeymapSource::Preset))
        );
        // 未知の名前は標準
        assert_eq!(keymap_result(&map, Some("bogus")).preset, "standard");
    }
}
