//! 確定後の遡及再評価（`retro_review` 通知。コアの `input.fuzzy.retro_review`）の提案の状態機械と文言（OS 非依存）。
//!
//! コアは、確定した直後の文節が打ち間違いだった可能性を、続きの入力（2 文字以上）と合わせて見直し、
//! 修正候補を `retro_review` 通知で届ける。確定済みの文字列の置換はフロントの仕事（`accept_retro_review` が
//! 置換する文字列を返す）。ここは「いつ提案を出し、いつ引っ込め、どのキーで採用するか」を決める。
//!
//! - 状態: `Idle` → `Offered` → `Accepted` / `Dismissed` / `Expired`（終端の 3 つは `Idle` と同じく次の提案を受けられる）
//! - 採用: 既定は Ctrl+Shift+Y（[`DEFAULT_ACCEPT_KEY`]。コアのキーマップのどの割当とも衝突しない）。
//!   提案が出ている間だけ食う（出ていなければ何もしない）
//! - 無視: Esc（食う。入力中の文字列は消えない）、または次のキー入力（修飾キーだけの変化は数えない）
//! - 破棄: 一定時間（[`RETRO_TTL`]）・フォーカス移動・入力の終了（確定・取消）・secure input・古い revision
//!
//! 時刻は呼び出し側が単調時計の `Duration` で渡す（テストしやすくするため）。

use std::time::Duration;

use nagi_protocol::key::keysym;
use nagi_protocol::methods::RetroReviewParams;
use nagi_protocol::{KeyEvent, KeyKind};

use crate::anchor_wait::{Anchors, Source};
use crate::candidates::Rect;
use crate::keynorm::keysym_for_char;

/// 提案を出しておく時間
pub const RETRO_TTL: Duration = Duration::from_secs(6);

/// 採用キーの既定（コアのキーマップに無い組み合わせ。Windows の標準操作とも衝突しにくい）
pub const DEFAULT_ACCEPT_KEY: &str = "Ctrl+Shift+Y";

/// 提案の文言の、各表記の最大文字数（超えたら末尾を「…」にする）
pub const LABEL_MAX_CHARS: usize = 10;

/// 提案 1 件（`retro_review` 通知のうち、表示と採用に使う部分）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetroOffer {
    /// 通知の revision
    pub revision: u32,
    /// 直前に確定した文字列（置換される側）
    pub original: String,
    /// 置換される長さ（UTF-16 コード単位）
    pub replace_len: u32,
    /// 採用候補の表記（先頭 = 最良）
    pub surface: String,
}

/// 状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetroState {
    /// 何も出していない
    Idle,
    /// 提案を出している
    Offered {
        /// 提案の内容
        offer: RetroOffer,
        /// 出した時刻
        since: Duration,
    },
    /// 採用した（置換は呼び出し側が行う）
    Accepted,
    /// 無視した（Esc・次のキー入力）
    Dismissed,
    /// 破棄した（時間切れ・フォーカス移動・入力の終了・古い revision）
    Expired,
}

/// 提案を受け付ける状況
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfferEnv {
    /// このセッションが前面でフォーカスを持っているか
    pub focused: bool,
    /// 入力中（composition あり）か。コアの通知は続きの入力があるときだけ出るので、無ければ古い通知
    pub composing: bool,
    /// アプリが候補を自前で描く（UILess）モードか。自前のポップアップは出さない
    pub ui_less: bool,
}

/// 受け付けなかった理由（ログ・テスト用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// フォーカスが無い・UILess・入力中でない
    NotApplicable,
    /// 修正候補が空・置換の長さが不正
    Invalid,
    /// 古い revision
    Stale,
}

/// キーに対する判断
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDecision {
    /// 提案とは無関係（通常どおりエンジンへ送る）
    PassThrough,
    /// 採用する（エンジンへは送らない）
    Accept,
    /// 無視する。`consume` なら食う（Esc）、そうでなければ通常どおりエンジンへ送る
    Dismiss {
        /// そのキーを食うか
        consume: bool,
    },
}

/// 採用キー
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptKey {
    /// 表記（文言に出す。`Ctrl+Shift+Y` 形式）
    label: String,
    /// 小文字化した keysym
    keysym: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

impl AcceptKey {
    /// `Ctrl+Shift+Y` 形式（修飾は Ctrl / Shift / Alt / Meta（Win）、大文字小文字は区別しない）から作る。
    /// 修飾キーが無い・知らない名前・空のキーは `None`
    pub fn parse(spec: &str) -> Option<Self> {
        let mut key = Self {
            label: String::new(),
            keysym: 0,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        let parts: Vec<&str> = spec.split('+').map(str::trim).collect();
        let (last, mods) = parts.split_last()?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => key.ctrl = true,
                "shift" => key.shift = true,
                "alt" => key.alt = true,
                "meta" | "win" | "super" => key.meta = true,
                _ => return None,
            }
        }
        // 単独の文字キーは、入力中の文字と区別できないので採用キーにしない
        if !(key.ctrl || key.alt || key.meta) {
            return None;
        }
        key.keysym = match last.to_ascii_lowercase().as_str() {
            "tab" => keysym::TAB,
            "space" => keysym::SPACE,
            "enter" | "return" => keysym::RETURN,
            s => {
                let mut chars = s.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                fold(keysym_for_char(c)?)
            }
        };
        key.label = parts.join("+");
        Some(key)
    }

    /// 文言に出す表記
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 既定の採用キー
    pub fn default_key() -> Self {
        Self::parse(DEFAULT_ACCEPT_KEY).expect("default accept key is valid")
    }

    /// 押下イベントがこのキーか（Caps Lock は無視する）
    pub fn matches(&self, key: &KeyEvent) -> bool {
        key.kind == KeyKind::Down
            && fold(key.keysym) == self.keysym
            && key.modifiers.ctrl == self.ctrl
            && key.modifiers.shift == self.shift
            && key.modifiers.alt == self.alt
            && key.modifiers.meta == self.meta
    }
}

/// 英大文字を小文字にそろえる（Shift・Caps Lock による大小の違いを無視する）
fn fold(sym: u32) -> u32 {
    match sym {
        0x41..=0x5A => sym + 0x20,
        _ => sym,
    }
}

/// 提案の状態機械
#[derive(Debug, Clone)]
pub struct RetroReview {
    state: RetroState,
    key: AcceptKey,
    /// これまでに受け付けた revision の最大（古い通知を捨てる）
    latest_revision: u32,
}

impl Default for RetroReview {
    fn default() -> Self {
        Self::new(AcceptKey::default_key())
    }
}

impl RetroReview {
    /// 採用キーを指定して作る
    pub fn new(key: AcceptKey) -> Self {
        Self {
            state: RetroState::Idle,
            key,
            latest_revision: 0,
        }
    }

    /// 現在の状態
    pub fn state(&self) -> &RetroState {
        &self.state
    }

    /// 提案を出している最中か
    pub fn is_offered(&self) -> bool {
        matches!(self.state, RetroState::Offered { .. })
    }

    /// 採用キー
    pub fn accept_key(&self) -> &AcceptKey {
        &self.key
    }

    /// 同期応答の revision を覚える（これより古い通知を捨てるため。小さい値では巻き戻さない）
    pub fn note_revision(&mut self, revision: u32) {
        self.latest_revision = self.latest_revision.max(revision);
    }

    /// revision の記録を捨てる（エンジンの再起動で revision が 0 から始まり直すため）。提案も破棄する
    pub fn reset_revision(&mut self) {
        self.latest_revision = 0;
        self.expire();
    }

    /// 通知を受ける。条件を満たせば `Offered` にする（出ていた提案は置き換える）
    pub fn offer(
        &mut self,
        params: &RetroReviewParams,
        env: OfferEnv,
        now: Duration,
    ) -> Result<(), RejectReason> {
        if !env.focused || !env.composing || env.ui_less {
            return Err(RejectReason::NotApplicable);
        }
        if params.revision < self.latest_revision {
            return Err(RejectReason::Stale);
        }
        let first = params.candidates.first();
        let valid = first.is_some_and(|c| !c.surface.is_empty())
            && !params.original_surface.is_empty()
            && params.replace_len_utf16 > 0
            && params.replace_len_utf16 as usize == utf16_len(&params.original_surface);
        let Some(first) = first.filter(|_| valid) else {
            return Err(RejectReason::Invalid);
        };
        self.latest_revision = params.revision;
        self.state = RetroState::Offered {
            offer: RetroOffer {
                revision: params.revision,
                original: params.original_surface.clone(),
                replace_len: params.replace_len_utf16,
                surface: first.surface.clone(),
            },
            since: now,
        };
        Ok(())
    }

    /// キーに対する判断（状態は変えない。`OnTestKeyDown` と `OnKeyDown` の両方から呼べる）。
    /// 時間切れの提案は無いものとして扱う
    pub fn peek_key(&self, key: &KeyEvent, now: Duration) -> KeyDecision {
        let RetroState::Offered { since, .. } = &self.state else {
            return KeyDecision::PassThrough;
        };
        if now.saturating_sub(*since) >= RETRO_TTL || key.kind != KeyKind::Down {
            // 修飾キーだけの変化は提案に影響しない（Ctrl+Shift+Y の途中で消えないように）
            return KeyDecision::PassThrough;
        }
        if self.key.matches(key) {
            return KeyDecision::Accept;
        }
        let m = key.modifiers;
        let plain = !(m.ctrl || m.alt || m.meta || m.shift);
        KeyDecision::Dismiss {
            consume: key.keysym == keysym::ESCAPE && plain,
        }
    }

    /// 採用する。提案が出ていれば `Accepted` にして内容を返す（置換は呼び出し側）
    pub fn accept(&mut self, now: Duration) -> Option<RetroOffer> {
        match std::mem::replace(&mut self.state, RetroState::Accepted) {
            RetroState::Offered { offer, since } if now.saturating_sub(since) < RETRO_TTL => {
                Some(offer)
            }
            RetroState::Offered { .. } => {
                self.state = RetroState::Expired;
                None
            }
            other => {
                self.state = other;
                None
            }
        }
    }

    /// 無視する。出ていたなら `true`
    pub fn dismiss(&mut self) -> bool {
        self.finish(RetroState::Dismissed)
    }

    /// 破棄する（フォーカス移動・入力の終了など）。出ていたなら `true`
    pub fn expire(&mut self) -> bool {
        self.finish(RetroState::Expired)
    }

    /// 時間切れなら `Expired` にして `true`
    pub fn tick(&mut self, now: Duration) -> bool {
        match &self.state {
            RetroState::Offered { since, .. } if now.saturating_sub(*since) >= RETRO_TTL => {
                self.state = RetroState::Expired;
                true
            }
            _ => false,
        }
    }

    fn finish(&mut self, next: RetroState) -> bool {
        if self.is_offered() {
            self.state = next;
            true
        } else {
            false
        }
    }

    /// 提案の文言。出していなければ `None`
    pub fn prompt(&self) -> Option<String> {
        match &self.state {
            RetroState::Offered { offer, .. } => Some(prompt_text(offer, self.key.label())),
            _ => None,
        }
    }
}

/// 提案の文言（例: `「有り難う」→「ありがとう」に直す: Ctrl+Shift+Y（無視: Esc）`）。
/// 長い表記は [`LABEL_MAX_CHARS`] 文字で省略する
pub fn prompt_text(offer: &RetroOffer, key_label: &str) -> String {
    format!(
        "「{}」→「{}」に直す: {key_label}（無視: Esc）",
        ellipsize(&offer.original, LABEL_MAX_CHARS),
        ellipsize(&offer.surface, LABEL_MAX_CHARS),
    )
}

/// `max` 文字（Unicode スカラー）を超えたら、先頭 `max - 1` 文字 + 「…」にする
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

/// 提案の基準位置。優先順位は候補ウィンドウと同じ（H-123）: 文書から測れた位置（selection / composition）
/// → 最後の既知位置 → GUI キャレット → フォーカスウィンドウ。マウス位置・画面の隅は使わない
/// （どれも無ければ `None` で出さない）
pub fn popup_anchor(anchors: &Anchors) -> Option<(Source, Rect)> {
    let usable = |r: &Rect| !r.is_zero() && r.bottom > r.top;
    anchors
        .measured
        .filter(|(_, r)| usable(r))
        .or_else(|| {
            anchors
                .last_known
                .filter(usable)
                .map(|r| (Source::LastKnown, r))
        })
        .or_else(|| {
            anchors
                .gui_caret
                .filter(usable)
                .map(|r| (Source::GuiCaret, r))
        })
        .or_else(|| {
            anchors
                .focus_window
                .filter(usable)
                .map(|r| (Source::FocusWindow, r))
        })
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// ポップアップを `anchor`（入力中の文字列の矩形）の上に置く左上位置。上に収まらなければ下。
/// 水平は作業領域内に収める。`work_areas` は先頭がプライマリ（無ければ丸めない）。
/// 候補ウィンドウは下に出るので、重ならないよう上を優先する
pub fn place_above(
    anchor: crate::candidates::Rect,
    size: crate::candidates::Size,
    work_areas: &[crate::candidates::Rect],
    gap: i32,
) -> crate::candidates::Point {
    use crate::candidates::{Point, place_window_with_gap};
    let at = Point {
        x: anchor.left,
        y: anchor.top,
    };
    let work = work_areas
        .iter()
        .find(|w| w.contains(at))
        .or_else(|| work_areas.first());
    let above_top = anchor.top - size.height - gap;
    match work {
        Some(w) if above_top >= w.top => Point {
            x: anchor.left.min(w.right - size.width).max(w.left),
            y: above_top,
        },
        // 上に収まらない: 通常の配置（下、収まらなければ反転・押し込み）
        _ => place_window_with_gap(anchor, size, work_areas, gap).origin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidates::{Point, Rect, Size};
    use nagi_protocol::Modifiers;
    use nagi_protocol::methods::RetroReviewCandidate;

    const T0: Duration = Duration::from_secs(100);

    fn params(revision: u32) -> RetroReviewParams {
        RetroReviewParams {
            session_id: "s".into(),
            revision,
            original_surface: "有り難う".into(),
            replace_len_utf16: 4,
            candidates: vec![
                RetroReviewCandidate {
                    reading: "ありがとう".into(),
                    surface: "ありがとう".into(),
                },
                RetroReviewCandidate {
                    reading: "ありかとう".into(),
                    surface: "有りか党".into(),
                },
            ],
        }
    }

    fn env() -> OfferEnv {
        OfferEnv {
            focused: true,
            composing: true,
            ui_less: false,
        }
    }

    fn down(sym: u32, m: Modifiers) -> KeyEvent {
        KeyEvent {
            keysym: sym,
            keycode: 0,
            text: String::new(),
            modifiers: m,
            kind: KeyKind::Down,
        }
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::default()
        }
    }

    fn offered() -> RetroReview {
        let mut r = RetroReview::default();
        r.offer(&params(5), env(), T0).unwrap();
        r
    }

    #[test]
    fn offer_moves_idle_to_offered_with_best_candidate() {
        let r = offered();
        match r.state() {
            RetroState::Offered { offer, since } => {
                assert_eq!(offer.surface, "ありがとう");
                assert_eq!(offer.original, "有り難う");
                assert_eq!(offer.replace_len, 4);
                assert_eq!(*since, T0);
            }
            s => panic!("unexpected {s:?}"),
        }
        assert!(r.is_offered());
    }

    #[test]
    fn offer_rejected_when_not_applicable_invalid_or_stale() {
        let mut r = RetroReview::default();
        for tweak in [
            (|e: &mut OfferEnv| e.focused = false) as fn(&mut OfferEnv),
            |e| e.composing = false,
            |e| e.ui_less = true,
        ] {
            let mut e = env();
            tweak(&mut e);
            assert_eq!(r.offer(&params(1), e, T0), Err(RejectReason::NotApplicable));
        }
        let mut p = params(1);
        p.candidates.clear();
        assert_eq!(r.offer(&p, env(), T0), Err(RejectReason::Invalid));
        let mut p = params(1);
        p.replace_len_utf16 = 9;
        assert_eq!(r.offer(&p, env(), T0), Err(RejectReason::Invalid));
        let mut p = params(1);
        p.candidates[0].surface.clear();
        assert_eq!(r.offer(&p, env(), T0), Err(RejectReason::Invalid));
        r.note_revision(10);
        assert_eq!(r.offer(&params(9), env(), T0), Err(RejectReason::Stale));
        assert!(r.offer(&params(10), env(), T0).is_ok());
    }

    #[test]
    fn accept_key_default_matches_either_case_and_exact_modifiers() {
        let k = AcceptKey::default_key();
        assert_eq!(k.label(), "Ctrl+Shift+Y");
        assert!(k.matches(&down(0x79, ctrl_shift())));
        assert!(k.matches(&down(0x59, ctrl_shift())));
        // Caps Lock は無視
        let caps = Modifiers {
            caps_lock: true,
            ..ctrl_shift()
        };
        assert!(k.matches(&down(0x79, caps)));
        // 修飾が足りない・多い
        let ctrl_only = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert!(!k.matches(&down(0x79, ctrl_only)));
        let with_alt = Modifiers {
            alt: true,
            ..ctrl_shift()
        };
        assert!(!k.matches(&down(0x79, with_alt)));
        assert!(!k.matches(&down(0x7a, ctrl_shift())));
    }

    #[test]
    fn accept_key_parse() {
        assert!(AcceptKey::parse("ctrl+alt+r").is_some());
        assert!(AcceptKey::parse("Ctrl+Tab").is_some());
        // 修飾なし・未知の修飾・複数文字のキー・空は不可
        assert!(AcceptKey::parse("y").is_none());
        assert!(AcceptKey::parse("Shift+Y").is_none());
        assert!(AcceptKey::parse("Hyper+Y").is_none());
        assert!(AcceptKey::parse("Ctrl+Yy").is_none());
        assert!(AcceptKey::parse("").is_none());
    }

    #[test]
    fn peek_key_decisions() {
        let r = offered();
        let now = T0 + Duration::from_secs(1);
        assert_eq!(
            r.peek_key(&down(0x79, ctrl_shift()), now),
            KeyDecision::Accept
        );
        assert_eq!(
            r.peek_key(&down(keysym::ESCAPE, Modifiers::default()), now),
            KeyDecision::Dismiss { consume: true }
        );
        // 次のキー入力は無視（キー自体は通常どおり処理）
        assert_eq!(
            r.peek_key(&down(0x61, Modifiers::default()), now),
            KeyDecision::Dismiss { consume: false }
        );
        // Shift+Esc は Esc として食わない
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            r.peek_key(&down(keysym::ESCAPE, shift), now),
            KeyDecision::Dismiss { consume: false }
        );
        // 修飾キーだけの変化は影響しない
        let mut flags = down(0xffe3, ctrl_shift());
        flags.kind = KeyKind::FlagsChanged;
        assert_eq!(r.peek_key(&flags, now), KeyDecision::PassThrough);
    }

    #[test]
    fn peek_key_passes_through_when_idle_or_expired() {
        let idle = RetroReview::default();
        assert_eq!(
            idle.peek_key(&down(0x79, ctrl_shift()), T0),
            KeyDecision::PassThrough
        );
        let r = offered();
        assert_eq!(
            r.peek_key(&down(0x79, ctrl_shift()), T0 + RETRO_TTL),
            KeyDecision::PassThrough
        );
    }

    #[test]
    fn accept_returns_offer_once() {
        let mut r = offered();
        let offer = r.accept(T0 + Duration::from_secs(2)).unwrap();
        assert_eq!(offer.surface, "ありがとう");
        assert_eq!(*r.state(), RetroState::Accepted);
        assert!(r.accept(T0).is_none());
        assert_eq!(*r.state(), RetroState::Accepted);
    }

    #[test]
    fn accept_after_ttl_expires_instead() {
        let mut r = offered();
        assert!(r.accept(T0 + RETRO_TTL).is_none());
        assert_eq!(*r.state(), RetroState::Expired);
    }

    #[test]
    fn dismiss_and_expire_only_when_offered() {
        let mut r = RetroReview::default();
        assert!(!r.dismiss());
        assert!(!r.expire());
        assert_eq!(*r.state(), RetroState::Idle);
        let mut r = offered();
        assert!(r.dismiss());
        assert_eq!(*r.state(), RetroState::Dismissed);
        assert!(!r.expire());
        assert_eq!(*r.state(), RetroState::Dismissed);
        let mut r = offered();
        assert!(r.expire());
        assert_eq!(*r.state(), RetroState::Expired);
    }

    #[test]
    fn tick_expires_after_ttl() {
        let mut r = offered();
        assert!(!r.tick(T0 + RETRO_TTL - Duration::from_millis(1)));
        assert!(r.is_offered());
        assert!(r.tick(T0 + RETRO_TTL));
        assert_eq!(*r.state(), RetroState::Expired);
        assert!(!r.tick(T0 + RETRO_TTL * 2));
    }

    #[test]
    fn new_offer_after_terminal_state() {
        let mut r = offered();
        r.dismiss();
        assert!(r.offer(&params(6), env(), T0).is_ok());
        assert!(r.is_offered());
    }

    #[test]
    fn prompt_shows_before_and_after_and_truncates() {
        let r = offered();
        assert_eq!(
            r.prompt().as_deref(),
            Some("「有り難う」→「ありがとう」に直す: Ctrl+Shift+Y（無視: Esc）")
        );
        assert!(RetroReview::default().prompt().is_none());
        let long = RetroOffer {
            revision: 1,
            original: "あいうえおかきくけこさしすせそ".into(),
            replace_len: 15,
            surface: "短い".into(),
        };
        let text = prompt_text(&long, "Ctrl+Alt+R");
        assert!(text.starts_with("「あいうえおかきくけ…」→「短い」"));
        assert!(text.contains("Ctrl+Alt+R"));
        assert_eq!(ellipsize("12345", 5), "12345");
        assert_eq!(ellipsize("123456", 5), "1234…");
    }

    #[test]
    fn popup_anchor_priority_and_no_screen_or_mouse() {
        let r = |l: i32| Rect {
            left: l,
            top: 10,
            right: l + 2,
            bottom: 30,
        };
        let mut a = Anchors {
            measured: Some((Source::Composition, r(1))),
            last_known: Some(r(2)),
            gui_caret: Some(r(3)),
            focus_window: Some(r(4)),
            screen: Some(r(5)),
        };
        assert_eq!(popup_anchor(&a), Some((Source::Composition, r(1))));
        a.measured = None;
        assert_eq!(popup_anchor(&a), Some((Source::LastKnown, r(2))));
        a.last_known = None;
        assert_eq!(popup_anchor(&a), Some((Source::GuiCaret, r(3))));
        a.gui_caret = None;
        assert_eq!(popup_anchor(&a), Some((Source::FocusWindow, r(4))));
        a.focus_window = None;
        // 画面の隅しか無いときは出さない
        assert_eq!(popup_anchor(&a), None);
        // ゼロ矩形は取得失敗
        a.measured = Some((Source::Selection, Rect::default()));
        assert_eq!(popup_anchor(&a), None);
    }

    #[test]
    fn reset_revision_accepts_restarted_revisions_and_drops_offer() {
        let mut r = RetroReview::default();
        r.note_revision(50);
        r.reset_revision();
        assert!(r.offer(&params(1), env(), T0).is_ok());
        r.reset_revision();
        assert!(!r.is_offered());
    }

    fn work() -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        }
    }

    #[test]
    fn popup_goes_above_the_anchor_and_falls_back_below() {
        let size = Size {
            width: 300,
            height: 30,
        };
        let anchor = Rect {
            left: 100,
            top: 200,
            right: 150,
            bottom: 220,
        };
        let p = place_above(anchor, size, &[work()], 4);
        assert_eq!(
            p,
            Point {
                x: 100,
                y: 200 - 30 - 4
            }
        );
        // 上に収まらない（画面の上端付近）: 下に出す
        let top = Rect {
            top: 10,
            bottom: 30,
            ..anchor
        };
        let p = place_above(top, size, &[work()], 4);
        assert!(p.y >= top.bottom);
        // 右端ははみ出さない
        let right = Rect {
            left: 1900,
            right: 1910,
            ..anchor
        };
        let p = place_above(right, size, &[work()], 4);
        assert_eq!(p.x, 1920 - 300);
    }
}
