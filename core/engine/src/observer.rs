//! 確定の通知先（学習など）。
//!
//! エンジンは「変換を経て確定した文節」と「候補の学習削除の要求」を [`CommitObserver`] に渡す。
//! 学習の有無・secure input・アプリごとの停止判断は observer 側が行う（エンジンは文脈を渡すだけ）。

use crate::ir::SegmentKind;

/// 確定・削除が起きたセッションの状況
#[derive(Debug, Clone, Copy)]
pub struct CommitContext<'a> {
    /// セッション ID
    pub session_id: &'a str,
    /// フロントが報告したアプリケーション識別子（`set_context`）
    pub app_id: Option<&'a str>,
    /// secure input（パスワード入力など）が有効か。有効なら学習してはならない
    pub secure_input: bool,
}

/// 確定した 1 文節
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedSegment {
    /// 読み（ひらがな。Literal では入力した文字）
    pub reading: String,
    /// 確定した表記
    pub surface: String,
    /// 種別
    pub kind: SegmentKind,
    /// この文節の左文脈（Surrounding Text + 前の文節の表記の末尾。変換時に backend へ渡したものと同じ）
    pub left_context: String,
    /// 直前に確定した語（表記。変換を経た日本語の文節。前の文節、または直前の確定の最後の語）。
    /// 無ければ空。次語予測のバイグラム（直前の語 → この語）の学習に使う。
    /// 確定直後の次語候補をそのまま選んだときは `reading` が空で通知する
    pub prev_word: String,
}

/// 確定の通知先
pub trait CommitObserver: Send + Sync {
    /// 変換（Converting）を経て確定した文節列を受け取る。ひらがなのまま確定した入力は通知しない
    fn on_commit(&self, ctx: &CommitContext<'_>, segments: &[CommittedSegment]);

    /// 候補ウィンドウでの学習削除（`Ctrl+Delete`）の要求。`reading` に対する `surface` の学習を
    /// 忘れさせ、削除できたら `true` を返す
    fn on_forget(&self, ctx: &CommitContext<'_>, reading: &str, surface: &str) -> bool;

    /// 曖昧入力の「もしかして」候補（読み `typed` を `corrected` に直した候補）が、候補ウィンドウ・
    /// 入力中の候補として利用者に提示された。同じ入力エピソード（確定または入力の破棄まで）の中で
    /// 同じ `(typed, corrected)` は 1 回しか通知しない。既定は何もしない
    fn on_fuzzy_shown(&self, ctx: &CommitContext<'_>, typed: &str, corrected: &str) {
        let _ = (ctx, typed, corrected);
    }

    /// 確定した内容を短時間だけメモリ上に持つ機能（確定後の遡及再評価）を使ってよい状況か。
    /// 学習を止める状況（secure input・停止アプリなど）では `false` にする。既定は `true`
    fn allows_retro_review(&self, ctx: &CommitContext<'_>) -> bool {
        let _ = ctx;
        true
    }

    /// 提示された「もしかして」候補（`typed` → `corrected`）が選ばれて確定された。既定は何もしない
    fn on_fuzzy_adopted(&self, ctx: &CommitContext<'_>, typed: &str, corrected: &str) {
        let _ = (ctx, typed, corrected);
    }
}
