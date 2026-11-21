//! UILess モード（`ITfCandidateListUIElement`）の候補リストのモデル（OS 非依存）。
//!
//! 全画面ゲームなど、アプリが候補を自前で描画する場合、TSF は候補を `ITfCandidateListUIElement`
//! 経由で渡すことを求める。ここは「コアの `CandidatesView` → TSF が問い合わせる値」の変換だけを持つ:
//! 候補数・選択・文字列・ページの先頭インデックスの列・現在ページ、そして更新で変わった項目の
//! フラグ（`TF_CLUIE_*`）。ページ分割は候補ウィンドウ（[`crate::candidates::CandidatePage`]）と同じ
//! （`page_size` ごと。選択を含むページが現在ページ）。COM の実装は TSF 層が行う。

use nagi_protocol::CandidatesView;

/// `TF_CLUIE_DOCUMENTMGR`: 対象の文書が変わった
pub const CLUIE_DOCUMENTMGR: u32 = 0x01;
/// `TF_CLUIE_COUNT`: 候補数が変わった
pub const CLUIE_COUNT: u32 = 0x02;
/// `TF_CLUIE_SELECTION`: 選択が変わった
pub const CLUIE_SELECTION: u32 = 0x04;
/// `TF_CLUIE_STRING`: 候補の文字列が変わった
pub const CLUIE_STRING: u32 = 0x08;
/// `TF_CLUIE_PAGEINDEX`: ページ分割が変わった
pub const CLUIE_PAGEINDEX: u32 = 0x10;
/// `TF_CLUIE_CURRENTPAGE`: 現在ページが変わった
pub const CLUIE_CURRENTPAGE: u32 = 0x20;

/// 要素を作った直後（`BeginUIElement` の時点）は、すべてが「変わった」扱い
pub const CLUIE_ALL: u32 = CLUIE_DOCUMENTMGR
    | CLUIE_COUNT
    | CLUIE_SELECTION
    | CLUIE_STRING
    | CLUIE_PAGEINDEX
    | CLUIE_CURRENTPAGE;

/// 候補リストの内容
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateListData {
    items: Vec<String>,
    selected: u32,
    page_size: u32,
    /// アプリが `SetPageIndex` で指定したページ分割（無ければ `page_size` ごと）
    custom_pages: Option<Vec<u32>>,
}

impl CandidateListData {
    /// 非表示・候補なしは `None`（要素を閉じる）。選択は範囲内に丸め、`page_size` は 1 以上にする
    pub fn from_view(view: &CandidatesView) -> Option<Self> {
        if !view.visible || view.items.is_empty() {
            return None;
        }
        let total = view.items.len();
        Some(Self {
            items: view.items.iter().map(|i| i.surface.clone()).collect(),
            selected: (view.selected as usize).min(total - 1) as u32,
            page_size: view.page_size.max(1),
            custom_pages: None,
        })
    }

    /// 候補数
    pub fn count(&self) -> u32 {
        self.items.len() as u32
    }

    /// 選択中の候補のインデックス
    pub fn selection(&self) -> u32 {
        self.selected
    }

    /// `index` 番目の候補の文字列（範囲外は `None`）
    pub fn string(&self, index: u32) -> Option<&str> {
        self.items.get(index as usize).map(String::as_str)
    }

    /// 各ページの先頭の候補インデックス（昇順・先頭は 0）
    pub fn page_starts(&self) -> Vec<u32> {
        if let Some(pages) = &self.custom_pages {
            return pages.clone();
        }
        (0..self.count()).step_by(self.page_size as usize).collect()
    }

    /// 現在ページ（`page_starts` の添字）。選択を含むページ
    pub fn current_page(&self) -> u32 {
        let starts = self.page_starts();
        let after = starts.partition_point(|&s| s <= self.selected);
        after.saturating_sub(1) as u32
    }

    /// アプリが指定したページ分割を受け入れる。先頭が 0・狭義単調増加・すべて候補数未満でなければ `false`
    pub fn set_page_starts(&mut self, starts: &[u32]) -> bool {
        let valid = starts.first() == Some(&0)
            && starts.windows(2).all(|w| w[0] < w[1])
            && starts.last().is_some_and(|&l| l < self.count());
        if valid {
            self.custom_pages = Some(starts.to_vec());
        }
        valid
    }

    /// 内容を `next` に置き換え、変わった項目の `CLUIE_*` を返す。
    /// アプリ指定のページ分割は、候補数が同じ間だけ引き継ぐ
    pub fn replace(&mut self, mut next: CandidateListData) -> u32 {
        let flags = self.updated_flags(&next);
        if self.count() == next.count() && self.page_size == next.page_size {
            next.custom_pages = self.custom_pages.take();
        }
        *self = next;
        flags
    }

    /// 現在の内容から `next` に変わったときの `CLUIE_*`（文書は変わらないので `DOCUMENTMGR` は含まない）
    pub fn updated_flags(&self, next: &CandidateListData) -> u32 {
        let mut flags = 0;
        if self.count() != next.count() {
            flags |= CLUIE_COUNT;
        }
        if self.selection() != next.selection() {
            flags |= CLUIE_SELECTION;
        }
        if self.items != next.items {
            flags |= CLUIE_STRING;
        }
        // 引き継ぎ後のページ分割で比べる
        let carried = if self.count() == next.count() && self.page_size == next.page_size {
            self.custom_pages.clone()
        } else {
            None
        };
        let mut next_view = next.clone();
        next_view.custom_pages = carried;
        if self.page_starts() != next_view.page_starts() {
            flags |= CLUIE_PAGEINDEX;
        }
        if self.current_page() != next_view.current_page() {
            flags |= CLUIE_CURRENTPAGE;
        }
        flags
    }
}

/// `GetPageIndex(pIndex, uSize, puPageCnt)` の結果。`starts` を `capacity` 個まで写す
/// （`capacity` が `None` なら個数だけを知りたい呼び出し）。戻り値は（写す値, 総ページ数）
pub fn page_index_reply(starts: &[u32], capacity: Option<usize>) -> (Vec<u32>, u32) {
    let total = starts.len() as u32;
    let copied = match capacity {
        Some(n) => starts[..n.min(starts.len())].to_vec(),
        None => Vec::new(),
    };
    (copied, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::{CandidateSource, CandidateView};

    fn view(n: usize, selected: u32, page_size: u32) -> CandidatesView {
        CandidatesView {
            items: (0..n)
                .map(|i| CandidateView {
                    surface: format!("候補{i}"),
                    sources: vec![CandidateSource::SystemDict],
                    covers: None,
                    reading: None,
                })
                .collect(),
            selected,
            page_size,
            visible: true,
            unselected: None,
        }
    }

    fn data(n: usize, selected: u32, page_size: u32) -> CandidateListData {
        CandidateListData::from_view(&view(n, selected, page_size)).unwrap()
    }

    #[test]
    fn hidden_or_empty_is_none() {
        let mut v = view(3, 0, 9);
        v.visible = false;
        assert!(CandidateListData::from_view(&v).is_none());
        assert!(CandidateListData::from_view(&view(0, 0, 9)).is_none());
    }

    #[test]
    fn pages_follow_page_size_like_the_window() {
        let d = data(20, 10, 9);
        assert_eq!(d.count(), 20);
        assert_eq!(d.page_starts(), vec![0, 9, 18]);
        assert_eq!(d.current_page(), 1);
        assert_eq!(d.selection(), 10);
        assert_eq!(d.string(3), Some("候補3"));
        assert_eq!(d.string(20), None);
    }

    #[test]
    fn selection_and_page_size_are_clamped() {
        let d = data(3, 99, 0);
        assert_eq!(d.selection(), 2);
        assert_eq!(d.page_starts(), vec![0, 1, 2]);
        assert_eq!(d.current_page(), 2);
    }

    #[test]
    fn page_boundaries() {
        let d = data(18, 8, 9);
        assert_eq!(d.page_starts(), vec![0, 9]);
        assert_eq!(d.current_page(), 0);
        assert_eq!(data(18, 9, 9).current_page(), 1);
        assert_eq!(data(18, 17, 9).current_page(), 1);
    }

    #[test]
    fn flags_for_selection_move_within_page() {
        let a = data(5, 0, 9);
        assert_eq!(a.updated_flags(&data(5, 1, 9)), CLUIE_SELECTION);
    }

    #[test]
    fn flags_for_page_turn() {
        let a = data(20, 8, 9);
        assert_eq!(
            a.updated_flags(&data(20, 9, 9)),
            CLUIE_SELECTION | CLUIE_CURRENTPAGE
        );
    }

    #[test]
    fn flags_for_new_candidates() {
        let a = data(3, 0, 9);
        let mut b_view = view(3, 0, 9);
        b_view.items[1].surface = "別".to_owned();
        let b = CandidateListData::from_view(&b_view).unwrap();
        assert_eq!(a.updated_flags(&b), CLUIE_STRING);
        // 候補数が増えて、ページ数も増える
        assert_eq!(
            a.updated_flags(&data(12, 0, 9)),
            CLUIE_COUNT | CLUIE_STRING | CLUIE_PAGEINDEX
        );
    }

    #[test]
    fn no_change_no_flags() {
        assert_eq!(data(5, 2, 9).updated_flags(&data(5, 2, 9)), 0);
    }

    #[test]
    fn custom_pages_validation() {
        let mut d = data(10, 0, 9);
        assert!(!d.set_page_starts(&[]));
        assert!(!d.set_page_starts(&[1, 5]));
        assert!(!d.set_page_starts(&[0, 5, 5]));
        assert!(!d.set_page_starts(&[0, 10]));
        assert!(d.set_page_starts(&[0, 4, 8]));
        assert_eq!(d.page_starts(), vec![0, 4, 8]);
        d = {
            let mut e = data(10, 5, 9);
            e.set_page_starts(&[0, 4, 8]);
            e
        };
        assert_eq!(d.current_page(), 1);
    }

    #[test]
    fn replace_keeps_custom_pages_only_while_count_is_same() {
        let mut d = data(10, 0, 9);
        assert!(d.set_page_starts(&[0, 5]));
        let flags = d.replace(data(10, 6, 9));
        assert_eq!(d.page_starts(), vec![0, 5]);
        assert_eq!(d.current_page(), 1);
        assert_eq!(flags, CLUIE_SELECTION | CLUIE_CURRENTPAGE);
        let flags = d.replace(data(11, 6, 9));
        assert_eq!(d.page_starts(), vec![0, 9]);
        assert_ne!(flags & CLUIE_COUNT, 0);
        assert_ne!(flags & CLUIE_PAGEINDEX, 0);
    }

    #[test]
    fn page_index_reply_copies_up_to_capacity() {
        let starts = [0, 9, 18];
        assert_eq!(page_index_reply(&starts, None), (vec![], 3));
        assert_eq!(page_index_reply(&starts, Some(2)), (vec![0, 9], 3));
        assert_eq!(page_index_reply(&starts, Some(8)), (vec![0, 9, 18], 3));
    }
}
