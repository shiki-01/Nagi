//! 読みの索引: 「始点キー × 終点キー」の箱ごとに読みを並べたフラットなバイナリ形式。
//!
//! グライドは軌跡の始点・終点が分かれば、理想経路の始点・終点がその近くのキーの読みだけを
//! 調べればよい（SHARK2 の枝刈り）。辞書の全読みを毎回舐めないよう、読みをローマ字の経路にしたときの
//! 先頭キー・末尾キー（26 × 26 の箱）で並べ替えた索引を持つ。
//!
//! # 形式（すべてリトルエンディアン、アラインメント不要）
//!
//! ```text
//! "NAGIGLID" (8B) | version u32
//! セクション × 3（それぞれ len u64 + 本体）:
//!   1. buckets  u32 × (26 × 26 + 1)  箱 b の項目 = 項目 buckets[b]..buckets[b+1]（b = 先頭キー × 26 + 末尾キー）
//!   2. records  12B × 項目数          項目: rd_off u32 | path_off u32 | cost u16 | rd_len u8 | path_len u8
//!                                     rd_off = 読みの UTF-8 の位置（reading_pool 内）、rd_len = そのバイト数、
//!                                     path_off = キー経路の位置（path_pool 内）、path_len = キー数、
//!                                     cost = 語コスト（小さいほど出やすい。Mozc 辞書の尺度）
//!   3. pools    rd_pool_len u32 | 読みの UTF-8（同じ読みは 1 回だけ）| キー経路のバイト列
//!                （1 バイト = キー番号 0..26、同じキーの連続（ループ）があれば 0x80 を足す。
//!                項目と同じ順に並べるので、箱を走査するときは連続したメモリを読む）
//! ```
//!
//! 項目は「箱 × 綴りの流儀」ごとなので、同じ読みでも、ヘボン式で先頭キーが変わる（ふ = `hu` / `fu`）、
//! または経路が変わる（し = `si` / `shi`）読みは複数の項目になる（読みの文字列は共有）。
//! 経路を持つので、照合時にローマ字化をせず、軌跡が各キーの近くを通るかを表引きだけで調べられる。ファイル全体を 1 つのバイト列として持ち、各配列は
//! 要素ごとに読むだけなので、非 Windows では mmap でき、触れたページしか物理メモリに載らない
//! （iOS キーボード拡張のメモリ上限対策。辞書 `nagi-dict` と同じ方針。更新は別 inode への rename で行うこと）。

use crate::spell::key_paths;
use nagi_dict::Dictionary;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

const MAGIC: &[u8; 8] = b"NAGIGLID";
const VERSION: u32 = 2;
const SECTION_COUNT: usize = 3;
/// 項目 1 つのバイト数
const RECORD_BYTES: usize = 12;
/// 箱の数（先頭キー × 末尾キー）
pub const BUCKETS: usize = 26 * 26;
/// 索引に入れる読みの最大バイト数
const MAX_READING_BYTES: usize = 255;

/// 索引の読み込み・構築のエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexError(pub String);

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "グライドの索引が不正です: {}", self.0)
    }
}

impl std::error::Error for IndexError {}

/// ファイル全体のバイト列の持ち主（メモリ上の `Vec` か、mmap）
enum Backing {
    Owned(Vec<u8>),
    #[cfg(not(windows))]
    Mapped(memmap2::Mmap),
}

impl Backing {
    fn as_slice(&self) -> &[u8] {
        match self {
            Backing::Owned(v) => v,
            #[cfg(not(windows))]
            Backing::Mapped(m) => m,
        }
    }
}

/// 読み込み済みの索引（不変。`Send + Sync`）
pub struct GlideIndex {
    backing: Backing,
    buckets: Range<usize>,
    records: Range<usize>,
    /// 読みの文字列
    reading_pool: Range<usize>,
    /// キー経路のバイト列
    path_pool: Range<usize>,
    n_entries: usize,
}

/// キー経路の 1 バイトのうち、同じキーの連続（ループ）を表す印
pub const LOOP_FLAG: u8 = 0x80;

impl std::fmt::Debug for GlideIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlideIndex")
            .field("entries", &self.n_entries)
            .field("bytes", &self.backing.as_slice().len())
            .finish()
    }
}

fn u32_at(bytes: &[u8], section: &Range<usize>, index: usize) -> u32 {
    let p = section.start + index * 4;
    u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]])
}

/// 構築器: `(読み, コスト)` を足していき、`build_bytes` でバイナリ形式にする
#[derive(Debug, Default)]
pub struct IndexBuilder {
    /// 読みの pool（同じ読みは 1 回だけ）
    readings: Vec<u8>,
    /// キー経路の pool
    paths: Vec<u8>,
    /// (箱, 読みの位置, 読みの長さ, 経路の位置, 経路のキー数, コスト)
    entries: Vec<(u16, u32, u8, u32, u8, u16)>,
    /// 読み → 読みの pool 上の位置（箱が複数でも pool を共有するための一時表）
    seen: HashMap<String, u32>,
}

impl IndexBuilder {
    /// 空の構築器
    pub fn new() -> Self {
        Self::default()
    }

    /// 読みを足す。キー経路にできない読み（ひらがな以外を含む・キーが 2 つ未満・長すぎる）は足さない。
    /// 足したら `true`
    pub fn add(&mut self, reading: &str, cost: u16) -> bool {
        if reading.len() > MAX_READING_BYTES {
            return false;
        }
        let paths = key_paths(reading);
        if paths.is_empty() {
            return false;
        }
        let rd_off = match self.seen.get(reading) {
            Some(&o) => o,
            None => {
                let o = self.readings.len() as u32;
                self.readings.extend_from_slice(reading.as_bytes());
                self.seen.insert(reading.to_string(), o);
                o
            }
        };
        for (_, path) in paths {
            let (Some(first), Some(last)) = (path.first(), path.last()) else {
                continue;
            };
            let off = self.paths.len() as u32;
            self.paths.extend(
                path.keys
                    .iter()
                    .zip(&path.loops)
                    .map(|(&k, &l)| if l { k | LOOP_FLAG } else { k }),
            );
            self.entries.push((
                bucket_of(first, last) as u16,
                rd_off,
                reading.len() as u8,
                off,
                path.keys.len().min(255) as u8,
                cost,
            ));
        }
        true
    }

    /// 項目数
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// バイナリ形式のバイト列にする
    pub fn build_bytes(mut self) -> Vec<u8> {
        // 読みの重複検査用の表は、ここから先は要らない（ピークのメモリを減らす）
        self.seen = HashMap::new();
        // 箱ごとに並べる（箱の中は語コストの小さい順にしておくと、同点の扱いが決定的になる）
        self.entries
            .sort_by_key(|&(b, rd_off, _, path_off, _, cost)| (b, cost, rd_off, path_off));
        let mut buckets = vec![0u32; BUCKETS + 1];
        for e in &self.entries {
            buckets[e.0 as usize + 1] += 1;
        }
        for i in 0..BUCKETS {
            buckets[i + 1] += buckets[i];
        }
        // 経路は項目の順に並べ直す（箱の走査が連続したメモリ読みになる）
        let mut paths = Vec::with_capacity(self.paths.len());
        let mut records = Vec::with_capacity(self.entries.len() * RECORD_BYTES);
        for &(_, ro, rl, po, pl, cost) in &self.entries {
            let new_off = paths.len() as u32;
            paths.extend_from_slice(&self.paths[po as usize..po as usize + pl as usize]);
            records.extend_from_slice(&ro.to_le_bytes());
            records.extend_from_slice(&new_off.to_le_bytes());
            records.extend_from_slice(&cost.to_le_bytes());
            records.push(rl);
            records.push(pl);
        }
        let mut pools = Vec::with_capacity(4 + self.readings.len() + paths.len());
        pools.extend_from_slice(&(self.readings.len() as u32).to_le_bytes());
        pools.extend_from_slice(&self.readings);
        pools.extend_from_slice(&paths);
        let sections: [Vec<u8>; SECTION_COUNT] = [
            buckets.iter().flat_map(|x| x.to_le_bytes()).collect(),
            records,
            pools,
        ];
        let mut out = Vec::with_capacity(sections.iter().map(|s| s.len() + 8).sum::<usize>() + 12);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        for s in &sections {
            out.extend_from_slice(&(s.len() as u64).to_le_bytes());
            out.extend_from_slice(s);
        }
        out
    }
}

/// 箱の番号（先頭キー × 26 + 末尾キー）
pub fn bucket_of(first: u8, last: u8) -> usize {
    first as usize * 26 + last as usize
}

impl GlideIndex {
    /// 辞書の全読みから索引を作る（メモリ上）
    pub fn from_dictionary(dict: &Dictionary) -> GlideIndex {
        Self::from_bytes_unchecked(Self::dictionary_bytes(dict))
    }

    /// 辞書の全読みから索引のバイト列を作る（ファイルに書き出す用）
    pub fn dictionary_bytes(dict: &Dictionary) -> Vec<u8> {
        let mut b = IndexBuilder::new();
        dict.for_each_reading(|reading, cost| {
            b.add(reading, cost);
        });
        b.build_bytes()
    }

    /// 辞書ファイル（`nagi-dict.bin`）から索引ファイルを作る（iOS の本体アプリなど、xtask を使えない環境向け）。
    /// 一時ファイルに書いてから rename するので、読み込み中の索引ファイルを壊さず、途中で失敗しても
    /// 既存の索引は残る。書き出したあとに読み直して検査し、項目数を返す
    pub fn build_file(dict_path: &Path, out_path: &Path) -> Result<usize, String> {
        let dictionary = Dictionary::load(dict_path)
            .map_err(|e| format!("辞書を読み込めません {}: {e}", dict_path.display()))?;
        let bytes = Self::dictionary_bytes(&dictionary);
        drop(dictionary);
        if let Some(dir) = out_path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("保存先を作れません {}: {e}", dir.display()))?;
        }
        let tmp = out_path.with_extension("tmp");
        let result = std::fs::write(&tmp, &bytes)
            .and_then(|()| std::fs::rename(&tmp, out_path))
            .map_err(|e| format!("索引を書き出せません {}: {e}", out_path.display()));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result?;
        drop(bytes);
        let index = Self::load(out_path).map_err(|e| format!("書き出した索引が不正です: {e}"))?;
        Ok(index.len())
    }

    /// `(読み, コスト)` の列から索引を作る（メモリ上。テスト・評価用）
    pub fn from_readings<'a>(readings: impl IntoIterator<Item = (&'a str, u16)>) -> GlideIndex {
        let mut b = IndexBuilder::new();
        for (r, c) in readings {
            b.add(r, c);
        }
        Self::from_bytes_unchecked(b.build_bytes())
    }

    fn from_bytes_unchecked(bytes: Vec<u8>) -> GlideIndex {
        Self::from_backing(Backing::Owned(bytes)).expect("構築器の出力は常に正しい")
    }

    /// バイト列から読み込む
    pub fn from_vec(bytes: Vec<u8>) -> Result<GlideIndex, IndexError> {
        Self::from_backing(Backing::Owned(bytes))
    }

    /// 索引ファイルを読み込む。非 Windows では mmap し、失敗したら全量を読む
    pub fn load(path: &Path) -> Result<GlideIndex, IndexError> {
        #[cfg(not(windows))]
        if let Some(mapped) = map_file(path) {
            return Self::from_backing(Backing::Mapped(mapped));
        }
        let bytes = std::fs::read(path).map_err(|e| IndexError(e.to_string()))?;
        Self::from_backing(Backing::Owned(bytes))
    }

    /// 索引ファイルを原子的に書き出す（一時ファイル → rename。mmap 中のファイルを壊さないため）
    pub fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }

    fn from_backing(backing: Backing) -> Result<GlideIndex, IndexError> {
        let bytes = backing.as_slice();
        let mut pos = 0usize;
        let mut take = |n: usize| -> Result<Range<usize>, IndexError> {
            let end = pos
                .checked_add(n)
                .filter(|&end| end <= bytes.len())
                .ok_or_else(|| IndexError("データが途中で終わっています".into()))?;
            let r = pos..end;
            pos = end;
            Ok(r)
        };
        if &bytes[take(8)?] != MAGIC {
            return Err(IndexError("マジックが一致しません".into()));
        }
        let version = u32::from_le_bytes(bytes[take(4)?].try_into().expect("4 バイト"));
        if version != VERSION {
            return Err(IndexError(format!("未対応のバージョン {version}")));
        }
        let mut sections: Vec<Range<usize>> = Vec::with_capacity(SECTION_COUNT);
        for _ in 0..SECTION_COUNT {
            let len = u64::from_le_bytes(bytes[take(8)?].try_into().expect("8 バイト"));
            let len = usize::try_from(len).map_err(|_| IndexError("大きすぎます".into()))?;
            sections.push(take(len)?);
        }
        let [buckets, records, pools]: [Range<usize>; SECTION_COUNT] =
            sections.try_into().expect("セクション数は固定");
        if buckets.len() != (BUCKETS + 1) * 4
            || records.len() % RECORD_BYTES != 0
            || pools.len() < 4
        {
            return Err(IndexError("セクションの長さが整合しません".into()));
        }
        let n = records.len() / RECORD_BYTES;
        if u32_at(bytes, &buckets, BUCKETS) as usize != n {
            return Err(IndexError("箱の表と項目数が合いません".into()));
        }
        let rd_pool_len = u32_at(bytes, &(pools.start..pools.start + 4), 0) as usize;
        if pools.len() < 4 + rd_pool_len {
            return Err(IndexError("読みの pool が不正です".into()));
        }
        let reading_pool = pools.start + 4..pools.start + 4 + rd_pool_len;
        let path_pool = reading_pool.end..pools.end;
        // 箱の境界は単調で、範囲内であること（壊れたファイルでも panic しない）
        let mut prev = 0u32;
        for i in 0..=BUCKETS {
            let v = u32_at(bytes, &buckets, i);
            if v < prev {
                return Err(IndexError("箱の表が不正です".into()));
            }
            prev = v;
        }
        Ok(GlideIndex {
            backing,
            buckets,
            records,
            reading_pool,
            path_pool,
            n_entries: n,
        })
    }

    /// 項目数（箱への所属の数）
    pub fn len(&self) -> usize {
        self.n_entries
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.n_entries == 0
    }

    /// 索引全体のバイト数（ファイルサイズ）
    pub fn byte_len(&self) -> usize {
        self.backing.as_slice().len()
    }

    /// 箱 `(first, last)` の項目番号の範囲
    pub fn bucket(&self, first: u8, last: u8) -> Range<usize> {
        let bytes = self.backing.as_slice();
        let b = bucket_of(first, last);
        u32_at(bytes, &self.buckets, b) as usize..u32_at(bytes, &self.buckets, b + 1) as usize
    }

    fn record(&self, entry: usize) -> &[u8] {
        let start = self.records.start + entry * RECORD_BYTES;
        &self.backing.as_slice()[start..start + RECORD_BYTES]
    }

    /// 項目の読み
    pub fn reading(&self, entry: usize) -> &str {
        let r = self.record(entry);
        let off = u32::from_le_bytes([r[0], r[1], r[2], r[3]]) as usize;
        let len = r[10] as usize;
        self.backing.as_slice()[self.reading_pool.clone()]
            .get(off..off + len)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    /// 項目のキー経路（1 バイト = キー番号 0..26、ループなら [`LOOP_FLAG`] を足す）。
    /// 壊れたファイルで範囲外なら空
    pub fn path(&self, entry: usize) -> &[u8] {
        let r = self.record(entry);
        let off = u32::from_le_bytes([r[4], r[5], r[6], r[7]]) as usize;
        let len = r[11] as usize;
        self.backing.as_slice()[self.path_pool.clone()]
            .get(off..off + len)
            .unwrap_or(&[])
    }

    /// 項目の語コスト
    pub fn cost(&self, entry: usize) -> u16 {
        let r = self.record(entry);
        u16::from_le_bytes([r[8], r[9]])
    }
}

/// 索引ファイルを mmap する。失敗したら `None`（呼び出し側が全量読み込みに戻す）
#[cfg(not(windows))]
#[allow(unsafe_code)]
fn map_file(path: &Path) -> Option<memmap2::Mmap> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: 索引ファイルは読み取り専用で開く。マップ中に同じ inode が書き換え・短縮されると
    // アクセス時に SIGBUS になりうるため、更新は別 inode への rename で行う（`write_file`）
    unsafe { memmap2::Mmap::map(&file) }.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_buckets_by_first_and_last_key() {
        let idx = GlideIndex::from_readings([("さくら", 3000), ("すし", 4000), ("かんじ", 2000)]);
        // さくら: s…a、すし: s…i、かんじ: k…i（ヘボン式でも先頭・末尾は同じ）
        let sa = idx.bucket(b's' - b'a', b'a' - b'a');
        assert_eq!(sa.len(), 1);
        assert_eq!(idx.reading(sa.start), "さくら");
        assert_eq!(idx.cost(sa.start), 3000);
        let ki = idx.bucket(b'k' - b'a', b'i' - b'a');
        assert_eq!(idx.reading(ki.start), "かんじ");
    }

    #[test]
    fn hepburn_variants_use_both_buckets_and_share_the_pool() {
        let idx = GlideIndex::from_readings([("ふじさん", 3000)]);
        assert_eq!(idx.len(), 2);
        let h = idx.bucket(b'h' - b'a', b'n' - b'a');
        let f = idx.bucket(b'f' - b'a', b'n' - b'a');
        assert_eq!(idx.reading(h.start), "ふじさん");
        assert_eq!(idx.reading(f.start), "ふじさん");
    }

    #[test]
    fn skips_unspellable_and_single_key_readings() {
        let idx = GlideIndex::from_readings([("漢字", 1), ("あ", 1), ("カタカナ", 1), ("ねこ", 5)]);
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn round_trips_through_bytes_and_files() {
        let mut b = IndexBuilder::new();
        b.add("さくら", 3000);
        b.add("きっぷ", 5000);
        let bytes = b.build_bytes();
        let a = GlideIndex::from_vec(bytes.clone()).unwrap();
        assert_eq!(a.len(), 2);
        let dir = std::env::temp_dir().join(format!("nagi-glide-index-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("idx.bin");
        GlideIndex::write_file(&path, &bytes).unwrap();
        let loaded = GlideIndex::load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.byte_len(), bytes.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_corrupt_data_without_panicking() {
        assert!(GlideIndex::from_vec(Vec::new()).is_err());
        assert!(GlideIndex::from_vec(b"NAGIGLID".to_vec()).is_err());
        let mut b = IndexBuilder::new();
        b.add("さくら", 1);
        let mut bytes = b.build_bytes();
        bytes.truncate(bytes.len() - 3);
        assert!(GlideIndex::from_vec(bytes).is_err());
        let mut bad = vec![0u8; 64];
        bad[..8].copy_from_slice(b"XXXXXXXX");
        assert!(GlideIndex::from_vec(bad).is_err());
    }
}
