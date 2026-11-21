//! キーマップ: キー入力 → [`Action`] の解決。
//!
//! 解決の優先順位（高い順）:
//!
//! 1. プリセットの「共通」割当（全文脈・全入力モード。モード切替キーを持つプリセット用。
//!    `SetMode` を使うので、利用者の上書きでは作れない）
//! 2. 内蔵の共通割当（英数・かな・全角/半角などのモード切替キー。常に有効）
//! 3. 英数系のモードでは、ここまでに無ければ素通し（`None`）
//! 4. 利用者の上書き（`[keymap.composing]` / `[keymap.converting]`。[`KeyMap::with_preset`]）
//! 5. プリセットの文脈別の割当
//! 6. 内蔵の既定テーブル（文脈別）
//!
//! 解決結果が `None` のときの扱い（素通し / 消費のみ）は状態機械側が文脈で決める。
//! 利用者の上書きは `composing` / `converting` に限る（`Direct` 文脈・`Insert` / `SetMode` は
//! 対象外。設計上の判断）。プリセットだけが `Direct` 文脈・モード切替の割当を持てる。

use crate::action::{Action, Direction};
use crate::keys::{KeyParseError, parse_key};
use nagi_protocol::key::{KeyEvent, KeyKind, keysym};
use nagi_protocol::view::InputMode;

/// キー入力の文脈（状態機械の状態に対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyContext {
    /// 未入力（英数モードもここ）
    Direct,
    /// ローマ字入力中
    Composing,
    /// 変換中
    Converting,
}

/// キー入力の識別（上書きテーブルのキー）。テキストは無視し、キーそのものと修飾だけを見る。
/// 英字の keysym は小文字にそろえる（大文字は `shift` で区別する）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KeySpec {
    pub(crate) keysym: u32,
    pub(crate) ctrl: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
    pub(crate) meta: bool,
}

impl KeySpec {
    pub(crate) fn from_event(key: &KeyEvent) -> Self {
        let keysym = match char::from_u32(key.keysym) {
            Some(c) if c.is_ascii_uppercase() => c.to_ascii_lowercase() as u32,
            _ => key.keysym,
        };
        KeySpec {
            keysym,
            ctrl: key.modifiers.ctrl,
            shift: key.modifiers.shift,
            alt: key.modifiers.alt,
            meta: key.modifiers.meta,
        }
    }

    /// キー仕様文字列（`Ctrl+Shift+Right` 形式）から作る
    pub(crate) fn parse(spec: &str) -> Result<Self, KeyParseError> {
        parse_key(spec).map(|k| KeySpec::from_event(&k))
    }

    /// キー仕様文字列にする（`parse` で同じ `KeySpec` に戻る）。名前を持たない keysym は `None`
    pub(crate) fn format(&self) -> Option<String> {
        let name = match NAMED_KEYS.iter().find(|(k, _)| *k == self.keysym) {
            Some((_, name)) => (*name).to_string(),
            None => {
                let c = char::from_u32(self.keysym).filter(|c| !c.is_control() && *c != ' ')?;
                c.to_string()
            }
        };
        let mut out = String::new();
        for (on, label) in [
            (self.ctrl, "Ctrl"),
            (self.shift, "Shift"),
            (self.alt, "Alt"),
            (self.meta, "Meta"),
        ] {
            if on {
                out.push_str(label);
                out.push('+');
            }
        }
        out.push_str(&name);
        Some(out)
    }

    pub(crate) fn has_command_modifier(&self) -> bool {
        self.ctrl || self.alt || self.meta
    }
}

/// 名前を持つキーの keysym と、キー仕様文字列での名前（`parse_key` が受け付ける名前）
const NAMED_KEYS: &[(u32, &str)] = &[
    (keysym::SPACE, "Space"),
    (keysym::RETURN, "Enter"),
    (keysym::ESCAPE, "Escape"),
    (keysym::BACKSPACE, "BackSpace"),
    (keysym::DELETE, "Delete"),
    (keysym::TAB, "Tab"),
    (keysym::LEFT, "Left"),
    (keysym::RIGHT, "Right"),
    (keysym::UP, "Up"),
    (keysym::DOWN, "Down"),
    (keysym::HOME, "Home"),
    (keysym::END, "End"),
    (keysym::PAGE_UP, "PageUp"),
    (keysym::PAGE_DOWN, "PageDown"),
    (keysym::F1, "F1"),
    (keysym::F2, "F2"),
    (keysym::F3, "F3"),
    (keysym::F4, "F4"),
    (keysym::F5, "F5"),
    (keysym::F6, "F6"),
    (keysym::F7, "F7"),
    (keysym::F8, "F8"),
    (keysym::F9, "F9"),
    (keysym::F10, "F10"),
    (keysym::F11, "F11"),
    (keysym::F12, "F12"),
    (keysym::EISU_TOGGLE, "Eisu"),
    (keysym::HIRAGANA_KATAKANA, "Kana"),
    (keysym::HIRAGANA, "Hiragana"),
    (keysym::KATAKANA, "Katakana"),
    (keysym::ZENKAKU_HANKAKU, "Zenkaku"),
    (keysym::MUHENKAN, "Muhenkan"),
    (keysym::HENKAN, "Henkan"),
];

/// 同梱のキーマッププリセット
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Preset {
    /// 標準（内蔵の既定テーブルそのまま）
    #[default]
    Standard,
    /// ファンクションキー重視（Ctrl+文字キーをアプリへ渡す）
    FunctionKeys,
    /// Ctrl+文字キー重視（Ctrl+U / I / O / P / T で表記の切替）
    CtrlLetters,
    /// 変換/無変換キー重視（無変換 = 英数モード、変換 = 変換・再変換・かなモード）
    HenkanKeys,
}

/// プリセットの割当が効く入力モード
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeFilter {
    /// どのモードでも
    Any,
    /// 英数系（素通しのモード）のときだけ
    Passthrough,
}

impl ModeFilter {
    fn matches(self, mode: InputMode) -> bool {
        match self {
            ModeFilter::Any => true,
            ModeFilter::Passthrough => mode.is_passthrough(),
        }
    }
}

impl Preset {
    /// 同梱のプリセット（表示順）
    pub const ALL: [Preset; 4] = [
        Preset::Standard,
        Preset::FunctionKeys,
        Preset::CtrlLetters,
        Preset::HenkanKeys,
    ];

    /// `keymap.preset` に書く正規の名前
    pub fn name(self) -> &'static str {
        match self {
            Preset::Standard => "standard",
            Preset::FunctionKeys => "function_keys",
            Preset::CtrlLetters => "ctrl_letters",
            Preset::HenkanKeys => "henkan_keys",
        }
    }

    /// 名前（正規の名前、または旧名 `default`）からプリセットを引く
    pub fn from_name(name: &str) -> Option<Preset> {
        if name == "default" {
            return Some(Preset::Standard);
        }
        Preset::ALL.into_iter().find(|p| p.name() == name)
    }

    /// 表示名（日本語）
    pub fn label(self) -> &'static str {
        match self {
            Preset::Standard => "標準",
            Preset::FunctionKeys => "ファンクションキー重視",
            Preset::CtrlLetters => "Ctrl+文字キー重視",
            Preset::HenkanKeys => "変換・無変換キー重視",
        }
    }

    /// 説明（日本語）
    pub fn description(self) -> &'static str {
        match self {
            Preset::Standard => {
                "標準の割当。F6〜F10 で表記を切り替え、無変換キーで表記を循環します。"
            }
            Preset::FunctionKeys => {
                "標準に加えて、Ctrl+K / Ctrl+N / Ctrl+P をアプリへ渡します（確定してから素通し）。F6〜F10 で表記を切り替える使い方向け。"
            }
            Preset::CtrlLetters => {
                "Ctrl+U = ひらがな、Ctrl+I = カタカナ、Ctrl+O = 半角カナ、Ctrl+P = 全角英数、Ctrl+T = 半角英数、Ctrl+G = 取消。Ctrl+P を使うので、変換中の前候補は ↑ と Shift+Space です（Ctrl+N は次候補のまま）。"
            }
            Preset::HenkanKeys => {
                "無変換 = 英数モード（入力中なら確定して切り替え）、変換 = 入力中は変換・変換中は次候補・未入力は再変換、英数モードの変換 = ひらがなモード。"
            }
        }
    }

    /// 全文脈・全入力モードで効く割当（`SetMode` を含められる。キー仕様, モードの条件, Action）
    pub(crate) fn common_entries(self) -> Vec<(&'static str, ModeFilter, Action)> {
        match self {
            Preset::HenkanKeys => vec![
                (
                    "Muhenkan",
                    ModeFilter::Any,
                    Action::SetMode(InputMode::Alphanumeric),
                ),
                (
                    "Henkan",
                    ModeFilter::Passthrough,
                    Action::SetMode(InputMode::Hiragana),
                ),
            ],
            _ => Vec::new(),
        }
    }

    /// 文脈別の割当（キー仕様, Action）。`Direct` 文脈も持てる
    pub(crate) fn context_entries(self) -> Vec<(KeyContext, &'static str, Action)> {
        use KeyContext::{Composing, Converting};
        match self {
            Preset::Standard => Vec::new(),
            Preset::FunctionKeys => vec![
                (Composing, "Ctrl+k", Action::CommitAndPassThrough),
                (Converting, "Ctrl+k", Action::CommitAndPassThrough),
                (Converting, "Ctrl+n", Action::CommitAndPassThrough),
                (Converting, "Ctrl+p", Action::CommitAndPassThrough),
            ],
            Preset::CtrlLetters => {
                let mut v = Vec::new();
                for ctx in [Composing, Converting] {
                    v.extend([
                        (ctx, "Ctrl+u", Action::ToHiragana),
                        (ctx, "Ctrl+i", Action::ToKatakana),
                        (ctx, "Ctrl+o", Action::ToHankakuKana),
                        (ctx, "Ctrl+p", Action::ToZenkakuAlnum),
                        (ctx, "Ctrl+t", Action::ToHankakuAlnum),
                        (ctx, "Ctrl+g", Action::Cancel),
                    ]);
                }
                v
            }
            Preset::HenkanKeys => vec![
                (Composing, "Henkan", Action::StartConvert),
                (Converting, "Henkan", Action::NextCand),
            ],
        }
    }
}

/// キーマップ。プリセットと、`composing` / `converting` 文脈の利用者の上書きを持つ
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    preset: Preset,
    /// 利用者の上書き
    pub(crate) user: std::collections::HashMap<(KeyContext, KeySpec), Action>,
    /// プリセットの文脈別の割当
    pub(crate) preset_ctx: std::collections::HashMap<(KeyContext, KeySpec), Action>,
    /// プリセットの共通の割当
    pub(crate) preset_common: Vec<(KeySpec, ModeFilter, Action)>,
}

/// 印字可能文字（Ctrl/Alt/Meta なし、Space 除く）。`text`（OS の配列が生成した文字）を優先する
fn printable(key: &KeyEvent) -> Option<char> {
    let m = key.modifiers;
    if m.ctrl || m.alt || m.meta {
        return None;
    }
    let mut chars = key.text.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && !c.is_control()
        && c != ' '
    {
        return Some(c);
    }
    if key.text.is_empty() && (0x21..=0x7e).contains(&key.keysym) {
        return char::from_u32(key.keysym);
    }
    None
}

/// 修飾なしで 2 文字以上のテキストが生成されたときの挿入（絵文字の ZWJ 列など）
fn multi_text(key: &KeyEvent) -> Option<Action> {
    let m = key.modifiers;
    if m.ctrl
        || m.alt
        || m.meta
        || key.text.chars().count() < 2
        || key.text.chars().any(char::is_control)
    {
        return None;
    }
    Some(Action::InsertText(key.text.clone()))
}

/// Ctrl 単独（Shift 可）+ 英字のときのその小文字
fn ctrl_letter(key: &KeyEvent) -> Option<char> {
    let m = key.modifiers;
    if !m.ctrl || m.alt || m.meta {
        return None;
    }
    let c = char::from_u32(key.keysym)?;
    c.is_ascii_alphabetic().then(|| c.to_ascii_lowercase())
}

/// Ctrl+Backspace（確定取消。設計書 §4 状態遷移表の既定キー。Direct 文脈でのみ意味を持つ）
fn is_undo_commit(key: &KeyEvent) -> bool {
    let m = key.modifiers;
    m.ctrl && !m.alt && !m.meta && key.keysym == keysym::BACKSPACE
}

/// Ctrl+Shift だけが押されている（Alt / Meta なし）
fn is_ctrl_shift(key: &KeyEvent) -> bool {
    let m = key.modifiers;
    m.ctrl && m.shift && !m.alt && !m.meta
}

/// Ctrl / Alt / Meta が押されている
fn has_command_modifier(key: &KeyEvent) -> bool {
    let m = key.modifiers;
    m.ctrl || m.alt || m.meta
}

/// 修飾付きの「文字キー」（Cmd+C など）。確定して素通しする対象
fn is_command_chord(key: &KeyEvent) -> bool {
    has_command_modifier(key) && (!key.text.is_empty() || (0x20..=0x7e).contains(&key.keysym))
}

impl KeyMap {
    /// 既定プリセット（標準。上書きなし）
    pub fn default_preset() -> Self {
        KeyMap::default()
    }

    /// 標準プリセットに、`composing` / `converting` のキー上書きを重ねたキーマップを作る
    /// （[`KeyMap::with_preset`] の標準版）
    pub fn with_overrides(bindings: &[(KeyContext, String, String)]) -> (KeyMap, Vec<String>) {
        KeyMap::with_preset(Preset::Standard.name(), bindings)
    }

    /// プリセット（名前。未知の名前は標準として扱う。警告は設定の読み込み側が行番号つきで出す）に、
    /// 利用者の上書きを重ねたキーマップを作る。利用者の上書きは常にプリセットに優先する。
    ///
    /// `bindings` は `(文脈, キー仕様文字列, Action 仕様文字列)` の組。解析できないキー・
    /// Action はスキップし、その理由を人間向けの文言として 2 番目の戻り値に集める
    /// （呼び出し側が `notify` などで知らせるため）。有効なものだけで上書きテーブルを作る。
    /// 同じキーが複数あれば後のものが有効
    pub fn with_preset(
        preset_name: &str,
        bindings: &[(KeyContext, String, String)],
    ) -> (KeyMap, Vec<String>) {
        let preset = Preset::from_name(preset_name).unwrap_or_default();
        let mut map = KeyMap::for_preset(preset);
        let mut warnings = Vec::new();
        for (context, key_spec, action_spec) in bindings {
            let spec = match KeySpec::parse(key_spec) {
                Ok(k) => k,
                Err(e) => {
                    warnings.push(format!("キーマップ: 不明なキー '{key_spec}'（{e}）"));
                    continue;
                }
            };
            let action = match parse_action(action_spec) {
                Ok(a) => a,
                Err(e) => {
                    warnings.push(format!(
                        "キーマップ: 不明なアクション '{action_spec}'（{e}）"
                    ));
                    continue;
                }
            };
            map.user.insert((*context, spec), action);
        }
        (map, warnings)
    }

    /// プリセットだけ（利用者の上書きなし）のキーマップ
    pub fn for_preset(preset: Preset) -> KeyMap {
        let spec = |s: &str| KeySpec::parse(s).expect("プリセットのキー仕様は解釈できる");
        KeyMap {
            preset,
            user: Default::default(),
            preset_ctx: preset
                .context_entries()
                .into_iter()
                .map(|(ctx, key, action)| ((ctx, spec(key)), action))
                .collect(),
            preset_common: preset
                .common_entries()
                .into_iter()
                .map(|(key, filter, action)| (spec(key), filter, action))
                .collect(),
        }
    }

    /// 利用者の上書きを保ったまま、プリセットだけを差し替えたキーマップ（プリセット切替の確認用）
    pub fn with_other_preset(&self, preset: Preset) -> KeyMap {
        let mut map = KeyMap::for_preset(preset);
        map.user = self.user.clone();
        map
    }

    /// 選択中のプリセット
    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// キーを Action に解決する
    ///
    /// `None` = 割当なし。英数系のモードでは入力モード切替キー以外は常に `None`（素通し）。
    pub fn resolve(&self, context: KeyContext, mode: InputMode, key: &KeyEvent) -> Option<Action> {
        if key.kind == KeyKind::FlagsChanged {
            return None;
        }
        let spec = KeySpec::from_event(key);
        // プリセットの共通割当（モード切替を含められる）
        if let Some((_, _, action)) = self
            .preset_common
            .iter()
            .find(|(s, filter, _)| *s == spec && filter.matches(mode))
        {
            return Some(action.clone());
        }
        // 共通（全文脈・全モード）
        match key.keysym {
            keysym::EISU_TOGGLE => return Some(Action::SetMode(InputMode::Alphanumeric)),
            keysym::HIRAGANA_KATAKANA | keysym::HIRAGANA => {
                // ひらがなモードの未入力で押したら再変換（macOS 標準と同じく、英数モードからは
                // 1 回目でひらがなモードになり、2 回目で選択中のテキストを再変換する）
                if context == KeyContext::Direct && mode == InputMode::Hiragana {
                    return Some(Action::Reconvert);
                }
                return Some(Action::SetMode(InputMode::Hiragana));
            }
            keysym::KATAKANA => return Some(Action::SetMode(InputMode::Katakana)),
            keysym::ZENKAKU_HANKAKU => return Some(Action::ToggleMode),
            // 無変換: 未入力では英数/かなの切替。入力中・変換中は表記の循環（下の文脈別の表）
            keysym::MUHENKAN if context == KeyContext::Direct && !has_command_modifier(key) => {
                return Some(Action::ToggleMode);
            }
            _ => {}
        }
        // macOS 標準の日本語入力と同じモード切替（かな/英数キーの無い US 配列向け）:
        // ⌃⇧J = ひらがな、⌃⇧; = 英数（Shift で `:` になる配列も受ける）
        if is_ctrl_shift(key) {
            match char::from_u32(key.keysym) {
                Some('j' | 'J') => return Some(Action::SetMode(InputMode::Hiragana)),
                Some(';' | ':') => return Some(Action::SetMode(InputMode::Alphanumeric)),
                _ => {}
            }
        }
        if mode.is_passthrough() {
            return None;
        }
        if let Some(action) = self.user.get(&(context, spec)) {
            return Some(action.clone());
        }
        if let Some(action) = self.preset_ctx.get(&(context, spec)) {
            return Some(action.clone());
        }
        match context {
            KeyContext::Direct => {
                if is_undo_commit(key) {
                    Some(Action::UndoCommit)
                } else if key.keysym == keysym::HENKAN && !has_command_modifier(key) {
                    // 変換キー: 選択中のテキストの再変換（MS-IME と同じ）
                    Some(Action::Reconvert)
                } else if is_ctrl_shift(key)
                    && matches!(char::from_u32(key.keysym), Some('r' | 'R'))
                {
                    // ⌃⇧R: 選択中のテキストの再変換（かなキーの無い US 配列向け）
                    Some(Action::Reconvert)
                } else if mode == InputMode::ZenkakuAlnum {
                    // 全角英数モード: 打った文字を全角にしてそのまま確定する
                    printable(key)
                        .map(|c| {
                            Action::CommitText(nagi_rewriter::to_fullwidth_alnum(&c.to_string()))
                        })
                        .or_else(|| {
                            multi_text(key).map(|a| match a {
                                Action::InsertText(t) => Action::CommitText(t),
                                other => other,
                            })
                        })
                } else {
                    printable(key)
                        .map(Action::Insert)
                        .or_else(|| multi_text(key))
                }
            }
            KeyContext::Composing => self.resolve_composing(key),
            KeyContext::Converting => self.resolve_converting(key),
        }
    }

    /// Composing でサジェスト（入力中の候補）が表示されているときの、↓ / Tab / ↑ の割当。
    /// 既定の解決（`resolve`）は Tab = `Predict`・↑↓ = `NoOp` のままで、サジェストの有無は
    /// コンテキストだけでは分からないので、状態機械が表示中のときだけこれを先に引く。
    /// キーマップ（利用者・プリセット）で上書きされているキーは `None`（上書きを優先する）
    pub fn suggest_action(&self, key: &KeyEvent) -> Option<Action> {
        let m = key.modifiers;
        if m.ctrl || m.alt || m.meta {
            return None;
        }
        let action = match key.keysym {
            keysym::DOWN | keysym::TAB => Action::SuggestNext,
            keysym::UP => Action::SuggestPrev,
            _ => return None,
        };
        let slot = (KeyContext::Composing, KeySpec::from_event(key));
        let overridden = self.user.contains_key(&slot) || self.preset_ctx.contains_key(&slot);
        (!overridden).then_some(action)
    }

    fn resolve_composing(&self, key: &KeyEvent) -> Option<Action> {
        let m = key.modifiers;
        if let Some(c) = ctrl_letter(key) {
            return match c {
                'h' => Some(Action::Backspace),
                'a' => Some(Action::MoveCursorHome),
                'e' => Some(Action::MoveCursorEnd),
                'k' => Some(Action::ToKatakana),
                _ => Some(Action::CommitAndPassThrough),
            };
        }
        if let Some(c) = printable(key) {
            return Some(Action::Insert(c));
        }
        if let Some(action) = multi_text(key) {
            return Some(action);
        }
        if let Some(action) = form_key(key) {
            return Some(action);
        }
        match key.keysym {
            keysym::SPACE if !m.ctrl && !m.alt && !m.meta => Some(Action::StartConvert),
            keysym::RETURN => Some(Action::Commit),
            keysym::ESCAPE => Some(Action::Cancel),
            keysym::BACKSPACE => Some(Action::Backspace),
            keysym::DELETE => Some(Action::DeleteForward),
            keysym::LEFT => Some(Action::MoveCursor(Direction::Left)),
            keysym::RIGHT => Some(Action::MoveCursor(Direction::Right)),
            keysym::HOME => Some(Action::MoveCursorHome),
            keysym::END => Some(Action::MoveCursorEnd),
            _ if is_command_chord(key) => Some(Action::CommitAndPassThrough),
            keysym::TAB if !m.ctrl && !m.alt && !m.meta => Some(Action::Predict),
            keysym::PAGE_UP => Some(Action::PrevPage),
            keysym::PAGE_DOWN => Some(Action::NextPage),
            keysym::UP | keysym::DOWN | keysym::TAB => Some(Action::NoOp),
            _ => None,
        }
    }

    fn resolve_converting(&self, key: &KeyEvent) -> Option<Action> {
        let m = key.modifiers;
        if let Some(c) = ctrl_letter(key) {
            return match c {
                'n' => Some(Action::NextCand),
                'p' => Some(Action::PrevCand),
                'k' => Some(Action::ToKatakana),
                'h' => Some(Action::Cancel),
                _ => Some(Action::CommitAndPassThrough),
            };
        }
        if let Some(c) = printable(key) {
            return Some(match c.to_digit(10) {
                Some(n) if (1..=9).contains(&n) => Action::SelectCand(n as usize),
                _ => Action::Insert(c),
            });
        }
        if let Some(action) = multi_text(key) {
            return Some(action);
        }
        if let Some(action) = form_key(key) {
            return Some(action);
        }
        let dir = |d: Direction| {
            if m.ctrl && m.shift {
                Action::ExtendSelection(d)
            } else if m.shift {
                Action::ResizeSegment(d)
            } else {
                Action::MoveFocus(d)
            }
        };
        match key.keysym {
            keysym::SPACE if m.shift && !m.ctrl && !m.alt && !m.meta => Some(Action::PrevCand),
            keysym::SPACE if !m.ctrl && !m.alt && !m.meta => Some(Action::NextCand),
            keysym::DOWN => Some(Action::NextCand),
            keysym::UP => Some(Action::PrevCand),
            keysym::LEFT => Some(dir(Direction::Left)),
            keysym::RIGHT => Some(dir(Direction::Right)),
            keysym::RETURN => Some(Action::Commit),
            keysym::ESCAPE | keysym::BACKSPACE => Some(Action::Cancel),
            keysym::DELETE if m.ctrl => Some(Action::DeleteLearning),
            _ if is_command_chord(key) => Some(Action::CommitAndPassThrough),
            keysym::TAB if !m.ctrl && !m.alt && !m.meta => Some(Action::NextCand),
            keysym::PAGE_UP => Some(Action::PrevPage),
            keysym::PAGE_DOWN => Some(Action::NextPage),
            keysym::TAB | keysym::HOME | keysym::END => Some(Action::NoOp),
            _ => None,
        }
    }
}

/// 入力中・変換中に共通の「表記を切り替えるキー」: F6〜F10 と無変換（Ctrl / Alt / Meta なし）
fn form_key(key: &KeyEvent) -> Option<Action> {
    if has_command_modifier(key) {
        return None;
    }
    match key.keysym {
        keysym::F6 => Some(Action::ToHiragana),
        keysym::F7 => Some(Action::ToKatakana),
        keysym::F8 => Some(Action::ToHankakuKana),
        keysym::F9 => Some(Action::ToZenkakuAlnum),
        keysym::F10 => Some(Action::ToHankakuAlnum),
        keysym::MUHENKAN => Some(Action::CycleForm),
        _ => None,
    }
}

/// キーマップの上書き設定で使う Action 名のパース。
///
/// 引数を持たない Action はそのままの名前、`Direction` を持つものは `名前:Left` / `名前:Right`、
/// `SelectCand` は `SelectCand:1`〜`SelectCand:9`。`CommitPartial` は `CommitFocused` の別名（設計書 §4.9 の名前）。
/// `Insert` / `InsertText` / `CommitText` / `SetMode` は対象外（挿入・モード切替は上書きの対象外。設計上の判断）
pub fn parse_action(spec: &str) -> Result<Action, String> {
    let (name, arg) = match spec.split_once(':') {
        Some((n, a)) => (n, Some(a)),
        None => (spec, None),
    };
    let direction = |a: Option<&str>| -> Result<Direction, String> {
        match a {
            Some("Left") => Ok(Direction::Left),
            Some("Right") => Ok(Direction::Right),
            other => Err(format!("Direction が必要です（Left/Right）: {other:?}")),
        }
    };
    match (name, arg) {
        ("StartConvert", None) => Ok(Action::StartConvert),
        ("Predict", None) => Ok(Action::Predict),
        ("NextCand", None) => Ok(Action::NextCand),
        ("PrevCand", None) => Ok(Action::PrevCand),
        ("Commit", None) => Ok(Action::Commit),
        ("CommitFocused" | "CommitPartial", None) => Ok(Action::CommitFocused),
        ("Cancel", None) => Ok(Action::Cancel),
        ("Backspace", None) => Ok(Action::Backspace),
        ("DeleteForward", None) => Ok(Action::DeleteForward),
        ("MoveCursorHome", None) => Ok(Action::MoveCursorHome),
        ("MoveCursorEnd", None) => Ok(Action::MoveCursorEnd),
        ("DeleteLearning", None) => Ok(Action::DeleteLearning),
        ("ToKatakana", None) => Ok(Action::ToKatakana),
        ("ToHiragana", None) => Ok(Action::ToHiragana),
        ("ToHankaku", None) => Ok(Action::ToHankaku),
        ("ToHankakuKana", None) => Ok(Action::ToHankakuKana),
        ("ToZenkakuAlnum", None) => Ok(Action::ToZenkakuAlnum),
        ("ToHankakuAlnum", None) => Ok(Action::ToHankakuAlnum),
        ("CycleForm", None) => Ok(Action::CycleForm),
        ("NextPage", None) => Ok(Action::NextPage),
        ("PrevPage", None) => Ok(Action::PrevPage),
        ("ToggleLive", None) => Ok(Action::ToggleLive),
        ("ConvertSelection", None) => Ok(Action::ConvertSelection),
        ("ToggleMode", None) => Ok(Action::ToggleMode),
        ("CommitAndPassThrough", None) => Ok(Action::CommitAndPassThrough),
        ("SuggestNext", None) => Ok(Action::SuggestNext),
        ("SuggestPrev", None) => Ok(Action::SuggestPrev),
        ("NoOp", None) => Ok(Action::NoOp),
        ("MoveCursor", a) => Ok(Action::MoveCursor(direction(a)?)),
        ("MoveFocus", a) => Ok(Action::MoveFocus(direction(a)?)),
        ("ResizeSegment", a) => Ok(Action::ResizeSegment(direction(a)?)),
        ("ExtendSelection", a) => Ok(Action::ExtendSelection(direction(a)?)),
        ("SelectCand", Some(n)) => n
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=9).contains(n))
            .map(Action::SelectCand)
            .ok_or_else(|| format!("SelectCand は 1〜9 の数字が必要です: {n}")),
        _ => Err(format!(
            "不明な Action 名、または引数の付け方が不正です: {spec}"
        )),
    }
}

/// Action を設定の書式の名前にする（[`parse_action`] の逆。`SetMode:...` のように上書きでは書けない
/// ものも、一覧表示のために名前にする。`CommitText` は `CommitText` とだけ）
pub fn action_spec(action: &Action) -> String {
    let dir = |d: &Direction| match d {
        Direction::Left => "Left",
        Direction::Right => "Right",
    };
    match action {
        Action::Insert(_) => "Insert".to_string(),
        Action::InsertText(_) => "InsertText".to_string(),
        Action::CommitText(_) => "CommitText".to_string(),
        Action::SelectCand(n) => format!("SelectCand:{n}"),
        Action::MoveCursor(d) => format!("MoveCursor:{}", dir(d)),
        Action::MoveFocus(d) => format!("MoveFocus:{}", dir(d)),
        Action::ResizeSegment(d) => format!("ResizeSegment:{}", dir(d)),
        Action::ExtendSelection(d) => format!("ExtendSelection:{}", dir(d)),
        Action::SetMode(mode) => format!("SetMode:{}", mode_name(*mode)),
        other => other.to_string(),
    }
}

fn mode_name(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Hiragana => "Hiragana",
        InputMode::Alphanumeric => "Alphanumeric",
        InputMode::Katakana => "Katakana",
        InputMode::HankakuKatakana => "HankakuKatakana",
        InputMode::ZenkakuAlnum => "ZenkakuAlnum",
        InputMode::HankakuAlnum => "HankakuAlnum",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::parse_key;

    fn resolve(ctx: KeyContext, mode: InputMode, key: &str) -> Option<Action> {
        KeyMap::default_preset().resolve(ctx, mode, &parse_key(key).unwrap())
    }
    use InputMode::{Alphanumeric, Hiragana};
    use KeyContext::{Composing, Converting, Direct};

    #[test]
    fn mode_keys_work_everywhere() {
        for ctx in [Direct, Composing, Converting] {
            for mode in [Hiragana, Alphanumeric] {
                assert_eq!(
                    resolve(ctx, mode, "Eisu"),
                    Some(Action::SetMode(Alphanumeric))
                );
                // ひらがなモードの未入力では再変換（下のテスト）
                if !(ctx == Direct && mode == Hiragana) {
                    assert_eq!(resolve(ctx, mode, "Kana"), Some(Action::SetMode(Hiragana)));
                }
                assert_eq!(resolve(ctx, mode, "Zenkaku"), Some(Action::ToggleMode));
            }
        }
    }

    #[test]
    fn ctrl_shift_mode_keys_like_macos() {
        for ctx in [Direct, Composing, Converting] {
            for mode in [Hiragana, Alphanumeric] {
                assert_eq!(
                    resolve(ctx, mode, "Ctrl+Shift+J"),
                    Some(Action::SetMode(Hiragana))
                );
                assert_eq!(
                    resolve(ctx, mode, "Ctrl+Shift+;"),
                    Some(Action::SetMode(Alphanumeric))
                );
            }
        }
        assert_eq!(
            resolve(Direct, Hiragana, "Ctrl+Shift+R"),
            Some(Action::Reconvert)
        );
        assert_ne!(
            resolve(Composing, Hiragana, "Ctrl+Shift+R"),
            Some(Action::Reconvert)
        );
    }

    #[test]
    fn reconvert_keys_in_direct_hiragana() {
        assert_eq!(resolve(Direct, Hiragana, "Kana"), Some(Action::Reconvert));
        assert_eq!(resolve(Direct, Hiragana, "Henkan"), Some(Action::Reconvert));
        // 英数モードからのかなキーはモード切替（2 回目で再変換）
        assert_eq!(
            resolve(Direct, Alphanumeric, "Kana"),
            Some(Action::SetMode(Hiragana))
        );
        assert_eq!(resolve(Direct, Alphanumeric, "Henkan"), None);
    }

    #[test]
    fn alphanumeric_passes_everything_else() {
        for key in ["a", "Space", "Enter", "Left", "Ctrl+k", "A"] {
            assert_eq!(resolve(Direct, Alphanumeric, key), None, "{key}");
        }
    }

    #[test]
    fn direct_inserts_printables_only() {
        assert_eq!(resolve(Direct, Hiragana, "a"), Some(Action::Insert('a')));
        assert_eq!(resolve(Direct, Hiragana, "A"), Some(Action::Insert('A')));
        assert_eq!(resolve(Direct, Hiragana, "-"), Some(Action::Insert('-')));
        assert_eq!(resolve(Direct, Hiragana, "1"), Some(Action::Insert('1')));
        for key in [
            "Space",
            "Enter",
            "Escape",
            "BackSpace",
            "Left",
            "Ctrl+a",
            "Meta+c",
            "Alt+a",
            "Tab",
        ] {
            assert_eq!(resolve(Direct, Hiragana, key), None, "{key}");
        }
    }

    #[test]
    fn composing_table() {
        let cases = [
            ("a", Action::Insert('a')),
            ("Shift+a", Action::Insert('A')),
            ("Space", Action::StartConvert),
            ("Shift+Space", Action::StartConvert),
            ("Enter", Action::Commit),
            ("Escape", Action::Cancel),
            ("BackSpace", Action::Backspace),
            ("Ctrl+h", Action::Backspace),
            ("Delete", Action::DeleteForward),
            ("Left", Action::MoveCursor(Direction::Left)),
            ("Right", Action::MoveCursor(Direction::Right)),
            ("Home", Action::MoveCursorHome),
            ("Ctrl+a", Action::MoveCursorHome),
            ("End", Action::MoveCursorEnd),
            ("Ctrl+e", Action::MoveCursorEnd),
            ("Ctrl+k", Action::ToKatakana),
            ("F7", Action::ToKatakana),
            ("F6", Action::ToHiragana),
            ("Meta+c", Action::CommitAndPassThrough),
            ("Ctrl+c", Action::CommitAndPassThrough),
            ("Alt+x", Action::CommitAndPassThrough),
            ("Up", Action::NoOp),
            ("Down", Action::NoOp),
            ("Tab", Action::Predict),
            ("PageUp", Action::PrevPage),
            ("PageDown", Action::NextPage),
            ("F8", Action::ToHankakuKana),
            ("F9", Action::ToZenkakuAlnum),
            ("F10", Action::ToHankakuAlnum),
            ("Muhenkan", Action::CycleForm),
        ];
        for (key, expected) in cases {
            assert_eq!(resolve(Composing, Hiragana, key), Some(expected), "{key}");
        }
        assert_eq!(resolve(Composing, Hiragana, "F1"), None);
    }

    #[test]
    fn converting_table() {
        let cases = [
            ("Space", Action::NextCand),
            ("Down", Action::NextCand),
            ("Ctrl+n", Action::NextCand),
            ("Shift+Space", Action::PrevCand),
            ("Up", Action::PrevCand),
            ("Ctrl+p", Action::PrevCand),
            ("Left", Action::MoveFocus(Direction::Left)),
            ("Right", Action::MoveFocus(Direction::Right)),
            ("Shift+Left", Action::ResizeSegment(Direction::Left)),
            ("Shift+Right", Action::ResizeSegment(Direction::Right)),
            ("Ctrl+Shift+Left", Action::ExtendSelection(Direction::Left)),
            (
                "Ctrl+Shift+Right",
                Action::ExtendSelection(Direction::Right),
            ),
            ("Enter", Action::Commit),
            ("Escape", Action::Cancel),
            ("BackSpace", Action::Cancel),
            ("Ctrl+k", Action::ToKatakana),
            ("F7", Action::ToKatakana),
            ("Ctrl+Delete", Action::DeleteLearning),
            ("F6", Action::ToHiragana),
            ("1", Action::SelectCand(1)),
            ("9", Action::SelectCand(9)),
            ("0", Action::Insert('0')),
            ("a", Action::Insert('a')),
            ("Meta+c", Action::CommitAndPassThrough),
            ("Tab", Action::NextCand),
        ];
        for (key, expected) in cases {
            assert_eq!(resolve(Converting, Hiragana, key), Some(expected), "{key}");
        }
    }

    #[test]
    fn flags_changed_is_never_bound() {
        let mut key = parse_key("Eisu").unwrap();
        key.kind = KeyKind::FlagsChanged;
        assert_eq!(
            KeyMap::default_preset().resolve(Composing, Hiragana, &key),
            None
        );
    }

    #[test]
    fn text_from_layout_wins_over_keysym() {
        // Dvorak 等: keysym と text が食い違う場合は text を使う
        let mut key = parse_key("a").unwrap();
        key.text = "ー".into();
        assert_eq!(
            KeyMap::default_preset().resolve(Direct, Hiragana, &key),
            Some(Action::Insert('ー'))
        );
    }

    #[test]
    fn multi_scalar_text_becomes_insert_text() {
        let mut key = parse_key("a").unwrap();
        key.keysym = 0;
        key.text = "👨\u{200d}👩".into();
        for ctx in [Direct, Composing, Converting] {
            assert_eq!(
                KeyMap::default_preset().resolve(ctx, Hiragana, &key),
                Some(Action::InsertText("👨\u{200d}👩".into())),
                "{ctx:?}"
            );
        }
        // Alphanumeric・修飾付きでは素通し / 確定して素通し
        assert_eq!(
            KeyMap::default_preset().resolve(Direct, Alphanumeric, &key),
            None
        );
        key.modifiers.meta = true;
        assert_eq!(
            KeyMap::default_preset().resolve(Direct, Hiragana, &key),
            None
        );
    }

    #[test]
    fn overrides_replace_composing_key() {
        let (km, warnings) =
            KeyMap::with_overrides(&[(Composing, "Ctrl+j".to_string(), "ToKatakana".to_string())]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            km.resolve(Composing, Hiragana, &parse_key("Ctrl+j").unwrap()),
            Some(Action::ToKatakana)
        );
    }

    #[test]
    fn overrides_leave_unmapped_keys_at_default() {
        let (km, warnings) =
            KeyMap::with_overrides(&[(Composing, "Ctrl+j".to_string(), "ToKatakana".to_string())]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            km.resolve(Composing, Hiragana, &parse_key("a").unwrap()),
            Some(Action::Insert('a'))
        );
    }

    #[test]
    fn overrides_report_unknown_key_and_action() {
        let (km, warnings) = KeyMap::with_overrides(&[
            (Composing, "NotAKey".to_string(), "ToKatakana".to_string()),
            (Composing, "Ctrl+j".to_string(), "NotAnAction".to_string()),
        ]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        // どちらも上書きテーブルには入らず、既定の挙動のまま
        assert_eq!(
            km.resolve(Composing, Hiragana, &parse_key("Ctrl+j").unwrap()),
            Some(Action::CommitAndPassThrough)
        );
    }

    #[test]
    fn parse_action_no_arg() {
        assert_eq!(parse_action("NextCand"), Ok(Action::NextCand));
    }

    #[test]
    fn parse_action_predict() {
        assert_eq!(parse_action("Predict"), Ok(Action::Predict));
    }

    #[test]
    fn parse_action_to_hankaku() {
        assert_eq!(parse_action("ToHankaku"), Ok(Action::ToHankaku));
    }

    #[test]
    fn parse_action_commit_focused() {
        assert_eq!(parse_action("CommitFocused"), Ok(Action::CommitFocused));
    }

    #[test]
    fn parse_action_direction() {
        assert_eq!(
            parse_action("MoveCursor:Left"),
            Ok(Action::MoveCursor(Direction::Left))
        );
    }

    #[test]
    fn parse_action_select_cand() {
        assert_eq!(parse_action("SelectCand:3"), Ok(Action::SelectCand(3)));
        assert!(parse_action("SelectCand:0").is_err());
        assert!(parse_action("SelectCand:10").is_err());
    }

    #[test]
    fn parse_action_rejects_insert_and_unknown() {
        assert!(parse_action("Insert").is_err());
        assert!(parse_action("NotAnAction").is_err());
    }

    #[test]
    fn parse_action_suggest() {
        assert_eq!(parse_action("SuggestNext"), Ok(Action::SuggestNext));
        assert_eq!(parse_action("SuggestPrev"), Ok(Action::SuggestPrev));
        assert!(parse_action("SuggestNext:Left").is_err());
    }

    #[test]
    fn suggest_action_defaults_and_overrides() {
        let key = |s: &str| parse_key(s).unwrap();
        let map = KeyMap::default_preset();
        // 既定の解決は従来どおり（サジェストの有無は状態機械が見る）
        assert_eq!(resolve(Composing, Hiragana, "Tab"), Some(Action::Predict));
        assert_eq!(resolve(Composing, Hiragana, "Down"), Some(Action::NoOp));
        assert_eq!(map.suggest_action(&key("Tab")), Some(Action::SuggestNext));
        assert_eq!(map.suggest_action(&key("Down")), Some(Action::SuggestNext));
        assert_eq!(map.suggest_action(&key("Up")), Some(Action::SuggestPrev));
        assert_eq!(map.suggest_action(&key("Ctrl+Down")), None);
        assert_eq!(map.suggest_action(&key("Left")), None);
        // 上書きされたキーは対象外
        let (map, w) = KeyMap::with_overrides(&[(Composing, "Tab".into(), "ToKatakana".into())]);
        assert!(w.is_empty());
        assert_eq!(map.suggest_action(&key("Tab")), None);
        assert_eq!(map.suggest_action(&key("Down")), Some(Action::SuggestNext));
    }

    #[test]
    fn new_form_keys_and_muhenkan_defaults() {
        for ctx in [Composing, Converting] {
            for (key, action) in [
                ("F6", Action::ToHiragana),
                ("F7", Action::ToKatakana),
                ("F8", Action::ToHankakuKana),
                ("F9", Action::ToZenkakuAlnum),
                ("F10", Action::ToHankakuAlnum),
                ("Muhenkan", Action::CycleForm),
                ("Shift+F9", Action::ToZenkakuAlnum),
            ] {
                assert_eq!(resolve(ctx, Hiragana, key), Some(action), "{ctx:?} {key}");
            }
            // Ctrl / Alt 付きの F キーは表記の切替ではない
            assert_ne!(
                resolve(ctx, Hiragana, "Ctrl+F9"),
                Some(Action::ToZenkakuAlnum)
            );
            assert_ne!(
                resolve(ctx, Hiragana, "Alt+F8"),
                Some(Action::ToHankakuKana)
            );
        }
        // 未入力の無変換は英数 / かなの切替（英数モードからも）
        assert_eq!(
            resolve(Direct, Hiragana, "Muhenkan"),
            Some(Action::ToggleMode)
        );
        assert_eq!(
            resolve(Direct, Alphanumeric, "Muhenkan"),
            Some(Action::ToggleMode)
        );
        // F8〜F10 は未入力では素通し
        assert_eq!(resolve(Direct, Hiragana, "F9"), None);
    }

    #[test]
    fn katakana_and_hiragana_keys() {
        for ctx in [Direct, Composing, Converting] {
            assert_eq!(
                resolve(ctx, Hiragana, "Katakana"),
                Some(Action::SetMode(InputMode::Katakana)),
                "{ctx:?}"
            );
            assert_eq!(
                resolve(ctx, Alphanumeric, "Katakana"),
                Some(Action::SetMode(InputMode::Katakana))
            );
        }
        // ひらがなキー: ひらがなモードの未入力は再変換、それ以外はひらがなモードへ
        assert_eq!(
            resolve(Direct, Hiragana, "Hiragana"),
            Some(Action::Reconvert)
        );
        assert_eq!(
            resolve(Direct, InputMode::Katakana, "Hiragana"),
            Some(Action::SetMode(Hiragana))
        );
        assert_eq!(
            resolve(Composing, Hiragana, "Hiragana"),
            Some(Action::SetMode(Hiragana))
        );
    }

    #[test]
    fn page_keys_map_to_page_actions() {
        for ctx in [Composing, Converting] {
            assert_eq!(resolve(ctx, Hiragana, "PageUp"), Some(Action::PrevPage));
            assert_eq!(resolve(ctx, Hiragana, "PageDown"), Some(Action::NextPage));
        }
        assert_eq!(resolve(Direct, Hiragana, "PageDown"), None);
    }

    #[test]
    fn new_modes_resolve_like_their_family() {
        // 半角英数は英数と同じ（モード切替キー以外は素通し）
        assert_eq!(resolve(Direct, InputMode::HankakuAlnum, "a"), None);
        assert_eq!(resolve(Direct, InputMode::HankakuAlnum, "Space"), None);
        assert_eq!(
            resolve(Direct, InputMode::HankakuAlnum, "Kana"),
            Some(Action::SetMode(Hiragana))
        );
        // カタカナ・半角カナは文字をそのまま入力に使う
        for mode in [InputMode::Katakana, InputMode::HankakuKatakana] {
            assert_eq!(resolve(Direct, mode, "a"), Some(Action::Insert('a')));
        }
        // 全角英数は打った文字を全角にして確定する
        let full = InputMode::ZenkakuAlnum;
        assert_eq!(
            resolve(Direct, full, "a"),
            Some(Action::CommitText("ａ".into()))
        );
        assert_eq!(
            resolve(Direct, full, "Shift+a"),
            Some(Action::CommitText("Ａ".into()))
        );
        assert_eq!(
            resolve(Direct, full, "1"),
            Some(Action::CommitText("１".into()))
        );
        assert_eq!(resolve(Direct, full, "Ctrl+a"), None);
        assert_eq!(resolve(Direct, full, "Enter"), None);
    }

    #[test]
    fn presets_by_name_and_alias() {
        assert_eq!(Preset::from_name("standard"), Some(Preset::Standard));
        assert_eq!(Preset::from_name("default"), Some(Preset::Standard));
        assert_eq!(Preset::from_name("henkan_keys"), Some(Preset::HenkanKeys));
        assert_eq!(Preset::from_name("bogus"), None);
        for p in Preset::ALL {
            assert_eq!(Preset::from_name(p.name()), Some(p));
            // 表のキー仕様はすべて解釈できる（`for_preset` が panic しない）
            let map = KeyMap::for_preset(p);
            assert_eq!(map.preset(), p);
        }
        let (map, w) = KeyMap::with_preset("bogus", &[]);
        assert!(w.is_empty());
        assert_eq!(map.preset(), Preset::Standard);
    }

    fn with_preset(name: &str, ctx: KeyContext, mode: InputMode, key: &str) -> Option<Action> {
        KeyMap::with_preset(name, &[])
            .0
            .resolve(ctx, mode, &parse_key(key).unwrap())
    }

    #[test]
    fn preset_ctrl_letters_table_and_conflict_resolution() {
        for ctx in [Composing, Converting] {
            for (key, action) in [
                ("Ctrl+u", Action::ToHiragana),
                ("Ctrl+i", Action::ToKatakana),
                ("Ctrl+o", Action::ToHankakuKana),
                ("Ctrl+p", Action::ToZenkakuAlnum),
                ("Ctrl+t", Action::ToHankakuAlnum),
                ("Ctrl+g", Action::Cancel),
            ] {
                assert_eq!(
                    with_preset("ctrl_letters", ctx, Hiragana, key),
                    Some(action),
                    "{ctx:?} {key}"
                );
            }
        }
        // Ctrl+P は変換中の前候補をやめて全角英数に。次候補は Ctrl+N・↓・Space のまま、前候補は ↑・Shift+Space
        assert_eq!(
            with_preset("ctrl_letters", Converting, Hiragana, "Ctrl+n"),
            Some(Action::NextCand)
        );
        assert_eq!(
            with_preset("ctrl_letters", Converting, Hiragana, "Up"),
            Some(Action::PrevCand)
        );
        assert_eq!(
            with_preset("ctrl_letters", Converting, Hiragana, "Shift+Space"),
            Some(Action::PrevCand)
        );
        // 標準は変わらない
        assert_eq!(
            with_preset("standard", Converting, Hiragana, "Ctrl+p"),
            Some(Action::PrevCand)
        );
    }

    #[test]
    fn preset_function_keys_passes_ctrl_letters_to_app() {
        assert_eq!(
            with_preset("function_keys", Composing, Hiragana, "Ctrl+k"),
            Some(Action::CommitAndPassThrough)
        );
        for key in ["Ctrl+k", "Ctrl+n", "Ctrl+p"] {
            assert_eq!(
                with_preset("function_keys", Converting, Hiragana, key),
                Some(Action::CommitAndPassThrough),
                "{key}"
            );
        }
        // F キーは標準のまま
        assert_eq!(
            with_preset("function_keys", Composing, Hiragana, "F9"),
            Some(Action::ToZenkakuAlnum)
        );
    }

    #[test]
    fn preset_henkan_keys_has_builtin_mode_switching() {
        // 無変換 = 英数（全文脈・全モード。SetMode は利用者の上書きでは作れない専用の組み込み割当）
        for ctx in [Direct, Composing, Converting] {
            for mode in [Hiragana, Alphanumeric, InputMode::Katakana] {
                assert_eq!(
                    with_preset("henkan_keys", ctx, mode, "Muhenkan"),
                    Some(Action::SetMode(Alphanumeric)),
                    "{ctx:?} {mode:?}"
                );
            }
        }
        // 変換: 英数系ではひらがなモードへ。かな系では文脈どおり
        assert_eq!(
            with_preset("henkan_keys", Direct, Alphanumeric, "Henkan"),
            Some(Action::SetMode(Hiragana))
        );
        assert_eq!(
            with_preset("henkan_keys", Direct, Hiragana, "Henkan"),
            Some(Action::Reconvert)
        );
        assert_eq!(
            with_preset("henkan_keys", Composing, Hiragana, "Henkan"),
            Some(Action::StartConvert)
        );
        assert_eq!(
            with_preset("henkan_keys", Converting, Hiragana, "Henkan"),
            Some(Action::NextCand)
        );
        // 標準では入力中の変換キーは未割当
        assert_eq!(resolve(Composing, Hiragana, "Henkan"), None);
    }

    #[test]
    fn user_overrides_beat_preset_but_not_builtin_mode_keys() {
        let (km, w) = KeyMap::with_preset(
            "ctrl_letters",
            &[
                (Composing, "Ctrl+u".into(), "NoOp".into()),
                (Composing, "Eisu".into(), "NoOp".into()),
            ],
        );
        assert!(w.is_empty());
        let r = |key: &str| km.resolve(Composing, Hiragana, &parse_key(key).unwrap());
        assert_eq!(r("Ctrl+u"), Some(Action::NoOp));
        // モード切替キーは先に評価される（上書きできない）
        assert_eq!(r("Eisu"), Some(Action::SetMode(Alphanumeric)));
        // プリセットのままのキー
        assert_eq!(r("Ctrl+i"), Some(Action::ToKatakana));
    }

    #[test]
    fn preset_binding_on_tab_disables_suggest_navigation() {
        // プリセット・利用者のどちらで上書きしたキーも、サジェストの選択に読み替えない
        let (km, _) = KeyMap::with_preset("standard", &[(Composing, "Tab".into(), "NoOp".into())]);
        assert_eq!(km.suggest_action(&parse_key("Tab").unwrap()), None);
    }

    #[test]
    fn parse_action_new_names_and_alias() {
        for (name, action) in [
            ("ToHankakuKana", Action::ToHankakuKana),
            ("ToZenkakuAlnum", Action::ToZenkakuAlnum),
            ("ToHankakuAlnum", Action::ToHankakuAlnum),
            ("CycleForm", Action::CycleForm),
            ("NextPage", Action::NextPage),
            ("PrevPage", Action::PrevPage),
            ("ToggleLive", Action::ToggleLive),
            ("ConvertSelection", Action::ConvertSelection),
            ("CommitPartial", Action::CommitFocused),
            ("CommitFocused", Action::CommitFocused),
        ] {
            assert_eq!(parse_action(name), Ok(action), "{name}");
        }
        // 内部用・モード切替は書けない
        assert!(parse_action("CommitText").is_err());
        assert!(parse_action("SetMode:Hiragana").is_err());
    }

    #[test]
    fn action_spec_round_trips_through_parse_action() {
        for spec in [
            "StartConvert",
            "NextPage",
            "ToggleLive",
            "CycleForm",
            "MoveFocus:Left",
            "ResizeSegment:Right",
            "ExtendSelection:Left",
            "SelectCand:7",
            "ToHankakuAlnum",
        ] {
            assert_eq!(action_spec(&parse_action(spec).unwrap()), spec);
        }
        assert_eq!(
            action_spec(&Action::SetMode(InputMode::ZenkakuAlnum)),
            "SetMode:ZenkakuAlnum"
        );
        // CommitPartial は正規の名前 CommitFocused で出る
        assert_eq!(
            action_spec(&parse_action("CommitPartial").unwrap()),
            "CommitFocused"
        );
    }

    #[test]
    fn key_spec_format_round_trips() {
        for spec in [
            "Ctrl+Shift+Right",
            "Shift+Space",
            "F10",
            "Muhenkan",
            "Katakana",
            "Hiragana",
            "Ctrl+Alt+Delete",
            "Meta+j",
            "Shift+a",
            ";",
        ] {
            let parsed = KeySpec::parse(spec).unwrap();
            let text = parsed.format().unwrap();
            assert_eq!(KeySpec::parse(&text).unwrap(), parsed, "{spec} → {text}");
        }
        // 大文字・小文字の書き方の違いは同じキー
        assert_eq!(
            KeySpec::parse("Ctrl+Shift+J").unwrap(),
            KeySpec::parse("Ctrl+Shift+j").unwrap()
        );
        assert_eq!(
            KeySpec::parse("A").unwrap(),
            KeySpec::parse("Shift+a").unwrap()
        );
        assert_ne!(KeySpec::parse("a").unwrap(), KeySpec::parse("A").unwrap());
    }

    #[test]
    fn user_override_matches_event_with_uppercase_keysym() {
        // 実キーが大文字の keysym で届いても（Shift・Caps）、仕様の書き方の大小に関わらず一致する
        let (km, _) = KeyMap::with_overrides(&[(Composing, "Ctrl+Shift+k".into(), "NoOp".into())]);
        let mut ev = parse_key("Ctrl+Shift+k").unwrap();
        ev.keysym = 'K' as u32;
        assert_eq!(km.resolve(Composing, Hiragana, &ev), Some(Action::NoOp));
    }
}
