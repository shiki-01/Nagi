//! Nagi グライド入力: QWERTY 上のなぞり軌跡から読み（ひらがな）の候補を求める純粋なロジック。
//!
//! プラットフォーム非依存で、モバイル（iOS / Android）でもビルドできる依存だけを使う
//! （`docs/mobile-design.md` §3）。方式は SHARK2 系（`docs/plan-fuzzy-and-glide.md` §2）:
//!
//! - [`spell`] — 読み → ローマ字のキー経路（代表の綴りに正規化。ヘボン式・ループ・長音の扱い）
//! - [`geom`] — キー配置（キー単位への正規化）、点の間引き・再サンプリング・正規化
//! - [`index`] — 読みの索引（始点 × 終点キーの箱。mmap できるフラット形式）
//! - [`recognize`] — 形状距離 + 位置距離（ガウス）+ 語頻度の事前確率による順位付け
//! - [`synth`] — 合成軌跡ジェネレータ（実機データが無い段階の評価用。シード固定）
//!
//! 使い方: 辞書から [`GlideIndex::from_dictionary`]（または [`GlideIndex::load`]）で索引を作り、
//! キー矩形から [`GlideLayout::new`] → [`Recognizer::new`] を作って [`Recognizer::recognize`] を呼ぶ。

#![warn(missing_docs)]

pub mod geom;
pub mod index;
pub mod recognize;
pub mod spell;
pub mod synth;

pub use geom::{GlideLayout, KeyRect, Pt, TracePoint};
pub use index::{GlideIndex, IndexBuilder, IndexError};
pub use recognize::{GlideMatch, MAX_INPUT_POINTS, Params, Recognizer, Stats};
