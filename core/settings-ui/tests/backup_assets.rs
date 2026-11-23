//! 「バックアップと移行」ページの埋め込みファイル（backup.js / backup.css）の静的な検査。

use nagi_settings_ui::assets::{find_static, render_page};

fn asset(path: &str) -> &'static str {
    find_static(path)
        .unwrap_or_else(|| panic!("{path} が配られていない"))
        .body
}

#[test]
fn page_loads_the_backup_files_from_the_same_origin_with_the_token() {
    let page = render_page("abc123");
    assert!(page.contains("<script src=\"/backup.js?t=abc123\" defer></script>"));
    assert!(page.contains("<link rel=\"stylesheet\" href=\"/backup.css?t=abc123\">"));
    // 追加ページの登録は app.js より前に読まれる
    assert!(page.find("/backup.js").unwrap() < page.find("/app.js").unwrap());
    assert_eq!(
        find_static("/backup.js").unwrap().content_type,
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        find_static("/backup.css").unwrap().content_type,
        "text/css; charset=utf-8"
    );
}

#[test]
fn backup_script_is_csp_safe_and_never_builds_html() {
    let js = asset("/backup.js");
    for needle in [
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "eval(",
        "new Function",
        "setAttribute(\"style\"",
        "cssText",
        "http://",
        "https://",
        "localStorage",
    ] {
        assert!(!js.contains(needle), "{needle}");
    }
    // 呼ぶのは同一オリジンの固定の API だけ
    for path in [
        "/api/backup/export",
        "/api/backup/preview",
        "/api/backup/apply",
        "/api/backup/list",
        "/api/backup/restore",
        "/api/learning/stats",
        "/api/settings",
    ] {
        assert!(js.contains(path), "{path}");
    }
    assert!(js.contains("window.NagiPages"));
    // サイドバーの「システム」グループに、データ・ファイルの近く（order 50 の次）で入る
    assert!(js.contains("group: \"system\""));
    assert!(js.contains("order: 55"));
    // 学習データは既定で書き出さず、入力履歴を含むことを明示する
    assert!(js.contains("item.id !== \"learning\""));
    assert!(js.contains("入力した内容の履歴が含まれます"));
    // 適用前の確認と、自動バックアップの案内
    assert!(js.contains("現在の設定を上書きします。適用前のバックアップを自動で保存します。"));
    // 上限 20MB
    assert!(js.contains("20 * 1024 * 1024"));
}

#[test]
fn backup_css_has_no_external_references_and_keeps_the_flat_look() {
    let css = asset("/backup.css");
    for needle in [
        "url(",
        "@import",
        "http://",
        "https://",
        "transform",
        "box-shadow",
    ] {
        assert!(!css.contains(needle), "{needle}");
    }
    assert!(css.contains("forced-colors: active"));
    // 余白は --space-* のスケールから取る（px の直書きは 1px の線・角丸・寸法だけ）
    assert!(css.contains("var(--space-"));
}

#[test]
fn backup_script_passes_a_node_syntax_check_when_node_is_available() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/backup.js");
    match std::process::Command::new("node")
        .arg("--check")
        .arg(&path)
        .output()
    {
        Ok(out) => assert!(
            out.status.success(),
            "node --check: {}",
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(_) => eprintln!("skip: node が無い"),
    }
}
