//! ページの定義表と、設定項目以外のページ（更新・データ・ファイル・状態表示）の中身。
//!
//! ページを増やすには、[`PAGES`] に 1 行足し（`Page::Extra("識別子")`・サイドバーの表示名・
//! 行を組み立てる関数）、行の組み立て関数を書く。サイドバー・見出し・配置・Tab 順・スクロールは
//! 共通の仕組み（`page.rs`）が面倒を見る。ボタンを置くなら `native_model::ids` に ID を足し、
//! `window.rs` の `on_command` に処理を足す。

use super::state::{Model, Phase, Role};
use crate::native_diag::{self, BadgeKind, PlanRow};
use crate::native_layout::{Gap, RowKind, RowStyle, gap_between, row_in_card};
use crate::native_model::{FieldView, Page, ids, update_headline};

/// ボタン 1 つの定義（幅は [`crate::native_layout::BUTTON_W`] で全ボタン共通）
pub(super) struct BtnDef {
    pub id: i32,
    pub text: &'static str,
    pub enabled: bool,
    /// 読み上げ用の名前（同じ文字のボタンが並ぶとき、項目名つきで区別する）
    pub accessible_name: Option<String>,
}

/// 1 行の定義
pub(super) enum RowDef {
    /// 設定項目（スキーマから描画）
    Field {
        field: Box<FieldView>,
        error: Option<String>,
    },
    /// 文字だけの行
    Text { role: Role, text: String },
    /// ボタンの行（横に並べる）
    Buttons(Vec<BtnDef>),
    /// 診断項目の結果行（状態のバッジ + 項目名）
    Result {
        badge: BadgeKind,
        label: String,
        title: String,
    },
}

/// 文字だけの行
pub(super) fn text_row(role: Role, text: impl Into<String>) -> RowDef {
    RowDef::Text {
        role,
        text: text.into(),
    }
}

/// 節（グループ）の見出し。下に区切り線が付く
pub(super) fn section(text: impl Into<String>) -> RowDef {
    text_row(Role::Section, text)
}

/// 説明（グレー）
pub(super) fn desc(text: impl Into<String>) -> RowDef {
    text_row(Role::Desc, text)
}

/// ボタンを 1 つだけ置く行
pub(super) fn button_row(id: i32, text: &'static str, enabled: bool) -> RowDef {
    RowDef::Buttons(vec![BtnDef {
        id,
        text,
        enabled,
        accessible_name: None,
    }])
}

/// ページの定義（サイドバーの 1 項目）
pub(super) struct PageDef {
    /// 一覧でのページの識別
    pub page: Page,
    /// サイドバーの表示名（ページの見出しにもなる）
    pub title: &'static str,
    /// 見出しの下の 1 行の説明
    pub subtitle: &'static str,
    /// 見出しより下の行を組み立てる
    pub build: fn(&Model) -> Vec<RowDef>,
}

/// 設定カテゴリの後ろに並べる固定ページ。表示順 = この表の順
pub(super) const PAGES: &[PageDef] = &[
    PageDef {
        page: Page::Update,
        title: "更新",
        subtitle: "Nagi を最新の状態に保ちます。",
        build: update_rows,
    },
    PageDef {
        page: Page::Data,
        title: "データ・ファイル",
        subtitle: "設定ファイルの場所と、変換の学習データを管理します。",
        build: data_rows,
    },
    PageDef {
        page: Page::Extra(native_diag::PAGE_ID),
        title: native_diag::PAGE_TITLE,
        subtitle: "この PC での入力・変換・更新の動作を、ボタン一つで確認します。",
        build: diag_rows,
    },
];

/// サイドバーに渡す `(ページ, 表示名)`
pub(super) fn sidebar_entries() -> Vec<(Page, &'static str)> {
    PAGES.iter().map(|p| (p.page, p.title)).collect()
}

/// 固定ページの定義を引く
pub(super) fn find_page(page: Page) -> Option<&'static PageDef> {
    PAGES.iter().find(|p| p.page == page)
}

/// 行の見た目の役割。先頭のページの見出しは [`RowStyle::Hero`]（アクセントの線が付く）、節の見出しは
/// [`RowStyle::Section`]（左にアクセントの印が付く）
pub(super) fn style_of(row: usize, def: &RowDef) -> RowStyle {
    match def {
        RowDef::Text {
            role: Role::Heading,
            ..
        } if row == 0 => RowStyle::Hero,
        RowDef::Text {
            role: Role::Section,
            ..
        } => RowStyle::Section,
        _ => RowStyle::Plain,
    }
}

/// 行の種類（空きの決定用）
fn kind_of(def: &RowDef) -> RowKind {
    match def {
        RowDef::Field { .. } => RowKind::Field,
        RowDef::Buttons(_) => RowKind::Buttons,
        RowDef::Result { .. } => RowKind::Text,
        RowDef::Text { role, .. } => match role {
            Role::Heading => RowKind::Heading,
            Role::Subtitle => RowKind::Subtitle,
            Role::Section => RowKind::Section,
            // 値は直前のラベルに付属する行（説明と同じ詰めた間隔）
            Role::Desc | Role::Value => RowKind::Desc,
            Role::Error => RowKind::Error,
            _ => RowKind::Text,
        },
    }
}

/// その行をカードの中に置くか
pub(super) fn in_card(def: &RowDef) -> bool {
    row_in_card(kind_of(def))
}

/// 前の行との間の空き
pub(super) fn gap_before(prev: Option<&RowDef>, this: &RowDef) -> Gap {
    gap_between(prev.map(kind_of), kind_of(this))
}

fn update_rows(model: &Model) -> Vec<RowDef> {
    let mut rows = vec![
        section("バージョン"),
        text_row(Role::Label, "現在の版"),
        text_row(Role::Value, env!("CARGO_PKG_VERSION")),
        section("更新"),
    ];
    // 幅の狭いウィンドウでも収まるよう、主要な 2 つを 1 行、リリースノートは次の行に置く
    let mut buttons = vec![BtnDef {
        id: ids::UPDATE_CHECK,
        text: "更新を確認",
        enabled: !model.update.busy,
        accessible_name: None,
    }];
    if model.update.can_apply {
        buttons.push(BtnDef {
            id: ids::UPDATE_APPLY,
            text: "更新を適用…",
            enabled: !model.update.busy,
            accessible_name: None,
        });
    }
    rows.push(RowDef::Buttons(buttons));
    if model.update.notes_url.is_some() {
        rows.push(button_row(ids::UPDATE_NOTES, "リリースノートを開く", true));
    }
    // 結果は状態のバッジ（文字つき）+ 見出しにし、詳しい文言は下に説明として添える
    if let Some(head) = update_headline(model.update.busy, &model.update.text) {
        rows.push(RowDef::Result {
            badge: head.badge,
            label: head.label.to_owned(),
            title: head.title.to_owned(),
        });
        if !model.update.busy && !model.update.text.is_empty() {
            rows.push(desc(model.update.text.clone()));
        }
    }
    rows
}

fn data_rows(model: &Model) -> Vec<RowDef> {
    let path = model.view.as_ref().and_then(|v| v.config_path.clone());
    let enabled = path.is_some();
    let mut rows = vec![
        section("設定ファイル"),
        desc(path.unwrap_or_else(|| "（設定ファイルの場所は未定です）".to_owned())),
    ];
    // 幅の狭いウィンドウでも収まるよう、1 行に 1 つずつ置く
    for (id, text) in [
        (ids::OPEN_CONFIG, "config.toml を開く"),
        (ids::OPEN_CONFIG_DIR, "設定フォルダを開く"),
        (ids::OPEN_USER_DICT, "ユーザー辞書フォルダを開く"),
    ] {
        rows.push(button_row(id, text, enabled));
    }
    rows.push(section("学習データ"));
    rows.push(desc("変換の学習結果をすべて削除します。元に戻せません。"));
    rows.push(button_row(ids::CLEAR_LEARNING, "学習データを全削除…", true));
    rows
}

/// 「動作確認」ページ（行の中身は OS 非依存の `native_diag::plan_rows` が決める）
fn diag_rows(model: &Model) -> Vec<RowDef> {
    native_diag::plan_rows(&model.diag)
        .into_iter()
        .map(|row| match row {
            PlanRow::Section(text) => section(text),
            PlanRow::Text(text) => text_row(Role::Title, text),
            PlanRow::Desc(text) => desc(text),
            PlanRow::Result {
                badge,
                label,
                title,
            } => RowDef::Result {
                badge,
                label,
                title,
            },
            PlanRow::Buttons(buttons) => RowDef::Buttons(
                buttons
                    .into_iter()
                    .map(|b| BtnDef {
                        id: b.id,
                        text: b.text,
                        enabled: b.enabled,
                        accessible_name: b.accessible_name,
                    })
                    .collect(),
            ),
        })
        .collect()
}

/// 設定を読み込めるまで（読み込み中・失敗）の「設定」ページ
pub(super) fn status_rows(model: &Model) -> Vec<RowDef> {
    match &model.phase {
        Phase::Failed(message) => vec![
            text_row(Role::Title, "設定を読み込めませんでした"),
            text_row(Role::Error, message.clone()),
            button_row(ids::RETRY, "再試行", true),
        ],
        _ => vec![
            text_row(Role::Title, "読み込み中…"),
            desc(
                "変換エンジンを起動して設定を読み込んでいます。初回は 1 分ほどかかることがあります。",
            ),
        ],
    }
}
