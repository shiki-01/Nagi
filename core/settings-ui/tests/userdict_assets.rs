//! 「ユーザー辞書」「学習と抑制語」ページの埋め込みファイル（userdict.js / userdict.css）の静的な検査。

use nagi_settings_ui::assets::{find_static, render_page};

fn asset(path: &str) -> &'static str {
    find_static(path)
        .unwrap_or_else(|| panic!("{path} が配られていない"))
        .body
}

#[test]
fn page_loads_the_userdict_files_from_the_same_origin_with_the_token() {
    let page = render_page("abc123");
    assert!(page.contains("<script src=\"/userdict.js?t=abc123\" defer></script>"));
    assert!(page.contains("<link rel=\"stylesheet\" href=\"/userdict.css?t=abc123\">"));
    // 追加ページの登録は app.js より前に読まれる（NagiPages が app.js の起動時に揃っている）
    let user = page.find("/userdict.js").unwrap();
    let app = page.find("/app.js").unwrap();
    assert!(user < app);
    assert_eq!(
        find_static("/userdict.js").unwrap().content_type,
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        find_static("/userdict.css").unwrap().content_type,
        "text/css; charset=utf-8"
    );
}

#[test]
fn userdict_script_is_csp_safe_and_never_builds_html() {
    let js = asset("/userdict.js");
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
        "/api/userdict/list",
        "/api/userdict/add",
        "/api/userdict/update",
        "/api/userdict/remove",
        "/api/userdict/import",
        "/api/userdict/export",
        "/api/learning/stats",
        "/api/suppressed/list",
        "/api/suppressed/add",
        "/api/suppressed/remove",
        "/api/clear_learning",
    ] {
        assert!(js.contains(path), "{path}");
    }
    assert!(js.contains("window.NagiPages"));
}

#[test]
fn userdict_css_has_no_external_references_and_keeps_the_flat_look() {
    let css = asset("/userdict.css");
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
    // ハイコントラストのときも壊れない
    assert!(css.contains("forced-colors: active"));
}

#[test]
fn userdict_script_passes_a_node_syntax_check_when_node_is_available() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/userdict.js");
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
