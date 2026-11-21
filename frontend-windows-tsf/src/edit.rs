//! `ITfEditSession` による文書の編集。`render::EditPlan` を実行するだけで、判断はしない。
//!
//! 編集セッションの中身は `FnOnce(ec)` のクロージャで渡す。キーイベントのコールバック内では同期
//! （`TF_ES_SYNC`）、非同期通知の反映など文書ロックの外では `TF_ES_ASYNCDONTCARE`（ロックが空いていれば
//! 即時、塞がっていれば後で実行）。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use nagi_protocol::ProcessKeyResult;
use nagi_windows::candidates::{AnchorSource, Rect, choose_anchor_or_previous, needs_remeasure};
use nagi_windows::preedit_style::AttrRun;
use nagi_windows::reconvert::{RECONVERT_READ_LIMIT, eligible_text};
use windows::Win32::Foundation::{E_FAIL, FALSE, RECT};
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
    ITfEditSession_Impl, ITfInsertAtSelection, ITfRange, TF_AE_NONE, TF_ANCHOR_END,
    TF_ANCHOR_START, TF_CONTEXT_EDIT_CONTEXT_FLAGS, TF_DEFAULT_SELECTION, TF_ES_ASYNC,
    TF_ES_ASYNCDONTCARE, TF_ES_READ, TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY, TF_SELECTION,
    TF_SELECTIONSTYLE,
};
use windows::core::{BOOL, Interface, Result, implement};

use crate::context_info::{self, Surroundings};
use crate::display_attr::{self, AttrAtoms};
use crate::render::{EditPlan, PreeditUpdate, plan_from_result, to_utf16};
use crate::win::to_rect;

/// 編集セッションをまたいで持つ composition（UI スレッドの内側でのみ触る）
pub type SharedComposition = Rc<RefCell<Option<ITfComposition>>>;

/// 編集に必要な共有状態
#[derive(Clone)]
pub struct Env {
    /// 現在の composition
    pub composition: SharedComposition,
    /// composition の終了通知先
    pub sink: ITfCompositionSink,
    /// 表示属性の GUID 識別子（登録に失敗したら `None`＝装飾なし）
    pub atoms: Option<AttrAtoms>,
    /// 直近に測った composition（なければキャレット）の矩形（スクリーン座標）。候補ウィンドウの基準
    pub caret: Rc<Cell<Option<Rect>>>,
    /// TSF のクライアント ID（再測定の編集セッションを要求するのに使う）
    pub client_id: u32,
    /// 同じ composition で直前に測れた矩形。composition の開始・終了とフォーカスの変更で捨てる
    pub last_rect: Rc<Cell<Option<Rect>>>,
    /// 編集セッション完了後の再測定が予定されている間は true（最初の候補表示を遅らせる判定に使う）
    pub remeasure_pending: Rc<Cell<bool>>,
    /// 再測定で `caret` を更新した後の呼び出し（候補ウィンドウの位置の更新）
    pub on_remeasured: Rc<dyn Fn()>,
    /// 編集計画の反映後（composition の開始・終了があり得る）の呼び出し。`ITfTextEditSink` の購読の同期に使う
    pub on_composition_change: Rc<dyn Fn(&ITfContext)>,
}

/// 実行の仕方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// 同期（キーイベントのコールバック内）
    Sync,
    /// 文書ロックの空き次第（即時か後で）
    Deferred,
    /// 現在の編集セッションの完了後（`TF_ES_ASYNC`。再測定用）
    Async,
}

type Op = Box<dyn FnOnce(u32) -> Result<()>>;

thread_local! {
    /// 自分の編集セッションを実行・要求している間の深さ。`ITfTextEditSink::OnEndEdit` が
    /// 「自分以外の変更」かを見分けるのに使う
    static OWN_EDIT_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// `OWN_EDIT_DEPTH` を上げ、Drop で戻す
struct OwnEditGuard;

impl OwnEditGuard {
    fn enter() -> Self {
        OWN_EDIT_DEPTH.with(|d| d.set(d.get() + 1));
        Self
    }
}

impl Drop for OwnEditGuard {
    fn drop(&mut self) {
        OWN_EDIT_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// 自分の編集セッションの実行中（要求の呼び出し中を含む）か
pub fn in_own_edit() -> bool {
    OWN_EDIT_DEPTH.with(|d| d.get() > 0)
}

#[implement(ITfEditSession)]
struct EditSession {
    op: RefCell<Option<Op>>,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        let op = self.op.borrow_mut().take();
        let _own = OwnEditGuard::enter();
        match op {
            // パニックを COM の境界（TSF の呼び出し元）から出さない
            Some(op) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(ec)))
                .unwrap_or_else(|_| Err(E_FAIL.into())),
            None => Ok(()),
        }
    }
}

/// 編集セッションを要求する。戻り値は同期なら `DoEditSession` の結果、`Deferred` が後で実行されるなら `Ok`
pub fn run(
    context: &ITfContext,
    client_id: u32,
    timing: Timing,
    write: bool,
    op: impl FnOnce(u32) -> Result<()> + 'static,
) -> Result<()> {
    let session: ITfEditSession = EditSession {
        op: RefCell::new(Some(Box::new(op))),
    }
    .into();
    let access = if write { TF_ES_READWRITE } else { TF_ES_READ };
    let flags = TF_CONTEXT_EDIT_CONTEXT_FLAGS(
        access.0
            | match timing {
                Timing::Sync => TF_ES_SYNC.0,
                Timing::Deferred => TF_ES_ASYNCDONTCARE.0,
                Timing::Async => TF_ES_ASYNC.0,
            },
    );
    // 同期の編集セッションの `OnEndEdit` は要求の呼び出し中に届く
    let _own = OwnEditGuard::enter();
    let hr = unsafe { context.RequestEditSession(client_id, &session, flags) }?;
    // 延期された場合は成功コード（TF_S_ASYNC）。失敗コードだけをエラーにする
    hr.ok()
}

/// 計画を反映する
pub fn run_plan(
    context: &ITfContext,
    client_id: u32,
    env: &Env,
    plan: EditPlan,
    timing: Timing,
) -> Result<()> {
    let (ctx, env) = (context.clone(), env.clone());
    run(context, client_id, timing, true, move |ec| {
        apply_plan(&ctx, ec, &env, &plan, None)
    })
}

/// composition をテキストを残したまま終了する（復元失敗時の「表示中の文字列の確定」）
pub fn run_end_keeping_text(
    context: &ITfContext,
    client_id: u32,
    env: &Env,
    timing: Timing,
) -> Result<()> {
    let (ctx, env) = (context.clone(), env.clone());
    run(context, client_id, timing, true, move |ec| {
        let taken = env.composition.borrow_mut().take();
        if let Some(c) = taken {
            if let Ok(range) = unsafe { c.GetRange() } {
                let _ = display_attr::clear(&ctx, ec, &range);
            }
            let ended = unsafe { c.EndComposition(ec) };
            (env.on_composition_change)(&ctx);
            ended?;
            crate::e2e_probe::record(Some(""), "");
        }
        Ok(())
    })
}

/// キャレット直前のテキストと入力欄の種別を読む（同期・読み取り）。失敗は `None`
pub fn read_surroundings(context: &ITfContext, client_id: u32) -> Option<Surroundings> {
    let out: Rc<RefCell<Option<Surroundings>>> = Rc::default();
    let slot = Rc::clone(&out);
    let ctx = context.clone();
    run(context, client_id, Timing::Sync, false, move |ec| {
        *slot.borrow_mut() = Some(context_info::read_surroundings(&ctx, ec));
        Ok(())
    })
    .ok()?;
    out.borrow_mut().take()
}

/// 確定済みテキストの再変換。`range` が `None` なら現在の選択を対象にする（空の選択なら何もしない）。
/// `convert` は対象のテキストを受けてエンジンへ `reconvert` を送り、応答（preedit）を返す。
/// 応答の preedit で対象の範囲を置き換え、composition にする
pub fn run_reconvert(
    context: &ITfContext,
    client_id: u32,
    env: &Env,
    range: Option<ITfRange>,
    timing: Timing,
    convert: impl FnOnce(&str) -> Option<ProcessKeyResult> + 'static,
    applied: impl FnOnce(&ProcessKeyResult) + 'static,
) -> Result<()> {
    let (ctx, env) = (context.clone(), env.clone());
    run(context, client_id, timing, true, move |ec| {
        // 入力中の文字列があるときは再変換しない（コアも変換中は別の状態）
        if env.composition.borrow().is_some() {
            return Ok(());
        }
        let target = match range {
            Some(r) => r,
            None => selection_range(&ctx, ec)?,
        };
        if unsafe { target.IsEmpty(ec) }?.as_bool() {
            return Ok(());
        }
        let text = range_text(&target, ec)?;
        let Some(text) = eligible_text(&text) else {
            return Ok(());
        };
        let Some(result) = convert(text) else {
            return Ok(());
        };
        apply_plan(&ctx, ec, &env, &plan_from_result(&result), Some(&target))?;
        applied(&result);
        Ok(())
    })
}

/// 範囲の文字列を読む（長すぎる範囲は先頭の `RECONVERT_READ_LIMIT` UTF-16 まで。判定は呼び出し側）
fn range_text(range: &ITfRange, ec: u32) -> Result<String> {
    let mut buf = [0u16; RECONVERT_READ_LIMIT];
    let mut fetched = 0u32;
    unsafe { range.GetText(ec, 0, &mut buf, &mut fetched) }?;
    Ok(String::from_utf16_lossy(
        &buf[..(fetched as usize).min(buf.len())],
    ))
}

/// 範囲の文字列を読む（同期・読み取り。`ITfFnReconversion::GetReconversion` 用）。
/// 文書ロックを他が持っているなどで読めなければ `None`
pub fn read_range_text(context: &ITfContext, client_id: u32, range: &ITfRange) -> Option<String> {
    let out: Rc<RefCell<Option<String>>> = Rc::default();
    let slot = Rc::clone(&out);
    let range = range.clone();
    run(context, client_id, Timing::Sync, false, move |ec| {
        *slot.borrow_mut() = Some(range_text(&range, ec)?);
        Ok(())
    })
    .ok()?;
    out.borrow_mut().take()
}

/// 範囲の文字列を `text` に置き換え、キャレットを末尾に置く（再変換の候補の確定）
pub fn replace_range_text(
    context: &ITfContext,
    client_id: u32,
    range: ITfRange,
    text: String,
    timing: Timing,
) -> Result<()> {
    let ctx = context.clone();
    run(context, client_id, timing, true, move |ec| {
        unsafe { range.SetText(ec, 0, &to_utf16(&text)) }?;
        set_caret_at_end(&ctx, ec, &range)
    })
}

/// キャレットを `range` の末尾に置く
fn set_caret_at_end(context: &ITfContext, ec: u32, range: &ITfRange) -> Result<()> {
    let caret = unsafe { range.Clone() }?;
    unsafe { caret.Collapse(ec, TF_ANCHOR_END) }?;
    set_selection(context, ec, caret)
}

fn set_selection(context: &ITfContext, ec: u32, range: ITfRange) -> Result<()> {
    let selection = TF_SELECTION {
        range: std::mem::ManuallyDrop::new(Some(range)),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: FALSE,
        },
    };
    unsafe { context.SetSelection(ec, &[selection]) }
}

/// 現在の選択範囲（なければキャレット位置）
pub fn selection_range(context: &ITfContext, ec: u32) -> Result<ITfRange> {
    let mut selection = [TF_SELECTION::default()];
    let mut fetched = 0u32;
    unsafe { context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched) }?;
    let [first] = selection;
    // 所有権を取り戻して Drop させる
    std::mem::ManuallyDrop::into_inner(first.range)
        .ok_or_else(|| windows::core::Error::from(E_FAIL))
}

/// 範囲の画面上の矩形（`GetTextExt`）。失敗したら `None`
fn text_ext(context: &ITfContext, ec: u32, range: &ITfRange) -> Option<Rect> {
    let view = unsafe { context.GetActiveView() }.ok()?;
    let mut rc = RECT::default();
    let mut clipped = BOOL::default();
    unsafe { view.GetTextExt(ec, range, &mut rc, &mut clipped) }.ok()?;
    Some(to_rect(rc))
}

/// 現在の選択範囲（キャレット）の画面上の矩形を測り、`done` に渡す（読み取り専用の編集セッション）。
/// 測れなければ `None`。セッションを要求できなかったときは `Err`（`done` は呼ばれない）
pub fn measure_selection_rect(
    context: &ITfContext,
    client_id: u32,
    timing: Timing,
    done: impl FnOnce(Option<Rect>) + 'static,
) -> Result<()> {
    let ctx = context.clone();
    run(context, client_id, timing, false, move |ec| {
        let rect = selection_range(&ctx, ec)
            .ok()
            .and_then(|r| text_ext(&ctx, ec, &r));
        done(rect);
        Ok(())
    })
}

/// 候補ウィンドウの基準を測って `env.caret` に入れ、由来を返す。composition があれば入力部分の左端
/// （先頭の点範囲 → composition 全体）、なければ（または測れなければ）選択範囲（キャレット）、
/// それも測れなければ同じ composition で直前に測れた矩形。どれも無ければ `None`（マウス位置になる）
fn measure_caret(context: &ITfContext, ec: u32, env: &Env) -> AnchorSource {
    let current = env.composition.borrow().clone();
    let composing = current.is_some();
    let (mut start, mut range) = (None, None);
    if let Some(c) = current
        && let Ok(whole) = unsafe { c.GetRange() }
    {
        // 先頭の点範囲: 複数行でも先頭が含まれる行の下端が取れる
        start = unsafe { whole.Clone() }
            .and_then(|p| {
                unsafe { p.Collapse(ec, TF_ANCHOR_START) }?;
                Ok(p)
            })
            .ok()
            .and_then(|p| text_ext(context, ec, &p));
        range = text_ext(context, ec, &whole);
    }
    let caret = if start.is_some() || range.is_some() {
        // 先頭か全体が測れていれば、選択範囲は後回し（測定を省く）
        None
    } else {
        selection_range(context, ec)
            .ok()
            .and_then(|r| text_ext(context, ec, &r))
    };
    // 直前の矩形は composition の間だけ有効
    let previous = if composing { env.last_rect.get() } else { None };
    let chosen = choose_anchor_or_previous(start, range, caret, previous);
    match chosen {
        Some((AnchorSource::Previous, _)) | None => {}
        Some((_, rect)) if composing => env.last_rect.set(Some(rect)),
        Some(_) => {}
    }
    if !composing {
        env.last_rect.set(None);
    }
    env.caret.set(chosen.map(|(_, rect)| rect));
    chosen.map_or(AnchorSource::Mouse, |(source, _)| source)
}

/// 最初の測定がマウス位置になったとき、編集セッションの完了後（読み取り専用・非同期）に測り直す。
/// 測り直しも `Mouse` なら上限（`MAX_REMEASURE`）まで繰り返す。
/// 測り終えたら `caret` を更新して `on_remeasured`（候補ウィンドウの位置更新）を呼ぶ
fn request_remeasure(context: &ITfContext, env: &Env, attempt: u32) {
    env.remeasure_pending.set(true);
    let (ctx, env2) = (context.clone(), env.clone());
    let result = run(context, env.client_id, Timing::Async, false, move |ec| {
        let source = measure_caret(&ctx, ec, &env2);
        if attempt == 0 {
            crate::tsf_log::log(
                "cand",
                &format!("anchor remeasure source={}", source.as_str()),
            );
        }
        let composing = env2.composition.borrow().is_some();
        if needs_remeasure(source, composing, attempt + 1) {
            request_remeasure(&ctx, &env2, attempt + 1);
        } else {
            env2.remeasure_pending.set(false);
            (env2.on_remeasured)();
        }
        Ok(())
    });
    if result.is_err() {
        env.remeasure_pending.set(false);
        (env.on_remeasured)();
    }
}

/// 基準の由来が変わったときだけ診断ログに出す（矩形の値は書かない）
fn log_anchor_source(source: AnchorSource) {
    thread_local! {
        static LAST: Cell<Option<AnchorSource>> = const { Cell::new(None) };
    }
    if LAST.with(|l| l.replace(Some(source))) != Some(source) {
        crate::tsf_log::log("cand", &format!("anchor source={}", source.as_str()));
    }
}

/// 編集計画を反映する。順序は「直前の確定の削除 → 確定 → preedit の更新」。
/// `start_at` は composition を始める範囲（再変換で選択範囲を置き換えるときに指定する）
fn apply_plan(
    context: &ITfContext,
    ec: u32,
    env: &Env,
    plan: &EditPlan,
    start_at: Option<&ITfRange>,
) -> Result<()> {
    let result = apply_plan_inner(context, ec, env, plan, start_at);
    // 失敗しても composition の有無は変わり得る。購読を実態に合わせる
    (env.on_composition_change)(context);
    result
}

fn apply_plan_inner(
    context: &ITfContext,
    ec: u32,
    env: &Env,
    plan: &EditPlan,
    start_at: Option<&ITfRange>,
) -> Result<()> {
    let was_composing = env.composition.borrow().is_some();
    if let Some(n) = plan.delete_before {
        delete_before_caret(context, ec, n)?;
    }
    if let Some(text) = &plan.commit {
        commit(context, ec, &env.composition, text)?;
    }
    match &plan.preedit {
        PreeditUpdate::Keep => {}
        PreeditUpdate::Clear => clear_preedit(ec, &env.composition)?,
        PreeditUpdate::Set {
            text,
            cursor,
            attrs,
        } => set_preedit(context, ec, env, text, *cursor, attrs, start_at)?,
    }
    record_probe(plan);
    // 新しい composition には前の矩形を持ち越さない
    if !was_composing {
        env.last_rect.set(None);
    }
    let source = measure_caret(context, ec, env);
    log_anchor_source(source);
    if needs_remeasure(source, env.composition.borrow().is_some(), 0) {
        // 開始直後などレイアウトが未反映で測れない。セッション完了後に測り直す
        request_remeasure(context, env, 0);
    } else {
        env.remeasure_pending.set(false);
    }
    Ok(())
}

/// E2E 用プローブへ反映後の状態を出す（`NAGI_E2E_PROBE` 未設定なら何もしない。`e2e_probe` 参照）
fn record_probe(plan: &EditPlan) {
    let committed = plan.commit.as_deref().unwrap_or("");
    let marked = match &plan.preedit {
        PreeditUpdate::Set { text, .. } => Some(text.as_str()),
        PreeditUpdate::Clear => Some(""),
        // 確定は composition を終えるので、preedit が続かなければ未確定は空になる
        PreeditUpdate::Keep => (!committed.is_empty()).then_some(""),
    };
    if marked.is_some() || !committed.is_empty() {
        crate::e2e_probe::record(marked, committed);
    }
}

/// 確定文字列を入れる。composition があればその範囲を置き換えて終了し、無ければ選択位置に挿入する
fn commit(
    context: &ITfContext,
    ec: u32,
    composition: &SharedComposition,
    text: &str,
) -> Result<()> {
    let utf16 = to_utf16(text);
    let current = composition.borrow_mut().take();
    let range = match current {
        Some(c) => {
            let range = unsafe { c.GetRange() }?;
            unsafe { range.SetText(ec, 0, &utf16) }?;
            // 確定したテキストに入力中の下線を残さない
            let _ = display_attr::clear(context, ec, &range);
            set_caret_at_end(context, ec, &range)?;
            unsafe { c.EndComposition(ec) }?;
            range
        }
        None => {
            let insert: ITfInsertAtSelection = context.cast()?;
            let range = unsafe { insert.InsertTextAtSelection(ec, Default::default(), &utf16) }?;
            set_caret_at_end(context, ec, &range)?;
            range
        }
    };
    drop(range);
    Ok(())
}

fn clear_preedit(ec: u32, composition: &SharedComposition) -> Result<()> {
    let taken = composition.borrow_mut().take();
    if let Some(c) = taken {
        let range = unsafe { c.GetRange() }?;
        unsafe { range.SetText(ec, 0, &[]) }?;
        unsafe { c.EndComposition(ec) }?;
    }
    Ok(())
}

/// preedit を composition の範囲に書き、表示属性とキャレット（`cursor`、UTF-16）を設定する。
/// composition が無ければ開始する
fn set_preedit(
    context: &ITfContext,
    ec: u32,
    env: &Env,
    text: &str,
    cursor: u32,
    attrs: &[AttrRun],
    start_at: Option<&ITfRange>,
) -> Result<()> {
    let existing = env.composition.borrow().clone();
    let comp = match existing {
        Some(c) => c,
        None => {
            let at = match start_at {
                Some(r) => r.clone(),
                None => {
                    let insert: ITfInsertAtSelection = context.cast()?;
                    // 空の範囲を選択位置に得る（文書は変更しない）
                    unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[]) }?
                }
            };
            let ctx_comp: ITfContextComposition = context.cast()?;
            let c = unsafe { ctx_comp.StartComposition(ec, &at, &env.sink) }?;
            *env.composition.borrow_mut() = Some(c.clone());
            c
        }
    };
    let range = unsafe { comp.GetRange() }?;
    unsafe { range.SetText(ec, 0, &to_utf16(text)) }?;

    // 表示属性は失敗しても入力は続ける（装飾が無くなるだけ）
    if let Some(atoms) = &env.atoms {
        let _ = display_attr::apply(context, ec, &range, attrs, atoms);
    }

    // キャレット: 範囲の先頭に潰してから cursor 単位だけ終端を進め、終端に潰す
    let caret = unsafe { range.Clone() }?;
    unsafe { caret.Collapse(ec, TF_ANCHOR_START) }?;
    let mut moved = 0i32;
    let shift = i32::try_from(cursor).unwrap_or(i32::MAX);
    unsafe { caret.ShiftEnd(ec, shift, &mut moved, std::ptr::null()) }?;
    unsafe { caret.Collapse(ec, TF_ANCHOR_END) }?;
    set_selection(context, ec, caret)
}

/// キャレット直前の `n`（UTF-16）を削除する（確定取消）。composition 中は何もしない
fn delete_before_caret(context: &ITfContext, ec: u32, n: u32) -> Result<()> {
    let range = selection_range(context, ec)?;
    let target = unsafe { range.Clone() }?;
    unsafe { target.Collapse(ec, TF_ANCHOR_START) }?;
    let mut moved = 0i32;
    let back = -i32::try_from(n).unwrap_or(i32::MAX);
    unsafe { target.ShiftStart(ec, back, &mut moved, std::ptr::null()) }?;
    unsafe { target.SetText(ec, 0, &[]) }
}
