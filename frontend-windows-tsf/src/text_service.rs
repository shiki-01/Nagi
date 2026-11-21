//! TSF の入力方式本体: `ITfTextInputProcessorEx`、`ITfKeyEventSink`、`ITfCompositionSink`、
//! `ITfDisplayAttributeProvider`、`ITfCompartmentEventSink`。
//!
//! 役割は変換と描画のみ。キーは `nagi_windows::keynorm` で正規化してエンジンへ送り、
//! 応答の確定・preedit を編集セッションで反映する。何を食うか・何を確定するかはエンジンが決める。
//! インスタンスは `Activate` までエンジンに触れない（表示属性の提供など、アクティブ化されない
//! プロセスでも COM オブジェクトだけが作られるため）。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use nagi_protocol::{
    CandidatesView, InputMode, KeyKind, ProcessKeyResult, StatusResult, TouchAction,
};
use nagi_windows::anchor_wait::{AnchorWait, ShowDecision, TimerOutcome};
use nagi_windows::candidate_style::{
    CandidateStyle, FOCUS_REFRESH_INTERVAL, ModePopupPosition, RefreshGate, SHOW_REFRESH_INTERVAL,
};
use nagi_windows::candidates::{CandidatePage, Point, Rect};
use nagi_windows::context::{build_context, should_report_before_key};
use nagi_windows::environment::{
    EnvironmentReporter, OverridingProbe, ResumeDetector, ResumeSource,
};
use nagi_windows::focus::{FocusPlan, FocusTracker, selection_outside_composition};
use nagi_windows::keynorm;
use nagi_windows::mode::{ModeSync, compartment_values};
use nagi_windows::process::SpawnHook;
use nagi_windows::status::{
    MenuCommand, MenuEntry, MenuTargets, PathEnv, ToastThrottle, toast_for,
};
use nagi_windows::supervisor::EngineEvent;
use nagi_windows::toast::{
    ModeChangeCause, ModePopupInput, mode_popup_anchor, mode_popup_text, should_show_mode_popup,
};
use nagi_windows::ui_element::CandidateListData;
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER, HWND, LPARAM, WPARAM};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, ITfCompartmentEventSink, ITfCompartmentEventSink_Impl,
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfDocumentMgr, ITfEditRecord, ITfFnConfigure, ITfFnConfigure_Impl, ITfFunction_Impl,
    ITfFunctionProvider, ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfRange,
    ITfSource, ITfSourceSingle, ITfTextEditSink, ITfTextEditSink_Impl, ITfTextInputProcessor_Impl,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl, ITfThreadMgr, ITfThreadMgrEventSink,
    ITfThreadMgrEventSink_Impl, ITfUIElement, ITfUIElementMgr, TF_ANCHOR_END, TF_ANCHOR_START,
};
use windows::core::{BOOL, BSTR, GUID, IUnknown, Interface, Ref, Result, implement};

use crate::candidate_window::CandidateWindow;
use crate::display_attr::{self, AttrAtoms};
use crate::dll;
use crate::edit::{self, Env, SharedComposition, Timing};
use crate::engine::{EngineHost, EngineLocation, RECONVERT_TIMEOUT};
use crate::langbar::{self, LangBar};
use crate::message_window::{MessageEvent, MessageWindow};
use crate::mode_comp::{self, ModeCompartments};
use crate::probe::Win32Probe;
use crate::reconversion::FunctionProvider;
use crate::render::{self, EditPlan, HandoffCache};
use crate::toast_window::ToastWindow;
use crate::ui_element::{CandidateListElement, ElementState};
use crate::win;

/// プロセス内で共有するエンジン（ホストプロセスごとに 1 つ）。作るだけでは imserver を起動しない
/// （`EngineHost::ensure_started` を、フォーカスが当たったとき・最初のキーで呼ぶ。入力しないホストで
/// 起動しないため。decisions.md H-96）。
/// TODO: 複数のホストプロセスでエンジンを共有する（名前付きパイプ + 常駐ブローカー）。
/// 現状は各ホストプロセスが imserver を起動する（メモリ・起動時間の観点で要再検討。decisions.md H-39）
static ENGINE: OnceLock<Arc<EngineHost>> = OnceLock::new();

/// プロセス内で共有する環境の報告（変化したときだけ送る。エンジンは 1 つなので 1 つ）
static ENVIRONMENT: OnceLock<Mutex<EnvironmentReporter<EnvProbe>>> = OnceLock::new();

/// 通知の間引き（同じ文言を 30 秒は繰り返さない）。プロセス内で 1 つ
static TOASTS: OnceLock<Mutex<ToastThrottle>> = OnceLock::new();

/// 直近に記録した「機密欄か」の状態（プロセス内。変化したときだけログに書く）
static SECURE_INPUT_LOGGED: AtomicBool = AtomicBool::new(false);

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

/// 単調な経過時間（候補の表示保留のタイムアウト用。起点は最初の呼び出し）
fn monotonic_now() -> std::time::Duration {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed()
}

/// このホストプロセスのアプリ別の位置の補正（dp）を `%APPDATA%\Nagi\window-offsets.tsv` から読む。
/// ファイルが無い・大きすぎる・読めなければ 0（補正なし）。手で編集されるので、フォーカスを得るたびに読み直す
fn load_window_offset() -> Point {
    use nagi_windows::placement::{MAX_FILE_BYTES, OffsetTable, offsets_path};
    static APP: OnceLock<Option<String>> = OnceLock::new();
    let Some(app) = APP.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| nagi_windows::placement::app_key(&p.to_string_lossy()))
    }) else {
        return Point::default();
    };
    let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) else {
        return Point::default();
    };
    let path = offsets_path(std::path::Path::new(&appdata));
    let small = std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_FILE_BYTES);
    if !small {
        return Point::default();
    }
    std::fs::read_to_string(&path)
        .map(|text| OffsetTable::parse(&text).get(app))
        .unwrap_or_default()
}

/// 再変換に時間がかかったときの通知
const RECONVERT_SLOW: &str =
    "再変換の準備に時間がかかっています。少し待ってからもう一度お試しください";

fn engine_location() -> EngineLocation {
    // imserver は DLL と同じディレクトリ。辞書・モデルは %APPDATA%\Nagi\data
    let executable = dll::module_directory()
        .unwrap_or_default()
        .join("imserver.exe");
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    EngineLocation {
        executable,
        data_dir: appdata.join("Nagi").join("data"),
        // 取得した更新はローミングしない %LOCALAPPDATA% に置く（apply-update.ps1 の既定と同じ場所）
        update_dir: std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|d| PathBuf::from(d).join("Nagi").join("update")),
    }
}

/// 取得済みの更新の `ready.json`（`%LOCALAPPDATA%\Nagi\update\ready.json`）の中身。無い・大きすぎる・
/// 読めなければ `None`。利用者が書き換えられるファイルなので、表示にしか使わない（適用時にコアが再検証する）
fn read_staged_ready(local_app_data: Option<PathBuf>) -> Option<String> {
    use std::io::Read;
    let path = local_app_data?
        .join("Nagi")
        .join("update")
        .join("ready.json");
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(64 * 1024)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

fn shared_engine() -> Arc<EngineHost> {
    ENGINE
        .get_or_init(|| {
            let location = engine_location();
            crate::tsf_log::log(
                "engine",
                &format!(
                    "location exe={} exists={} data_dir={} exists={}",
                    location.executable.display(),
                    location.executable.exists(),
                    location.data_dir.display(),
                    location.data_dir.exists()
                ),
            );
            // 起動した imserver は Job Object に入れる（ホストの終了・クラッシュで OS が終了させる）
            let host = Arc::new(EngineHost::with_spawn_hook(
                &location,
                env!("CARGO_PKG_VERSION"),
                Some(SpawnHook::new(|child| {
                    crate::proc_mem::record_engine_pid(child.id());
                    crate::job::assign(child);
                })),
            ));
            // エンジンを作った後は DLL をアンロードさせない（`DllCanUnloadNow` が `S_FALSE`）
            dll::pin_for_engine();
            host
        })
        .clone()
}

/// OS の検出値。`%LOCALAPPDATA%\Nagi\debug-env.json` があるときだけ上書きされる（**検証専用**。
/// `verify.ps1 env`。通常の利用ではファイルが無いので実値のまま）
type EnvProbe = OverridingProbe<Win32Probe>;

fn environment() -> &'static Mutex<EnvironmentReporter<EnvProbe>> {
    ENVIRONMENT.get_or_init(|| {
        let path = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|dir| nagi_windows::diag::debug_env_path(std::path::Path::new(&dir)))
            // LOCALAPPDATA が無ければ存在しないパス（上書きなし）
            .unwrap_or_default();
        let probe = OverridingProbe::new(
            Win32Probe,
            path,
            Box::new(|message| crate::tsf_log::log("env", message)),
        );
        Mutex::new(EnvironmentReporter::new(probe))
    })
}

fn toasts() -> &'static Mutex<ToastThrottle> {
    TOASTS.get_or_init(|| Mutex::new(ToastThrottle::default()))
}

/// Activate 中の失敗を tsf.log に残す（HRESULT のみ）
fn log_failure(what: &str, error: &windows::core::Error) {
    crate::tsf_log::log(
        "tsf",
        &format!(
            "{what} failed hr={}",
            nagi_windows::diag::hresult_text(error.code().0)
        ),
    );
}

/// COM のエントリポイントでパニックを止める（パニックを FFI 境界から出すとホストアプリごと落ちる）
pub(crate) fn guarded<T>(fallback: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fallback())
}

/// 応答のうち、編集の反映後に UI へ伝える部分（候補・モード・再変換の要求）
#[derive(Debug, Default)]
struct Followup {
    candidates: Option<CandidatesView>,
    mode: Option<InputMode>,
    reconvert: bool,
}

impl Followup {
    fn from_result(result: &ProcessKeyResult) -> Self {
        Self {
            candidates: result.candidates.clone(),
            mode: result.mode,
            reconvert: result.reconvert_selection == Some(true),
        }
    }
}

/// 1 キー分の判断結果（`OnTestKeyDown` で得て `OnKeyDown` へ引き継ぐ）
#[derive(Debug)]
struct KeyOutcome {
    /// TSF に「食った」と返すか
    eaten: bool,
    /// 反映する編集（無ければ空の計画）
    plan: EditPlan,
    /// 反映後の UI 更新
    followup: Followup,
}

/// UI スレッド上の共有状態（TSF のオブジェクトは単一スレッド・アパートメントで使う）
pub(crate) struct Core {
    me: Weak<Core>,
    thread_mgr: RefCell<Option<ITfThreadMgr>>,
    client_id: Cell<u32>,
    session_id: String,
    engine: Arc<EngineHost>,
    composition: SharedComposition,
    composition_sink: RefCell<Option<ITfCompositionSink>>,
    atoms: Cell<Option<AttrAtoms>>,
    /// 直近に測ったキャレット（composition）の矩形。候補ウィンドウの基準
    caret: Rc<Cell<Option<Rect>>>,
    /// 同じ composition で直前に測れた矩形（`edit::Env::last_rect`）
    last_rect: Rc<Cell<Option<Rect>>>,
    /// 編集セッション完了後の再測定待ち（`edit::Env::remeasure_pending`）
    remeasure_pending: Rc<Cell<bool>>,
    /// 候補の表示保留のタイムアウト（H-102 の緩和）
    anchor_wait: RefCell<AnchorWait>,
    /// 最後に候補の基準にできた矩形（保留がタイムアウトしたときの表示位置。フォーカスを失うと捨てる）
    last_anchor: Cell<Option<Rect>>,
    handoff: RefCell<HandoffCache<KeyOutcome>>,
    candidate_window: CandidateWindow,
    toast: ToastWindow,
    /// モード切替ポップアップ（既定はキャレット付近に 1 秒。通知とは別のウィンドウ）
    mode_popup: ToastWindow,
    /// 変換 UI の見た目（設定 `windows.candidate_*` などの実効値。H-119）
    ui_style: Cell<CandidateStyle>,
    /// 設定を一度でも読めたか（読めるまでは候補の表示のたびに再試行する）
    ui_style_loaded: Cell<bool>,
    /// 設定の読み直しの間引き
    ui_style_gate: RefCell<RefreshGate>,
    /// 直近に記録した、設定値の補正の理由（同じ内容を何度もログに出さない）
    ui_style_notes: RefCell<Vec<String>>,
    /// UILess モードで `BeginUIElement` 中の候補リスト要素（出していなければ `None`）
    ui: RefCell<Option<UiSession>>,
    /// 直近の `BeginUIElement` でアプリが「自前で描く」と答えたか（モード切替ポップアップを出さない判断用。
    /// フォーカスが変わったら捨てる）
    ui_less: Cell<bool>,
    /// 利用者が言語バーでモードを切り替えている最中（コンパートメントの変更通知を利用者の操作と見分ける）
    user_mode_request: Cell<bool>,
    /// このセッションが前面でフォーカスを持っているか（通知はここだけ出す）
    focused: Cell<bool>,
    message_window: RefCell<Option<MessageWindow>>,
    mode_sync: RefCell<ModeSync>,
    compartments: RefCell<Option<ModeCompartments>>,
    /// 言語バーのボタン（登録に失敗したら `None`。タスクトレイの表示はコンパートメントなので入力はできる）
    lang_bar: RefCell<Option<LangBar>>,
    /// 自分でコンパートメントを書いている間（2 つの値を順に書く途中の通知を OS 側の操作と取り違えない）
    writing_mode: Cell<bool>,
    resume: RefCell<ResumeDetector>,
    /// 直近に送った focus_in / focus_out（2 つの通知経路で二重に送らない）
    focus: RefCell<FocusTracker>,
    /// `ITfThreadMgrEventSink` の購読（Deactivate で解除する）
    thread_sink: RefCell<Option<(ITfSource, u32)>>,
    /// composition 中の文書の `ITfTextEditSink` の購読
    text_edit: RefCell<Option<TextEditAdvise>>,
}

/// `BeginUIElement` 中の候補リスト要素
struct UiSession {
    mgr: ITfUIElementMgr,
    id: u32,
    state: Rc<ElementState>,
    /// 自前の候補ウィンドウを出す場合（`Show(TRUE)`）の表示内容
    page: CandidatePage,
}

/// `ITfTextEditSink` の購読（composition を持つ context への）
struct TextEditAdvise {
    context: ITfContext,
    source: ITfSource,
    cookie: u32,
}

/// 同じ COM オブジェクトか（`IUnknown` の同一性で比べる）
fn same_object<T: Interface>(a: &T, b: &T) -> bool {
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(a), Ok(b)) => a.as_raw() == b.as_raw(),
        _ => false,
    }
}

impl Core {
    fn new(me: Weak<Core>) -> Self {
        let n = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let interval =
            std::time::Duration::from_millis(u64::from(crate::message_window::TICK_INTERVAL_MS));
        Self {
            me,
            thread_mgr: RefCell::new(None),
            client_id: Cell::new(0),
            session_id: format!("win-{}-{n}", std::process::id()),
            engine: shared_engine(),
            composition: Rc::new(RefCell::new(None)),
            composition_sink: RefCell::new(None),
            atoms: Cell::new(None),
            caret: Rc::new(Cell::new(None)),
            last_rect: Rc::new(Cell::new(None)),
            remeasure_pending: Rc::new(Cell::new(false)),
            anchor_wait: RefCell::new(AnchorWait::new()),
            last_anchor: Cell::new(None),
            handoff: RefCell::new(HandoffCache::new()),
            candidate_window: CandidateWindow::new(),
            toast: ToastWindow::new(),
            mode_popup: ToastWindow::new(),
            ui_style: Cell::new(CandidateStyle::default()),
            ui_style_loaded: Cell::new(false),
            ui_style_gate: RefCell::new(RefreshGate::new()),
            ui_style_notes: RefCell::new(Vec::new()),
            ui: RefCell::new(None),
            ui_less: Cell::new(false),
            user_mode_request: Cell::new(false),
            focused: Cell::new(false),
            message_window: RefCell::new(None),
            mode_sync: RefCell::new(ModeSync::new()),
            compartments: RefCell::new(None),
            lang_bar: RefCell::new(None),
            writing_mode: Cell::new(false),
            resume: RefCell::new(ResumeDetector::new(
                interval,
                std::time::Duration::from_secs(60),
            )),
            focus: RefCell::new(FocusTracker::new()),
            thread_sink: RefCell::new(None),
            text_edit: RefCell::new(None),
        }
    }

    fn is_composing(&self) -> bool {
        self.composition.borrow().is_some()
    }

    fn env(&self) -> Option<Env> {
        let me = Weak::clone(&self.me);
        Some(Env {
            composition: Rc::clone(&self.composition),
            sink: self.composition_sink.borrow().clone()?,
            atoms: self.atoms.get(),
            caret: Rc::clone(&self.caret),
            client_id: self.client_id.get(),
            last_rect: Rc::clone(&self.last_rect),
            remeasure_pending: Rc::clone(&self.remeasure_pending),
            on_remeasured: Rc::new(move || {
                if let Some(core) = me.upgrade() {
                    core.anchor_wait.borrow_mut().finish();
                    core.note_anchor();
                    core.candidate_window.reposition(core.caret.get());
                }
            }),
            on_composition_change: {
                let me = Weak::clone(&self.me);
                Rc::new(move |context: &ITfContext| {
                    if let Some(core) = me.upgrade() {
                        core.sync_edit_sink(context);
                    }
                })
            },
        })
    }

    // ---- 起動・終了 ----

    fn activate(self: &Rc<Self>, thread_mgr: ITfThreadMgr, client_id: u32) -> Result<()> {
        crate::tsf_log::log("tsf", &format!("Activate client_id={client_id}"));
        self.client_id.set(client_id);
        *self.thread_mgr.borrow_mut() = Some(thread_mgr.clone());

        let sink: ITfCompositionSink = CompositionSink {
            core: Rc::clone(self),
        }
        .into();
        *self.composition_sink.borrow_mut() = Some(sink);
        // 表示属性の登録に失敗しても入力はできる（下線の装飾が無くなるだけ）
        self.atoms.set(display_attr::register_atoms().ok());

        // 隠しウィンドウ: 別スレッドのエンジン通知・タイマー・電源通知を UI スレッドで受ける
        let weak = Rc::downgrade(self);
        let window = MessageWindow::create(Box::new(move |event| {
            if let Some(core) = weak.upgrade() {
                core.on_message(event);
            }
        }));
        if let Some(window) = &window {
            self.engine.subscribe(&self.session_id, window.waker());
        }
        *self.message_window.borrow_mut() = window;

        let key_sink: ITfKeyEventSink = KeyEventSink {
            core: Rc::clone(self),
        }
        .into();
        let keystroke: ITfKeystrokeMgr = thread_mgr
            .cast()
            .inspect_err(|e| log_failure("ITfKeystrokeMgr", e))?;
        // fforeground = true: フォーカスが無くても先にキーを受ける（IME として標準）
        unsafe { keystroke.AdviseKeyEventSink(client_id, &key_sink, true) }
            .inspect_err(|e| log_failure("AdviseKeyEventSink", e))?;

        // 文書（入力欄・タブ）の切り替え。失敗してもアプリ単位のフォーカス通知で動く
        if let Ok(source) = thread_mgr.cast::<ITfSource>() {
            let sink: ITfThreadMgrEventSink = ThreadMgrSink {
                core: Rc::downgrade(self),
            }
            .into();
            match unsafe { source.AdviseSink(&ITfThreadMgrEventSink::IID, &sink) } {
                Ok(cookie) => *self.thread_sink.borrow_mut() = Some((source, cookie)),
                Err(e) => log_failure("AdviseSink(ThreadMgrEventSink)", &e),
            }
        }

        // モード（コンパートメント）。失敗しても入力はできる（OS 側の表示とは同期しない）
        let compartment_sink: ITfCompartmentEventSink = CompartmentSink {
            core: Rc::downgrade(self),
        }
        .into();
        *self.compartments.borrow_mut() =
            ModeCompartments::new(&thread_mgr, client_id, &compartment_sink)
                .inspect_err(|e| log_failure("mode compartments", e))
                .ok();
        self.sync_mode_with_os();

        // 言語バーのボタン（トレイの入力インジケーター・モード切り替え・メニュー）。
        // エンジンの接続状態には依存しない（エンジンは別スレッドで起動中でも登録する）。
        // 失敗の理由は `LangBar::new` が tsf.log に残す
        *self.lang_bar.borrow_mut() = LangBar::new(&thread_mgr, Rc::downgrade(self)).ok();
        self.refresh_lang_bar();

        // 再変換（ITfFnReconversion）の入口
        if let Ok(single) = thread_mgr.cast::<ITfSourceSingle>() {
            let provider = FunctionProvider::create(Rc::downgrade(self));
            let _ =
                unsafe { single.AdviseSingleSink(client_id, &ITfFunctionProvider::IID, &provider) };
        }

        self.poll_environment();
        crate::tsf_log::log(
            "tsf",
            &format!(
                "Activate done langbar={} compartments={} message_window={}",
                self.lang_bar.borrow().is_some(),
                self.compartments.borrow().is_some(),
                self.message_window.borrow().is_some()
            ),
        );
        Ok(())
    }

    fn deactivate(&self) {
        crate::tsf_log::log("tsf", "Deactivate");
        let thread_mgr = self.thread_mgr.borrow().clone();
        let id = self.client_id.get();
        if let Some(tm) = &thread_mgr {
            if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                let _ = unsafe { km.UnadviseKeyEventSink(id) };
            }
            if let Ok(single) = tm.cast::<ITfSourceSingle>() {
                let _ = unsafe { single.UnadviseSingleSink(id, &ITfFunctionProvider::IID) };
            }
        }
        if let Some((source, cookie)) = self.thread_sink.borrow_mut().take() {
            let _ = unsafe { source.UnadviseSink(cookie) };
        }
        // 表示中の文字列は確定してから閉じる（状態に関わらず送る）
        self.focus.borrow_mut().reset();
        self.leave_focus(self.focused_context());
        self.release_edit_sink();
        self.engine.forget_session(&self.session_id);
        self.end_ui_element();
        self.candidate_window.destroy();
        self.toast.destroy();
        self.mode_popup.destroy();
        *self.message_window.borrow_mut() = None;
        *self.lang_bar.borrow_mut() = None;
        *self.compartments.borrow_mut() = None;
        *self.composition.borrow_mut() = None;
        *self.composition_sink.borrow_mut() = None;
        *self.thread_mgr.borrow_mut() = None;
        self.client_id.set(0);
    }

    // ---- 隠しウィンドウのイベント ----

    fn on_message(&self, event: MessageEvent) {
        match event {
            MessageEvent::EngineEvents => {
                self.pump_events(self.focused_context().as_ref(), Timing::Deferred)
            }
            MessageEvent::Tick => {
                self.poll_environment();
                // imserver の常駐メモリを診断ログへ（H-99 の閾値の実測用。10 分に 1 回まで）
                crate::proc_mem::log_engine_memory_throttled();
                if self.resume.borrow_mut().tick(SystemTime::now()) {
                    self.engine.restart_in_background(ResumeSource::TickGap);
                }
            }
            MessageEvent::Resume => {
                if self.resume.borrow_mut().notified(SystemTime::now()) {
                    self.engine
                        .restart_in_background(ResumeSource::OsNotification);
                }
                self.poll_environment();
            }
            MessageEvent::PowerChanged => self.poll_environment(),
            MessageEvent::DeferTimeout => self.on_defer_timeout(),
        }
    }

    // ---- 環境 ----

    fn poll_environment(&self) {
        let params = {
            let mut reporter = environment().lock().unwrap_or_else(|e| e.into_inner());
            reporter.poll_with_source()
        };
        if let Some((params, overridden)) = params {
            // 列挙値だけを記録する（変化が無い poll は `poll_with_source` が `None` を返すので書かない）
            crate::tsf_log::log(
                "env",
                &nagi_windows::diag::describe_environment_sent(&params, overridden),
            );
            let _ = self.engine.set_environment(&params);
        }
    }

    /// エンジンが（再）起動した。新しいエンジンは前回値もモードも知らない
    fn on_engine_started(&self) {
        environment()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reset();
        self.poll_environment();
        self.sync_mode_with_os();
    }

    // ---- 入力モード ----

    /// 新しいセッション（エンジン）はひらがなで始まる。OS 側の値と合わせる:
    /// OS が未設定ならひらがなを書き、設定済みで違えばコアへ `SetMode` を送る
    fn sync_mode_with_os(&self) {
        self.mode_sync.borrow_mut().engine_restarted();
        let current = self
            .compartments
            .borrow()
            .as_ref()
            .map(ModeCompartments::read);
        match current {
            None => {}
            Some(None) => self.write_mode_values(compartment_values(InputMode::Hiragana)),
            Some(Some(v)) => {
                self.request_mode_from_os(v.open, v.conversion, ModeChangeCause::Automatic)
            }
        }
        self.refresh_lang_bar();
    }

    /// モードの値をコンパートメントへ書く。書き込み中の変更通知は無視する
    fn write_mode_values(&self, values: nagi_windows::mode::CompartmentValues) {
        self.writing_mode.set(true);
        if let Some(c) = self.compartments.borrow().as_ref() {
            let _ = c.write(values);
        }
        self.writing_mode.set(false);
    }

    /// OS のモードがコアと違うなら、コアへ送って結果を反映する
    fn request_mode_from_os(&self, open: bool, conversion: u32, cause: ModeChangeCause) {
        let previous = self.mode_sync.borrow().known();
        let requested = self.mode_sync.borrow_mut().os_changed(open, conversion);
        let Some(mode) = requested else {
            return;
        };
        self.refresh_lang_bar();
        if let Ok(result) = self.engine.set_mode(&self.session_id, mode) {
            // SetMode は入力中の文字列を確定する
            if let Some(ctx) = self.focused_context() {
                let _ = self.apply(&ctx, render::plan_from_result(&result), Timing::Deferred);
            }
            // この経路のモードは `os_changed` で合意済みなので、`finish` はポップアップを出さない
            self.finish(&Followup::from_result(&result), ModeChangeCause::Automatic);
            self.notify_mode_changed(previous, mode, cause);
        }
    }

    /// コンパートメントが変わった（トレイのクリック・他の IME との往復・自分の書き込み）
    fn on_compartment_changed(&self) {
        if self.writing_mode.get() {
            return;
        }
        let values = self
            .compartments
            .borrow()
            .as_ref()
            .and_then(ModeCompartments::read);
        // 言語バーの操作（`request_mode`）による変更だけを利用者の操作とする。
        // アプリや他の IME・他スレッドによる変更は自動の同期として扱う
        let cause = if self.user_mode_request.get() {
            ModeChangeCause::User
        } else {
            ModeChangeCause::Automatic
        };
        if let Some(v) = values {
            self.request_mode_from_os(v.open, v.conversion, cause);
        }
    }

    /// コアでモードが変わった。OS へ書く（同じなら書かない）。`cause` は変わった原因
    /// （キー操作の応答なら利用者の操作）
    fn apply_core_mode(&self, mode: InputMode, cause: ModeChangeCause) {
        let previous = self.mode_sync.borrow().known();
        let values = self.mode_sync.borrow_mut().core_changed(mode);
        if let Some(values) = values {
            self.write_mode_values(values);
        }
        self.refresh_lang_bar();
        if values.is_some() {
            self.notify_mode_changed(previous, mode, cause);
        }
    }

    // ---- モード切替ポップアップ ----

    /// モードが変わったことを知らせるポップアップを（必要なら）出す。判定は `should_show_mode_popup`
    fn notify_mode_changed(
        &self,
        previous: Option<InputMode>,
        mode: InputMode,
        cause: ModeChangeCause,
    ) {
        let input = ModePopupInput {
            enabled: self.ui_style.get().mode_popup,
            previous,
            mode,
            cause,
            focused: self.focused.get(),
            ui_less: self.ui_less.get(),
        };
        if !should_show_mode_popup(&input) {
            return;
        }
        let text = mode_popup_text(mode);
        // 画面の隅に出す設定なら、キャレットは測らない
        if matches!(
            self.ui_style.get().mode_popup_position,
            ModePopupPosition::Corner(_)
        ) {
            self.present_mode_popup(text, None);
            return;
        }
        // キャレットの矩形は文書を読んで測る（読めなければマウス位置）
        let Some(context) = self.focused_context() else {
            self.present_mode_popup(text, None);
            return;
        };
        let me = Weak::clone(&self.me);
        let measured = edit::measure_selection_rect(
            &context,
            self.client_id.get(),
            Timing::Deferred,
            move |rect| {
                if let Some(core) = me.upgrade() {
                    core.present_mode_popup(text, rect);
                }
            },
        );
        if measured.is_err() {
            self.present_mode_popup(text, None);
        }
    }

    /// 設定の位置にポップアップを出す。キャレットの近くなら、測ったキャレットの近く（測れなければ
    /// マウス位置、それも無ければ出さない）、画面の隅ならその隅。
    /// 測っている間にフォーカスが外れた・UILess になった場合も出さない
    fn present_mode_popup(&self, text: &str, caret: Option<Rect>) {
        if !self.focused.get() || self.ui_less.get() {
            return;
        }
        if let ModePopupPosition::Corner(position) = self.ui_style.get().mode_popup_position {
            self.mode_popup.show_corner(text, position);
            return;
        }
        let Some(anchor) = mode_popup_anchor(caret, win::try_cursor_position()) else {
            return;
        };
        self.mode_popup.show_near(text, anchor);
    }

    // ---- 変換 UI の見た目（設定） ----

    /// 設定 `windows.candidate_*` などをコアから読み直し、候補ウィンドウ・通知・モード表示・下線へ渡す
    /// （次の表示から効く。再起動は不要）。`min_interval` より短い間隔の呼び出しは何もしない。
    /// 読めない（エンジン未接続など）・設定ファイルが壊れているときは、直前の見た目を保つ
    fn refresh_ui_style(&self, min_interval: std::time::Duration) {
        let now = Instant::now();
        if !self.ui_style_gate.borrow().due(now, min_interval) {
            return;
        }
        self.ui_style_gate.borrow_mut().record(now);
        let Ok(snapshot) = self.engine.settings() else {
            return;
        };
        if snapshot.error.is_some() {
            return;
        }
        let (style, notes) = CandidateStyle::from_items(&snapshot.items);
        self.ui_style_loaded.set(true);
        if *self.ui_style_notes.borrow() != notes {
            for note in &notes {
                crate::tsf_log::log("ui", &format!("style note: {note}"));
            }
            *self.ui_style_notes.borrow_mut() = notes;
        }
        if style == self.ui_style.get() {
            return;
        }
        crate::tsf_log::log(
            "ui",
            &format!("style changed theme={}", style.theme.as_str()),
        );
        self.ui_style.set(style);
        self.candidate_window.set_style(style);
        self.toast.set_style(style);
        self.mode_popup.set_style(style);
        render::set_normal_underline(style.underline);
    }

    // ---- 言語バー ----

    /// 言語バーのボタンの表示を描き直させる
    fn refresh_lang_bar(&self) {
        if let Some(bar) = self.lang_bar.borrow().as_ref() {
            bar.update();
        }
    }

    /// 設定 `windows.indicator_icon`（言語バーのアイコンの配色）をコアから読む。読めなければ `None`
    pub(crate) fn fetch_indicator_style(&self) -> Option<nagi_windows::indicator::IndicatorStyle> {
        let snapshot = self.engine.settings().ok()?;
        Some(nagi_windows::indicator::IndicatorStyle::from_items(
            &snapshot.items,
        ))
    }

    /// コアと合意している現在のモード（ボタンの表示用）
    pub(crate) fn lang_bar_mode(&self) -> Option<InputMode> {
        self.mode_sync.borrow().known()
    }

    /// モードを切り替える。他のクライアントやトレイと同じく、コンパートメントへ書き、
    /// その変更通知（`on_compartment_changed`）経由でコアへ送る
    fn request_mode(&self, mode: InputMode) {
        if self.lang_bar_mode() == Some(mode) {
            return;
        }
        // 書き込みに伴う変更通知を、利用者の操作として扱う
        self.user_mode_request.set(true);
        if let Some(c) = self.compartments.borrow().as_ref() {
            let _ = c.write(compartment_values(mode));
        }
        self.user_mode_request.set(false);
    }

    /// ボタンの左クリック（ひらがな ⇄ 英数）
    pub(crate) fn toggle_mode_from_lang_bar(&self) {
        let current = self.lang_bar_mode().unwrap_or(InputMode::Hiragana);
        self.request_mode(nagi_windows::status::toggled(current));
    }

    /// メニューの各「開く」項目の対象（存在しないものは `None`。作成はしない）
    fn menu_targets(&self, status: Option<&StatusResult>) -> MenuTargets {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let env = PathEnv {
            app_data: var("APPDATA"),
            local_app_data: var("LOCALAPPDATA"),
            module_dir: dll::module_directory(),
            staged_ready_json: read_staged_ready(var("LOCALAPPDATA")),
            current_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        };
        nagi_windows::status::resolve_targets(
            nagi_windows::status::settings_path(status),
            &env,
            |p| p.exists(),
        )
    }

    /// メニューの内容。状態の取得は UI スレッドで最大 250ms 待つ（メニューを開いたときだけ）
    pub(crate) fn lang_bar_menu(&self) -> Vec<MenuEntry> {
        let status = self.engine.status().ok();
        let targets = self.menu_targets(status.as_ref());
        nagi_windows::status::menu_entries(self.lang_bar_mode(), status.as_ref(), &targets)
    }

    /// メニューの項目が選ばれた
    pub(crate) fn run_menu_command(&self, command: MenuCommand) {
        if let MenuCommand::SetMode(mode) = command {
            self.request_mode(mode);
            return;
        }
        let status = self.engine.status().ok();
        let targets = self.menu_targets(status.as_ref());
        let kind = command.kind();
        let Some(path) = targets.target(command) else {
            // 灰色のはずだが、メニューを開いてから対象が消えた場合
            crate::tsf_log::log("menu", &format!("open skipped kind={kind} (no target)"));
            return;
        };
        match command {
            MenuCommand::SetMode(_) => {}
            MenuCommand::OpenSettingsGui => langbar::run_settings_gui(kind, path),
            MenuCommand::OpenSettings => langbar::open_in_notepad(kind, path),
            MenuCommand::OpenSettingsFolder
            | MenuCommand::OpenUserDictFolder
            | MenuCommand::OpenLogFolder => langbar::open_folder(kind, path),
            MenuCommand::RunVerify => langbar::run_verify(kind, path),
            MenuCommand::ApplyUpdate => langbar::run_apply_update(kind, path),
            MenuCommand::CheckUpdate => langbar::run_check_update(kind, path),
        }
    }

    fn show_candidates(&self, view: Option<&CandidatesView>) {
        // 設定がまだ読めていなければ（エンジンの起動待ちなど）、表示のたびに試す
        if !self.ui_style_loaded.get() {
            self.refresh_ui_style(SHOW_REFRESH_INTERVAL);
        }
        let page = view.and_then(CandidatePage::from_view);
        let data = view.and_then(CandidateListData::from_view);
        let (Some(page), Some(data)) = (page, data) else {
            self.hide_candidates();
            return;
        };
        // UILess: アプリが自前で描くと答えたら、自前の候補ウィンドウは出さない
        if !self.sync_ui_element(data, &page) {
            self.candidate_window.hide();
            return;
        }
        self.note_anchor();
        // 位置が未確定で再測定待ちなら、マウス位置に出してから動くのを避けて保留する。
        // ただし期限（`DEFER_TIMEOUT`）を過ぎたら、最後に測れた位置（無ければマウス位置）で出す
        let decision = self.anchor_wait.borrow_mut().on_show_request(
            monotonic_now(),
            self.caret.get(),
            self.remeasure_pending.get(),
            self.last_anchor.get(),
        );
        match decision {
            ShowDecision::Show(anchor) => self.candidate_window.show(&page, anchor),
            ShowDecision::Defer { wake_after } => {
                self.candidate_window.defer(&page);
                self.arm_defer_timer(wake_after);
            }
        }
    }

    /// 測れた基準位置を「最後の既知位置」として覚える
    fn note_anchor(&self) {
        if let Some(rect) = self.caret.get() {
            self.last_anchor.set(Some(rect));
        }
    }

    fn arm_defer_timer(&self, wake_after: std::time::Duration) {
        if let Some(window) = self.message_window.borrow().as_ref() {
            // 切り上げて、期限より早く鳴らさない
            let ms = wake_after.as_millis().saturating_add(1);
            window.arm_defer_timer(u32::try_from(ms).unwrap_or(u32::MAX));
        }
    }

    /// 表示保留のタイマーが鳴った。期限切れなら保留中の候補を最後の既知位置（無ければマウス位置）で出す
    fn on_defer_timeout(&self) {
        let outcome = self
            .anchor_wait
            .borrow_mut()
            .on_timer(monotonic_now(), self.last_anchor.get());
        match outcome {
            TimerOutcome::Idle => {}
            TimerOutcome::Rearm { wake_after } => self.arm_defer_timer(wake_after),
            TimerOutcome::Show(anchor) => {
                crate::tsf_log::log(
                    "cand",
                    &format!(
                        "defer timeout fallback={}",
                        if anchor.is_some() {
                            "last-known"
                        } else {
                            "mouse"
                        }
                    ),
                );
                self.candidate_window.reposition(anchor);
            }
        }
    }

    /// 候補を閉じる（自前のウィンドウと、UILess の要素）
    fn hide_candidates(&self) {
        self.anchor_wait.borrow_mut().finish();
        self.end_ui_element();
        self.candidate_window.hide();
    }

    // ---- UILess（ITfUIElement） ----

    /// 候補リスト要素を `BeginUIElement`（初回）または `UpdateUIElement`（2 回目以降）する。
    /// 戻り値は「自前の候補ウィンドウを出してよいか」。要素を渡せないホスト（`ITfUIElementMgr` が無い等）は
    /// 従来どおり出す
    fn sync_ui_element(&self, data: CandidateListData, page: &CandidatePage) -> bool {
        // COM 呼び出し（アプリが要素へ呼び返す）の間は `ui` を借りたままにしない
        let existing = self.ui.borrow_mut().as_mut().map(|u| {
            u.page = page.clone();
            (u.id, Rc::clone(&u.state), u.mgr.clone())
        });
        if let Some((id, state, mgr)) = existing {
            state.update(data);
            let _ = unsafe { mgr.UpdateUIElement(id) };
            return state.is_shown();
        }
        let Some(mgr) = self
            .thread_mgr
            .borrow()
            .as_ref()
            .and_then(|tm| tm.cast::<ITfUIElementMgr>().ok())
        else {
            return true;
        };
        let state = Rc::new(ElementState::new(data));
        let Ok(element) = CandidateListElement::create(Rc::clone(&state), Weak::clone(&self.me))
            .cast::<ITfUIElement>()
        else {
            return true;
        };
        let mut show = BOOL(1);
        let mut id = 0u32;
        if unsafe { mgr.BeginUIElement(&element, &mut show, &mut id) }.is_err() {
            return true;
        }
        let show = show.as_bool();
        state.set_shown(show);
        self.ui_less.set(!show);
        crate::tsf_log::log("ui", &format!("BeginUIElement show={show}"));
        *self.ui.borrow_mut() = Some(UiSession {
            mgr,
            id,
            state,
            page: page.clone(),
        });
        show
    }

    /// 要素を閉じる（`EndUIElement`）。無ければ何もしない
    fn end_ui_element(&self) {
        let taken = self.ui.borrow_mut().take();
        if let Some(session) = taken {
            let _ = unsafe { session.mgr.EndUIElement(session.id) };
        }
    }

    /// アプリが要素の `Show` を呼んだ。自前の候補ウィンドウの表示・非表示を合わせる
    pub(crate) fn on_ui_element_show(&self, visible: bool) {
        self.ui_less.set(!visible);
        let page = self.ui.borrow().as_ref().map(|u| u.page.clone());
        let Some(page) = page else {
            return;
        };
        if visible {
            self.candidate_window.show(&page, self.caret.get());
        } else {
            self.candidate_window.hide();
        }
    }

    /// フォーカス中の文書（`ITfCandidateListUIElement::GetDocumentMgr`）
    pub(crate) fn focused_document_mgr(&self) -> Option<ITfDocumentMgr> {
        let tm = self.thread_mgr.borrow().clone()?;
        unsafe { tm.GetFocus() }.ok()
    }

    /// 候補リストの選択を `index` へ動かす（`SetSelection`）。コアの次候補・前候補の操作を繰り返して
    /// 選択をそこへ移す（ロジックはコアが持つ）。1 ステップごとの編集を反映し、最後に UI を更新する
    pub(crate) fn ui_set_selection(&self, index: u32) -> Result<()> {
        let current = self
            .ui
            .borrow()
            .as_ref()
            .map(|u| u.state.selection())
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let (action, steps) = if index >= current {
            (TouchAction::NextCandidate, index - current)
        } else {
            (TouchAction::PrevCandidate, current - index)
        };
        let context = self.focused_context();
        let mut last = None;
        for _ in 0..steps {
            let result = self
                .engine
                .perform_action(&self.session_id, action.clone())
                .map_err(|_| windows::core::Error::from(E_FAIL))?;
            if let Some(ctx) = &context {
                let _ = self.apply(ctx, render::plan_from_result(&result), Timing::Deferred);
            }
            let reached = result
                .candidates
                .as_ref()
                .is_some_and(|c| c.selected == index);
            last = Some(result);
            if reached {
                break;
            }
        }
        if let Some(result) = last {
            self.finish(&Followup::from_result(&result), ModeChangeCause::Automatic);
        }
        Ok(())
    }

    /// 候補の確定（`Finalize`）。コアの確定操作を送る
    pub(crate) fn ui_finalize(&self) -> Result<()> {
        self.ui_action(TouchAction::Commit)
    }

    /// 候補の取消（`Abort`）。コアの取消操作を送る
    pub(crate) fn ui_abort(&self) -> Result<()> {
        self.ui_action(TouchAction::Cancel)
    }

    fn ui_action(&self, action: TouchAction) -> Result<()> {
        let result = self
            .engine
            .perform_action(&self.session_id, action)
            .map_err(|_| windows::core::Error::from(E_FAIL))?;
        if let Some(ctx) = self.focused_context() {
            let _ = self.apply(&ctx, render::plan_from_result(&result), Timing::Deferred);
        }
        self.finish(&Followup::from_result(&result), ModeChangeCause::Automatic);
        Ok(())
    }

    // ---- 候補・通知 ----

    fn show_toast(&self, message: &str) {
        if !self.focused.get() {
            return;
        }
        let allowed = toasts()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .should_show(message, Instant::now());
        if allowed {
            self.toast.show(message);
        }
    }

    /// 編集を反映した後の UI 更新（モード・候補・再変換の要求）
    /// `mode_cause` は応答のモード変化の原因（キー操作の応答は利用者の操作、それ以外は自動）
    fn finish(&self, followup: &Followup, mode_cause: ModeChangeCause) {
        if let Some(mode) = followup.mode {
            self.apply_core_mode(mode, mode_cause);
        }
        if let Some(view) = &followup.candidates {
            self.show_candidates(Some(view));
        }
        // 入力中でなくなったら候補は出さない
        if !self.is_composing() {
            self.hide_candidates();
        }
    }

    // ---- 再変換 ----

    /// エンジンに再変換を頼む。待ちきれなければ状態を戻して通知する
    fn reconvert_text(&self, text: &str) -> Option<ProcessKeyResult> {
        match self
            .engine
            .reconvert(&self.session_id, text, RECONVERT_TIMEOUT)
        {
            Ok(result) => Some(result),
            Err(_) => {
                // コアは遅れて変換中になりうるので取り消す（要求は順に処理される）
                let _ = self.engine.reset(&self.session_id);
                self.show_toast(RECONVERT_SLOW);
                None
            }
        }
    }

    /// 範囲を再変換する（`range` が `None` なら現在の選択）
    fn reconvert(
        &self,
        context: &ITfContext,
        range: Option<ITfRange>,
        timing: Timing,
    ) -> Result<()> {
        let env = self
            .env()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let (convert, applied) = (self.me.clone(), self.me.clone());
        edit::run_reconvert(
            context,
            self.client_id.get(),
            &env,
            range,
            timing,
            move |text| convert.upgrade()?.reconvert_text(text),
            move |result| {
                if let Some(core) = applied.upgrade() {
                    core.finish(&Followup::from_result(result), ModeChangeCause::Automatic);
                }
            },
        )
    }

    /// 範囲の再変換候補（`ITfFnReconversion::GetReconversion`）。範囲全体を置き換える文字列の一覧。
    /// 範囲の文字列は同期の編集セッションで読む（アプリが文書ロックを持つ最中に呼ばれたら失敗する）。
    /// コアの `reconvert` で変換中になった状態は、候補を作ったらすぐ `reset` で戻す
    /// （候補を選ぶかどうかはアプリ側で、確定は `replace_range` が行う）
    pub(crate) fn reconversion_candidates(&self, range: &ITfRange) -> Result<Vec<String>> {
        let fail = || windows::core::Error::from(E_FAIL);
        // 入力中は再変換しない（`reconvert` と同じ）
        if self.is_composing() {
            return Err(fail());
        }
        let context = self.focused_context().ok_or_else(fail)?;
        let text = edit::read_range_text(&context, self.client_id.get(), range).ok_or_else(fail)?;
        let text = nagi_windows::reconvert::eligible_text(&text).ok_or_else(fail)?;
        let result = self.reconvert_text(text).ok_or_else(fail)?;
        let candidates = nagi_windows::reconvert::range_candidates(&result);
        let _ = self.engine.reset(&self.session_id);
        if candidates.is_empty() {
            Err(fail())
        } else {
            Ok(candidates)
        }
    }

    /// 再変換の候補が選ばれた。範囲の文字列を置き換える
    pub(crate) fn replace_range(&self, range: ITfRange, text: String) -> Result<()> {
        let context = self
            .focused_context()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        edit::replace_range_text(
            &context,
            self.client_id.get(),
            range,
            text,
            Timing::Deferred,
        )
    }

    /// アプリや OS からの再変換（`ITfFnReconversion::Reconvert`）
    pub(crate) fn reconvert_range(&self, range: ITfRange) -> Result<()> {
        let context = self
            .focused_context()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        self.reconvert(&context, Some(range), Timing::Deferred)
    }

    // ---- キー ----

    /// キー処理の前に文脈（キャレット直前のテキスト・入力欄の種別・アプリ ID）をコアへ送る
    fn report_context(&self, context: &ITfContext) {
        let Some(info) = edit::read_surroundings(context, self.client_id.get()) else {
            return;
        };
        // 機密欄かどうかが変わったときだけ記録する（周辺テキストは書かない）
        if SECURE_INPUT_LOGGED.swap(info.secure, Ordering::Relaxed) != info.secure {
            crate::tsf_log::log(
                "ctx",
                &nagi_windows::diag::describe_secure_input(
                    info.secure,
                    crate::context_info::app_id(),
                ),
            );
        }
        let params = build_context(
            &self.session_id,
            info.secure,
            crate::context_info::app_id(),
            info.text,
        );
        let _ = self.engine.set_context(&params);
    }

    /// キーをエンジンに送り、食うか・何を反映するかを決める。
    /// エンジンが答えられない（未接続・タイムアウト）ときは、composition 中は食い（入力中の文字列を
    /// 壊さない）、そうでなければ素通しする
    fn evaluate(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
        is_up: bool,
    ) -> KeyOutcome {
        let passthrough = KeyOutcome {
            eaten: false,
            plan: render::plan_from_result(&ProcessKeyResult::default()),
            followup: Followup::default(),
        };
        let raw = crate::keys::raw_key_input(wparam, lparam, is_up);
        let Some(key) = keynorm::normalize(&raw) else {
            return passthrough;
        };
        // フォーカスの通知を取りこぼした場合の保険（起動済みなら何もしない）。起動が済むまでのキーは素通しになる
        self.engine.ensure_started();
        // 入力の開始になりうるキー（文字を生むキー）だけ、文脈を送る。毎キーで文書を読まないため
        if let Some(ctx) = context
            && !key.text.is_empty()
            && should_report_before_key(
                self.mode_sync
                    .borrow()
                    .known()
                    .unwrap_or(InputMode::Hiragana),
                !self.is_composing(),
                key.kind == KeyKind::FlagsChanged,
            )
        {
            self.report_context(ctx);
        }
        let started = std::time::Instant::now();
        match self.engine.process_key(&self.session_id, &key) {
            Ok(result) => {
                crate::e2e_probe::record_event(
                    "engine_result",
                    &format!(
                        "n={} consumed={} has_preedit={} has_commit={}",
                        crate::e2e_probe::bump_engine_results(),
                        result.consumed,
                        result.preedit.is_some(),
                        result.commit.is_some()
                    ),
                );
                self.outcome_from(&result)
            }
            Err(e) => {
                crate::e2e_probe::record_event("engine_error", &format!("{e:?}"));
                log_key_error(&e, self.is_composing(), started.elapsed());
                KeyOutcome {
                    eaten: self.is_composing(),
                    ..passthrough
                }
            }
        }
    }

    fn outcome_from(&self, result: &ProcessKeyResult) -> KeyOutcome {
        let plan = render::plan_from_result(result);
        // 反映すべき編集があるのに「食わない」と返すと、TSF が OnKeyDown を呼ばず編集が失われる
        let eaten = result.consumed || !plan.is_empty();
        KeyOutcome {
            eaten,
            plan,
            followup: Followup::from_result(result),
        }
    }

    fn apply(&self, context: &ITfContext, plan: EditPlan, timing: Timing) -> Result<()> {
        if plan.is_empty() {
            return Ok(());
        }
        let env = self
            .env()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        edit::run_plan(context, self.client_id.get(), &env, plan, timing)
    }

    /// フォーカス中のコンテキスト
    fn focused_context(&self) -> Option<ITfContext> {
        let tm = self.thread_mgr.borrow().clone()?;
        let dm: ITfDocumentMgr = unsafe { tm.GetFocus() }.ok()?;
        unsafe { dm.GetTop() }.ok()
    }

    /// このセッション宛ての非同期イベント（別スレッドで受けてキューに積んだもの）を反映する。
    /// UI スレッドで呼ぶ。キーイベント内では同期（`Sync`）、隠しウィンドウ経由では文書ロック次第（`Deferred`）
    fn pump_events(&self, context: Option<&ITfContext>, timing: Timing) {
        for event in self.engine.drain_events(&self.session_id) {
            if let Some(message) = toast_for(&event) {
                self.show_toast(&message);
            }
            match event {
                EngineEvent::UpdatePreedit(p) => {
                    if let Some(ctx) = context {
                        let _ = self.apply(ctx, render::plan_from_preedit(&p.preedit), timing);
                    }
                }
                EngineEvent::UpdateCandidates(c) => self.show_candidates(Some(&c.candidates)),
                EngineEvent::Restored { result, .. } => {
                    if let Some(ctx) = context {
                        let _ = self.apply(ctx, render::plan_from_result(&result), timing);
                    }
                    self.finish(&Followup::from_result(&result), ModeChangeCause::Automatic);
                }
                EngineEvent::RestoreFailed { .. } => {
                    // 表示中の文字列をそのまま確定する（設計書 §3.3「入力を失わない」）
                    if let (Some(ctx), Some(env)) = (context, self.env()) {
                        let _ = edit::run_end_keeping_text(ctx, self.client_id.get(), &env, timing);
                    }
                    self.hide_candidates();
                }
                EngineEvent::Started { .. } => self.on_engine_started(),
                // 通知は上の `toast_for` で表示済み。それ以外は表示するものが無い
                _ => {}
            }
        }
    }

    fn key_down(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
        test_only: bool,
    ) -> bool {
        let id = render::key_id((wparam.0 & 0xFFFF) as u16, lparam.0);
        if test_only {
            let outcome = self.evaluate(context, wparam, lparam, false);
            let eaten = outcome.eaten;
            crate::e2e_probe::record_event(
                "test_key_down",
                &format!(
                    "vk={} lparam=0x{:X} id=0x{id:X} eaten={eaten} test_n={}",
                    wparam.0 & 0xFFFF,
                    lparam.0,
                    crate::e2e_probe::bump_test_evals()
                ),
            );
            self.handoff.borrow_mut().store(id, outcome);
            return eaten;
        }
        self.pump_events(context, Timing::Sync);
        let cached = self.handoff.borrow_mut().take(id);
        let handoff_hit = cached.is_some();
        let outcome = cached.unwrap_or_else(|| self.evaluate(context, wparam, lparam, false));
        if let Some(ctx) = context
            && self.apply(ctx, outcome.plan, Timing::Sync).is_err()
        {
            // 編集に失敗したらエンジン側の状態と食い違う。次の入力で揃うようリセットする
            let _ = self.engine.reset(&self.session_id);
        }
        crate::e2e_probe::record_event(
            "key_down",
            &format!(
                "vk={} lparam=0x{:X} id=0x{id:X} handoff={} eaten={}",
                wparam.0 & 0xFFFF,
                lparam.0,
                if handoff_hit { "hit" } else { "miss" },
                outcome.eaten
            ),
        );
        self.finish(&outcome.followup, ModeChangeCause::User);
        if outcome.followup.reconvert
            && let Some(ctx) = context
        {
            let _ = self.reconvert(ctx, None, Timing::Sync);
        }
        outcome.eaten
    }

    fn key_up(&self, wparam: WPARAM, lparam: LPARAM) -> bool {
        // 修飾キーの離上は flags_changed としてエンジンに届ける（macOS と同じ）。キー離上は食わない
        // TODO: 押下を食ったキーの離上も食うか（アプリへ離上だけが届く問題）を実機で確認する
        let raw = crate::keys::raw_key_input(wparam, lparam, true);
        if let Some(key) = keynorm::normalize(&raw) {
            let _ = self.engine.process_key(&self.session_id, &key);
        }
        false
    }

    /// アプリ単位の前景の変化（`ITfKeyEventSink::OnSetFocus`）
    fn set_focus(&self, foreground: bool) {
        let plan = self.focus.borrow_mut().app_foreground(foreground);
        self.run_focus_plan(plan, || self.focused_context());
    }

    /// 文書のフォーカスの変化（`ITfThreadMgrEventSink::OnSetFocus`）。旧文書の確定は旧文書の context に反映する
    fn document_focus_changed(&self, new: Option<&ITfDocumentMgr>, prev: Option<&ITfDocumentMgr>) {
        let changed = match (new, prev) {
            (Some(a), Some(b)) => !same_object(a, b),
            _ => false,
        };
        crate::tsf_log::log(
            "focus",
            &format!(
                "doc focus new={} prev={} changed={changed}",
                new.is_some(),
                prev.is_some()
            ),
        );
        let plan = self
            .focus
            .borrow_mut()
            .document_focus(new.is_some(), changed);
        self.run_focus_plan(plan, || prev.and_then(|d| unsafe { d.GetTop() }.ok()));
    }

    /// `out` → `into` の順に送る。`out_context` は旧文書の context（composition の文書が分かればそちらを優先）
    fn run_focus_plan(&self, plan: FocusPlan, out_context: impl FnOnce() -> Option<ITfContext>) {
        if plan.out {
            self.leave_focus(out_context());
        }
        if plan.into {
            self.enter_focus();
        }
    }

    fn enter_focus(&self) {
        self.focused.set(true);
        // アプリ別の位置の補正（手編集されるので、フォーカスのたびに読み直す）
        let offset = load_window_offset();
        self.candidate_window.set_offset(offset);
        self.mode_popup.set_offset(offset);
        self.ui_less.set(false);
        crate::e2e_probe::record_event("set_focus", "foreground=true");
        // 入力する可能性のあるホストになったので、ここでエンジンを起動する（先読み。H-96）
        self.engine.ensure_started();
        let _ = self.engine.focus_in(&self.session_id);
        // 設定画面で変えた見た目の反映（フォーカスを得るたびに、間引いて読み直す）
        self.refresh_ui_style(FOCUS_REFRESH_INTERVAL);
    }

    /// フォーカス喪失をエンジンに伝え、確定文字列で composition を閉じる。`fallback` は composition の
    /// 文書が分からないときの反映先
    fn leave_focus(&self, fallback: Option<ITfContext>) {
        self.focused.set(false);
        self.ui_less.set(false);
        self.mode_popup.hide();
        crate::e2e_probe::record_event("set_focus", "foreground=false");
        let context = self
            .text_edit
            .borrow()
            .as_ref()
            .map(|a| a.context.clone())
            .or(fallback);
        self.flush_composition(context.as_ref(), Timing::Sync);
    }

    /// エンジンの `focus_out`（確定・学習の flush）を送り、結果を `context` に反映して composition を閉じる
    fn flush_composition(&self, context: Option<&ITfContext>, timing: Timing) {
        self.handoff.borrow_mut().clear();
        self.hide_candidates();
        // 前の矩形・再測定待ちを持ち越さない
        self.last_rect.set(None);
        self.remeasure_pending.set(false);
        self.last_anchor.set(None);
        let mut apply_failed = false;
        if let Ok(result) = self.engine.focus_out(&self.session_id) {
            let plan = render::plan_from_result(&result);
            if let Some(ctx) = context {
                apply_failed = self.apply(ctx, plan, timing).is_err();
            }
        }
        if !self.is_composing() {
            return;
        }
        // 確定文字列が無い・反映できなかった。表示中の文字列をそのまま確定して閉じる（入力を失わない）
        if (apply_failed || timing == Timing::Sync)
            && let (Some(ctx), Some(env)) = (context, self.env())
        {
            let _ = edit::run_end_keeping_text(ctx, self.client_id.get(), &env, timing);
        }
        if timing == Timing::Sync {
            // 文書が無い・閉じられなかった。解放済みの composition を持ち越さない
            *self.composition.borrow_mut() = None;
            self.release_edit_sink();
        }
    }

    // ---- ITfTextEditSink（キャレットの composition 範囲外への移動） ----

    /// composition の有無に合わせて `ITfTextEditSink` の購読を付け外しする（編集セッションの後に呼ぶ）
    fn sync_edit_sink(&self, context: &ITfContext) {
        let composing = self.is_composing();
        let current = self.text_edit.borrow().as_ref().map(|a| a.context.clone());
        match (composing, current) {
            (false, Some(_)) => self.release_edit_sink(),
            (true, Some(old)) if same_object(&old, context) => {}
            (true, old) => {
                if old.is_some() {
                    self.release_edit_sink();
                }
                self.advise_edit_sink(context);
            }
            (false, None) => {}
        }
    }

    fn advise_edit_sink(&self, context: &ITfContext) {
        let Ok(source) = context.cast::<ITfSource>() else {
            return;
        };
        let sink: ITfTextEditSink = TextEditSink {
            core: Weak::clone(&self.me),
        }
        .into();
        match unsafe { source.AdviseSink(&ITfTextEditSink::IID, &sink) } {
            Ok(cookie) => {
                crate::tsf_log::log("edit", "text edit sink advised");
                *self.text_edit.borrow_mut() = Some(TextEditAdvise {
                    context: context.clone(),
                    source,
                    cookie,
                });
            }
            Err(e) => log_failure("AdviseSink(TextEditSink)", &e),
        }
    }

    fn release_edit_sink(&self) {
        let taken = self.text_edit.borrow_mut().take();
        if let Some(advise) = taken {
            let _ = unsafe { advise.source.UnadviseSink(advise.cookie) };
            crate::tsf_log::log("edit", "text edit sink unadvised");
        }
    }

    /// 破棄される context なら購読と composition の参照を外す
    fn forget_context(&self, context: &ITfContext) {
        let matches = self
            .text_edit
            .borrow()
            .as_ref()
            .is_some_and(|a| same_object(&a.context, context));
        if matches {
            self.release_edit_sink();
            *self.composition.borrow_mut() = None;
        }
    }

    /// 破棄される文書（の top context）なら `forget_context` する
    fn forget_document(&self, document: &ITfDocumentMgr) {
        if let Ok(top) = unsafe { document.GetTop() } {
            self.forget_context(&top);
        }
    }

    /// 編集の終了通知。自分以外の編集で選択が composition の外へ出たら確定して閉じる（MS-IME と同じ）。
    /// 判定に迷う場合（読み取り失敗・選択が変わっていない等）は何もしない
    fn on_end_edit(&self, context: &ITfContext, ec: u32, record: &ITfEditRecord) {
        if edit::in_own_edit() {
            return;
        }
        let Some(composition) = self.composition.borrow().clone() else {
            return;
        };
        if !unsafe { record.GetSelectionStatus() }.is_ok_and(|b| b.as_bool()) {
            return;
        }
        let (Ok(range), Ok(selection)) = (
            unsafe { composition.GetRange() },
            edit::selection_range(context, ec),
        ) else {
            return;
        };
        let (Ok(start), Ok(end)) = (
            unsafe { selection.CompareStart(ec, &range, TF_ANCHOR_START) },
            unsafe { selection.CompareEnd(ec, &range, TF_ANCHOR_END) },
        ) else {
            return;
        };
        if !selection_outside_composition(start.cmp(&0), end.cmp(&0)) {
            return;
        }
        crate::tsf_log::log("edit", "caret left composition; committing");
        // 文書ロック中なので反映は後回し（`Deferred`）。エンジンは続けて入力できるよう focus_in し直す
        self.flush_composition(Some(context), Timing::Deferred);
        let _ = self.engine.focus_in(&self.session_id);
    }
}

fn ctx_ref<'a>(pic: &'a Ref<'_, ITfContext>) -> Option<&'a ITfContext> {
    pic.as_ref()
}

/// 入力方式本体（`ITfTextInputProcessorEx` + 表示属性の提供者）
#[implement(ITfTextInputProcessorEx, ITfDisplayAttributeProvider, ITfFnConfigure)]
pub struct TextService {
    /// `Activate` から `Deactivate` の間だけ存在する
    core: RefCell<Option<Rc<Core>>>,
}

impl TextService {
    /// 新しいインスタンス（`IClassFactory::CreateInstance` から呼ぶ）
    pub(crate) fn new() -> Self {
        dll::add_object();
        Self {
            core: RefCell::new(None),
        }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        dll::release_object();
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                let core = self.core.borrow_mut().take();
                if let Some(core) = core {
                    core.deactivate();
                }
                Ok(())
            },
        )
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, _dwflags: u32) -> Result<()> {
        guarded(
            || {
                crate::tsf_log::log("tsf", "ActivateEx panicked");
                Err(E_FAIL.into())
            },
            || {
                let tm = ptim.ok()?.clone();
                // 二重の Activate に備えて、前の状態を閉じる
                if let Some(old) = self.core.borrow_mut().take() {
                    old.deactivate();
                }
                let core = Rc::new_cyclic(|me| Core::new(me.clone()));
                *self.core.borrow_mut() = Some(Rc::clone(&core));
                core.activate(tm, tid)
                    .inspect_err(|e| log_failure("Activate", e))
            },
        )
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(display_attr::enumerate())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if guid.is_null() {
            return Err(E_POINTER.into());
        }
        display_attr::info_for(unsafe { &*guid })
    }
}

/// 「言語の設定 → Nagi → オプション」。TSF は `CoCreateInstance` で TIP を作って `ITfFnConfigure` を
/// 問い合わせるので、`Activate` 前（`core` が無い状態）でも動くこと
impl ITfFunction_Impl for TextService_Impl {
    fn GetDisplayName(&self) -> Result<BSTR> {
        Ok(BSTR::from(format!(
            "{} の設定",
            crate::guids::PROFILE_DESCRIPTION
        )))
    }
}

impl ITfFnConfigure_Impl for TextService_Impl {
    fn Show(&self, _hwndparent: HWND, _langid: u16, _rguidprofile: *const GUID) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                crate::tsf_log::log("menu", "configure requested");
                if langbar::open_settings_gui_from_module("configure") {
                    Ok(())
                } else {
                    crate::tsf_log::log("menu", "configure failed (no settings exe)");
                    Err(E_FAIL.into())
                }
            },
        )
    }
}

/// 文書（入力欄・タブ）の切り替え通知。`Core` は弱い参照
#[implement(ITfThreadMgrEventSink)]
struct ThreadMgrSink {
    core: Weak<Core>,
}

impl ITfThreadMgrEventSink_Impl for ThreadMgrSink_Impl {
    fn OnInitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                // 破棄される文書の context を保持していたら参照を外す（解放済みの context に書かない）
                if let (Some(core), Some(dim)) = (self.core.upgrade(), pdim.as_ref()) {
                    core.forget_document(dim);
                }
                Ok(())
            },
        )
    }

    fn OnSetFocus(
        &self,
        pdimfocus: Ref<ITfDocumentMgr>,
        pdimprevfocus: Ref<ITfDocumentMgr>,
    ) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                if let Some(core) = self.core.upgrade() {
                    core.document_focus_changed(pdimfocus.as_ref(), pdimprevfocus.as_ref());
                }
                Ok(())
            },
        )
    }

    fn OnPushContext(&self, _pic: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }

    fn OnPopContext(&self, pic: Ref<ITfContext>) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                if let (Some(core), Some(ctx)) = (self.core.upgrade(), pic.as_ref()) {
                    core.forget_context(ctx);
                }
                Ok(())
            },
        )
    }
}

/// composition 中の文書の編集通知。`Core` は弱い参照
#[implement(ITfTextEditSink)]
struct TextEditSink {
    core: Weak<Core>,
}

impl ITfTextEditSink_Impl for TextEditSink_Impl {
    fn OnEndEdit(
        &self,
        pic: Ref<ITfContext>,
        ecreadonly: u32,
        peditrecord: Ref<ITfEditRecord>,
    ) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                if let (Some(core), Some(ctx), Some(record)) =
                    (self.core.upgrade(), pic.as_ref(), peditrecord.as_ref())
                {
                    core.on_end_edit(ctx, ecreadonly, record);
                }
                Ok(())
            },
        )
    }
}

#[implement(ITfKeyEventSink)]
struct KeyEventSink {
    core: Rc<Core>,
}

impl ITfKeyEventSink_Impl for KeyEventSink_Impl {
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                self.core.set_focus(fforeground.as_bool());
                Ok(())
            },
        )
    }

    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(
            || Ok(BOOL(0)),
            || {
                Ok(self
                    .core
                    .key_down(ctx_ref(&pic), wparam, lparam, true)
                    .into())
            },
        )
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(
            || Ok(BOOL(0)),
            || {
                Ok(self
                    .core
                    .key_down(ctx_ref(&pic), wparam, lparam, false)
                    .into())
            },
        )
    }

    fn OnKeyUp(&self, _pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(
            || Ok(BOOL(0)),
            || Ok(self.core.key_up(wparam, lparam).into()),
        )
    }

    fn OnPreservedKey(&self, _pic: Ref<ITfContext>, rguid: *const GUID) -> Result<BOOL> {
        // TODO: 半角/全角などを ITfKeystrokeMgr::PreserveKey で登録する場合に使う。いまは使わない
        if rguid.is_null() {
            return Err(E_INVALIDARG.into());
        }
        Ok(BOOL(0))
    }
}

#[implement(ITfCompositionSink)]
struct CompositionSink {
    core: Rc<Core>,
}

impl ITfCompositionSink_Impl for CompositionSink_Impl {
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _pcomposition: Ref<ITfComposition>,
    ) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                // アプリ側が composition を終了した（フォーカス移動・クリック等）。エンジンの状態も揃える
                *self.core.composition.borrow_mut() = None;
                self.core.release_edit_sink();
                // E2E プローブ: composition が（アプリ側の都合で）空になったことを記録する
                crate::e2e_probe::record(Some(""), "");
                self.core.handoff.borrow_mut().clear();
                self.core.hide_candidates();
                let _ = self.core.engine.reset(&self.core.session_id);
                Ok(())
            },
        )
    }
}

/// モードのコンパートメントの変更通知。`Core` は弱い参照（TSF が保持するため循環させない）
#[implement(ITfCompartmentEventSink)]
struct CompartmentSink {
    core: Weak<Core>,
}

impl ITfCompartmentEventSink_Impl for CompartmentSink_Impl {
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                if rguid.is_null() {
                    return Ok(());
                }
                let guid = unsafe { *rguid };
                if mode_comp::WATCHED.contains(&guid)
                    && let Some(core) = self.core.upgrade()
                {
                    core.on_compartment_changed();
                }
                Ok(())
            },
        )
    }
}

/// キー応答の失敗を診断ログに出す。1 秒あたり最大 1 行（入力内容・キーの種類は書かない）
fn log_key_error(
    error: &nagi_windows::client::ClientError,
    composing: bool,
    elapsed: std::time::Duration,
) {
    thread_local! {
        static THROTTLE: RefCell<nagi_windows::diag::LogThrottle> = RefCell::default();
        static START: std::time::Instant = std::time::Instant::now();
    }
    let now = START.with(|s| s.elapsed());
    let allowed = THROTTLE.with(|t| t.borrow_mut().allow(now, std::time::Duration::from_secs(1)));
    if allowed {
        crate::tsf_log::log(
            "key",
            &nagi_windows::diag::format_key_error(error, composing, elapsed),
        );
    }
}
