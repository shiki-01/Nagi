//! グライド入力（なぞり入力）の認識器の置き場。
//!
//! 認識そのものは `nagi-glide` の純粋な処理で、ここでは辞書に接続する:
//!
//! - **索引**: 起動時に `<data_dir>/dict/nagi-glide.bin`（`cargo xtask build-glide-index` が辞書から作る。
//!   mmap で読むので iOS 拡張でも常駐は触れたページだけ）があればそれを使う。無ければ、最初の認識のときに
//!   辞書の全読みから作る（メモリ上、約 1.3 秒・約 39MB。デスクトップ向けの縮退）。
//!   **ファイル専用**（`set_file_only`。iOS 拡張など、メモリ上限が厳しくロックも持つ環境）では、索引ファイルが
//!   無い間は「グライド利用不可」（認識結果は空 = `glide_input` は `consumed = false`）で、メモリ上に作らない。
//!   ファイルは認識のたびに（無い間だけ）探し直すので、本体アプリが後から置いた索引も次のなぞりから使える
//! - **認識器**: フロントが渡したキー矩形（`set_layout_geometry`）から作り、同じ内容の間は使い回す
//!
//! 認識結果は読み（ひらがな）とスコアだけで、表記への変換は engine が既存の変換（辞書 Viterbi + LLM）で行う。

use nagi_dict::Dictionary;
use nagi_engine::GlideReading;
use nagi_glide::{GlideIndex, GlideLayout, KeyRect, Recognizer, TracePoint};
use nagi_protocol::methods::{GlidePoint, LayoutKey, SetLayoutGeometryParams};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// 認識器を作ったときのキー配置（同じ配置なら作り直さない）
struct Cached {
    layout_id: String,
    keys: Vec<LayoutKey>,
    recognizer: Arc<Recognizer>,
}

/// グライド入力の認識（索引と認識器のキャッシュ）
#[derive(Default)]
pub struct GlideEngine {
    /// 事前に読み込んだ索引（`set_index`）
    preloaded: Option<Arc<GlideIndex>>,
    /// 実際に使う索引。事前に無ければ最初の認識で辞書から作る
    index: OnceLock<Arc<GlideIndex>>,
    /// ファイル専用の設定。`Some(path)` なら辞書から索引を作らず、`path`（`None` なら常に利用不可）のファイルだけを使う
    file_only: Option<Option<PathBuf>>,
    /// ファイル専用のとき、起動後に見つかって読み込んだ索引
    late: Mutex<Option<Arc<GlideIndex>>>,
    cache: Mutex<Option<Cached>>,
}

impl GlideEngine {
    /// 事前に読み込んだ索引（ファイル）を使う。`None` なら辞書から作る
    pub fn set_index(&mut self, index: Option<Arc<GlideIndex>>) {
        self.preloaded = index;
        self.index = OnceLock::new();
        *self.late.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// ファイル専用にする（辞書から索引をメモリ上に作らない）。`path` は索引ファイルの場所で、
    /// `None`（データディレクトリ無し）なら常に利用不可
    pub fn set_file_only(&mut self, path: Option<PathBuf>) {
        self.file_only = Some(path);
    }

    /// 索引を持っているか（ファイルで読み込み済み、または一度認識して辞書から作った）
    pub fn has_index(&self) -> bool {
        self.preloaded.is_some()
            || self.index.get().is_some()
            || self
                .late
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
    }

    /// 使う索引。ファイル専用で索引が無ければ `None`（グライド利用不可）
    fn index_for(&self, dict: &Dictionary) -> Option<Arc<GlideIndex>> {
        if let Some(index) = &self.preloaded {
            return Some(index.clone());
        }
        let Some(path) = &self.file_only else {
            return Some(
                self.index
                    .get_or_init(|| Arc::new(GlideIndex::from_dictionary(dict)))
                    .clone(),
            );
        };
        let mut late = self.late.lock().unwrap_or_else(|e| e.into_inner());
        if late.is_none()
            && let Some(path) = path
            && path.is_file()
        {
            // 壊れた・書きかけのファイルは使わない（次のなぞりでまた探す。mmap は先頭の検査だけで軽い）
            *late = GlideIndex::load(path).ok().map(Arc::new);
        }
        late.clone()
    }

    /// キー矩形から認識器を求める。英字キーが足りない配置は `None`
    fn recognizer_for(&self, geometry: &SetLayoutGeometryParams) -> Option<Arc<Recognizer>> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = cache.as_ref()
            && c.layout_id == geometry.layout_id
            && c.keys == geometry.keys
        {
            return Some(c.recognizer.clone());
        }
        let rects: Vec<KeyRect> = geometry
            .letter_keys()
            .map(|(label, k)| KeyRect {
                label,
                x: k.x as f32,
                y: k.y as f32,
                w: k.w as f32,
                h: k.h as f32,
            })
            .collect();
        let recognizer = Arc::new(Recognizer::new(GlideLayout::new(&rects)?));
        *cache = Some(Cached {
            layout_id: geometry.layout_id.clone(),
            keys: geometry.keys.clone(),
            recognizer: recognizer.clone(),
        });
        Some(recognizer)
    }

    /// 軌跡から読みの候補を良い順に最大 `limit` 件
    pub fn readings(
        &self,
        dict: &Dictionary,
        points: &[GlidePoint],
        geometry: &SetLayoutGeometryParams,
        limit: usize,
    ) -> Vec<GlideReading> {
        let Some(recognizer) = self.recognizer_for(geometry) else {
            return Vec::new();
        };
        let trace: Vec<TracePoint> = points
            .iter()
            .map(|p| TracePoint {
                x: p.x,
                y: p.y,
                t: p.t,
            })
            .collect();
        let Some(index) = self.index_for(dict) else {
            return Vec::new();
        };
        recognizer
            .recognize(&index, &trace)
            .into_iter()
            .take(limit)
            .map(|m| GlideReading {
                reading: m.reading,
                score: m.score,
            })
            .collect()
    }
}
