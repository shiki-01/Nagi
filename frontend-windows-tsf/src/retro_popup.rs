//! 確定後の見直し（`retro_review`）の採用: 確定済みテキストの置換（TSF の編集セッション）。
//!
//! 提案の状態と文言は `nagi_windows::retro`、通知の表示は `toast_window::ToastWindow::show_above`。
//! ここは「入力中の文字列（composition）の直前にある、確定済みの `len`（UTF-16）を、コアが返す文字列に置き換える」
//! 処理だけを行う。判断（採用するか）はコア（`accept_retro_review`）とフロントの状態機械が決める。
//!
//! 安全のため、置換の前に文書の実際の文字列が通知の `original_surface` と一致することを確かめる
//! （一致しなければ何もしない）。置換で composition の範囲に変化が出たら、入力を失わないよう
//! composition を文字列を残したまま終える（呼び出し側がエンジンをリセットする）。

#![allow(unsafe_code)]

use std::cell::Cell;
use std::rc::Rc;

use nagi_windows::reconvert::RECONVERT_READ_LIMIT;
use windows::Win32::UI::TextServices::{ITfContext, TF_ANCHOR_START};

use crate::display_attr;
use crate::edit::{self, Env, Timing};
use crate::render::to_utf16;

/// 置換の結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceOutcome {
    /// 置換した
    Replaced,
    /// 置換したが、composition の範囲が変わったため、入力中の文字列を残したまま composition を終えた
    /// （呼び出し側はエンジンをリセットすること）
    ReplacedAndFlushed,
    /// 文書の文字列が通知と一致しない・範囲が取れない（何も変えていない。コアにも採用を伝えていない）
    Mismatch,
    /// コアが採用を受け付けなかった（すでに無効になった）。何も変えていない
    Stale,
    /// 編集セッションを実行できなかった・編集に失敗した
    Failed,
}

impl ReplaceOutcome {
    /// ログの語（`[retro] replace-failed reason=…` の理由）。成功なら `None`
    pub fn failure_reason(self) -> Option<&'static str> {
        match self {
            Self::Replaced | Self::ReplacedAndFlushed => None,
            Self::Mismatch => Some("mismatch"),
            Self::Stale => Some("stale"),
            Self::Failed => Some("edit"),
        }
    }
}

/// 確定済みの `len`（UTF-16）が `original` であることを確かめ、`accept` が返した文字列に置き換える。
/// `accept` は文書の確認が済んだ後に 1 回だけ呼ばれ、`(置換する長さ, 文字列)` を返す（`None` なら採用されなかった）。
/// 同期の編集セッションで行う（キーイベントの中で呼ぶこと）
pub fn run_replace(
    context: &ITfContext,
    client_id: u32,
    env: &Env,
    original: String,
    len: u32,
    accept: impl FnOnce() -> Option<(u32, String)> + 'static,
) -> ReplaceOutcome {
    let slot: Rc<Cell<Option<ReplaceOutcome>>> = Rc::default();
    let (ctx, env2, out) = (context.clone(), env.clone(), Rc::clone(&slot));
    let requested = edit::run(context, client_id, Timing::Sync, true, move |ec| {
        out.set(Some(replace_in_session(
            &ctx, ec, &env2, &original, len, accept,
        )));
        Ok(())
    });
    match requested {
        Ok(()) => slot.take().unwrap_or(ReplaceOutcome::Failed),
        Err(_) => ReplaceOutcome::Failed,
    }
}

fn replace_in_session(
    context: &ITfContext,
    ec: u32,
    env: &Env,
    original: &str,
    len: u32,
    accept: impl FnOnce() -> Option<(u32, String)>,
) -> ReplaceOutcome {
    let n = len as usize;
    if n == 0 || n >= RECONVERT_READ_LIMIT {
        return ReplaceOutcome::Mismatch;
    }
    // 入力中ならその先頭、そうでなければ選択位置（キャレット）の直前が、確定済みの文字列の末尾
    let composition = env.composition.borrow().clone();
    let base = match &composition {
        Some(c) => unsafe { c.GetRange() },
        None => edit::selection_range(context, ec),
    };
    let Ok(base) = base else {
        return ReplaceOutcome::Failed;
    };
    let Ok(target) = (unsafe { base.Clone() }) else {
        return ReplaceOutcome::Failed;
    };
    let back = -(len as i32);
    let mut moved = 0i32;
    if unsafe { target.Collapse(ec, TF_ANCHOR_START) }.is_err()
        || unsafe { target.ShiftStart(ec, back, &mut moved, std::ptr::null()) }.is_err()
        || moved != back
    {
        return ReplaceOutcome::Mismatch;
    }
    // 実際の文書の文字列が、通知の確定文字列と同じときだけ置き換える
    match edit::range_text(&target, ec) {
        Ok(text) if text == original => {}
        _ => return ReplaceOutcome::Mismatch,
    }
    let before = composition
        .as_ref()
        .and_then(|c| unsafe { c.GetRange() }.ok())
        .and_then(|r| edit::range_text(&r, ec).ok());

    let Some((replace_len, text)) = accept() else {
        return ReplaceOutcome::Stale;
    };
    if replace_len != len || text.is_empty() {
        return ReplaceOutcome::Stale;
    }
    if unsafe { target.SetText(ec, 0, &to_utf16(&text)) }.is_err() {
        return ReplaceOutcome::Failed;
    }

    // composition の範囲が置換の影響を受けていないか（範囲の先頭に挿入された文字列が composition に
    // 取り込まれると、次の preedit 更新で置換した文字列が消えてしまう）
    if let (Some(c), Some(before)) = (&composition, before) {
        let after = unsafe { c.GetRange() }
            .ok()
            .and_then(|r| edit::range_text(&r, ec).ok());
        if after.as_deref() != Some(before.as_str()) {
            let taken = env.composition.borrow_mut().take();
            if let Some(c) = taken {
                if let Ok(range) = unsafe { c.GetRange() } {
                    let _ = display_attr::clear(context, ec, &range);
                }
                let _ = unsafe { c.EndComposition(ec) };
            }
            (env.on_composition_change)(context);
            return ReplaceOutcome::ReplacedAndFlushed;
        }
    }
    // 置換で文字列の幅が変わるので、候補ウィンドウの基準を測り直す
    let _ = edit::measure_caret(context, ec, env);
    ReplaceOutcome::Replaced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_reasons() {
        assert_eq!(ReplaceOutcome::Replaced.failure_reason(), None);
        assert_eq!(ReplaceOutcome::ReplacedAndFlushed.failure_reason(), None);
        assert_eq!(ReplaceOutcome::Mismatch.failure_reason(), Some("mismatch"));
        assert_eq!(ReplaceOutcome::Stale.failure_reason(), Some("stale"));
        assert_eq!(ReplaceOutcome::Failed.failure_reason(), Some("edit"));
    }
}
