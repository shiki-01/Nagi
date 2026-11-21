//! キー割当の一覧（`get_keymap`）と検証（`validate_keymap`）。
//!
//! 一覧は内蔵の既定テーブル（[`default_rows`]。`resolve` との一致をテストで検査する）に、プリセット・
//! 利用者の上書きを重ねて作る。検証は「拒否」ではなく警告を返す（キーコンフィグ UI が確認に使う）。

use crate::action::{Action, Direction};
use crate::keymap::{KeyContext, KeyMap, KeySpec, ModeFilter, Preset, action_spec, parse_action};
use nagi_protocol::key::keysym;
use nagi_protocol::methods::{
    KeymapActionInfo, KeymapBinding, KeymapContext, KeymapIssue, KeymapIssueKind, KeymapPresetInfo,
    KeymapSource,
};
use nagi_protocol::view::InputMode;
use std::collections::HashMap;

impl From<KeyContext> for KeymapContext {
    fn from(c: KeyContext) -> Self {
        match c {
            KeyContext::Direct => KeymapContext::Direct,
            KeyContext::Composing => KeymapContext::Composing,
            KeyContext::Converting => KeymapContext::Converting,
        }
    }
}

/// 内蔵の既定テーブルのうち、一覧に載せる行（文脈, キー仕様, Action）。文字入力（印字可能な文字キー）と
/// 数字 0・修飾付きの文字キー（確定して素通し）は載せない。`resolve` と一致することをテストで確かめる
pub(crate) fn default_rows() -> Vec<(KeymapContext, &'static str, Action)> {
    use Action::*;
    use Direction::{Left, Right};
    use KeymapContext::{Common, Composing, Converting, Direct};
    let mut rows = vec![
        (Common, "Eisu", SetMode(InputMode::Alphanumeric)),
        (Common, "Kana", SetMode(InputMode::Hiragana)),
        (Common, "Hiragana", SetMode(InputMode::Hiragana)),
        (Common, "Katakana", SetMode(InputMode::Katakana)),
        (Common, "Zenkaku", ToggleMode),
        (Common, "Ctrl+Shift+j", SetMode(InputMode::Hiragana)),
        (Common, "Ctrl+Shift+;", SetMode(InputMode::Alphanumeric)),
        (Direct, "Ctrl+BackSpace", UndoCommit),
        (Direct, "Henkan", Reconvert),
        (Direct, "Kana", Reconvert),
        (Direct, "Hiragana", Reconvert),
        (Direct, "Ctrl+Shift+r", Reconvert),
        (Direct, "Muhenkan", ToggleMode),
        // 入力中
        (Composing, "Space", StartConvert),
        (Composing, "Shift+Space", StartConvert),
        (Composing, "Enter", Commit),
        (Composing, "Escape", Cancel),
        (Composing, "BackSpace", Backspace),
        (Composing, "Ctrl+h", Backspace),
        (Composing, "Delete", DeleteForward),
        (Composing, "Left", MoveCursor(Left)),
        (Composing, "Right", MoveCursor(Right)),
        (Composing, "Home", MoveCursorHome),
        (Composing, "Ctrl+a", MoveCursorHome),
        (Composing, "End", MoveCursorEnd),
        (Composing, "Ctrl+e", MoveCursorEnd),
        (Composing, "Ctrl+k", ToKatakana),
        (Composing, "Tab", Predict),
        (Composing, "Up", NoOp),
        (Composing, "Down", NoOp),
        (Composing, "PageUp", PrevPage),
        (Composing, "PageDown", NextPage),
        // 変換中
        (Converting, "Space", NextCand),
        (Converting, "Down", NextCand),
        (Converting, "Ctrl+n", NextCand),
        (Converting, "Tab", NextCand),
        (Converting, "Shift+Space", PrevCand),
        (Converting, "Up", PrevCand),
        (Converting, "Ctrl+p", PrevCand),
        (Converting, "Left", MoveFocus(Left)),
        (Converting, "Right", MoveFocus(Right)),
        (Converting, "Shift+Left", ResizeSegment(Left)),
        (Converting, "Shift+Right", ResizeSegment(Right)),
        (Converting, "Ctrl+Shift+Left", ExtendSelection(Left)),
        (Converting, "Ctrl+Shift+Right", ExtendSelection(Right)),
        (Converting, "Enter", Commit),
        (Converting, "Escape", Cancel),
        (Converting, "BackSpace", Cancel),
        (Converting, "Ctrl+h", Cancel),
        (Converting, "Ctrl+k", ToKatakana),
        (Converting, "Ctrl+Delete", DeleteLearning),
        (Converting, "Home", NoOp),
        (Converting, "End", NoOp),
        (Converting, "PageUp", PrevPage),
        (Converting, "PageDown", NextPage),
    ];
    // 表記を切り替えるキー（入力中・変換中で同じ）
    for ctx in [Composing, Converting] {
        rows.extend([
            (ctx, "F6", ToHiragana),
            (ctx, "F7", ToKatakana),
            (ctx, "F8", ToHankakuKana),
            (ctx, "F9", ToZenkakuAlnum),
            (ctx, "F10", ToHankakuAlnum),
            (ctx, "Muhenkan", CycleForm),
        ]);
    }
    for (n, key) in ["1", "2", "3", "4", "5", "6", "7", "8", "9"]
        .into_iter()
        .enumerate()
    {
        rows.push((Converting, key, SelectCand(n + 1)));
    }
    rows
}

struct Row {
    context: KeymapContext,
    spec: KeySpec,
    action: Action,
    source: KeymapSource,
    when_mode: Option<&'static str>,
}

impl KeyMap {
    /// 現在有効な割当の一覧（文脈別。同じ文脈・同じキーは優先の高い 1 件だけ）。
    /// 出所は 既定 < プリセット < 利用者。利用者の上書きのうち、モード切替キーに隠れて効かないものは載せない
    pub fn bindings(&self) -> Vec<KeymapBinding> {
        let mut rows: Vec<Row> = default_rows()
            .into_iter()
            .map(|(context, key, action)| Row {
                context,
                spec: KeySpec::parse(key).expect("既定テーブルのキー仕様は解釈できる"),
                action,
                source: KeymapSource::Default,
                when_mode: None,
            })
            .collect();
        for (spec, filter, action) in &self.preset_common {
            let when_mode = match filter {
                ModeFilter::Any => {
                    // すべての文脈の同じキーを隠す
                    rows.retain(|r| r.spec != *spec);
                    None
                }
                ModeFilter::Passthrough => Some("passthrough"),
            };
            rows.push(Row {
                context: KeymapContext::Common,
                spec: *spec,
                action: action.clone(),
                source: KeymapSource::Preset,
                when_mode,
            });
        }
        let put = |rows: &mut Vec<Row>, row: Row| match rows.iter_mut().find(|r| {
            r.context == row.context && r.spec == row.spec && r.when_mode == row.when_mode
        }) {
            Some(existing) => *existing = row,
            None => rows.push(row),
        };
        let mut preset_rows: Vec<_> = self.preset_ctx.iter().collect();
        preset_rows.sort_by_key(|((ctx, spec), _)| (*ctx as u8, spec.format()));
        for ((ctx, spec), action) in preset_rows {
            put(
                &mut rows,
                Row {
                    context: (*ctx).into(),
                    spec: *spec,
                    action: action.clone(),
                    source: KeymapSource::Preset,
                    when_mode: None,
                },
            );
        }
        let mut user_rows: Vec<_> = self.user.iter().collect();
        user_rows.sort_by_key(|((ctx, spec), _)| (*ctx as u8, spec.format()));
        for ((ctx, spec), action) in user_rows {
            // 共通（モード切替）のキーは先に評価されるので、上書きは効かない
            if is_shadowed_by_common(self, *ctx, spec) {
                continue;
            }
            put(
                &mut rows,
                Row {
                    context: (*ctx).into(),
                    spec: *spec,
                    action: action.clone(),
                    source: KeymapSource::User,
                    when_mode: None,
                },
            );
        }
        let order = |c: KeymapContext| match c {
            KeymapContext::Common => 0,
            KeymapContext::Direct => 1,
            KeymapContext::Composing => 2,
            KeymapContext::Converting => 3,
        };
        rows.sort_by_key(|r| order(r.context));
        rows.into_iter()
            .filter_map(|r| {
                Some(KeymapBinding {
                    context: r.context,
                    key: r.spec.format()?,
                    action: action_spec(&r.action),
                    source: r.source,
                    when_mode: r.when_mode.map(str::to_string),
                })
            })
            .collect()
    }
}

/// 内蔵の共通割当（モード切替キー）・プリセットの共通割当（全モード）に隠れるキーか。
/// 無変換は `Direct` 文脈だけが共通扱い（入力中・変換中は表記の循環として上書きできる）
fn is_shadowed_by_common(map: &KeyMap, ctx: KeyContext, spec: &KeySpec) -> bool {
    let builtin = matches!(
        spec.keysym,
        keysym::EISU_TOGGLE
            | keysym::HIRAGANA_KATAKANA
            | keysym::HIRAGANA
            | keysym::KATAKANA
            | keysym::ZENKAKU_HANKAKU
    ) || (ctx == KeyContext::Direct
        && spec.keysym == keysym::MUHENKAN
        && !spec.has_command_modifier())
        || (spec.ctrl
            && spec.shift
            && !spec.alt
            && !spec.meta
            && matches!(char::from_u32(spec.keysym), Some('j' | ';' | ':')));
    builtin
        || map
            .preset_common
            .iter()
            .any(|(s, filter, _)| s == spec && *filter == ModeFilter::Any)
}

/// 同梱のプリセットの一覧
pub fn preset_infos() -> Vec<KeymapPresetInfo> {
    Preset::ALL
        .into_iter()
        .map(|p| KeymapPresetInfo {
            name: p.name().to_string(),
            label: p.label().to_string(),
            description: p.description().to_string(),
        })
        .collect()
}

/// `[keymap.*]` で割り当てられる Action の一覧（表示名つき）
pub fn assignable_actions() -> Vec<KeymapActionInfo> {
    use KeymapContext::{Composing, Converting};
    let both = || vec![Composing, Converting];
    let mut v: Vec<(String, String, Vec<KeymapContext>)> = Vec::new();
    let mut add = |name: &str, label: &str, contexts: Vec<KeymapContext>| {
        v.push((name.to_string(), label.to_string(), contexts));
    };
    add("StartConvert", "変換（候補を開く）", vec![Composing]);
    add("Predict", "予測変換", vec![Composing]);
    add("NextCand", "次の候補", vec![Converting]);
    add("PrevCand", "前の候補", vec![Converting]);
    add("NextPage", "候補の次のページ", both());
    add("PrevPage", "候補の前のページ", both());
    add("Commit", "確定", both());
    add(
        "CommitFocused",
        "注目文節までを確定（別名 CommitPartial）",
        vec![Converting],
    );
    add("Cancel", "取消", both());
    add("Backspace", "1 文字削除", vec![Composing]);
    add("DeleteForward", "カーソルの後ろを削除", vec![Composing]);
    add("MoveCursor:Left", "カーソルを左へ", vec![Composing]);
    add("MoveCursor:Right", "カーソルを右へ", vec![Composing]);
    add("MoveCursorHome", "カーソルを先頭へ", vec![Composing]);
    add("MoveCursorEnd", "カーソルを末尾へ", vec![Composing]);
    add("MoveFocus:Left", "注目文節を左へ", vec![Converting]);
    add("MoveFocus:Right", "注目文節を右へ", vec![Converting]);
    add("ResizeSegment:Left", "文節の区切りを左へ", vec![Converting]);
    add(
        "ResizeSegment:Right",
        "文節の区切りを右へ",
        vec![Converting],
    );
    add(
        "ExtendSelection:Left",
        "文節の選択を左へ広げる",
        vec![Converting],
    );
    add(
        "ExtendSelection:Right",
        "文節の選択を右へ広げる",
        vec![Converting],
    );
    add("ConvertSelection", "選択した文節を再変換", vec![Converting]);
    add("DeleteLearning", "注目文節の学習を削除", vec![Converting]);
    add("ToHiragana", "ひらがなにする", both());
    add("ToKatakana", "全角カタカナにする", both());
    add("ToHankakuKana", "半角カナにする", both());
    add("ToZenkakuAlnum", "全角英数にする", both());
    add("ToHankakuAlnum", "半角英数にする", both());
    add("ToHankaku", "半角化", vec![Converting]);
    add("CycleForm", "表記を順に切り替える", both());
    add("ToggleLive", "ライブ変換のオン/オフ", both());
    add("ToggleMode", "入力モードの切り替え", both());
    add("CommitAndPassThrough", "確定してキーをアプリへ渡す", both());
    add("SuggestNext", "サジェストの次を選ぶ", vec![Composing]);
    add("SuggestPrev", "サジェストの前を選ぶ", vec![Composing]);
    add("NoOp", "何もしない（キーを消費する）", both());
    for n in 1..=9 {
        v.push((
            format!("SelectCand:{n}"),
            format!("{n} 番の候補を選ぶ"),
            vec![Converting],
        ));
    }
    v.into_iter()
        .map(|(name, label, contexts)| KeymapActionInfo {
            name,
            label,
            contexts,
        })
        .collect()
}

/// 検証する 1 行（利用者の上書きの 1 エントリ）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInput {
    /// 文脈
    pub context: KeyContext,
    /// 行番号（0 = 不明）
    pub line: u32,
    /// キー仕様文字列（書かれたまま）
    pub key: String,
    /// Action 仕様文字列（書かれたまま）
    pub action: String,
}

/// 上書きの検証結果: `(errors, warnings)`。`errors` はその行が無視されるもの（キー・Action が解釈できない）、
/// `warnings` は有効だが確認したいもの（拒否はしない）。`preset` は `keymap.preset` の（行番号, 値）
pub fn validate_bindings(
    preset: Option<(u32, &str)>,
    inputs: &[BindingInput],
) -> (Vec<KeymapIssue>, Vec<KeymapIssue>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let preset_value = match preset {
        Some((line, name)) => match Preset::from_name(name) {
            Some(p) => p,
            None => {
                warnings.push(KeymapIssue {
                    kind: KeymapIssueKind::UnknownPreset,
                    line,
                    context: None,
                    key: None,
                    message: format!(
                        "プリセット名が不明です（標準を使います）: {name}（使える名前: {}）",
                        Preset::ALL.map(Preset::name).join(" / ")
                    ),
                });
                Preset::Standard
            }
        },
        None => Preset::Standard,
    };
    let base = KeyMap::for_preset(preset_value);
    let mut seen: HashMap<(KeyContext, KeySpec), (u32, Action)> = HashMap::new();
    for input in inputs {
        let ctx: KeymapContext = input.context.into();
        let issue = |kind: KeymapIssueKind, message: String| KeymapIssue {
            kind,
            line: input.line,
            context: Some(ctx),
            key: Some(input.key.clone()),
            message,
        };
        let spec = match KeySpec::parse(&input.key) {
            Ok(s) => Some(s),
            Err(e) => {
                errors.push(issue(
                    KeymapIssueKind::InvalidKey,
                    format!(
                        "キーを解釈できません（この行は無視されます）: {}（{e}）",
                        input.key
                    ),
                ));
                None
            }
        };
        let action = match parse_action(&input.action) {
            Ok(a) => Some(a),
            Err(e) => {
                errors.push(issue(
                    KeymapIssueKind::InvalidAction,
                    format!(
                        "Action を解釈できません（この行は無視されます）: {}（{e}）",
                        input.action
                    ),
                ));
                None
            }
        };
        let (Some(spec), Some(action)) = (spec, action) else {
            continue;
        };
        let shown = spec.format().unwrap_or_else(|| input.key.clone());

        // 同じ文脈での二重割当（書き方が違っても同じキーになるもの。後のものが有効）
        if let Some((first_line, _)) = seen.get(&(input.context, spec)) {
            warnings.push(issue(
                KeymapIssueKind::DuplicateBinding,
                format!(
                    "同じ文脈で同じキー（{shown}）を 2 回割り当てています（{first_line} 行目と {} 行目。後のものが有効です）",
                    input.line
                ),
            ));
        }
        seen.insert((input.context, spec), (input.line, action.clone()));

        if is_shadowed_by_common(&base, input.context, &spec) {
            // モード切替キーは先に評価されるので、この割当は効かない
            warnings.push(issue(
                KeymapIssueKind::OverridesEssentialKey,
                format!("{shown} は入力モードの切り替えに使われているため、この割当は効きません"),
            ));
            continue;
        }

        // 重要キー（Enter / Esc / Space / 矢印 / Backspace）の割当の変更
        let event = crate::keys::parse_key(&input.key).ok();
        let default_action = event
            .as_ref()
            .and_then(|e| base.resolve(input.context, InputMode::Hiragana, e));
        let essential = !spec.has_command_modifier()
            && matches!(
                spec.keysym,
                keysym::RETURN
                    | keysym::ESCAPE
                    | keysym::SPACE
                    | keysym::LEFT
                    | keysym::RIGHT
                    | keysym::UP
                    | keysym::DOWN
                    | keysym::BACKSPACE
            );
        if essential && default_action.as_ref() != Some(&action) {
            warnings.push(issue(
                KeymapIssueKind::OverridesEssentialKey,
                format!(
                    "{shown} は基本の操作に使われています（現在の割当: {}）。変えると操作に迷うことがあります",
                    default_action
                        .as_ref()
                        .map_or("なし".to_string(), action_spec)
                ),
            ));
        }
        // 文字入力のキー（修飾なしの印字文字）を奪う
        let printable = !spec.has_command_modifier() && (0x21..=0x7e).contains(&spec.keysym);
        if printable && default_action.as_ref() != Some(&action) {
            warnings.push(issue(
                KeymapIssueKind::ShadowsTextInput,
                format!("{shown} は文字入力（または候補の番号選択）に使われています。割り当てるとこの文字を入力できなくなります"),
            ));
        }
        // OS が先に処理するキー
        if let Some(reason) = os_reserved_reason(&spec) {
            warnings.push(issue(
                KeymapIssueKind::OsReserved,
                format!("{shown} は {reason}ので、アプリ（Nagi）に届かないことがあります"),
            ));
        }
    }
    (errors, warnings)
}

/// OS が先に処理するキーの理由（届かないことがあるもの）
fn os_reserved_reason(spec: &KeySpec) -> Option<&'static str> {
    if spec.meta {
        return Some("Win / Cmd キーを含み、OS やアプリのショートカットに使われる");
    }
    let plain = |ctrl: bool, shift: bool, alt: bool| {
        spec.ctrl == ctrl && spec.shift == shift && spec.alt == alt
    };
    match spec.keysym {
        keysym::F4 if plain(false, false, true) => {
            Some("ウィンドウを閉じる Alt+F4 として OS が処理する")
        }
        keysym::TAB if plain(false, false, true) || plain(true, false, false) => {
            Some("ウィンドウ・タブの切り替えに使われる")
        }
        keysym::ESCAPE
            if plain(false, false, true)
                || plain(true, false, false)
                || plain(true, true, false) =>
        {
            Some("OS（タスク切り替え・スタートメニュー・タスクマネージャ）が処理する")
        }
        keysym::SPACE if plain(false, false, true) => {
            Some("ウィンドウメニューを開く Alt+Space として OS が処理する")
        }
        keysym::SPACE if plain(true, false, false) => {
            Some("入力ソースの切り替え（Ctrl+Space）として OS が処理することがある")
        }
        keysym::DELETE if plain(true, false, true) => Some("Ctrl+Alt+Delete として OS が処理する"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::parse_key;

    fn to_key_context(c: KeymapContext) -> Option<KeyContext> {
        match c {
            KeymapContext::Common => None,
            KeymapContext::Direct => Some(KeyContext::Direct),
            KeymapContext::Composing => Some(KeyContext::Composing),
            KeymapContext::Converting => Some(KeyContext::Converting),
        }
    }

    #[test]
    fn default_rows_match_resolve() {
        let map = KeyMap::default_preset();
        for (ctx, key, action) in default_rows() {
            let event = parse_key(key).unwrap_or_else(|e| panic!("{key}: {e}"));
            // Common は文脈によらない。入力中で確かめる
            let key_ctx = to_key_context(ctx).unwrap_or(KeyContext::Composing);
            assert_eq!(
                map.resolve(key_ctx, InputMode::Hiragana, &event),
                Some(action.clone()),
                "{ctx:?} {key}"
            );
            // 書式の往復
            assert_eq!(
                KeySpec::parse(key)
                    .unwrap()
                    .format()
                    .as_deref()
                    .map(|s| s.to_lowercase()),
                Some(key.to_lowercase()),
                "{key}"
            );
        }
    }

    #[test]
    fn standard_bindings_list_defaults_only() {
        let list = KeyMap::default_preset().bindings();
        assert!(list.iter().all(|b| b.source == KeymapSource::Default));
        let find = |ctx, key: &str| list.iter().find(|b| b.context == ctx && b.key == key);
        assert_eq!(
            find(KeymapContext::Composing, "F9").map(|b| b.action.as_str()),
            Some("ToZenkakuAlnum")
        );
        assert_eq!(
            find(KeymapContext::Common, "Eisu").map(|b| b.action.as_str()),
            Some("SetMode:Alphanumeric")
        );
        assert_eq!(
            find(KeymapContext::Converting, "Ctrl+Shift+Left").map(|b| b.action.as_str()),
            Some("ExtendSelection:Left")
        );
        assert_eq!(
            find(KeymapContext::Direct, "Muhenkan").map(|b| b.action.as_str()),
            Some("ToggleMode")
        );
    }

    #[test]
    fn preset_and_user_rows_replace_defaults_with_source() {
        let (map, w) = KeyMap::with_preset(
            "ctrl_letters",
            &[(KeyContext::Composing, "Ctrl+u".into(), "NoOp".into())],
        );
        assert!(w.is_empty(), "{w:?}");
        let list = map.bindings();
        let row = |ctx, key: &str| {
            list.iter()
                .filter(|b| b.context == ctx && b.key == key)
                .collect::<Vec<_>>()
        };
        // プリセット
        let p = row(KeymapContext::Converting, "Ctrl+p");
        assert_eq!(p.len(), 1);
        assert_eq!(
            (p[0].action.as_str(), p[0].source),
            ("ToZenkakuAlnum", KeymapSource::Preset)
        );
        // 利用者が最優先
        let u = row(KeymapContext::Composing, "Ctrl+u");
        assert_eq!(u.len(), 1);
        assert_eq!(
            (u[0].action.as_str(), u[0].source),
            ("NoOp", KeymapSource::User)
        );
        // 既定のまま
        assert_eq!(
            row(KeymapContext::Composing, "Space")[0].source,
            KeymapSource::Default
        );
    }

    #[test]
    fn henkan_keys_common_rows_hide_defaults() {
        let list = KeyMap::for_preset(Preset::HenkanKeys).bindings();
        // 無変換は全文脈で SetMode（既定の ToggleMode / CycleForm の行は隠れる）
        let muhenkan: Vec<_> = list.iter().filter(|b| b.key == "Muhenkan").collect();
        assert_eq!(muhenkan.len(), 1, "{muhenkan:?}");
        assert_eq!(muhenkan[0].context, KeymapContext::Common);
        assert_eq!(muhenkan[0].action, "SetMode:Alphanumeric");
        assert_eq!(muhenkan[0].source, KeymapSource::Preset);
        // 変換: 英数系のときだけの共通行 + 文脈別
        let henkan: Vec<_> = list.iter().filter(|b| b.key == "Henkan").collect();
        assert!(henkan.iter().any(|b| b.context == KeymapContext::Common
            && b.when_mode.as_deref() == Some("passthrough")
            && b.action == "SetMode:Hiragana"));
        assert!(
            henkan
                .iter()
                .any(|b| b.context == KeymapContext::Composing && b.action == "StartConvert")
        );
    }

    #[test]
    fn user_binding_on_mode_key_is_not_listed() {
        let (map, _) = KeyMap::with_preset(
            "standard",
            &[(KeyContext::Composing, "Eisu".into(), "NoOp".into())],
        );
        let list = map.bindings();
        assert!(
            !list
                .iter()
                .any(|b| b.source == KeymapSource::User && b.key == "Eisu")
        );
    }

    #[test]
    fn catalog_names_all_parse() {
        for info in assignable_actions() {
            assert!(parse_action(&info.name).is_ok(), "{}", info.name);
        }
        assert_eq!(preset_infos().len(), Preset::ALL.len());
    }

    fn input(ctx: KeyContext, line: u32, key: &str, action: &str) -> BindingInput {
        BindingInput {
            context: ctx,
            line,
            key: key.into(),
            action: action.into(),
        }
    }

    fn kinds(issues: &[KeymapIssue]) -> Vec<(KeymapIssueKind, u32)> {
        issues.iter().map(|i| (i.kind, i.line)).collect()
    }

    #[test]
    fn validate_reports_invalid_lines_as_errors() {
        let (errors, warnings) = validate_bindings(
            None,
            &[
                input(KeyContext::Composing, 2, "NotAKey", "NoOp"),
                input(KeyContext::Composing, 3, "Ctrl+j", "Insert"),
                input(KeyContext::Converting, 4, "Ctrl+j", "NextCand"),
            ],
        );
        assert_eq!(
            kinds(&errors),
            vec![
                (KeymapIssueKind::InvalidKey, 2),
                (KeymapIssueKind::InvalidAction, 3)
            ]
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn validate_warns_on_duplicate_in_same_context_only() {
        let (errors, warnings) = validate_bindings(
            None,
            &[
                input(KeyContext::Composing, 2, "Ctrl+j", "ToKatakana"),
                input(KeyContext::Composing, 3, "Control+j", "ToHiragana"),
                input(KeyContext::Converting, 6, "Ctrl+j", "ToKatakana"),
            ],
        );
        assert!(errors.is_empty());
        // 書き方が違っても同じキー。別の文脈は重複ではない
        assert_eq!(
            kinds(&warnings),
            vec![(KeymapIssueKind::DuplicateBinding, 3)]
        );
    }

    #[test]
    fn validate_warns_on_essential_and_printable_and_os_keys() {
        let (errors, warnings) = validate_bindings(
            None,
            &[
                input(KeyContext::Composing, 1, "Enter", "NoOp"),
                input(KeyContext::Converting, 2, "Enter", "Commit"),
                input(KeyContext::Converting, 3, "a", "NextCand"),
                input(KeyContext::Converting, 4, "1", "SelectCand:1"),
                input(KeyContext::Converting, 5, "Alt+F4", "Cancel"),
                input(KeyContext::Composing, 6, "Meta+Space", "NoOp"),
                input(KeyContext::Composing, 7, "Space", "Commit"),
                input(KeyContext::Composing, 8, "F9", "ToHankakuAlnum"),
            ],
        );
        assert!(errors.is_empty(), "{errors:?}");
        let got = kinds(&warnings);
        assert!(
            got.contains(&(KeymapIssueKind::OverridesEssentialKey, 1)),
            "{got:?}"
        );
        assert!(
            !got.iter().any(|(_, l)| *l == 2),
            "既定と同じ割当は警告しない: {got:?}"
        );
        assert!(
            got.contains(&(KeymapIssueKind::ShadowsTextInput, 3)),
            "{got:?}"
        );
        assert!(!got.iter().any(|(_, l)| *l == 4), "{got:?}");
        assert!(got.contains(&(KeymapIssueKind::OsReserved, 5)), "{got:?}");
        assert!(got.contains(&(KeymapIssueKind::OsReserved, 6)), "{got:?}");
        assert!(
            got.contains(&(KeymapIssueKind::OverridesEssentialKey, 7)),
            "{got:?}"
        );
        // 通常の F キーの再割当は警告しない
        assert!(!got.iter().any(|(_, l)| *l == 8), "{got:?}");
    }

    #[test]
    fn validate_warns_when_mode_key_cannot_be_overridden() {
        let (_, warnings) = validate_bindings(
            Some((1, "henkan_keys")),
            &[
                input(KeyContext::Composing, 3, "Eisu", "NoOp"),
                // 無変換は標準では入力中に上書きできるが、henkan_keys では共通割当になる
                input(KeyContext::Composing, 4, "Muhenkan", "NoOp"),
            ],
        );
        assert_eq!(
            kinds(&warnings),
            vec![
                (KeymapIssueKind::OverridesEssentialKey, 3),
                (KeymapIssueKind::OverridesEssentialKey, 4)
            ]
        );
        let (_, w2) =
            validate_bindings(None, &[input(KeyContext::Composing, 4, "Muhenkan", "NoOp")]);
        assert!(w2.is_empty(), "{w2:?}");
    }

    #[test]
    fn validate_unknown_preset_is_a_warning_with_line() {
        let (errors, warnings) = validate_bindings(Some((2, "bogus")), &[]);
        assert!(errors.is_empty());
        assert_eq!(kinds(&warnings), vec![(KeymapIssueKind::UnknownPreset, 2)]);
        let (_, ok) = validate_bindings(Some((2, "default")), &[]);
        assert!(ok.is_empty());
    }
}
