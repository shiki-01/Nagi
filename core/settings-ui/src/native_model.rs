//! ネイティブ設定ウィンドウの「画面モデル」（OS 非依存）。
//!
//! Win32 層（`native/`）はこのモデルだけを見る。ここに置くもの:
//! - コアの記述子（`get_settings`）から作るカテゴリ・項目の一覧（レベルでの絞り込み、iOS 専用項目の除外）
//! - 入力文字列 → 型つきの値への変換と検証（範囲・全角数字・空欄）、値の表示整形
//! - 更新確認の結果の文言
//! - ウィンドウメッセージ・コントロール ID の番号（衝突しないことをテストで保証する）
//!
//! 設定値の最終的な検証と `config.toml` への書き込みはコア側が行う。ここでの検証は、明らかに不正な
//! 入力（非数・範囲外）をコアに送る前に利用者へ伝えるためのもの。

use crate::backend::{UpdateCheck, UpdateStatus};
use crate::view::desktop_groups;
use nagi_protocol::settings::{
    SettingItem, SettingKind, SettingLevel, SettingValue, SettingsSnapshot,
};

/// 再起動が必要な項目のラベルに付ける注記
pub const RESTART_BADGE: &str = "（再起動後に反映）";

/// 項目 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct FieldView {
    /// 設定キー
    pub key: String,
    /// 表示名
    pub label: String,
    /// 説明
    pub description: String,
    /// 型
    pub kind: SettingKind,
    /// 現在値（`None` は未指定でプロファイルが決める）
    pub value: Option<SettingValue>,
    /// 既定値
    pub default: Option<SettingValue>,
    /// `config.toml` に明示指定があるか（`true` のとき「既定に戻す」を出す）
    pub explicit: bool,
    /// 反映に再起動が要るか
    pub requires_restart: bool,
    /// 表示レベル
    pub level: SettingLevel,
}

/// 選択肢 1 つ（コンボボックスの 1 行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceEntry {
    /// 設定値。`None` は「（自動）」（明示指定なし）
    pub value: Option<String>,
    /// 表示名
    pub label: String,
}

impl FieldView {
    fn from_item(i: &SettingItem) -> Self {
        Self {
            key: i.key.clone(),
            label: i.label.clone(),
            description: i.description.clone(),
            kind: i.kind.clone(),
            value: i.value.clone(),
            default: i.default.clone(),
            explicit: i.explicit,
            requires_restart: i.requires_restart,
            level: i.level,
        }
    }

    /// 画面に出すラベル（再起動が要る項目は注記つき）
    pub fn title(&self) -> String {
        if self.requires_restart {
            format!("{}{RESTART_BADGE}", self.label)
        } else {
            self.label.clone()
        }
    }

    /// 真偽値の現在値
    pub fn bool_value(&self) -> bool {
        matches!(self.value, Some(SettingValue::Bool(true)))
    }

    /// 入力欄に入れる文字列（未指定は空）
    pub fn display_text(&self) -> String {
        format_value(self.value.as_ref())
    }

    /// 入力欄の横に出す範囲の案内（数値項目のみ）
    pub fn range_hint(&self) -> Option<String> {
        let (min, max) = match &self.kind {
            SettingKind::Int(r) => (r.min.to_string(), r.max.to_string()),
            SettingKind::Float(r) => (format_float(r.min), format_float(r.max)),
            _ => return None,
        };
        let mut text = format!("範囲 {min} から {max}");
        if let Some(default) = &self.default {
            text.push_str(&format!("（既定 {}）", format_value(Some(default))));
        }
        Some(text)
    }

    /// 数値の刻み（スピナーなどの UI 用。入力の検証には使わない）
    pub fn step(&self) -> Option<f64> {
        match &self.kind {
            SettingKind::Int(r) => Some(f64::from(r.step)),
            SettingKind::Float(r) => Some(r.step),
            _ => None,
        }
    }

    /// コンボボックスの行（現在値が未指定のときだけ先頭に「（自動）」を足す）
    pub fn choice_entries(&self) -> Vec<ChoiceEntry> {
        let SettingKind::Choice(c) = &self.kind else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(c.options.len() + 1);
        if self.value.is_none() {
            out.push(ChoiceEntry {
                value: None,
                label: "（自動）".to_owned(),
            });
        }
        out.extend(c.options.iter().map(|o| ChoiceEntry {
            value: Some(o.value.clone()),
            label: o.label.clone(),
        }));
        out
    }

    /// 現在値に対応する `choice_entries` の添字（見つからなければ `None`）
    pub fn choice_index(&self) -> Option<usize> {
        let current = match &self.value {
            None => None,
            Some(SettingValue::Text(t)) => Some(t.as_str()),
            Some(_) => return None,
        };
        self.choice_entries()
            .iter()
            .position(|e| e.value.as_deref() == current)
    }

    /// `choice_entries` の `index` 番目を選んだときの値（「（自動）」は `None` = 既定に戻す）
    pub fn choice_value_at(&self, index: usize) -> Option<Option<SettingValue>> {
        self.choice_entries()
            .get(index)
            .map(|e| e.value.clone().map(SettingValue::Text))
    }

    /// 入力欄の文字列を型つきの値にする。`Ok(None)` は空欄（明示指定を消して既定に戻す）。
    /// `Bool` / `Choice` は文字列では入力しないので `Err`
    pub fn parse_input(&self, text: &str) -> Result<Option<SettingValue>, String> {
        match &self.kind {
            SettingKind::Int(r) => {
                let normalized = normalize_number(text);
                if normalized.is_empty() {
                    return Ok(None);
                }
                let n: i64 = normalized
                    .parse()
                    .map_err(|_| "整数で入力してください".to_owned())?;
                if n < i64::from(r.min) || n > i64::from(r.max) {
                    return Err(format!("{} から {} の範囲で入力してください", r.min, r.max));
                }
                // 範囲内なので i32 に収まる
                i32::try_from(n)
                    .map(|v| Some(SettingValue::Int(v)))
                    .map_err(|_| "整数で入力してください".to_owned())
            }
            SettingKind::Float(r) => {
                let normalized = normalize_number(text);
                if normalized.is_empty() {
                    return Ok(None);
                }
                let n: f64 = normalized
                    .parse()
                    .map_err(|_| "数値で入力してください".to_owned())?;
                if !n.is_finite() {
                    return Err("数値で入力してください".to_owned());
                }
                if n < r.min || n > r.max {
                    return Err(format!(
                        "{} から {} の範囲で入力してください",
                        format_float(r.min),
                        format_float(r.max)
                    ));
                }
                Ok(Some(SettingValue::Float(n)))
            }
            SettingKind::Text => {
                if text.trim().is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(SettingValue::Text(text.to_owned())))
                }
            }
            SettingKind::Bool | SettingKind::Choice(_) => {
                Err("この項目は文字列では入力できません".to_owned())
            }
        }
    }
}

/// 全角の数字・符号・小数点・空白を半角にし、前後の空白を除く（数値の入力欄用）
pub fn normalize_number(text: &str) -> String {
    text.trim_matches(|c: char| c.is_whitespace())
        .chars()
        .map(|c| match c {
            '０'..='９' => {
                char::from_u32(u32::from(c) - u32::from('０') + u32::from('0')).unwrap_or(c)
            }
            '－' | '−' | 'ー' => '-',
            '＋' => '+',
            '．' | '。' => '.',
            other => other,
        })
        .collect()
}

/// 小数を表示用の文字列にする（`1.0` は `1`、`0.05` は `0.05`）
pub fn format_float(value: f64) -> String {
    format!("{value}")
}

/// 値を入力欄に入れる文字列にする（`None` は空）
pub fn format_value(value: Option<&SettingValue>) -> String {
    match value {
        None => String::new(),
        Some(SettingValue::Bool(b)) => b.to_string(),
        Some(SettingValue::Int(i)) => i.to_string(),
        Some(SettingValue::Float(f)) => format_float(*f),
        Some(SettingValue::Text(t)) => t.clone(),
    }
}

/// カテゴリ 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryView {
    /// カテゴリ名（日本語）
    pub name: String,
    /// 項目（コアが返した順）
    pub fields: Vec<FieldView>,
}

impl CategoryView {
    /// レベルで絞り込んだ項目の添字（`detail` なら全項目）
    pub fn visible_fields(&self, detail: bool) -> Vec<usize> {
        self.fields
            .iter()
            .enumerate()
            .filter(|(_, f)| detail || f.level == SettingLevel::Simple)
            .map(|(i, _)| i)
            .collect()
    }
}

/// 設定画面全体のモデル
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsView {
    /// カテゴリ（コアが返した順。iOS 専用項目は含まない）
    pub categories: Vec<CategoryView>,
    /// `config.toml` の場所
    pub config_path: Option<String>,
    /// 設定ファイルが壊れている等で値を読めなかった理由
    pub error: Option<String>,
}

impl SettingsView {
    /// 記述子から作る（カテゴリの順序・iOS 専用項目の除外は Web 画面と同じ `view::desktop_groups`）
    pub fn from_snapshot(snapshot: &SettingsSnapshot, config_path: Option<String>) -> Self {
        Self {
            categories: desktop_groups(snapshot)
                .into_iter()
                .map(|(name, items)| CategoryView {
                    name,
                    fields: items.into_iter().map(FieldView::from_item).collect(),
                })
                .collect(),
            config_path,
            error: snapshot.error.clone(),
        }
    }

    /// キーから項目を探す（カテゴリの添字, 項目の添字, 項目）
    pub fn find(&self, key: &str) -> Option<(usize, usize, &FieldView)> {
        self.categories.iter().enumerate().find_map(|(ci, c)| {
            c.fields
                .iter()
                .enumerate()
                .find(|(_, f)| f.key == key)
                .map(|(fi, f)| (ci, fi, f))
        })
    }

    /// レベルで絞り込んだ結果が空でないカテゴリの添字
    pub fn visible_categories(&self, detail: bool) -> Vec<usize> {
        self.categories
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.visible_fields(detail).is_empty())
            .map(|(i, _)| i)
            .collect()
    }
}

/// 左の一覧に出すページ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 設定項目のカテゴリ（`SettingsView::categories` の添字）
    Category(usize),
    /// 更新
    Update,
    /// データ・ファイル
    Data,
    /// 登録式の追加ページ（`native/pages.rs` の表に 1 行足すだけで増やせる。値は表の中での識別子）
    Extra(&'static str),
    /// 読み込み中・失敗の表示（読み込めるまでの「設定」）
    Status,
}

/// 左の一覧: 絞り込み後のカテゴリ + 登録済みの固定ページ（`fixed`。「更新」「データ・ファイル」など。
/// 表示順は渡した順）。まだ読み込めていなければ（読み込み中・失敗）カテゴリの代わりに
/// 「設定」（状態の表示）を先頭に置く
pub fn sidebar(
    view: Option<&SettingsView>,
    detail: bool,
    fixed: &[(Page, &str)],
) -> Vec<(Page, String)> {
    let mut out = Vec::new();
    match view {
        Some(view) => {
            for ci in view.visible_categories(detail) {
                out.push((Page::Category(ci), view.categories[ci].name.clone()));
            }
        }
        None => out.push((Page::Status, "設定".to_owned())),
    }
    out.extend(fixed.iter().map(|(p, t)| (*p, (*t).to_owned())));
    out
}

/// 更新確認の結果の画面表示
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOutcome {
    /// 結果の文言
    pub text: String,
    /// 「更新を適用…」を出すか（取得済みのときだけ）
    pub can_apply: bool,
    /// 「リリースノートを開く」の URL（最新・失敗のときは `None`）
    pub notes_url: Option<String>,
}

/// 更新確認の結果を文言にする（Web 画面と同じ文言）。`notes_url` はここでは絞らない
/// （ワーカーが `is_safe_notes_url` を通してから渡す）
pub fn describe_update(check: &UpdateCheck) -> UpdateOutcome {
    let latest = check.latest.as_deref().unwrap_or("");
    let (text, can_apply) = match check.status {
        UpdateStatus::UpToDate => (format!("最新です（{}）。", check.current), false),
        UpdateStatus::Available => (format!("新しい版 {latest} があります。"), false),
        UpdateStatus::Staged => (
            format!(
                "新しい版 {latest} を取得しました。「更新を適用…」で入れ替えられます（管理者の確認が出ます）。"
            ),
            true,
        ),
        UpdateStatus::Error => (
            format!(
                "更新を確認できませんでした: {}",
                check.error.as_deref().unwrap_or("原因不明のエラーです")
            ),
            false,
        ),
    };
    let show_notes = matches!(check.status, UpdateStatus::Available | UpdateStatus::Staged);
    UpdateOutcome {
        text,
        can_apply,
        notes_url: check.notes_url.clone().filter(|_| show_notes),
    }
}

/// 設定カテゴリのページの、見出しの下の説明（表示している項目数。`detail` は詳細設定を含むか）
pub fn category_subtitle(visible: usize, detail: bool) -> String {
    if detail {
        format!("{visible} 件の設定項目（詳細設定を含む）")
    } else {
        format!("{visible} 件の設定項目")
    }
}

/// 更新ページの結果の見出し（状態のバッジ + 短い見出し）。詳しい文言は `text` をそのまま別に出す
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateHeadline {
    /// 色分け
    pub badge: crate::native_diag::BadgeKind,
    /// バッジの文字（色に頼らず文字でも示す）
    pub label: &'static str,
    /// 見出し
    pub title: &'static str,
}

/// 更新ページの状態（確認中・結果の文言）から、カードに出す見出しを決める。何も無ければ `None`。
/// 文言は [`describe_update`] などが作るもので、ここでは先頭の言い回しで種類を判断する
pub fn update_headline(busy: bool, text: &str) -> Option<UpdateHeadline> {
    use crate::native_diag::BadgeKind::{Info, Neutral, Ng, Ok, Warn};
    let (badge, label, title) = if busy {
        (Info, "確認中", "更新を確認しています")
    } else if text.is_empty() {
        return None;
    } else if text.starts_with("最新です") {
        (Ok, "最新", "最新の版です")
    } else if text.starts_with("新しい版") && text.contains("取得しました") {
        (Warn, "適用待ち", "新しい版を適用できます")
    } else if text.starts_with("新しい版") {
        (Info, "更新あり", "新しい版があります")
    } else if text.starts_with("更新の適用を開始") {
        (Info, "適用中", "更新を適用しています")
    } else if text.contains("できませんでした") {
        (Ng, "失敗", "更新に失敗しました")
    } else {
        (Neutral, "お知らせ", "更新の状態")
    };
    Some(UpdateHeadline {
        badge,
        label,
        title,
    })
}

/// ウィンドウメッセージの番号（`WM_APP` = 0x8000 以降）。Win32 層が使う
pub mod messages {
    /// `WM_APP`
    pub const APP_BASE: u32 = 0x8000;
    /// ワーカースレッドから UI スレッドへ: 返信がキューにある
    pub const WORKER_REPLY: u32 = APP_BASE + 1;
    /// 起動直後に 1 回: `--auto-check` の更新確認を始める
    pub const AUTO_CHECK: u32 = APP_BASE + 2;
    /// ページの作り直し（コントロールの通知の中で自分自身を壊さないよう、いったんキューを経由する）
    pub const REBUILD: u32 = APP_BASE + 3;
    /// すべて
    pub const ALL: [u32; 3] = [WORKER_REPLY, AUTO_CHECK, REBUILD];
}

/// コントロールの ID と、タイマーの ID
pub mod ids {
    /// 左のカテゴリ一覧
    pub const LIST: i32 = 100;
    /// 「詳細設定を表示」
    pub const DETAIL_CHECK: i32 = 101;
    /// ステータス行
    pub const STATUS: i32 = 102;
    /// 右のスクロールパネル
    pub const PANEL: i32 = 103;
    /// 「閉じる」
    pub const CLOSE: i32 = 104;
    /// サイドバーとパネルの間の縦線
    pub const SIDEBAR_RULE: i32 = 105;
    /// フッターの上の横線
    pub const FOOTER_RULE: i32 = 106;
    /// サイドバー上部のブランド領域（ロゴ + ワードマーク。オーナードロー）
    pub const BRAND: i32 = 107;
    /// 「更新を確認」
    pub const UPDATE_CHECK: i32 = 200;
    /// 「更新を適用…」
    pub const UPDATE_APPLY: i32 = 201;
    /// 「リリースノートを開く」
    pub const UPDATE_NOTES: i32 = 202;
    /// 「config.toml を開く」
    pub const OPEN_CONFIG: i32 = 210;
    /// 「設定フォルダを開く」
    pub const OPEN_CONFIG_DIR: i32 = 211;
    /// 「ユーザー辞書フォルダを開く」
    pub const OPEN_USER_DICT: i32 = 212;
    /// 「学習データを全削除…」
    pub const CLEAR_LEARNING: i32 = 213;
    /// 「再試行」
    pub const RETRY: i32 = 220;
    /// 動作確認: 「すべて確認」
    pub const DIAG_RUN_ALL: i32 = 300;
    /// 動作確認: 「中止」
    pub const DIAG_CANCEL: i32 = 301;
    /// 動作確認: 「レポートをコピー」
    pub const DIAG_COPY: i32 = 302;
    /// 動作確認: 「レポートを保存」
    pub const DIAG_SAVE: i32 = 303;
    /// 動作確認: 項目ごとの「この項目だけ確認」の ID の起点（以降は `ItemId::ALL` の順に連番）
    pub const DIAG_ITEM_BASE: i32 = 320;
    /// 設定項目のコントロールの ID の起点（以降は連番）
    pub const DYNAMIC_BASE: i32 = 1000;
    /// ステータス行を消すタイマー
    pub const TIMER_STATUS: usize = 1;
    /// ホイールのスクロールの補間（約 16ms ごと）
    pub const TIMER_SCROLL: usize = 2;

    /// 固定の ID すべて
    pub const FIXED: [i32; 20] = [
        LIST,
        DETAIL_CHECK,
        STATUS,
        PANEL,
        CLOSE,
        SIDEBAR_RULE,
        FOOTER_RULE,
        BRAND,
        UPDATE_CHECK,
        UPDATE_APPLY,
        UPDATE_NOTES,
        OPEN_CONFIG,
        OPEN_CONFIG_DIR,
        OPEN_USER_DICT,
        CLEAR_LEARNING,
        RETRY,
        DIAG_RUN_ALL,
        DIAG_CANCEL,
        DIAG_COPY,
        DIAG_SAVE,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::settings::{
        ChoiceOptions, FloatRange, IntRange, SettingChoice, SettingPlatforms,
    };

    #[test]
    fn category_subtitle_counts_items() {
        assert_eq!(category_subtitle(5, false), "5 件の設定項目");
        assert!(category_subtitle(9, true).contains("詳細設定"));
    }

    #[test]
    fn update_headline_follows_the_outcome_text() {
        use crate::native_diag::BadgeKind;
        let h = |busy, text: &str| update_headline(busy, text);
        assert_eq!(h(false, ""), None);
        assert_eq!(h(true, "").map(|x| x.badge), Some(BadgeKind::Info));
        assert_eq!(
            h(false, "最新です（0.1.19）。").map(|x| x.badge),
            Some(BadgeKind::Ok)
        );
        assert_eq!(
            h(false, "新しい版 0.2.0 があります。").map(|x| x.label),
            Some("更新あり")
        );
        assert_eq!(
            h(
                false,
                "新しい版 0.2.0 を取得しました。「更新を適用…」で入れ替えられます。"
            )
            .map(|x| x.badge),
            Some(BadgeKind::Warn)
        );
        assert_eq!(
            h(false, "更新を確認できませんでした: x").map(|x| x.badge),
            Some(BadgeKind::Ng)
        );
        assert_eq!(
            h(false, "更新を適用できませんでした: x").map(|x| x.badge),
            Some(BadgeKind::Ng)
        );
        assert_eq!(
            h(false, "更新の適用を開始しました。").map(|x| x.label),
            Some("適用中")
        );
        // 確認中は結果の文言より優先
        assert_eq!(h(true, "最新です").map(|x| x.label), Some("確認中"));
        // 文字（ラベル）は必ずある。色だけに頼らない
        assert!(h(false, "??").is_some_and(|x| !x.label.is_empty()));
    }

    fn item(key: &str, group: &str, kind: SettingKind) -> SettingItem {
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

    fn view(items: Vec<SettingItem>) -> SettingsView {
        SettingsView::from_snapshot(&SettingsSnapshot { items, error: None }, None)
    }

    fn int_field(min: i32, max: i32, step: i32) -> FieldView {
        let mut i = item("a.b", "g", SettingKind::Int(IntRange { min, max, step }));
        i.value = Some(SettingValue::Int(10));
        i.default = Some(SettingValue::Int(10));
        FieldView::from_item(&i)
    }

    fn float_field() -> FieldView {
        FieldView::from_item(&item(
            "f",
            "g",
            SettingKind::Float(FloatRange {
                min: 0.0,
                max: 1.0,
                step: 0.05,
            }),
        ))
    }

    fn choice_field(value: Option<&str>) -> FieldView {
        let mut i = item(
            "c",
            "g",
            SettingKind::Choice(ChoiceOptions {
                options: vec![
                    SettingChoice {
                        value: "auto".into(),
                        label: "自動判定".into(),
                    },
                    SettingChoice {
                        value: "light".into(),
                        label: "軽量".into(),
                    },
                ],
            }),
        );
        i.value = value.map(|v| SettingValue::Text(v.into()));
        FieldView::from_item(&i)
    }

    #[test]
    fn categories_keep_core_order_and_drop_ios_only() {
        let mut ios = item("t.x", "タッチ", SettingKind::Bool);
        ios.platforms = SettingPlatforms::Ios;
        let mut ios_in_group = item("c.ios", "変換", SettingKind::Bool);
        ios_in_group.platforms = SettingPlatforms::Ios;
        let v = view(vec![
            item("c.one", "変換", SettingKind::Bool),
            ios,
            ios_in_group,
            item("i.one", "入力", SettingKind::Text),
            item("c.two", "変換", SettingKind::Bool),
        ]);
        let names: Vec<_> = v.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["変換", "入力"]);
        let keys: Vec<_> = v.categories[0]
            .fields
            .iter()
            .map(|f| f.key.as_str())
            .collect();
        assert_eq!(keys, ["c.one", "c.two"]);
        assert_eq!(v.find("i.one").map(|(c, f, _)| (c, f)), Some((1, 0)));
        assert!(v.find("t.x").is_none());
    }

    #[test]
    fn level_filter_hides_detail_items_and_empty_categories() {
        let mut detail = item("d.one", "詳細だけ", SettingKind::Bool);
        detail.level = SettingLevel::Detail;
        let mut detail2 = item("c.detail", "変換", SettingKind::Bool);
        detail2.level = SettingLevel::Detail;
        let v = view(vec![
            item("c.simple", "変換", SettingKind::Bool),
            detail2,
            detail,
        ]);
        assert_eq!(v.visible_categories(false), [0]);
        assert_eq!(v.visible_categories(true), [0, 1]);
        assert_eq!(v.categories[0].visible_fields(false), [0]);
        assert_eq!(v.categories[0].visible_fields(true), [0, 1]);
        let fixed = [
            (Page::Update, "更新"),
            (Page::Data, "データ・ファイル"),
            (Page::Extra("diag"), "動作確認"),
        ];
        let side = sidebar(Some(&v), false, &fixed);
        let titles: Vec<_> = side.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(titles, ["変換", "更新", "データ・ファイル", "動作確認"]);
        assert_eq!(side[0].0, Page::Category(0));
        let side = sidebar(Some(&v), true, &fixed);
        assert_eq!(side.len(), 5);
        // 未読み込みでも更新・データのページは出る（先頭は状態の表示）
        let side = sidebar(None, true, &fixed);
        assert_eq!(
            side.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
            [Page::Status, Page::Update, Page::Data, Page::Extra("diag")]
        );
    }

    #[test]
    fn restart_badge_and_explicit_flags_are_carried() {
        let mut i = item("u.auto", "更新", SettingKind::Bool);
        i.requires_restart = true;
        i.explicit = true;
        i.value = Some(SettingValue::Bool(true));
        let f = FieldView::from_item(&i);
        assert_eq!(f.title(), "u.autoの名前（再起動後に反映）");
        assert!(f.explicit);
        assert!(f.bool_value());
        let plain = FieldView::from_item(&item("p", "g", SettingKind::Bool));
        assert_eq!(plain.title(), "pの名前");
        assert!(!plain.bool_value());
    }

    #[test]
    fn int_parse_accepts_fullwidth_and_rejects_bad_input() {
        let f = int_field(0, 5000, 10);
        assert_eq!(f.parse_input("228"), Ok(Some(SettingValue::Int(228))));
        assert_eq!(f.parse_input(" ２２８ "), Ok(Some(SettingValue::Int(228))));
        assert_eq!(f.parse_input("　５　"), Ok(Some(SettingValue::Int(5))));
        // step の倍数でなくても通す（step はスピナーの刻みで、検証はしない）
        assert_eq!(f.parse_input("7"), Ok(Some(SettingValue::Int(7))));
        assert_eq!(f.parse_input(""), Ok(None));
        assert_eq!(f.parse_input("   "), Ok(None));
        assert_eq!(f.parse_input("abc"), Err("整数で入力してください".into()));
        assert_eq!(f.parse_input("1.5"), Err("整数で入力してください".into()));
        assert_eq!(f.parse_input("1,000"), Err("整数で入力してください".into()));
        assert_eq!(
            f.parse_input("99999"),
            Err("0 から 5000 の範囲で入力してください".into())
        );
        assert_eq!(
            f.parse_input("-1"),
            Err("0 から 5000 の範囲で入力してください".into())
        );
        assert_eq!(
            f.parse_input("99999999999999999999"),
            Err("整数で入力してください".into())
        );
        let neg = int_field(-10, 10, 1);
        assert_eq!(neg.parse_input("－３"), Ok(Some(SettingValue::Int(-3))));
        assert_eq!(neg.parse_input("+4"), Ok(Some(SettingValue::Int(4))));
    }

    #[test]
    fn float_parse_handles_decimals_range_and_non_finite() {
        let f = float_field();
        assert_eq!(f.parse_input("0.25"), Ok(Some(SettingValue::Float(0.25))));
        assert_eq!(f.parse_input("０．５"), Ok(Some(SettingValue::Float(0.5))));
        assert_eq!(f.parse_input("1"), Ok(Some(SettingValue::Float(1.0))));
        assert_eq!(f.parse_input(""), Ok(None));
        assert_eq!(f.parse_input("x"), Err("数値で入力してください".into()));
        assert_eq!(f.parse_input("NaN"), Err("数値で入力してください".into()));
        assert_eq!(f.parse_input("inf"), Err("数値で入力してください".into()));
        assert_eq!(
            f.parse_input("1.5"),
            Err("0 から 1 の範囲で入力してください".into())
        );
        assert_eq!(
            f.parse_input("-0.1"),
            Err("0 から 1 の範囲で入力してください".into())
        );
    }

    #[test]
    fn text_choice_and_bool_input() {
        let t = FieldView::from_item(&item("t", "g", SettingKind::Text));
        assert_eq!(
            t.parse_input("abc"),
            Ok(Some(SettingValue::Text("abc".into())))
        );
        assert_eq!(t.parse_input("  "), Ok(None));
        assert!(choice_field(None).parse_input("x").is_err());
        assert!(
            FieldView::from_item(&item("b", "g", SettingKind::Bool))
                .parse_input("x")
                .is_err()
        );
    }

    #[test]
    fn choice_entries_add_auto_only_when_unset() {
        let set = choice_field(Some("light"));
        let entries = set.choice_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(set.choice_index(), Some(1));
        assert_eq!(
            set.choice_value_at(0),
            Some(Some(SettingValue::Text("auto".into())))
        );
        assert_eq!(set.choice_value_at(5), None);
        let unset = choice_field(None);
        let entries = unset.choice_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].value, None);
        assert_eq!(entries[0].label, "（自動）");
        assert_eq!(unset.choice_index(), Some(0));
        assert_eq!(unset.choice_value_at(0), Some(None));
        // 選択肢にない値は選択なし
        assert_eq!(choice_field(Some("zzz")).choice_index(), None);
    }

    #[test]
    fn display_text_and_range_hint() {
        let f = int_field(0, 5000, 10);
        assert_eq!(f.display_text(), "10");
        assert_eq!(
            f.range_hint().as_deref(),
            Some("範囲 0 から 5000（既定 10）")
        );
        assert_eq!(f.step(), Some(10.0));
        let mut fl = float_field();
        assert_eq!(fl.display_text(), "");
        assert_eq!(fl.range_hint().as_deref(), Some("範囲 0 から 1"));
        fl.value = Some(SettingValue::Float(0.05));
        fl.default = Some(SettingValue::Float(1.0));
        assert_eq!(fl.display_text(), "0.05");
        assert_eq!(fl.range_hint().as_deref(), Some("範囲 0 から 1（既定 1）"));
        assert_eq!(choice_field(None).range_hint(), None);
        assert_eq!(choice_field(None).step(), None);
    }

    fn check(status: UpdateStatus) -> UpdateCheck {
        UpdateCheck {
            current: "0.1.15".into(),
            latest: Some("0.2.0".into()),
            status,
            notes_url: Some("https://example.com/notes".into()),
            error: None,
        }
    }

    #[test]
    fn update_outcomes_match_the_web_wording() {
        let up = describe_update(&check(UpdateStatus::UpToDate));
        assert_eq!(up.text, "最新です（0.1.15）。");
        assert!(!up.can_apply);
        assert_eq!(up.notes_url, None);
        let avail = describe_update(&check(UpdateStatus::Available));
        assert_eq!(avail.text, "新しい版 0.2.0 があります。");
        assert!(!avail.can_apply);
        assert_eq!(
            avail.notes_url.as_deref(),
            Some("https://example.com/notes")
        );
        let staged = describe_update(&check(UpdateStatus::Staged));
        assert!(staged.can_apply);
        assert!(staged.text.contains("0.2.0 を取得しました"));
        assert!(staged.notes_url.is_some());
        let mut err = check(UpdateStatus::Error);
        err.error = Some("ネットワークに接続できません".into());
        let e = describe_update(&err);
        assert_eq!(
            e.text,
            "更新を確認できませんでした: ネットワークに接続できません"
        );
        assert!(!e.can_apply);
        assert_eq!(e.notes_url, None);
        err.error = None;
        assert!(describe_update(&err).text.ends_with("原因不明のエラーです"));
    }

    #[test]
    fn custom_message_numbers_do_not_collide() {
        let mut all = messages::ALL.to_vec();
        assert!(all.iter().all(|m| *m >= messages::APP_BASE && *m < 0xC000));
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), messages::ALL.len());
    }

    #[test]
    fn control_ids_do_not_collide() {
        let mut all = ids::FIXED.to_vec();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), ids::FIXED.len());
        // IDOK / IDCANCEL（ダイアログ管理が使う）と動的 ID の範囲を避ける
        assert!(all.iter().all(|i| *i > 2 && *i < ids::DYNAMIC_BASE));
    }
}
