//! 設定ウィンドウの UI の状態（「詳細設定を表示」・最後に開いたページ・ウィンドウの位置とサイズ）の保存と復元
//! （OS 非依存の純粋ロジック）。`docs/decisions.md` H-118。
//!
//! 保存先は Windows では `%LOCALAPPDATA%\Nagi\settings-ui-state.json`（ログ・更新と同じ `Nagi` フォルダ。
//! 設定の本体 `config.toml` とは別で、設定の項目ではない）。スキーマ番号つきの小さな JSON で、
//! 書き込みは一時ファイル → 置き換えで原子的に行う。読めない・壊れている・知らない版のときは
//! 既定の状態で起動する（項目ごとに検証し、有効な項目だけ使う）。入力内容は含まない。

use crate::native_layout::Rect;
use crate::native_model::Page;
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};

/// ファイルの形式の版
pub const SCHEMA_VERSION: u32 = 1;
/// ファイル名
pub const FILE_NAME: &str = "settings-ui-state.json";
/// 保存するクライアント領域の大きさ（dp）の許容範囲。これを外れた値は無視する
const SIZE_DP_RANGE: std::ops::RangeInclusive<i32> = 200..=10_000;
/// 位置（px）の許容範囲。これを外れた値は無視する
const POS_RANGE: std::ops::RangeInclusive<i32> = -100_000..=100_000;

/// ウィンドウの位置とサイズ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    /// ウィンドウ全体の左上（仮想スクリーン座標。px）
    pub x: i32,
    /// ウィンドウ全体の左上（仮想スクリーン座標。px）
    pub y: i32,
    /// クライアント領域の幅（dp。モニターの DPI が変わっても同じ見た目の大きさで復元できる）
    pub w_dp: i32,
    /// クライアント領域の高さ（dp）
    pub h_dp: i32,
    /// 最大化していたか（位置・大きさは最大化する前のもの）
    pub maximized: bool,
}

/// 保存する UI の状態
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UiState {
    /// 「詳細設定を表示」
    pub detail: bool,
    /// 最後に開いていたページ（[`page_key`] の文字列）
    pub page: Option<String>,
    /// ウィンドウの位置とサイズ
    pub window: Option<WindowState>,
}

/// 保存先。Windows のみ `%LOCALAPPDATA%\Nagi\settings-ui-state.json`（それ以外・決まらなければ `None`＝保存しない）
pub fn default_path(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if os != "windows" {
        return None;
    }
    let local = env("LOCALAPPDATA")?;
    Some(PathBuf::from(format!(
        "{}\\Nagi\\{FILE_NAME}",
        local.trim_end_matches(['\\', '/'])
    )))
}

/// JSON 文字列から状態を読む。形式が壊れている・知らない版のときは既定。項目ごとに検証し、
/// 不正な項目だけを既定にする
pub fn parse(text: &str) -> UiState {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
        return UiState::default();
    };
    if map.get("version").and_then(Value::as_u64) != Some(u64::from(SCHEMA_VERSION)) {
        return UiState::default();
    }
    let detail = map.get("detail").and_then(Value::as_bool).unwrap_or(false);
    let page = map
        .get("page")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty() && p.len() <= 200)
        .map(str::to_owned);
    let window = map.get("window").and_then(parse_window);
    UiState {
        detail,
        page,
        window,
    }
}

fn parse_window(v: &Value) -> Option<WindowState> {
    let int = |k: &str| -> Option<i32> {
        v.get(k)
            .and_then(Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
    };
    let (x, y, w_dp, h_dp) = (int("x")?, int("y")?, int("w_dp")?, int("h_dp")?);
    if !POS_RANGE.contains(&x)
        || !POS_RANGE.contains(&y)
        || !SIZE_DP_RANGE.contains(&w_dp)
        || !SIZE_DP_RANGE.contains(&h_dp)
    {
        return None;
    }
    Some(WindowState {
        x,
        y,
        w_dp,
        h_dp,
        maximized: v.get("maximized").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// 状態を JSON 文字列にする（[`parse`] で読み戻せる）
pub fn to_json(state: &UiState) -> String {
    let mut root = json!({
        "version": SCHEMA_VERSION,
        "detail": state.detail,
    });
    if let Some(page) = &state.page {
        root["page"] = json!(page);
    }
    if let Some(w) = &state.window {
        root["window"] = json!({
            "x": w.x,
            "y": w.y,
            "w_dp": w.w_dp,
            "h_dp": w.h_dp,
            "maximized": w.maximized,
        });
    }
    let mut text = serde_json::to_string_pretty(&root).unwrap_or_default();
    text.push('\n');
    text
}

/// ファイルから読む。無い・読めない・壊れているときは既定
pub fn load(path: &Path) -> UiState {
    std::fs::read_to_string(path)
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// ファイルへ原子的に書く（同じフォルダの一時ファイルに書いて、置き換える。フォルダが無ければ作る）。
/// 途中で失敗しても、元のファイルは壊れない
pub fn save(path: &Path, state: &UiState) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(to_json(state).as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

// ---- ページの識別 ----

/// ページを保存用の文字列にする。読み込み中の表示（`Page::Status`）は保存しない。
/// カテゴリは表示名（`title`）で識別する（添字は項目の増減で変わるため）
pub fn page_key(page: Page, title: &str) -> Option<String> {
    match page {
        Page::Category(_) => Some(format!("category:{title}")),
        Page::Update => Some("update".to_owned()),
        Page::Data => Some("data".to_owned()),
        Page::Extra(id) => Some(format!("extra:{id}")),
        Page::Status => None,
    }
}

/// 保存した文字列を、いま左の一覧にあるページへ戻す（`entries` は `(ページ, 表示名)`）。無ければ `None`
pub fn resolve_page(key: &str, entries: &[(Page, String)]) -> Option<Page> {
    entries
        .iter()
        .find(|(page, title)| page_key(*page, title).as_deref() == Some(key))
        .map(|(page, _)| *page)
}

// ---- ウィンドウの位置 ----

fn intersection_area(a: Rect, b: Rect) -> i64 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w > 0 && h > 0 {
        i64::from(w) * i64::from(h)
    } else {
        0
    }
}

fn center_distance_sq(a: Rect, b: Rect) -> i64 {
    let dx = i64::from(a.x + a.w / 2) - i64::from(b.x + b.w / 2);
    let dy = i64::from(a.y + a.h / 2) - i64::from(b.y + b.h / 2);
    dx * dx + dy * dy
}

/// 復元するウィンドウ（外側の矩形。px）を、モニターの作業領域（`work_areas`）に収める。
/// 最も重なるモニター（どれとも重ならなければ最も近いモニター）を選び、大きすぎれば縮め
/// （ただし `min_w` × `min_h` までで、作業領域より大きくはしない）、はみ出していれば寄せる。
/// モニターが 1 つも取れなければそのまま返す
pub fn clamp_to_monitors(rect: Rect, work_areas: &[Rect], min_w: i32, min_h: i32) -> Rect {
    let Some(area) = work_areas.iter().copied().max_by_key(|a| {
        (
            intersection_area(rect, *a),
            std::cmp::Reverse(center_distance_sq(rect, *a)),
        )
    }) else {
        return rect;
    };
    let w = rect.w.max(min_w).min(area.w).max(1);
    let h = rect.h.max(min_h).min(area.h).max(1);
    let x = rect.x.clamp(area.x, (area.x + area.w - w).max(area.x));
    let y = rect.y.clamp(area.y, (area.y + area.h - h).max(area.y));
    Rect { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> UiState {
        UiState {
            detail: true,
            page: Some("category:変換".to_owned()),
            window: Some(WindowState {
                x: -1200,
                y: 80,
                w_dp: 900,
                h_dp: 640,
                maximized: false,
            }),
        }
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nagi-ui-state-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn json_round_trips() {
        let s = sample();
        assert_eq!(parse(&to_json(&s)), s);
        let d = UiState::default();
        assert_eq!(parse(&to_json(&d)), d);
        // 最大化も保つ
        let mut m = sample();
        m.window.as_mut().unwrap().maximized = true;
        assert_eq!(parse(&to_json(&m)), m);
        // スキーマ番号が入っている
        assert!(to_json(&s).contains("\"version\": 1"));
    }

    #[test]
    fn broken_input_falls_back_to_the_default() {
        for text in [
            "",
            "not json",
            "{",
            "[]",
            "null",
            "\"x\"",
            "{}",
            r#"{"version": 1"#,
            // 知らない版は読まない（将来の版で意味が変わっていても誤用しない）
            r#"{"version": 2, "detail": true}"#,
            r#"{"version": "1", "detail": true}"#,
            r#"{"detail": true}"#,
        ] {
            assert_eq!(parse(text), UiState::default(), "{text:?}");
        }
    }

    #[test]
    fn invalid_fields_are_dropped_individually() {
        let s = parse(
            r#"{"version":1,"detail":"yes","page":42,"window":{"x":1,"y":2,"w_dp":900,"h_dp":"x"}}"#,
        );
        assert_eq!(s, UiState::default());
        // 詳細設定だけ有効
        let s = parse(r#"{"version":1,"detail":true,"page":"","window":{"x":1}}"#);
        assert_eq!(
            s,
            UiState {
                detail: true,
                page: None,
                window: None
            }
        );
        // 範囲外のサイズ・位置は無視（極端な値でウィンドウが巨大・画面外にならない）
        for w in [
            r#"{"x":0,"y":0,"w_dp":5,"h_dp":600}"#,
            r#"{"x":0,"y":0,"w_dp":900,"h_dp":99999999}"#,
            r#"{"x":999999999,"y":0,"w_dp":900,"h_dp":600}"#,
            r#"{"x":0,"y":0,"w_dp":900.5,"h_dp":600}"#,
        ] {
            let text = format!(r#"{{"version":1,"detail":true,"window":{w}}}"#);
            assert_eq!(parse(&text).window, None, "{w}");
            assert!(parse(&text).detail);
        }
        // 知らない項目があっても読める（将来の追加に前方互換）
        let s = parse(r#"{"version":1,"detail":true,"future":{"a":1}}"#);
        assert!(s.detail);
    }

    #[test]
    fn save_then_load_and_overwrite() {
        let dir = tmp_dir("save");
        let path = dir.join("Nagi").join(FILE_NAME);
        // フォルダが無くても作る
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path), sample());
        // 上書き: 一時ファイルは残らない
        let mut next = sample();
        next.detail = false;
        next.page = Some("update".to_owned());
        save(&path, &next).unwrap();
        assert_eq!(load(&path), next);
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![FILE_NAME.to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_or_corrupt_file_gives_the_default() {
        let dir = tmp_dir("load");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        assert_eq!(load(&path), UiState::default());
        std::fs::write(&path, b"\xff\xfe garbage").unwrap();
        assert_eq!(load(&path), UiState::default());
        std::fs::write(&path, "").unwrap();
        assert_eq!(load(&path), UiState::default());
        // 壊れたファイルは次の保存で正常なものに置き換わる
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path), sample());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_save_leaves_the_original_and_no_temp_file() {
        let dir = tmp_dir("fail");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        save(&path, &sample()).unwrap();
        // 保存先がディレクトリで置き換えられない → エラー。一時ファイルは片付く
        let blocked = dir.join("blocked.json");
        std::fs::create_dir_all(&blocked).unwrap();
        assert!(save(&blocked, &sample()).is_err());
        assert!(!dir.join("blocked.json.tmp").exists());
        assert_eq!(load(&path), sample());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_path_is_windows_local_app_data_only() {
        let env =
            |k: &str| (k == "LOCALAPPDATA").then(|| "C:\\Users\\a\\AppData\\Local\\".to_owned());
        assert_eq!(
            default_path("windows", &env),
            Some(PathBuf::from(
                "C:\\Users\\a\\AppData\\Local\\Nagi\\settings-ui-state.json"
            ))
        );
        assert_eq!(default_path("linux", &env), None);
        assert_eq!(default_path("macos", &env), None);
        assert_eq!(default_path("windows", &|_| None), None);
    }

    fn entries() -> Vec<(Page, String)> {
        vec![
            (Page::Category(0), "入力".to_owned()),
            (Page::Category(1), "変換".to_owned()),
            (Page::Update, "更新".to_owned()),
            (Page::Data, "データ・ファイル".to_owned()),
            (Page::Extra("diag"), "動作確認".to_owned()),
        ]
    }

    #[test]
    fn page_keys_round_trip_through_the_sidebar() {
        let entries = entries();
        for (page, title) in &entries {
            let key = page_key(*page, title).unwrap();
            assert_eq!(resolve_page(&key, &entries), Some(*page), "{key}");
        }
        assert_eq!(page_key(Page::Status, "設定"), None);
        // 無くなったページ・知らない識別子は復元しない（先頭のページで開く）
        assert_eq!(resolve_page("category:消えた", &entries), None);
        assert_eq!(resolve_page("extra:other", &entries), None);
        assert_eq!(resolve_page("", &entries), None);
        // カテゴリの並びが変わっても、名前で同じページに戻る
        let reordered = vec![
            (Page::Category(0), "変換".to_owned()),
            (Page::Category(1), "入力".to_owned()),
        ];
        assert_eq!(
            resolve_page("category:変換", &reordered),
            Some(Page::Category(0))
        );
    }

    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1040,
    };
    const LEFT: Rect = Rect {
        x: -1280,
        y: 0,
        w: 1280,
        h: 984,
    };

    #[test]
    fn clamp_keeps_a_visible_window_unchanged() {
        let r = Rect {
            x: 100,
            y: 50,
            w: 900,
            h: 700,
        };
        assert_eq!(clamp_to_monitors(r, &[SCREEN], 600, 400), r);
        // 左側のモニターにいたウィンドウも、そのモニターが残っていればそのまま
        let l = Rect {
            x: -1200,
            y: 40,
            w: 900,
            h: 700,
        };
        assert_eq!(clamp_to_monitors(l, &[LEFT, SCREEN], 600, 400), l);
    }

    #[test]
    fn clamp_moves_an_offscreen_window_back() {
        // モニターが外された（左のモニターが無くなった）: 残りのモニターへ寄せる
        let l = Rect {
            x: -1200,
            y: 40,
            w: 900,
            h: 700,
        };
        let got = clamp_to_monitors(l, &[SCREEN], 600, 400);
        assert_eq!(got, Rect { x: 0, y: 40, ..l });
        // 下・右にはみ出し
        let r = Rect {
            x: 1800,
            y: 1000,
            w: 900,
            h: 700,
        };
        let got = clamp_to_monitors(r, &[SCREEN], 600, 400);
        assert_eq!(got.x + got.w, 1920);
        assert_eq!(got.y + got.h, 1040);
        // 遠く離れた座標
        let far = Rect {
            x: 50_000,
            y: -30_000,
            w: 900,
            h: 700,
        };
        let got = clamp_to_monitors(far, &[SCREEN], 600, 400);
        assert!(got.x >= 0 && got.x + got.w <= 1920 && got.y >= 0 && got.y + got.h <= 1040);
    }

    #[test]
    fn clamp_shrinks_to_the_work_area_but_not_below_the_minimum() {
        let big = Rect {
            x: 0,
            y: 0,
            w: 5000,
            h: 4000,
        };
        let got = clamp_to_monitors(big, &[SCREEN], 600, 400);
        assert_eq!((got.w, got.h), (1920, 1040));
        // 小さすぎるサイズは最小まで広げる
        let tiny = Rect {
            x: 10,
            y: 10,
            w: 50,
            h: 50,
        };
        let got = clamp_to_monitors(tiny, &[SCREEN], 600, 400);
        assert_eq!((got.w, got.h), (600, 400));
        // 作業領域が最小より小さいときは作業領域に収める（画面外に出さない）
        let small = Rect {
            x: 0,
            y: 0,
            w: 500,
            h: 300,
        };
        let got = clamp_to_monitors(big, &[small], 600, 400);
        assert_eq!((got.w, got.h), (500, 300));
    }

    #[test]
    fn clamp_picks_the_monitor_with_the_most_overlap_or_the_nearest() {
        // 2 枚のモニターにまたがる: 重なりが大きい側
        let r = Rect {
            x: -100,
            y: 100,
            w: 900,
            h: 600,
        };
        let got = clamp_to_monitors(r, &[LEFT, SCREEN], 600, 400);
        assert!(got.x >= 0, "{got:?}"); // 右（SCREEN）側が 800px 重なる
        // どれとも重ならない: 近い側
        let off = Rect {
            x: -3000,
            y: 100,
            w: 900,
            h: 600,
        };
        let got = clamp_to_monitors(off, &[LEFT, SCREEN], 600, 400);
        assert!(
            got.x >= LEFT.x && got.x + got.w <= LEFT.x + LEFT.w,
            "{got:?}"
        );
        // モニターが取れないときはそのまま
        assert_eq!(clamp_to_monitors(off, &[], 600, 400), off);
    }
}
