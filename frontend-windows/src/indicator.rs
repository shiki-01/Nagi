//! タスクバー（入力インジケーター）・言語バーのアイコンの出し分け（OS 非依存）。
//!
//! 設定 `windows.indicator_icon`（`color` / `mono`）とシステムのテーマ（ライト/ダーク）から、
//! [`icon::render_status`] に渡す配色を決める。設定値の取得（コアの `get_settings`）・レジストリの読み取り・
//! `HICON` の生成は TSF 層（`frontend-windows-tsf/src/langbar.rs`）が行い、ここは判断だけを持つ
//! （decisions.md H-115）。

use std::time::{Duration, Instant};

use nagi_protocol::{InputMode, SettingItem, SettingValue};

use crate::icon::{ModeGlyph, Tone};

/// 設定項目のキー（`config.toml` の `[windows] indicator_icon`）
pub const SETTING_KEY: &str = "windows.indicator_icon";

/// 設定値を読み直す間隔。アイコンの更新（モード切替・フォーカス移動）のたびに `get_settings` を
/// 呼ばないための間引きで、設定を変えてから反映されるまでの上限でもある
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// タスクバーのアイコンの配色の設定
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndicatorStyle {
    /// Nagi のロゴの色（既定）
    #[default]
    Color,
    /// モノクロ（システムのテーマに合わせて白または黒）
    Mono,
}

impl IndicatorStyle {
    /// 設定値（`"color"` / `"mono"`）から。知らない値は `None`
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "color" => Some(IndicatorStyle::Color),
            "mono" => Some(IndicatorStyle::Mono),
            _ => None,
        }
    }

    /// `get_settings` の項目一覧から取り出す。項目が無い・値が不明なら既定（カラー）
    pub fn from_items(items: &[SettingItem]) -> Self {
        items
            .iter()
            .find(|item| item.key == SETTING_KEY)
            .and_then(|item| match &item.value {
                Some(SettingValue::Text(text)) => Self::parse(text),
                _ => None,
            })
            .unwrap_or_default()
    }
}

/// モノクロで描く色（RGB）。ライトテーマのタスクバーでは黒、ダークでは白。
/// テーマが読めないときはダーク扱い（白）にする。黒いタスクバーに黒い絵は消えるが、
/// 白いタスクバーに白い絵も消える。Windows 11 の入力インジケーターは既定でダークの利用者が多いため
pub fn mono_foreground(system_uses_light_theme: Option<bool>) -> [u8; 3] {
    if system_uses_light_theme == Some(true) {
        [0x00, 0x00, 0x00]
    } else {
        [0xFF, 0xFF, 0xFF]
    }
}

/// 設定とテーマから、アイコンの配色を決める
pub fn tone(style: IndicatorStyle, system_uses_light_theme: Option<bool>) -> Tone {
    match style {
        IndicatorStyle::Color => Tone::Color,
        IndicatorStyle::Mono => Tone::Mono(mono_foreground(system_uses_light_theme)),
    }
}

/// モードに対応する文字（接続前などモード不明のときは既定のひらがな。`status::button_text` と同じ）
pub fn glyph(mode: Option<InputMode>) -> ModeGlyph {
    match mode.unwrap_or(InputMode::Hiragana) {
        // カタカナ・半角カナはひらがな、全角英数・半角英数は英数の表示にする（拡張モードの表示は後続）
        InputMode::Hiragana | InputMode::Katakana | InputMode::HankakuKatakana => {
            ModeGlyph::Hiragana
        }
        InputMode::Alphanumeric | InputMode::ZenkakuAlnum | InputMode::HankakuAlnum => {
            ModeGlyph::Alphanumeric
        }
    }
}

/// 設定値のキャッシュ。取得できなかったときも最後に試した時刻を記録し、間引く
/// （エンジンが落ちている間に、アイコンの更新のたびに待たないため）
#[derive(Debug, Clone, Copy, Default)]
pub struct StyleCache {
    style: IndicatorStyle,
    last_attempt: Option<Instant>,
}

impl StyleCache {
    /// 空のキャッシュ（まだ取得していない。値はカラー）
    pub fn new() -> Self {
        Self::default()
    }

    /// いまの値
    pub fn style(&self) -> IndicatorStyle {
        self.style
    }

    /// 取得を試みてよいか（一度も試していない、または前回から [`REFRESH_INTERVAL`] 以上たった）
    pub fn due(&self, now: Instant) -> bool {
        self.last_attempt
            .is_none_or(|last| now.saturating_duration_since(last) >= REFRESH_INTERVAL)
    }

    /// 取得を試みた結果を記録する。`fetched` が `None`（取得に失敗）なら直前の値を保つ
    pub fn record(&mut self, now: Instant, fetched: Option<IndicatorStyle>) {
        self.last_attempt = Some(now);
        if let Some(style) = fetched {
            self.style = style;
        }
    }

    /// エンジンの再起動などで、次の機会にすぐ読み直させる
    pub fn invalidate(&mut self) {
        self.last_attempt = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::{SettingKind, SettingLevel, SettingPlatforms};

    fn item(key: &str, value: Option<SettingValue>) -> SettingItem {
        SettingItem {
            key: key.into(),
            group: "g".into(),
            label: "l".into(),
            description: "d".into(),
            kind: SettingKind::Bool,
            value,
            default: None,
            explicit: false,
            requires_restart: false,
            level: SettingLevel::Detail,
            platforms: SettingPlatforms::Desktop,
        }
    }

    #[test]
    fn parses_known_values_only() {
        assert_eq!(IndicatorStyle::parse("color"), Some(IndicatorStyle::Color));
        assert_eq!(IndicatorStyle::parse("mono"), Some(IndicatorStyle::Mono));
        assert_eq!(IndicatorStyle::parse("Mono"), None);
        assert_eq!(IndicatorStyle::parse(""), None);
        assert_eq!(IndicatorStyle::default(), IndicatorStyle::Color);
    }

    #[test]
    fn reads_the_style_from_settings_items() {
        let mono = item(SETTING_KEY, Some(SettingValue::Text("mono".into())));
        let other = item("update.auto_check", Some(SettingValue::Bool(true)));
        assert_eq!(
            IndicatorStyle::from_items(&[other.clone(), mono]),
            IndicatorStyle::Mono
        );
        // 項目が無い・値が型違い・不明な値は既定（カラー）
        assert_eq!(IndicatorStyle::from_items(&[other]), IndicatorStyle::Color);
        assert_eq!(
            IndicatorStyle::from_items(&[item(SETTING_KEY, Some(SettingValue::Bool(true)))]),
            IndicatorStyle::Color
        );
        assert_eq!(
            IndicatorStyle::from_items(&[item(SETTING_KEY, Some(SettingValue::Text("x".into())))]),
            IndicatorStyle::Color
        );
        assert_eq!(IndicatorStyle::from_items(&[]), IndicatorStyle::Color);
    }

    #[test]
    fn mono_follows_the_system_theme() {
        assert_eq!(mono_foreground(Some(true)), [0, 0, 0]);
        assert_eq!(mono_foreground(Some(false)), [0xFF, 0xFF, 0xFF]);
        assert_eq!(mono_foreground(None), [0xFF, 0xFF, 0xFF]);
        assert_eq!(tone(IndicatorStyle::Color, Some(true)), Tone::Color);
        assert_eq!(tone(IndicatorStyle::Color, Some(false)), Tone::Color);
        assert_eq!(
            tone(IndicatorStyle::Mono, Some(true)),
            Tone::Mono([0, 0, 0])
        );
        assert_eq!(
            tone(IndicatorStyle::Mono, Some(false)),
            Tone::Mono([0xFF, 0xFF, 0xFF])
        );
    }

    #[test]
    fn glyph_follows_the_mode_and_defaults_to_hiragana() {
        assert_eq!(glyph(Some(InputMode::Hiragana)), ModeGlyph::Hiragana);
        assert_eq!(
            glyph(Some(InputMode::Alphanumeric)),
            ModeGlyph::Alphanumeric
        );
        assert_eq!(glyph(None), ModeGlyph::Hiragana);
    }

    #[test]
    fn cache_throttles_and_keeps_the_last_value_on_failure() {
        let t0 = Instant::now();
        let mut cache = StyleCache::new();
        assert!(cache.due(t0), "未取得はすぐ取る");
        cache.record(t0, Some(IndicatorStyle::Mono));
        assert_eq!(cache.style(), IndicatorStyle::Mono);
        assert!(!cache.due(t0 + Duration::from_secs(1)));
        assert!(cache.due(t0 + REFRESH_INTERVAL));
        // 失敗しても値は保ち、試した時刻は進む
        let t1 = t0 + Duration::from_secs(10);
        cache.record(t1, None);
        assert_eq!(cache.style(), IndicatorStyle::Mono);
        assert!(!cache.due(t1 + Duration::from_secs(1)));
        cache.invalidate();
        assert!(cache.due(t1));
        cache.record(t1, Some(IndicatorStyle::Color));
        assert_eq!(cache.style(), IndicatorStyle::Color);
    }
}
