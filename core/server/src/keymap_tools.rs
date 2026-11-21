//! キー割当の取得・検証・書き出し（`get_keymap` / `validate_keymap` の実体）。
//!
//! 純粋な関数だけで、サーバーの状態には触らない。TOML 断片の構文は `nagi_config::keymap_fragment`、
//! キー・Action の意味の検査はエンジン（`nagi_engine::validate_bindings`）が行い、ここで 1 つにつなぐ。

use nagi_config::Severity;
use nagi_config::keymap_fragment::{FragmentContext, parse_keymap_fragment};
use nagi_engine::{BindingInput, KeyContext, KeyMap, Preset};
use nagi_protocol::methods::{GetKeymapResult, KeymapIssue, KeymapIssueKind, ValidateKeymapResult};

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
