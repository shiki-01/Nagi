//! `config.toml` の `[keymap.*]` だけを書き換える（キーコンフィグ UI の保存。純粋関数）。
//!
//! `toml_edit` で書き換えるので、利用者のコメント・並び・他のセクションは保たれる（`settings::apply_changes`
//! と同じ流儀）。キーの書き方の違い（`Ctrl+j` と `Control+j`）は呼び出し側が渡す `same_key` で同一視する
//! （キー仕様の解釈はエンジンの仕事で、この crate は持たない）。

use crate::keymap_fragment::FragmentContext;
use toml_edit::{DocumentMut, Item, Table, TableLike};

/// 書き換えの種類
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapEdit {
    /// 1 キーの割当を設定する（`action` が `None` なら、そのキーの上書きを消す）
    Set {
        /// 文脈
        context: FragmentContext,
        /// キー仕様文字列
        key: String,
        /// Action 仕様文字列。`None` = 上書きを消す
        action: Option<String>,
    },
    /// 1 つの文脈の上書きをすべて消す
    ClearContext(FragmentContext),
    /// すべての文脈の上書きを消す（`preset` は残す）
    ClearAll,
    /// 断片の内容で置き換える（インポート）。`merge` でなければ先に全文脈の上書きを消す。
    /// `preset` が `Some` なら `[keymap] preset` も書く
    Replace {
        /// 書く `preset`
        preset: Option<String>,
        /// 上書き（文脈, キー, Action）
        entries: Vec<(FragmentContext, String, String)>,
        /// 既存の上書きを残して足す
        merge: bool,
    },
}

const CONTEXTS: [FragmentContext; 2] = [FragmentContext::Composing, FragmentContext::Converting];

/// `source`（無ければ空）に `edit` を適用した新しい全文を返す。`same_key(a, b)` は 2 つのキー仕様が同じキーか。
/// `config.toml` が TOML として読めない・`[keymap]` が表でないときは理由を返す（書き込まない）
pub fn edit_keymap(
    source: Option<&str>,
    edit: &KeymapEdit,
    same_key: &dyn Fn(&str, &str) -> bool,
) -> Result<String, String> {
    let text = source.unwrap_or("");
    let (bom, text) = match text.strip_prefix('\u{feff}') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("config.toml の構文エラー: {}", e.message()))?;
    match edit {
        KeymapEdit::Set {
            context,
            key,
            action,
        } => set_binding(&mut doc, *context, key, action.as_deref(), same_key)?,
        KeymapEdit::ClearContext(context) => clear_context(&mut doc, *context)?,
        KeymapEdit::ClearAll => {
            for context in CONTEXTS {
                clear_context(&mut doc, context)?;
            }
        }
        KeymapEdit::Replace {
            preset,
            entries,
            merge,
        } => {
            if !merge {
                for context in CONTEXTS {
                    clear_context(&mut doc, context)?;
                }
            }
            if let Some(preset) = preset {
                keymap_table(&mut doc, true)?
                    .expect("作成を頼んだ")
                    .insert("preset", toml_edit::value(preset.as_str()));
            }
            for (context, key, action) in entries {
                set_binding(&mut doc, *context, key, Some(action), same_key)?;
            }
        }
    }
    let mut out = doc.to_string();
    if bom {
        out.insert(0, '\u{feff}');
    }
    Ok(out)
}

fn not_a_table(name: &str) -> String {
    format!("config.toml の {name} が表（テーブル）ではないため書き換えられません")
}

/// `[keymap]`（`create` なら無ければ作る）
fn keymap_table(doc: &mut DocumentMut, create: bool) -> Result<Option<&mut dyn TableLike>, String> {
    if !doc.contains_key("keymap") {
        if !create {
            return Ok(None);
        }
        let mut table = Table::new();
        // 子のテーブルだけを書くときは `[keymap]` の見出しを出さない
        table.set_implicit(true);
        doc.insert("keymap", Item::Table(table));
    }
    match doc
        .get_mut("keymap")
        .and_then(|item| item.as_table_like_mut())
    {
        Some(table) => Ok(Some(table)),
        None => Err(not_a_table("[keymap]")),
    }
}

/// `[keymap.<context>]`（`create` なら無ければ作る）
fn context_table(
    doc: &mut DocumentMut,
    context: FragmentContext,
    create: bool,
) -> Result<Option<&mut dyn TableLike>, String> {
    let name = context.table_name();
    let Some(keymap) = keymap_table(doc, create)? else {
        return Ok(None);
    };
    if !keymap.contains_key(name) {
        if !create {
            return Ok(None);
        }
        keymap.insert(name, Item::Table(Table::new()));
    }
    match keymap
        .get_mut(name)
        .and_then(|item| item.as_table_like_mut())
    {
        Some(table) => Ok(Some(table)),
        None => Err(not_a_table(&format!("[keymap.{name}]"))),
    }
}

fn set_binding(
    doc: &mut DocumentMut,
    context: FragmentContext,
    key: &str,
    action: Option<&str>,
    same_key: &dyn Fn(&str, &str) -> bool,
) -> Result<(), String> {
    let Some(table) = context_table(doc, context, action.is_some())? else {
        return Ok(());
    };
    // 同じキーを別の書き方で書いた行も消す（書かれたままのキーが完全に一致する行は、コメントを保つため残して値だけ替える）
    let same: Vec<String> = table
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| same_key(k, key))
        .collect();
    let exact = same.iter().any(|k| k == key);
    for k in &same {
        if !(exact && k == key && action.is_some()) {
            table.remove(k);
        }
    }
    if let Some(action) = action {
        if exact {
            if let Some(item) = table.get_mut(key) {
                let decor = item.as_value().map(|v| v.decor().clone());
                let mut new = toml_edit::Value::from(action);
                if let Some(decor) = decor {
                    *new.decor_mut() = decor;
                }
                *item = Item::Value(new);
            }
        } else {
            table.insert(key, toml_edit::value(action));
        }
    }
    prune(doc, context);
    Ok(())
}

fn clear_context(doc: &mut DocumentMut, context: FragmentContext) -> Result<(), String> {
    if let Some(keymap) = keymap_table(doc, false)? {
        keymap.remove(context.table_name());
    }
    prune(doc, context);
    Ok(())
}

/// 空になった `[keymap.<context>]` と `[keymap]` を片づける
fn prune(doc: &mut DocumentMut, context: FragmentContext) {
    let name = context.table_name();
    let Some(keymap) = doc
        .get_mut("keymap")
        .and_then(|item| item.as_table_like_mut())
    else {
        return;
    };
    if keymap
        .get(name)
        .and_then(Item::as_table_like)
        .is_some_and(|t| t.is_empty())
    {
        keymap.remove(name);
    }
    if keymap.is_empty() {
        doc.remove("keymap");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用: 小文字にして `Control` を `Ctrl` に読み替えて比べる
    fn same(a: &str, b: &str) -> bool {
        let norm = |s: &str| s.to_lowercase().replace("control", "ctrl");
        norm(a) == norm(b)
    }

    fn set(
        src: Option<&str>,
        ctx: FragmentContext,
        key: &str,
        action: Option<&str>,
    ) -> Result<String, String> {
        edit_keymap(
            src,
            &KeymapEdit::Set {
                context: ctx,
                key: key.into(),
                action: action.map(str::to_string),
            },
            &same,
        )
    }

    use FragmentContext::{Composing, Converting};

    #[test]
    fn creates_the_table_in_an_empty_or_missing_file() {
        let out = set(None, Composing, "Ctrl+j", Some("ToKatakana")).unwrap();
        assert_eq!(out, "[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n");
        let out = set(Some(""), Converting, "F2", Some("NextCand")).unwrap();
        assert_eq!(out, "[keymap.converting]\nF2 = \"NextCand\"\n");
    }

    #[test]
    fn keeps_comments_other_sections_and_the_preset() {
        let src = "# 先頭\n[conversion]\nsync_budget_ms = 150 # 待ち時間\n\n[keymap]\n# プリセット\npreset = \"ctrl_letters\"\n\n[keymap.composing]\n# 気に入っている\n\"Ctrl+j\" = \"ToKatakana\" # 行末\n\n[update]\nauto_check = true\n";
        let out = set(Some(src), Composing, "Ctrl+j", Some("ToHiragana")).unwrap();
        assert!(out.contains("# 先頭"), "{out}");
        assert!(out.contains("sync_budget_ms = 150 # 待ち時間"), "{out}");
        assert!(
            out.contains("# プリセット\npreset = \"ctrl_letters\""),
            "{out}"
        );
        assert!(out.contains("# 気に入っている"), "{out}");
        assert!(
            out.contains("\"Ctrl+j\" = \"ToHiragana\" # 行末"),
            "コメントは残る: {out}"
        );
        assert!(out.contains("[update]\nauto_check = true"), "{out}");
        // 読み直せる
        let (config, issues) = crate::parse(&out).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(config.keymap.preset, "ctrl_letters");
        assert_eq!(
            config.keymap.composing.get("Ctrl+j").map(String::as_str),
            Some("ToHiragana")
        );
    }

    #[test]
    fn appends_a_new_key_and_replaces_a_differently_spelled_duplicate() {
        let src = "[keymap.composing]\n\"Control+j\" = \"ToKatakana\"\nF2 = \"Commit\"\n";
        let out = set(Some(src), Composing, "Ctrl+j", Some("ToHiragana")).unwrap();
        assert!(!out.contains("Control+j"), "{out}");
        assert!(out.contains("\"Ctrl+j\" = \"ToHiragana\""), "{out}");
        assert!(out.contains("F2 = \"Commit\""), "{out}");
        let (config, _) = crate::parse(&out).unwrap();
        assert_eq!(config.keymap.composing.len(), 2);
    }

    #[test]
    fn removing_the_last_key_removes_the_empty_tables() {
        let src =
            "[update]\nauto_check = true\n\n[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n";
        let out = set(Some(src), Composing, "Control+J", None).unwrap();
        assert_eq!(out, "[update]\nauto_check = true\n");
        // 消すものが無ければ何も変えない（テーブルも作らない）
        assert_eq!(
            set(Some("a = 1\n"), Converting, "F2", None).unwrap(),
            "a = 1\n"
        );
        // プリセットがあれば [keymap] は残る
        let src = "[keymap]\npreset = \"henkan_keys\"\n[keymap.converting]\nF2 = \"NextCand\"\n";
        let out = set(Some(src), Converting, "F2", None).unwrap();
        assert!(out.contains("preset = \"henkan_keys\""), "{out}");
        assert!(!out.contains("converting"), "{out}");
    }

    #[test]
    fn clear_context_and_clear_all() {
        let src = "[keymap]\npreset = \"ctrl_letters\"\n[keymap.composing]\nF2 = \"Commit\"\n[keymap.converting]\nF3 = \"Cancel\"\n";
        let out = edit_keymap(Some(src), &KeymapEdit::ClearContext(Composing), &same).unwrap();
        assert!(!out.contains("composing") && out.contains("F3"), "{out}");
        let out = edit_keymap(Some(src), &KeymapEdit::ClearAll, &same).unwrap();
        assert_eq!(out, "[keymap]\npreset = \"ctrl_letters\"\n");
        // 何も無いファイルは変わらない
        assert_eq!(
            edit_keymap(Some("x = 1\n"), &KeymapEdit::ClearAll, &same).unwrap(),
            "x = 1\n"
        );
    }

    #[test]
    fn replace_overwrites_or_merges_and_sets_the_preset() {
        let src = "[keymap.composing]\nF2 = \"Commit\"\n[keymap.converting]\nF3 = \"Cancel\"\n";
        let entries = vec![(Composing, "F4".to_string(), "NoOp".to_string())];
        let out = edit_keymap(
            Some(src),
            &KeymapEdit::Replace {
                preset: Some("function_keys".into()),
                entries: entries.clone(),
                merge: false,
            },
            &same,
        )
        .unwrap();
        let (config, _) = crate::parse(&out).unwrap();
        assert_eq!(config.keymap.preset, "function_keys");
        assert_eq!(config.keymap.composing.len(), 1);
        assert!(config.keymap.converting.is_empty(), "{out}");
        let out = edit_keymap(
            Some(src),
            &KeymapEdit::Replace {
                preset: None,
                entries,
                merge: true,
            },
            &same,
        )
        .unwrap();
        let (config, _) = crate::parse(&out).unwrap();
        assert_eq!(config.keymap.preset, "standard");
        assert_eq!(config.keymap.composing.len(), 2);
        assert_eq!(config.keymap.converting.len(), 1);
    }

    #[test]
    fn handles_bom_and_refuses_broken_or_non_table_input() {
        let out = set(Some("\u{feff}a = 1\n"), Composing, "F2", Some("Commit")).unwrap();
        assert!(out.starts_with('\u{feff}'), "{out:?}");
        let e = set(Some("[keymap\nx"), Composing, "F2", Some("Commit")).unwrap_err();
        assert!(e.contains("構文エラー"), "{e}");
        let e = set(Some("keymap = 3\n"), Composing, "F2", Some("Commit")).unwrap_err();
        assert!(e.contains("[keymap]"), "{e}");
        let e = set(
            Some("[keymap]\ncomposing = 3\n"),
            Composing,
            "F2",
            Some("Commit"),
        )
        .unwrap_err();
        assert!(e.contains("[keymap.composing]"), "{e}");
    }

    #[test]
    fn dotted_and_inline_tables_are_edited_in_place() {
        let src = "keymap.composing.F2 = \"Commit\"\n";
        let out = set(Some(src), Composing, "F3", Some("Cancel")).unwrap();
        let (config, issues) = crate::parse(&out).unwrap();
        assert!(issues.is_empty(), "{issues:?} {out}");
        assert_eq!(config.keymap.composing.len(), 2, "{out}");
        let src = "[keymap]\ncomposing = { F2 = \"Commit\" }\n";
        let out = set(Some(src), Composing, "F3", Some("Cancel")).unwrap();
        let (config, _) = crate::parse(&out).unwrap();
        assert_eq!(config.keymap.composing.len(), 2, "{out}");
    }
}
