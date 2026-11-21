//! フォーカス・キャレット移動に伴う focus_in / focus_out の判定（OS 非依存の純粋なロジック）。
//!
//! フォーカスの通知は 2 経路ある。`ITfKeyEventSink::OnSetFocus`（アプリ単位の前景の変化）と
//! `ITfThreadMgrEventSink::OnSetFocus`（同じスレッド内での文書の切り替え。タブ・入力欄の移動）。
//! 同じ切り替えで両方が届くので、直近に送った状態を持って冪等にする（二重に送らない）。
//! また `ITfTextEditSink::OnEndEdit` で、自分以外の編集により選択が composition の外へ出たかを判定する。

use std::cmp::Ordering;

/// 送るべきメッセージ。`out` を先に、続いて `into` を送る（どちらも false なら何も送らない）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FocusPlan {
    /// `focus_out` を送る（旧文書の確定・学習の flush）
    pub out: bool,
    /// `focus_in` を送る
    pub into: bool,
}

impl FocusPlan {
    /// 何も送らない
    pub fn is_empty(self) -> bool {
        !self.out && !self.into
    }
}

/// 直近に送ったのが `focus_in` か `focus_out` かを持つ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FocusTracker {
    focused: bool,
}

impl FocusTracker {
    /// 初期状態（`focus_out` 済み相当）
    pub fn new() -> Self {
        Self::default()
    }

    /// 直近に送ったのが `focus_in` か
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// 状態を `focus_out` 済みに戻す（Deactivate 時）
    pub fn reset(&mut self) {
        self.focused = false;
    }

    /// アプリ単位の前景の変化（`ITfKeyEventSink::OnSetFocus`）
    pub fn app_foreground(&mut self, foreground: bool) -> FocusPlan {
        match (foreground, self.focused) {
            (true, false) => {
                self.focused = true;
                FocusPlan {
                    out: false,
                    into: true,
                }
            }
            (false, true) => {
                self.focused = false;
                FocusPlan {
                    out: true,
                    into: false,
                }
            }
            _ => FocusPlan::default(),
        }
    }

    /// 文書のフォーカスの変化（`ITfThreadMgrEventSink::OnSetFocus`）。
    /// `has_new`: フォーカス先の文書がある（無ければどの TSF 文書にもフォーカスが無い）。
    /// `changed`: 前後とも文書があり、別の文書になった
    pub fn document_focus(&mut self, has_new: bool, changed: bool) -> FocusPlan {
        match (has_new, self.focused) {
            (false, true) => {
                self.focused = false;
                FocusPlan {
                    out: true,
                    into: false,
                }
            }
            (true, false) => {
                self.focused = true;
                FocusPlan {
                    out: false,
                    into: true,
                }
            }
            (true, true) if changed => FocusPlan {
                out: true,
                into: true,
            },
            _ => FocusPlan::default(),
        }
    }
}

/// 選択（キャレット）が composition の範囲の外に出ているか。
/// `start`: 選択の開始と composition の開始の比較、`end`: 選択の終了と composition の終了の比較
/// （`ITfRange::CompareStart` / `CompareEnd` の結果。UTF-16 位置の大小と同じ）。
/// 範囲の端（先頭・末尾）は内側とみなす
pub fn selection_outside_composition(start: Ordering, end: Ordering) -> bool {
    start == Ordering::Less || end == Ordering::Greater
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN: FocusPlan = FocusPlan {
        out: false,
        into: true,
    };
    const OUT: FocusPlan = FocusPlan {
        out: true,
        into: false,
    };
    const BOTH: FocusPlan = FocusPlan {
        out: true,
        into: true,
    };

    #[test]
    fn app_foreground_is_idempotent() {
        let mut t = FocusTracker::new();
        assert_eq!(t.app_foreground(false), FocusPlan::default());
        assert_eq!(t.app_foreground(true), IN);
        assert_eq!(t.app_foreground(true), FocusPlan::default());
        assert_eq!(t.app_foreground(false), OUT);
        assert_eq!(t.app_foreground(false), FocusPlan::default());
    }

    #[test]
    fn document_switch_sends_out_then_in() {
        let mut t = FocusTracker::new();
        assert_eq!(t.document_focus(true, false), IN);
        assert_eq!(t.document_focus(true, true), BOTH);
        assert!(t.is_focused());
        // 同じ文書への再通知は何もしない
        assert_eq!(t.document_focus(true, false), FocusPlan::default());
    }

    #[test]
    fn no_document_sends_out_once() {
        let mut t = FocusTracker::new();
        t.app_foreground(true);
        assert_eq!(t.document_focus(false, false), OUT);
        assert_eq!(t.document_focus(false, false), FocusPlan::default());
        // 続けて届くアプリ前景の喪失は二重にならない
        assert_eq!(t.app_foreground(false), FocusPlan::default());
    }

    #[test]
    fn both_paths_do_not_double_send() {
        let mut t = FocusTracker::new();
        // アプリ前景 → 文書フォーカス（フォーカス取得）
        assert_eq!(t.app_foreground(true), IN);
        assert_eq!(t.document_focus(true, false), FocusPlan::default());
        // 文書フォーカス → アプリ前景（順序が逆でも 1 回）
        t.reset();
        assert_eq!(t.document_focus(true, false), IN);
        assert_eq!(t.app_foreground(true), FocusPlan::default());
        // 前景の喪失 → 文書なし
        assert_eq!(t.app_foreground(false), OUT);
        assert_eq!(t.document_focus(false, false), FocusPlan::default());
    }

    #[test]
    fn reset_returns_to_out() {
        let mut t = FocusTracker::new();
        t.app_foreground(true);
        t.reset();
        assert!(!t.is_focused());
        assert_eq!(t.app_foreground(true), IN);
    }

    #[test]
    fn selection_range_check() {
        use Ordering::*;
        // 範囲内・端はそのまま
        assert!(!selection_outside_composition(Equal, Equal));
        assert!(!selection_outside_composition(Greater, Less));
        assert!(!selection_outside_composition(Equal, Less));
        assert!(!selection_outside_composition(Greater, Equal));
        // 手前・後ろ・両側にはみ出す
        assert!(selection_outside_composition(Less, Less));
        assert!(selection_outside_composition(Greater, Greater));
        assert!(selection_outside_composition(Less, Greater));
        assert!(selection_outside_composition(Equal, Greater));
    }
}
