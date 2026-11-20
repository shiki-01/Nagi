//! 項目記述子（`get_settings`）→ 画面用 JSON の変換。
//!
//! 画面（JS）はここで作る JSON だけを描画する。項目の種類・範囲・選択肢・説明はすべてコアの記述子から来る。
//! iOS 専用の項目は出さない。表示レベル（シンプル / 詳細）の切り替えは画面側で `level` を見て行う。

use nagi_protocol::settings::{
    SettingItem, SettingKind, SettingLevel, SettingPlatforms, SettingValue, SettingsSnapshot,
};
use serde::Serialize;
use serde_json::{Value, json};

/// 画面全体
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Screen {
    /// 設定ファイル（`config.toml`）の場所
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    /// 設定ファイルが壊れている等で値を読めなかった理由
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// カテゴリ（コアが返した順）
    pub groups: Vec<Group>,
}

/// カテゴリ 1 つ
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Group {
    /// カテゴリ名（日本語）
    pub name: String,
    /// 項目
    pub items: Vec<Item>,
}

/// 項目 1 つ
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    /// 設定キー
    pub key: String,
    /// 表示名
    pub label: String,
    /// 説明
    pub description: String,
    /// 型（`{"type":"int","min":..,"max":..,"step":..}` など。JS が描画を決める）
    pub kind: Value,
    /// 現在値（`null` は未指定でプロファイルが決める）
    pub value: Value,
    /// 既定値
    pub default: Value,
    /// `config.toml` に明示指定があるか
    pub explicit: bool,
    /// 反映に再起動が要るか
    pub requires_restart: bool,
    /// `simple` / `detail`
    pub level: &'static str,
}

fn plain(value: &Option<SettingValue>) -> Value {
    match value {
        None => Value::Null,
        Some(SettingValue::Bool(b)) => json!(b),
        Some(SettingValue::Int(i)) => json!(i),
        Some(SettingValue::Float(f)) => json!(f),
        Some(SettingValue::Text(t)) => json!(t),
    }
}

fn kind(kind: &SettingKind) -> Value {
    match kind {
        SettingKind::Bool => json!({"type": "bool"}),
        SettingKind::Int(r) => json!({"type": "int", "min": r.min, "max": r.max, "step": r.step}),
        SettingKind::Float(r) => {
            json!({"type": "float", "min": r.min, "max": r.max, "step": r.step})
        }
        SettingKind::Choice(c) => json!({
            "type": "choice",
            "options": c.options.iter()
                .map(|o| json!({"value": o.value, "label": o.label}))
                .collect::<Vec<_>>(),
        }),
        SettingKind::Text => json!({"type": "text"}),
    }
}

fn item(i: &SettingItem) -> Item {
    Item {
        key: i.key.clone(),
        label: i.label.clone(),
        description: i.description.clone(),
        kind: kind(&i.kind),
        value: plain(&i.value),
        default: plain(&i.default),
        explicit: i.explicit,
        requires_restart: i.requires_restart,
        level: match i.level {
            SettingLevel::Simple => "simple",
            SettingLevel::Detail => "detail",
        },
    }
}

/// デスクトップに出す項目か（iOS 専用は出さない）
fn is_desktop(i: &SettingItem) -> bool {
    !matches!(i.platforms, SettingPlatforms::Ios)
}

/// 記述子から画面を作る。カテゴリはコアが返した順（同じ `group` が連続する前提だが、離れていても
/// 最初に現れた位置にまとめる）
pub fn build_screen(snapshot: &SettingsSnapshot, config_path: Option<String>) -> Screen {
    let mut groups: Vec<Group> = Vec::new();
    for i in snapshot.items.iter().filter(|i| is_desktop(i)) {
        let position = match groups.iter().position(|g| g.name == i.group) {
            Some(p) => p,
            None => {
                groups.push(Group {
                    name: i.group.clone(),
                    items: Vec::new(),
                });
                groups.len() - 1
            }
        };
        groups[position].items.push(item(i));
    }
    Screen {
        config_path,
        error: snapshot.error.clone(),
        groups,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::settings::{ChoiceOptions, FloatRange, IntRange, SettingChoice};

    fn base(key: &str, group: &str, kind: SettingKind) -> SettingItem {
        SettingItem {
            key: key.into(),
            group: group.into(),
            label: format!("{key}の名前"),
            description: "説明".into(),
            kind,
            value: None,
            default: None,
            explicit: false,
            requires_restart: false,
            level: SettingLevel::Simple,
            platforms: SettingPlatforms::Both,
        }
    }

    fn snapshot(items: Vec<SettingItem>) -> SettingsSnapshot {
        SettingsSnapshot { items, error: None }
    }

    #[test]
    fn groups_keep_core_order_and_drop_ios_only_items() {
        let mut ios = base("input.glide", "入力", SettingKind::Bool);
        ios.platforms = SettingPlatforms::Ios;
        let mut desktop = base("a.desktop", "入力", SettingKind::Bool);
        desktop.platforms = SettingPlatforms::Desktop;
        let only_ios_group = {
            let mut i = base("t.x", "タッチ", SettingKind::Bool);
            i.platforms = SettingPlatforms::Ios;
            i
        };
        let screen = build_screen(
            &snapshot(vec![
                base("c.one", "変換", SettingKind::Bool),
                ios,
                desktop,
                only_ios_group,
                base("c.two", "変換", SettingKind::Text),
                base("z.last", "学習", SettingKind::Bool),
            ]),
            Some("C:/n/config.toml".into()),
        );
        let names: Vec<_> = screen.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["変換", "入力", "学習"]);
        let keys = |g: &Group| g.items.iter().map(|i| i.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&screen.groups[0]), ["c.one", "c.two"]);
        assert_eq!(keys(&screen.groups[1]), ["a.desktop"]);
        assert_eq!(screen.config_path.as_deref(), Some("C:/n/config.toml"));
    }

    #[test]
    fn kinds_values_level_and_restart_are_carried() {
        let mut int = base(
            "conversion.delay",
            "変換",
            SettingKind::Int(IntRange {
                min: 0,
                max: 5000,
                step: 10,
            }),
        );
        int.value = Some(SettingValue::Int(228));
        int.default = Some(SettingValue::Int(228));
        int.level = SettingLevel::Detail;
        int.requires_restart = true;
        let mut float = base(
            "f",
            "変換",
            SettingKind::Float(FloatRange {
                min: 0.0,
                max: 1.0,
                step: 0.05,
            }),
        );
        float.value = Some(SettingValue::Float(0.5));
        float.explicit = true;
        let mut choice = base(
            "profile.mode",
            "プロファイル",
            SettingKind::Choice(ChoiceOptions {
                options: vec![SettingChoice {
                    value: "auto".into(),
                    label: "自動".into(),
                }],
            }),
        );
        choice.value = Some(SettingValue::Text("auto".into()));
        let unset = base("conversion.beam_width", "変換", SettingKind::Bool);
        let screen = build_screen(&snapshot(vec![int, float, choice, unset]), None);
        let v = serde_json::to_value(&screen).unwrap();
        let first = &v["groups"][0]["items"][0];
        assert_eq!(
            first["kind"],
            json!({"type":"int","min":0,"max":5000,"step":10})
        );
        assert_eq!(first["value"], 228);
        assert_eq!(first["level"], "detail");
        assert_eq!(first["requires_restart"], true);
        let second = &v["groups"][0]["items"][1];
        assert_eq!(second["kind"]["type"], "float");
        assert_eq!(second["value"], 0.5);
        assert_eq!(second["explicit"], true);
        assert_eq!(v["groups"][0]["items"][2]["value"], Value::Null);
        let choice = &v["groups"][1]["items"][0];
        assert_eq!(
            choice["kind"]["options"][0],
            json!({"value":"auto","label":"自動"})
        );
        assert_eq!(choice["value"], "auto");
        assert!(v.get("config_path").is_none());
    }

    #[test]
    fn snapshot_error_is_passed_through() {
        let s = SettingsSnapshot {
            items: vec![],
            error: Some("config.toml の 3 行目が壊れています".into()),
        };
        let screen = build_screen(&s, None);
        assert_eq!(
            screen.error.as_deref(),
            Some("config.toml の 3 行目が壊れています")
        );
        assert!(screen.groups.is_empty());
    }
}
