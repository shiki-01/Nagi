//! ウィンドウの状態。UI スレッドだけが触る（ワーカーとは `WorkerHandle` のチャネルだけでやりとりする）。
//!
//! ウィンドウプロシージャは再入しうる（`SetWindowPos` などが同じスレッドの別メッセージを処理する）ので、
//! 可変な値は `Cell` / `RefCell` に入れ、借用は短くして、借用中に Win32 を呼ばない。

use super::dpi::Fonts;
use crate::native_layout::{PanelLayout, RowSpec};
use crate::native_model::{Page, SettingsView};
use crate::native_worker::WorkerHandle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use windows::Win32::Foundation::HWND;

/// コントロールの役割
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// 項目名・見出し（STATIC）
    Title,
    /// 真偽値（チェックボックス。項目名を兼ねる）
    Check,
    /// 説明（STATIC・小さめ・グレー）
    Desc,
    /// エラー（STATIC・赤）
    Error,
    /// 入力欄（EDIT）
    Edit,
    /// 選択（COMBOBOX）
    Combo,
    /// 範囲の案内（STATIC・小さめ・グレー）
    Hint,
    /// 「既定に戻す」
    Reset,
    /// 固定の ID を持つボタン
    Button,
}

/// 右のパネルのコントロール 1 つ
pub(super) struct Ctl {
    pub hwnd: HWND,
    pub id: i32,
    pub role: Role,
    /// 何行目か
    pub row: usize,
    /// 設定キー（項目のコントロールのみ）
    pub key: Option<String>,
    /// 表示している文字列（折り返しの測定用）
    pub text: String,
    /// ボタンの幅（dp）
    pub width_dp: i32,
    /// 内容座標での上端と高さ（スクロール前。フォーカス時に見える位置へ動かすのに使う）
    pub content_y: i32,
    pub content_h: i32,
}

/// 行の構成（配置の分岐に使う）
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RowFlags {
    pub has_input: bool,
    pub has_hint: bool,
}

/// 今のページのコントロールと配置
#[derive(Default)]
pub(super) struct PageUi {
    pub ctls: Vec<Ctl>,
    pub flags: Vec<RowFlags>,
    pub specs: Vec<RowSpec>,
    pub layout: Option<PanelLayout>,
    pub scroll: i32,
    pub viewport_h: i32,
}

/// 読み込みの段階
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Phase {
    /// `imserver` の起動・設定の読み込み中
    Loading,
    /// 読み込めた
    Ready,
    /// 失敗（理由）
    Failed(String),
}

/// フォーカスの目印（ページを作り直しても同じコントロールに戻すため）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ident {
    pub role: Role,
    pub key: Option<String>,
    pub id: i32,
}

/// 更新ページの表示
#[derive(Debug, Clone, Default)]
pub(super) struct UpdateUi {
    pub busy: bool,
    pub text: String,
    pub can_apply: bool,
    pub notes_url: Option<String>,
}

/// 画面モデルと、それに対する UI の選択
pub(super) struct Model {
    pub phase: Phase,
    pub view: Option<SettingsView>,
    pub detail: bool,
    pub page: Page,
    /// 左の一覧の各行が指すページ
    pub sidebar: Vec<Page>,
    /// 項目ごとのエラー（キー → メッセージ）
    pub errors: HashMap<String, String>,
    pub update: UpdateUi,
    pub wanted_focus: Option<Ident>,
}

pub(super) struct State {
    pub hwnd: Cell<HWND>,
    pub list: Cell<HWND>,
    pub detail: Cell<HWND>,
    pub status: Cell<HWND>,
    pub panel: Cell<HWND>,
    pub dpi: Cell<u32>,
    pub fonts: Cell<Option<Fonts>>,
    pub worker: RefCell<Option<WorkerHandle>>,
    pub model: RefCell<Model>,
    pub ui: RefCell<PageUi>,
    pub status_is_error: Cell<bool>,
    pub rebuild_posted: Cell<bool>,
}

impl State {
    pub fn new(start_page: Page) -> Self {
        Self {
            hwnd: Cell::default(),
            list: Cell::default(),
            detail: Cell::default(),
            status: Cell::default(),
            panel: Cell::default(),
            dpi: Cell::new(96),
            fonts: Cell::new(None),
            worker: RefCell::new(None),
            model: RefCell::new(Model {
                phase: Phase::Loading,
                view: None,
                detail: false,
                page: start_page,
                sidebar: Vec::new(),
                errors: HashMap::new(),
                update: UpdateUi::default(),
                wanted_focus: None,
            }),
            ui: RefCell::new(PageUi::default()),
            status_is_error: Cell::new(false),
            rebuild_posted: Cell::new(false),
        }
    }
}
