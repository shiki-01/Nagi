//! UILess モードの候補リスト要素（`ITfUIElement` / `ITfCandidateListUIElement` /
//! `ITfCandidateListUIElementBehavior`）。
//!
//! 全画面ゲームなどのアプリが候補を自前で描くための TSF の仕組み。`BeginUIElement` で要素を渡し、
//! アプリが `show = FALSE` と答えたら自前の候補ウィンドウは出さない。ここは COM の入れ物で、
//! 値は `nagi_windows::ui_element`（OS 非依存）、選択・確定・取消は `Core` 経由でエンジンへ送る
//! （新しいロジックはフロントに持たない）。

#![allow(unsafe_code)]

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use nagi_windows::ui_element::{CLUIE_ALL, CandidateListData, page_index_reply};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER};
use windows::Win32::UI::TextServices::{
    ITfCandidateListUIElement_Impl, ITfCandidateListUIElementBehavior,
    ITfCandidateListUIElementBehavior_Impl, ITfDocumentMgr, ITfUIElement_Impl,
};
use windows::core::{BOOL, BSTR, GUID, Result, implement};

use crate::guids::GUID_NAGI_CANDIDATE_LIST_ELEMENT;
use crate::text_service::{Core, guarded};

/// 要素の状態。`Core`（更新する側）と COM オブジェクト（アプリから問い合わされる側）で共有する
pub(crate) struct ElementState {
    data: RefCell<CandidateListData>,
    /// 直近の更新で変わった項目（`TF_CLUIE_*`）
    flags: Cell<u32>,
    /// アプリが表示してよいと答えた（または `Show(TRUE)` した）か
    shown: Cell<bool>,
}

impl ElementState {
    pub(crate) fn new(data: CandidateListData) -> Self {
        Self {
            data: RefCell::new(data),
            flags: Cell::new(CLUIE_ALL),
            shown: Cell::new(true),
        }
    }

    /// 内容を置き換え、変わった項目を記録する（`UpdateUIElement` の前に呼ぶ）
    pub(crate) fn update(&self, next: CandidateListData) {
        let flags = self.data.borrow_mut().replace(next);
        self.flags.set(flags);
    }

    pub(crate) fn selection(&self) -> u32 {
        self.data.borrow().selection()
    }

    pub(crate) fn is_shown(&self) -> bool {
        self.shown.get()
    }

    pub(crate) fn set_shown(&self, shown: bool) {
        self.shown.set(shown);
    }
}

/// 候補リスト要素
#[implement(ITfCandidateListUIElementBehavior)]
pub(crate) struct CandidateListElement {
    state: Rc<ElementState>,
    core: Weak<Core>,
}

impl CandidateListElement {
    pub(crate) fn create(
        state: Rc<ElementState>,
        core: Weak<Core>,
    ) -> ITfCandidateListUIElementBehavior {
        Self { state, core }.into()
    }

    fn core(&self) -> Result<Rc<Core>> {
        self.core.upgrade().ok_or_else(|| E_FAIL.into())
    }
}

impl ITfUIElement_Impl for CandidateListElement_Impl {
    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from("Nagi candidate list"))
    }

    fn GetGUID(&self) -> Result<GUID> {
        Ok(GUID_NAGI_CANDIDATE_LIST_ELEMENT)
    }

    fn Show(&self, bshow: BOOL) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                let show = bshow.as_bool();
                self.state.set_shown(show);
                // 自前の候補ウィンドウの表示・非表示を合わせる
                if let Some(core) = self.core.upgrade() {
                    core.on_ui_element_show(show);
                }
                Ok(())
            },
        )
    }

    fn IsShown(&self) -> Result<BOOL> {
        Ok(self.state.is_shown().into())
    }
}

impl ITfCandidateListUIElement_Impl for CandidateListElement_Impl {
    fn GetUpdatedFlags(&self) -> Result<u32> {
        Ok(self.state.flags.get())
    }

    fn GetDocumentMgr(&self) -> Result<ITfDocumentMgr> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                self.core()?
                    .focused_document_mgr()
                    .ok_or_else(|| E_FAIL.into())
            },
        )
    }

    fn GetCount(&self) -> Result<u32> {
        Ok(self.state.data.borrow().count())
    }

    fn GetSelection(&self) -> Result<u32> {
        Ok(self.state.data.borrow().selection())
    }

    fn GetString(&self, uindex: u32) -> Result<BSTR> {
        self.state
            .data
            .borrow()
            .string(uindex)
            .map(BSTR::from)
            .ok_or_else(|| E_INVALIDARG.into())
    }

    fn GetPageIndex(&self, pindex: *mut u32, usize: u32, pupagecnt: *mut u32) -> Result<()> {
        if pupagecnt.is_null() {
            return Err(E_POINTER.into());
        }
        let starts = self.state.data.borrow().page_starts();
        // `pindex` が NULL なら個数だけを返す
        let capacity = (!pindex.is_null()).then_some(usize as usize);
        let (copied, total) = page_index_reply(&starts, capacity);
        if !pindex.is_null() {
            for (i, v) in copied.iter().enumerate() {
                unsafe { *pindex.add(i) = *v };
            }
        }
        unsafe { *pupagecnt = total };
        Ok(())
    }

    fn SetPageIndex(&self, pindex: *const u32, upagecnt: u32) -> Result<()> {
        if pindex.is_null() {
            return Err(E_POINTER.into());
        }
        let starts = unsafe { std::slice::from_raw_parts(pindex, upagecnt as usize) };
        if self.state.data.borrow_mut().set_page_starts(starts) {
            Ok(())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetCurrentPage(&self) -> Result<u32> {
        Ok(self.state.data.borrow().current_page())
    }
}

impl ITfCandidateListUIElementBehavior_Impl for CandidateListElement_Impl {
    fn SetSelection(&self, nindex: u32) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                let core = self.core()?;
                let count = self.state.data.borrow().count();
                if nindex >= count {
                    return Err(E_INVALIDARG.into());
                }
                core.ui_set_selection(nindex)
            },
        )
    }

    fn Finalize(&self) -> Result<()> {
        guarded(|| Err(E_FAIL.into()), || self.core()?.ui_finalize())
    }

    fn Abort(&self) -> Result<()> {
        guarded(|| Err(E_FAIL.into()), || self.core()?.ui_abort())
    }
}
