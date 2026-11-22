//! ウィンドウの状態。UI スレッドだけが触る（ワーカーとは `WorkerHandle` のチャネルだけでやりとりする）。
//!
//! ウィンドウプロシージャは再入しうる（`SetWindowPos` などが同じスレッドの別メッセージを処理する）ので、
//! 可変な値は `Cell` / `RefCell` に入れ、借用は短くして、借用中に Win32 を呼ばない。

use super::dpi::Fonts;
use crate::native_diag::BadgeKind;
use crate::native_diag::DiagModel;
use crate::native_layout::{Gap, PanelLayout, RowSpec, RowStyle};
use crate::native_model::{Page, SettingsView};
use crate::native_scroll::SmoothScroll;
use crate::native_ui_state::UiState;
use crate::native_worker::WorkerHandle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{DeleteObject, HBITMAP, HGDIOBJ};

/// コントロールの役割
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// ページの見出し（STATIC・大きい太字）
    Heading,
    /// ページの見出しの下の 1 行の説明（STATIC・グレー。カードの外）
    Subtitle,
    /// 節（グループ）の見出し（STATIC・セミボールド）。直下のカードの見出し。左にアクセントの印が付く
    Section,
    /// 診断項目の結果行（オーナードローの STATIC。状態のバッジ + 項目名。文字は `[状態] 項目名`）
    Result,
    /// 項目名・本文（STATIC）
    Title,
    /// 設定項目の名前（STATIC・セミボールド）
    Label,
    /// 値の強調表示（STATIC・節の見出しと同じ大きさのセミボールド。更新ページの版など）
    Value,
    /// 真偽値（チェックボックス。トグルスイッチとして描く。項目名を兼ねる）
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
    /// 「既定に戻す」（控えめなボタン）
    Reset,
    /// 固定の ID を持つボタン
    Button,
}

/// 診断の結果行の中身（オーナードローで描く）
#[derive(Debug, Clone)]
pub(super) struct ResultInfo {
    pub badge: BadgeKind,
    pub label: String,
    pub title: String,
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
    /// 結果行（[`Role::Result`]）の中身
    pub result: Option<ResultInfo>,
    /// 内容座標での上端と高さ（スクロール前。フォーカス時に見える位置へ動かすのに使う）
    pub content_y: i32,
    pub content_h: i32,
}

/// 行の構成（配置の分岐に使う）
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RowFlags {
    pub has_input: bool,
    /// カードの中の行か（見出し・節の見出しはカードの外）
    pub in_card: bool,
    /// 前の行との間の空き
    pub gap: Gap,
    /// 行の見た目の役割（見出し・節の見出し）
    pub style: RowStyle,
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
    /// ページ全体の背景（地・カード・影・区切り線・印）を描いたビットマップ。スクロールは転送だけにする。
    /// レイアウトを作り直すとき・テーマが変わるときに捨てる（`layout_page` / `paint_panel` の鍵の比較）
    pub backdrop: Option<Backdrop>,
}

/// 背景キャッシュの鍵（幅・高さ・DPI。これが変われば作り直す）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BackdropKey {
    pub width: i32,
    pub height: i32,
    pub dpi: u32,
}

/// ページ全体の背景のビットマップ（所有。破棄で解放する）
pub(super) struct Backdrop {
    pub bitmap: HBITMAP,
    pub key: BackdropKey,
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        // SAFETY: `paint_panel` が作ったビットマップ。どの DC にも選択したままにしない
        let _ = unsafe { DeleteObject(HGDIOBJ(self.bitmap.0)) };
    }
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
    /// 左の一覧の各行の表示名（`sidebar` と同じ並び。ページの保存・アイコンの選択に使う）
    pub sidebar_titles: Vec<String>,
    /// 前回開いていたページ（カテゴリは設定を読み込めてから戻すため、読み込みまで持ち越す）
    pub wanted_page: Option<String>,
    /// 項目ごとのエラー（キー → メッセージ）
    pub errors: HashMap<String, String>,
    pub update: UpdateUi,
    /// 「動作確認」ページ
    pub diag: DiagModel,
    pub wanted_focus: Option<Ident>,
}

pub(super) struct State {
    pub hwnd: Cell<HWND>,
    pub list: Cell<HWND>,
    pub detail: Cell<HWND>,
    pub status: Cell<HWND>,
    pub panel: Cell<HWND>,
    pub close: Cell<HWND>,
    pub sidebar_rule: Cell<HWND>,
    pub footer_rule: Cell<HWND>,
    pub brand: Cell<HWND>,
    pub dpi: Cell<u32>,
    pub fonts: Cell<Option<Fonts>>,
    pub worker: RefCell<Option<WorkerHandle>>,
    pub model: RefCell<Model>,
    pub ui: RefCell<PageUi>,
    pub status_is_error: Cell<bool>,
    pub rebuild_posted: Cell<bool>,
    /// ホイールの補間
    pub smooth: RefCell<SmoothScroll>,
    /// 補間の前フレームの時刻
    pub last_tick: Cell<Option<Instant>>,
    /// UI の状態の保存先（`None` なら保存しない）
    pub ui_state_path: Option<PathBuf>,
    /// 最後に読み込んだ・保存した UI の状態（ウィンドウの位置の持ち越しに使う）
    pub saved: RefCell<UiState>,
}

impl State {
    pub fn new(
        start_page: Page,
        saved: UiState,
        wanted_page: Option<String>,
        ui_state_path: Option<PathBuf>,
    ) -> Self {
        Self {
            hwnd: Cell::default(),
            list: Cell::default(),
            detail: Cell::default(),
            status: Cell::default(),
            panel: Cell::default(),
            close: Cell::default(),
            sidebar_rule: Cell::default(),
            footer_rule: Cell::default(),
            brand: Cell::default(),
            dpi: Cell::new(96),
            fonts: Cell::new(None),
            worker: RefCell::new(None),
            model: RefCell::new(Model {
                phase: Phase::Loading,
                view: None,
                detail: saved.detail,
                page: start_page,
                sidebar: Vec::new(),
                sidebar_titles: Vec::new(),
                wanted_page,
                errors: HashMap::new(),
                update: UpdateUi::default(),
                diag: DiagModel::default(),
                wanted_focus: None,
            }),
            ui: RefCell::new(PageUi::default()),
            status_is_error: Cell::new(false),
            rebuild_posted: Cell::new(false),
            smooth: RefCell::new(SmoothScroll::new(0)),
            last_tick: Cell::new(None),
            ui_state_path,
            saved: RefCell::new(saved),
        }
    }
}
