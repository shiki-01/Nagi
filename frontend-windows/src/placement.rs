//! アプリ単位の表示位置の補正（候補ウィンドウ・モード切替ポップアップ）。OS 非依存。
//!
//! 候補ウィンドウはフォーカスもクリックも奪わない設計（`WS_EX_NOACTIVATE` + `HTTRANSPARENT`、マウス操作は
//! v1 の非目標）で、利用者がドラッグして動かすことはできない。代わりに、キャレット位置を正しく測れない
//! アプリ向けに、実行ファイル名ごとの補正オフセット（96 DPI 基準の dp。右・下が正）を持つ。
//!
//! - 保存先は `%APPDATA%\Nagi\window-offsets.tsv`（設定ディレクトリ。`config.toml` と同じ場所）
//! - 形式は TSV（`<exe 名>\t<dx>\t<dy>`、`#` 始まりは注釈）。手で編集できる。書き出しは [`OffsetTable::to_tsv`]
//! - ここは解析・検索・更新・適用の純ロジックのみ。ファイル I/O と OS の API は TSF 層が行う

use std::path::{Path, PathBuf};

use crate::candidates::{Point, Rect};

/// ファイル名（`%APPDATA%\Nagi\` の下）
pub const FILE_NAME: &str = "window-offsets.tsv";

/// 先頭の注釈行（スキーマのバージョン）
const HEADER: &str = "# nagi window offsets v1: <exe>\\t<dx dp>\\t<dy dp>";

/// 補正量（片方向）の上限（dp）。誤入力で候補が画面外へ飛ばないようにする
pub const MAX_OFFSET_DP: i32 = 400;

/// 読み込む最大バイト数（これを超えるファイルは無視する）
pub const MAX_FILE_BYTES: u64 = 64 * 1024;

/// 保持する最大件数
pub const MAX_ENTRIES: usize = 256;

/// 保存先のパス（`app_data` は `%APPDATA%`）
pub fn offsets_path(app_data: &Path) -> PathBuf {
    app_data.join("Nagi").join(FILE_NAME)
}

/// 実行ファイルのパス（または名前）からキーを作る。ファイル名部分を小文字にする（Windows のパスは
/// 大文字小文字を区別しない）。空なら `None`
pub fn app_key(path_or_name: &str) -> Option<String> {
    let name = path_or_name
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path_or_name)
        .trim();
    (!name.is_empty()).then(|| name.to_lowercase())
}

/// 補正量を `±MAX_OFFSET_DP` に収める
pub fn clamp_offset(offset: Point) -> Point {
    Point {
        x: offset.x.clamp(-MAX_OFFSET_DP, MAX_OFFSET_DP),
        y: offset.y.clamp(-MAX_OFFSET_DP, MAX_OFFSET_DP),
    }
}

/// アプリ別の補正量の表
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OffsetTable {
    /// `(キー, 補正量 dp)`。キーは [`app_key`] の形（重複なし）
    entries: Vec<(String, Point)>,
}

impl OffsetTable {
    /// 空の表
    pub fn new() -> Self {
        Self::default()
    }

    /// TSV を解析する。壊れた行（列不足・数値でない・キーが空）は黙って飛ばす（他の行は生かす）。
    /// 同じキーが重複したら後の行が勝つ。値は `±MAX_OFFSET_DP` に収め、件数は `MAX_ENTRIES` まで
    pub fn parse(text: &str) -> Self {
        let mut table = Self::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let mut cols = line.split('\t');
            let (Some(name), Some(dx), Some(dy)) = (cols.next(), cols.next(), cols.next()) else {
                continue;
            };
            let (Some(key), Ok(dx), Ok(dy)) = (
                app_key(name),
                dx.trim().parse::<i32>(),
                dy.trim().parse::<i32>(),
            ) else {
                continue;
            };
            table.set(&key, Point { x: dx, y: dy });
        }
        table
    }

    /// TSV に書き出す（キー順。`parse` で元に戻る。補正量 0 の項目は書かない）
    pub fn to_tsv(&self) -> String {
        let mut sorted: Vec<&(String, Point)> = self.entries.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = String::from(HEADER);
        out.push('\n');
        for (key, p) in sorted {
            out.push_str(&format!("{key}\t{}\t{}\n", p.x, p.y));
        }
        out
    }

    /// 補正量を設定する（`app` は [`app_key`] を通すのでパスでもよい）。0 は項目の削除と同じ。
    /// 上限件数に達していて新しいキーなら無視する。戻り値は表が変わったか
    pub fn set(&mut self, app: &str, offset: Point) -> bool {
        let Some(key) = app_key(app) else {
            return false;
        };
        let offset = clamp_offset(offset);
        let position = self.entries.iter().position(|(k, _)| *k == key);
        if offset == Point::default() {
            return match position {
                Some(i) => {
                    self.entries.remove(i);
                    true
                }
                None => false,
            };
        }
        match position {
            Some(i) if self.entries[i].1 == offset => false,
            Some(i) => {
                self.entries[i].1 = offset;
                true
            }
            None if self.entries.len() >= MAX_ENTRIES => false,
            None => {
                self.entries.push((key, offset));
                true
            }
        }
    }

    /// アプリの補正量（dp）。無ければ 0
    pub fn get(&self, app: &str) -> Point {
        app_key(app)
            .and_then(|key| self.entries.iter().find(|(k, _)| *k == key))
            .map_or(Point::default(), |(_, p)| *p)
    }

    /// 件数
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 符号に対して対称な四捨五入で dp を px にする（`scale_px` は負の値で非対称になるため別に持つ）。
/// `dpi` が 0 なら等倍
pub fn scale_offset(value: i32, dpi: u32) -> i32 {
    if dpi == 0 {
        return value;
    }
    let scaled = (i64::from(value.abs()) * i64::from(dpi) + 48) / 96;
    let scaled = i32::try_from(scaled).unwrap_or(value.abs());
    if value < 0 { -scaled } else { scaled }
}

/// 基準の矩形を補正量（dp）だけ動かす。`dpi` はそのモニターの DPI
pub fn shift_rect(rect: Rect, offset: Point, dpi: u32) -> Rect {
    let dx = scale_offset(offset.x, dpi);
    let dy = scale_offset(offset.y, dpi);
    Rect {
        left: rect.left + dx,
        top: rect.top + dy,
        right: rect.right + dx,
        bottom: rect.bottom + dy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    #[test]
    fn app_key_takes_lowercase_file_name() {
        assert_eq!(
            app_key(r"C:\Program Files\App\Code.EXE").as_deref(),
            Some("code.exe")
        );
        assert_eq!(app_key("/usr/bin/Foo").as_deref(), Some("foo"));
        assert_eq!(app_key("notepad.exe").as_deref(), Some("notepad.exe"));
        assert_eq!(app_key("  "), None);
        assert_eq!(app_key(r"C:\dir\"), None);
        assert_eq!(app_key(""), None);
    }

    #[test]
    fn path_is_under_nagi_settings_dir() {
        assert_eq!(
            offsets_path(Path::new("C:/Users/u/AppData/Roaming")),
            Path::new("C:/Users/u/AppData/Roaming/Nagi/window-offsets.tsv")
        );
    }

    #[test]
    fn parse_skips_comments_and_broken_lines() {
        let text = "# comment\r\nCode.exe\t10\t-20\r\n\r\nbroken\t1\nbad\tx\t1\n\t1\t2\nchrome.exe\t 3 \t4\textra\n";
        let t = OffsetTable::parse(text);
        assert_eq!(t.len(), 2);
        assert_eq!(t.get("code.exe"), p(10, -20));
        assert_eq!(t.get(r"C:\x\CHROME.EXE"), p(3, 4));
        assert_eq!(t.get("other.exe"), p(0, 0));
    }

    #[test]
    fn parse_later_duplicate_wins_and_values_clamp() {
        let t = OffsetTable::parse("a.exe\t1\t1\nA.EXE\t9999\t-9999\n");
        assert_eq!(t.len(), 1);
        assert_eq!(t.get("a.exe"), p(MAX_OFFSET_DP, -MAX_OFFSET_DP));
    }

    #[test]
    fn parse_limits_entry_count() {
        let text: String = (0..MAX_ENTRIES + 10)
            .map(|i| format!("app{i}.exe\t1\t1\n"))
            .collect();
        assert_eq!(OffsetTable::parse(&text).len(), MAX_ENTRIES);
    }

    #[test]
    fn tsv_round_trips_sorted_and_omits_zero() {
        let mut t = OffsetTable::new();
        t.set("b.exe", p(5, 6));
        t.set("a.exe", p(-1, 2));
        t.set("zero.exe", p(0, 0));
        let tsv = t.to_tsv();
        assert!(tsv.starts_with("# nagi window offsets v1"));
        let a = tsv.find("a.exe").unwrap();
        let b = tsv.find("b.exe").unwrap();
        assert!(a < b);
        assert!(!tsv.contains("zero.exe"));
        assert_eq!(OffsetTable::parse(&tsv), {
            let mut expected = OffsetTable::new();
            expected.set("a.exe", p(-1, 2));
            expected.set("b.exe", p(5, 6));
            expected
        });
    }

    #[test]
    fn set_reports_changes_and_zero_removes() {
        let mut t = OffsetTable::new();
        assert!(t.set("a.exe", p(1, 2)));
        assert!(!t.set(r"C:\x\A.exe", p(1, 2)), "同じ値は変更なし");
        assert!(t.set("a.exe", p(3, 4)));
        assert_eq!(t.get("a.exe"), p(3, 4));
        assert!(t.set("a.exe", p(0, 0)));
        assert!(t.is_empty());
        assert!(!t.set("a.exe", p(0, 0)), "無い項目の削除は変更なし");
        assert!(!t.set("", p(1, 1)), "キーが空なら無視");
    }

    #[test]
    fn set_ignores_new_key_past_the_limit() {
        let mut t = OffsetTable::new();
        for i in 0..MAX_ENTRIES {
            assert!(t.set(&format!("app{i}.exe"), p(1, 1)));
        }
        assert!(!t.set("extra.exe", p(1, 1)));
        assert!(t.set("app0.exe", p(2, 2)), "既存キーの更新は可能");
    }

    #[test]
    fn scale_is_symmetric_for_negative_values() {
        assert_eq!(scale_offset(10, 96), 10);
        assert_eq!(scale_offset(10, 144), 15);
        assert_eq!(scale_offset(-10, 144), -15);
        assert_eq!(scale_offset(1, 120), 1); // 1.25 → 1
        assert_eq!(scale_offset(-1, 120), -1);
        assert_eq!(scale_offset(2, 120), 3); // 2.5 → 3
        assert_eq!(scale_offset(-2, 120), -3);
        assert_eq!(scale_offset(7, 0), 7);
        assert_eq!(scale_offset(0, 192), 0);
    }

    #[test]
    fn shift_moves_whole_rect() {
        let r = Rect {
            left: 100,
            top: 200,
            right: 110,
            bottom: 220,
        };
        let shifted = shift_rect(r, p(10, -20), 192);
        assert_eq!(
            shifted,
            Rect {
                left: 120,
                top: 160,
                right: 130,
                bottom: 180,
            }
        );
        assert_eq!(shift_rect(r, p(0, 0), 144), r);
    }
}
