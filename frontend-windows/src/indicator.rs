//! タスクバー（入力インジケーター）・言語バーのアイコンの出し分け（OS 非依存）。
//!
//! 設定 `windows.indicator_icon`（`color` / `mono`）とシステムのテーマ（ライト/ダーク）から、
//! [`icon::render_status`] に渡す配色を決める。設定値の取得（コアの `get_settings`）・レジストリの読み取り・
//! `HICON` の生成は TSF 層（`frontend-windows-tsf/src/langbar.rs`）が行い、ここは判断だけを持つ
//! （decisions.md H-115）。

use std::path::{Path, PathBuf};
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
        InputMode::Hiragana => ModeGlyph::Hiragana,
        InputMode::Katakana => ModeGlyph::Katakana,
        InputMode::HankakuKatakana => ModeGlyph::HankakuKatakana,
        InputMode::ZenkakuAlnum => ModeGlyph::ZenkakuAlnum,
        InputMode::Alphanumeric | InputMode::HankakuAlnum => ModeGlyph::Alphanumeric,
    }
}

/// 直近に確定した値（エンジンが返した値）を覚えておくファイル名（`%LOCALAPPDATA%\Nagi\` の下。1 行）
pub const CACHE_FILE_NAME: &str = "indicator-style";

/// 直近の値のキャッシュファイルの場所（`<LOCALAPPDATA>\Nagi\indicator-style`）
pub fn cache_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Nagi").join(CACHE_FILE_NAME)
}

/// `config.toml` の場所。`NAGI_CONFIG_DIR` が最優先、無ければ `<APPDATA>\Nagi`（`nagi_config::config_dir` の
/// Windows 側と同じ規則。TSF の DLL に設定クレートを入れないため、ここで写している）
pub fn config_file_path(
    config_dir_override: Option<&str>,
    app_data: Option<&str>,
) -> Option<PathBuf> {
    let dir = match config_dir_override.filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(app_data.filter(|d| !d.is_empty())?).join("Nagi"),
    };
    Some(dir.join("config.toml"))
}

/// 設定値の文字列表記（キャッシュファイルへ書く値。[`IndicatorStyle::parse`] の逆）
pub fn style_text(style: IndicatorStyle) -> &'static str {
    match style {
        IndicatorStyle::Color => "color",
        IndicatorStyle::Mono => "mono",
    }
}

/// キャッシュファイルの中身（1 行）から読む。空・知らない値は `None`
pub fn parse_cache_text(text: &str) -> Option<IndicatorStyle> {
    IndicatorStyle::parse(text.trim())
}

/// `config.toml` の本文から `[windows] indicator_icon` だけを軽く読む（全体は解釈しない）。
/// ファイルが壊れていても、その行が読めれば値を採る。キーが無い・値が不明なら `None`。
/// 対応する書式: `[windows]` 表の中の `indicator_icon = "mono"`（`'mono'`・行末コメント可）と、
/// 表の外の `windows.indicator_icon = "mono"`
pub fn parse_config_text(text: &str) -> Option<IndicatorStyle> {
    let mut in_windows = false;
    let mut in_root = true;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let name = line
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim();
            in_root = false;
            in_windows = !line.starts_with("[[") && name == "windows";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let hit =
            (in_windows && key == "indicator_icon") || (in_root && key == "windows.indicator_icon");
        if hit {
            return parse_quoted(value.trim()).and_then(IndicatorStyle::parse);
        }
    }
    None
}

/// `"text"` または `'text'`（後ろのコメントは無視）の中身
fn parse_quoted(value: &str) -> Option<&str> {
    let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let rest = &value[1..];
    rest.split_once(quote).map(|(inner, _)| inner)
}

/// アイコンの配色の決め方（優先順位）: エンジンが返した値 > `config.toml` > 直近のキャッシュ > 既定（カラー）
pub fn resolve(
    engine: Option<IndicatorStyle>,
    config: Option<IndicatorStyle>,
    cached: Option<IndicatorStyle>,
) -> IndicatorStyle {
    engine.or(config).or(cached).unwrap_or_default()
}

/// [`StyleCache::record`] の結果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Recorded {
    /// 採用される値が変わった（アイコンを描き直す必要がある）
    pub changed: bool,
    /// キャッシュファイルへ書くべき値（エンジンの値がキャッシュと違うとき）
    pub persist: Option<IndicatorStyle>,
}

/// 設定値のキャッシュ。エンジン・`config.toml`・キャッシュファイルの 3 つの出どころを持ち、
/// [`resolve`] の優先順位で採用する。エンジンの取得は、失敗したときも最後に試した時刻を記録して間引く
/// （エンジンが落ちている間に、アイコンの更新のたびに待たないため）
#[derive(Debug, Clone, Copy, Default)]
pub struct StyleCache {
    engine: Option<IndicatorStyle>,
    config: Option<IndicatorStyle>,
    cached: Option<IndicatorStyle>,
    last_attempt: Option<Instant>,
}

impl StyleCache {
    /// 空のキャッシュ（まだ何も分からない。値はカラー）
    pub fn new() -> Self {
        Self::default()
    }

    /// 起動時の値で作る（`config.toml` とキャッシュファイルから読んだもの。読めなければ `None`）。
    /// エンジンの取得は未実施なので、最初の [`due`](Self::due) は真
    pub fn seeded(config: Option<IndicatorStyle>, cached: Option<IndicatorStyle>) -> Self {
        Self {
            config,
            cached,
            ..Self::default()
        }
    }

    /// いまの値
    pub fn style(&self) -> IndicatorStyle {
        resolve(self.engine, self.config, self.cached)
    }

    /// エンジンの取得を試みてよいか（一度も試していない、または前回から [`REFRESH_INTERVAL`] 以上たった）
    pub fn due(&self, now: Instant) -> bool {
        self.last_attempt
            .is_none_or(|last| now.saturating_duration_since(last) >= REFRESH_INTERVAL)
    }

    /// 取得を試みた結果を記録する。`engine` が `None`（取得に失敗）なら直前のエンジンの値を保つ。
    /// `config` は読み直せたときだけ `Some`（読めなかったときは直前の値を保つ）
    pub fn record(
        &mut self,
        now: Instant,
        engine: Option<IndicatorStyle>,
        config: Option<IndicatorStyle>,
    ) -> Recorded {
        let before = self.style();
        self.last_attempt = Some(now);
        if engine.is_some() {
            self.engine = engine;
        }
        if config.is_some() {
            self.config = config;
        }
        let persist = engine.filter(|style| self.cached != Some(*style));
        if persist.is_some() {
            self.cached = persist;
        }
        Recorded {
            changed: self.style() != before,
            persist,
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
        assert_eq!(
            glyph(Some(InputMode::HankakuAlnum)),
            ModeGlyph::Alphanumeric
        );
        assert_eq!(glyph(Some(InputMode::Katakana)), ModeGlyph::Katakana);
        assert_eq!(
            glyph(Some(InputMode::HankakuKatakana)),
            ModeGlyph::HankakuKatakana
        );
        assert_eq!(
            glyph(Some(InputMode::ZenkakuAlnum)),
            ModeGlyph::ZenkakuAlnum
        );
        assert_eq!(glyph(None), ModeGlyph::Hiragana);
    }

    #[test]
    fn cache_throttles_and_keeps_the_last_value_on_failure() {
        let t0 = Instant::now();
        let mut cache = StyleCache::new();
        assert!(cache.due(t0), "未取得はすぐ取る");
        let r = cache.record(t0, Some(IndicatorStyle::Mono), None);
        assert!(r.changed);
        assert_eq!(cache.style(), IndicatorStyle::Mono);
        assert!(!cache.due(t0 + Duration::from_secs(1)));
        assert!(cache.due(t0 + REFRESH_INTERVAL));
        // 失敗しても値は保ち、試した時刻は進む
        let t1 = t0 + Duration::from_secs(10);
        let r = cache.record(t1, None, None);
        assert!(!r.changed);
        assert_eq!(cache.style(), IndicatorStyle::Mono);
        assert!(!cache.due(t1 + Duration::from_secs(1)));
        cache.invalidate();
        assert!(cache.due(t1));
        cache.record(t1, Some(IndicatorStyle::Color), None);
        assert_eq!(cache.style(), IndicatorStyle::Color);
    }

    #[test]
    fn priority_is_engine_then_config_then_cache_then_default() {
        use IndicatorStyle::{Color, Mono};
        assert_eq!(resolve(None, None, None), Color);
        assert_eq!(resolve(None, None, Some(Mono)), Mono);
        assert_eq!(resolve(None, Some(Color), Some(Mono)), Color);
        assert_eq!(resolve(Some(Mono), Some(Color), Some(Color)), Mono);
        assert_eq!(resolve(Some(Color), Some(Mono), Some(Mono)), Color);
    }

    #[test]
    fn first_draw_uses_config_before_the_engine_answers() {
        use IndicatorStyle::{Color, Mono};
        let t0 = Instant::now();
        // config.toml が読めた: エンジンが未接続でも最初からモノクロ
        let mut cache = StyleCache::seeded(Some(Mono), Some(Color));
        assert_eq!(cache.style(), Mono);
        assert!(cache.due(t0));
        let r = cache.record(t0, None, Some(Mono));
        assert!(!r.changed && r.persist.is_none());
        // config.toml が読めない: 直近のキャッシュ
        assert_eq!(StyleCache::seeded(None, Some(Mono)).style(), Mono);
        // 何も無い: 既定
        assert_eq!(StyleCache::seeded(None, None).style(), Color);
        // エンジンの値が食い違えばエンジンが正。変わったことと、キャッシュへ書く値が返る
        let r = cache.record(t0, Some(Color), Some(Mono));
        assert!(r.changed);
        assert_eq!(r.persist, None, "キャッシュ（color）と同じなので書かない");
        assert_eq!(cache.style(), Color);
        let r = cache.record(t0, Some(Mono), None);
        assert!(r.changed);
        assert_eq!(r.persist, Some(Mono));
        // 同じ値が続けば何も起きない
        let r = cache.record(t0, Some(Mono), None);
        assert_eq!(r, Recorded::default());
    }

    #[test]
    fn config_text_reads_only_the_windows_indicator_icon() {
        use IndicatorStyle::{Color, Mono};
        let text = "# c\n[profile]\nindicator_icon = \"color\"\n\n[windows]\ncandidate_page_indicator = \"x\"\nindicator_icon = \"mono\"  # モノクロ\n";
        assert_eq!(parse_config_text(text), Some(Mono));
        assert_eq!(
            parse_config_text("[windows]\nindicator_icon = 'color'\n"),
            Some(Color)
        );
        assert_eq!(
            parse_config_text("windows.indicator_icon = \"mono\"\n"),
            Some(Mono)
        );
        // 別の表・キー無し・不明な値・壊れた行
        assert_eq!(
            parse_config_text("[profile]\nindicator_icon = \"mono\"\n"),
            None
        );
        assert_eq!(parse_config_text("[windows]\n"), None);
        assert_eq!(
            parse_config_text("[windows]\nindicator_icon = \"x\"\n"),
            None
        );
        assert_eq!(
            parse_config_text("[windows]\nindicator_icon = mono\n"),
            None
        );
        assert_eq!(parse_config_text("[windows\n@@@\n"), None);
        assert_eq!(parse_config_text(""), None);
        // 後ろの表が壊れていても前の行は読める
        assert_eq!(
            parse_config_text("[windows]\nindicator_icon = \"mono\"\n[x\n???"),
            Some(Mono)
        );
    }

    #[test]
    fn paths_and_cache_text() {
        assert_eq!(
            config_file_path(Some("C:\\cfg"), Some("C:\\app")),
            Some(PathBuf::from("C:\\cfg").join("config.toml"))
        );
        assert_eq!(
            config_file_path(None, Some("C:\\app")),
            Some(Path::new("C:\\app").join("Nagi").join("config.toml"))
        );
        assert_eq!(config_file_path(Some(""), None), None);
        assert_eq!(
            cache_path(Path::new("L")),
            Path::new("L").join("Nagi").join("indicator-style")
        );
        assert_eq!(parse_cache_text("mono\n"), Some(IndicatorStyle::Mono));
        assert_eq!(parse_cache_text(" color \r\n"), Some(IndicatorStyle::Color));
        assert_eq!(parse_cache_text("junk"), None);
        for s in [IndicatorStyle::Color, IndicatorStyle::Mono] {
            assert_eq!(parse_cache_text(style_text(s)), Some(s));
        }
    }
}
