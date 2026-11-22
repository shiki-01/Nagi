"use strict";
// 「ユーザー辞書」「学習と抑制語」ページ。app.js の追加ページの仕組み（window.NagiPages）に登録する。
// 語の検証・重複の判定・保存・辞書への反映はすべてコア（imserver）が行い、ここは表示と操作だけを持つ。
// 文字列は textContent / DOM API だけで入れる（HTML 文字列の組み立ては使わない）。
(function () {
  var PAGE_SIZE = 50;
  var MAX_IMPORT_BYTES = 5 * 1024 * 1024;
  var SNIPPET = "スニペット";
  var LOCAL_FILES = { "registered.tsv": true, "imported.tsv": true };
  var SORTS = [
    { id: "registered:1", label: "登録が新しい順", sort: "registered", desc: true },
    { id: "registered:0", label: "登録が古い順", sort: "registered", desc: false },
    { id: "reading:0", label: "読み（あ → ん）", sort: "reading", desc: false },
    { id: "reading:1", label: "読み（ん → あ）", sort: "reading", desc: true },
    { id: "surface:0", label: "単語", sort: "surface", desc: false },
    { id: "pos:0", label: "品詞", sort: "pos", desc: false }
  ];

  // ページを開き直しても残す小さな状態（検索語・並べ替え・位置）
  var ud = { query: "", sortId: "registered:1", offset: 0, selected: {}, data: null, posOptions: [] };

  function keyOf(item) { return item.file + "\u0000" + item.reading + "\u0000" + item.surface; }
  function katakanaToHiragana(s) {
    return s.replace(/[\u30a1-\u30f6]/g, function (c) { return String.fromCharCode(c.charCodeAt(0) - 0x60); });
  }
  // コアの valid_reading と同じ範囲（ひらがな・長音・中黒・英数字・ハイフン）。最終的な判定はコア
  function readingProblem(raw) {
    var r = katakanaToHiragana(raw.trim());
    if (r === "") return "読みを入力してください";
    if (!/^[\u3041-\u3096\u309b-\u309f\u30fc\u30fb0-9A-Za-z-]+$/.test(r)) return "読みはひらがなで入力してください（長音・中黒・英数字・ハイフンも使えます）";
    return "";
  }

  // ---- 共通部品 ----
  function makeDialog(ui, title, bodyNodes, buttons, wide) {
    var el = ui.el;
    var dlg = el("dialog", { "class": "ud-dialog" + (wide ? " ud-wide" : ""), "aria-label": title });
    var body = el("div", { "class": "dialog-body" }, [el("h2", { text: title })]);
    bodyNodes.forEach(function (n) { if (n) body.appendChild(n); });
    var bar = el("div", { "class": "buttons end" });
    buttons.forEach(function (b) { bar.appendChild(b); });
    body.appendChild(bar);
    dlg.appendChild(body);
    document.body.appendChild(dlg);
    dlg.addEventListener("close", function () { if (dlg.parentNode) dlg.parentNode.removeChild(dlg); });
    return dlg;
  }
  function field(ui, id, label, control, help) {
    var el = ui.el;
    var node = el("div", { "class": "ud-field" }, [el("label", { "for": id, text: label }), control]);
    if (help) {
      var h = el("p", { "class": "ud-help", id: id + "-help", text: help });
      control.setAttribute("aria-describedby", id + "-help");
      node.appendChild(h);
    }
    return node;
  }
  function textInput(ui, id, value, placeholder) {
    var input = ui.el("input", { type: "text", id: id, "class": "field", autocomplete: "off", spellcheck: "false", placeholder: placeholder || null });
    input.value = value || "";
    return input;
  }
  // 入力欄で Enter を押したら実行する（日本語入力の変換確定の Enter は除く。ボタンの Enter・複数行欄は除く）
  function onEnter(form, run) {
    form.addEventListener("keydown", function (e) {
      if (e.key !== "Enter" || e.isComposing || e.defaultPrevented) return;
      var t = e.target;
      if (!t || t.tagName !== "INPUT" || t.type !== "text") return;
      e.preventDefault();
      run();
    });
    form.addEventListener("submit", function (e) { e.preventDefault(); });
  }
  function confirmDialog(ui, title, text, okLabel) {
    return new Promise(function (resolve) {
      var cancel = ui.button("キャンセル", "", null, function () { dlg.close("cancel"); });
      cancel.setAttribute("autofocus", "");
      var ok = ui.button(okLabel, "danger", null, function () { dlg.close("ok"); });
      var dlg = makeDialog(ui, title, [ui.el("p", { text: text })], [cancel, ok]);
      dlg.addEventListener("close", function () { resolve(dlg.returnValue === "ok"); });
      dlg.showModal();
    });
  }
  function formatTime(unix) {
    if (!unix) return "まだ保存されていません";
    try { return new Date(unix * 1000).toLocaleString("ja-JP", { dateStyle: "medium", timeStyle: "short" }); } catch (e) { return new Date(unix * 1000).toString(); }
  }
  function openCategoryOf(ui, key) {
    ui.api("/api/settings").then(function (screen) {
      var name = "";
      screen.groups.forEach(function (g) { g.items.forEach(function (i) { if (i.key === key) name = g.name; }); });
      var nav = name ? document.querySelector('.nav-item[data-page="' + "cat:" + name.replace(/["\\]/g, "") + '"]') : null;
      if (nav) nav.click();
      else ui.toast("設定のページを開けませんでした。左の一覧から「" + (name || "入力") + "」を選んでください", true);
    }, function (e) { ui.toast(e.message, true); });
  }

  // ==== ユーザー辞書 ====
  function renderUserDict(ui) {
    var el = ui.el;
    var refs = {};

    function currentSort() {
      for (var i = 0; i < SORTS.length; i++) if (SORTS[i].id === ud.sortId) return SORTS[i];
      return SORTS[0];
    }
    function selectedCount() { return Object.keys(ud.selected).length; }

    function load(keepOffset) {
      var s = currentSort();
      if (!keepOffset) ud.offset = 0;
      return ui.api("/api/userdict/list", { query: ud.query, sort: s.sort, descending: s.desc, offset: ud.offset, limit: PAGE_SIZE }).then(function (d) {
        // 削除などで最後のページが空になったら 1 つ戻る
        if (d.items.length === 0 && d.offset > 0) { ud.offset = Math.max(0, d.offset - PAGE_SIZE); return load(true); }
        ud.data = d;
        ud.posOptions = d.pos_options || [];
        draw();
      }, function (e) { ui.toast(e.message, true); });
    }

    // ---- 描画 ----
    function draw() {
      var d = ud.data;
      if (!d) return;
      refs.list.textContent = "";
      var n = selectedCount();
      var from = d.matched === 0 ? 0 : d.offset + 1;
      var to = d.offset + d.items.length;
      var countText = d.matched === d.total ? d.total + " 件" : d.total + " 件中 " + d.matched + " 件が一致";
      if (d.matched > 0) countText += "（" + from + " – " + to + " を表示）";
      refs.metaEl.textContent = d.total + " / " + d.max_entries + " 件";

      var head = el("div", { "class": "ud-head" });
      var all = el("input", { type: "checkbox", "aria-label": "表示中の語をすべて選択" });
      var allSel = d.items.length > 0 && d.items.every(function (i) { return ud.selected[keyOf(i)]; });
      all.checked = allSel;
      all.disabled = d.items.length === 0;
      all.addEventListener("change", function () {
        d.items.forEach(function (i) { if (all.checked) ud.selected[keyOf(i)] = { reading: i.reading, surface: i.surface, file: i.file }; else delete ud.selected[keyOf(i)]; });
        draw();
      });
      head.appendChild(el("span", { "class": "ud-check" }, [all]));
      head.appendChild(el("span", { "class": "ud-count", role: "status", text: n > 0 ? n + " 件を選択中" : countText }));
      if (n > 0) {
        head.appendChild(ui.button("選択を解除", "quiet sm", null, function () { ud.selected = {}; draw(); }));
        head.appendChild(ui.button(n + " 件を削除…", "danger sm", "trash", removeSelected));
      }
      refs.list.appendChild(head);

      if (d.items.length === 0) {
        refs.list.appendChild(el("p", { "class": "ud-empty", text: d.total === 0 ? "登録した語はまだありません。「単語を追加」から登録できます。" : "一致する語がありません。" }));
      }
      d.items.forEach(function (item) { refs.list.appendChild(rowFor(item)); });

      if (d.matched > PAGE_SIZE) {
        var prev = ui.button("前へ", "sm", null, function () { ud.offset = Math.max(0, ud.offset - PAGE_SIZE); load(true); });
        prev.disabled = d.offset === 0;
        var next = ui.button("次へ", "sm", null, function () { ud.offset += PAGE_SIZE; load(true); });
        next.disabled = d.offset + d.items.length >= d.matched;
        refs.list.appendChild(el("div", { "class": "ud-pager" }, [prev, el("span", { text: (Math.floor(d.offset / PAGE_SIZE) + 1) + " / " + Math.ceil(d.matched / PAGE_SIZE) + " ページ" }), next]));
      }
    }

    function rowFor(item) {
      var cb = el("input", { type: "checkbox", "aria-label": "選択: " + item.reading + " " + item.surface.split("\n")[0] });
      cb.checked = !!ud.selected[keyOf(item)];
      cb.addEventListener("change", function () {
        if (cb.checked) ud.selected[keyOf(item)] = { reading: item.reading, surface: item.surface, file: item.file };
        else delete ud.selected[keyOf(item)];
        draw();
      });
      var snippet = item.pos === SNIPPET;
      var shown = item.surface.replace(/\n/g, " ⏎ ");
      var sub = el("div", { "class": "ud-sub" }, [el("span", { "class": "tag" + (snippet ? " snippet" : ""), text: item.pos })]);
      if (item.comment) sub.appendChild(el("span", { text: item.comment }));
      if (!LOCAL_FILES[item.file]) sub.appendChild(el("span", { "class": "ud-file", text: item.file }));
      var label = item.reading + " " + item.surface.split("\n")[0];
      return el("div", { "class": "ud-row" }, [
        el("span", { "class": "ud-check" }, [cb]),
        el("div", { "class": "ud-main" }, [
          el("div", { "class": "ud-line" }, [el("span", { "class": "ud-word", text: shown }), el("span", { "class": "ud-reading", text: item.reading })]),
          sub
        ]),
        el("div", { "class": "ud-actions" }, [
          labelled(ui.button("編集", "quiet sm", null, function () { openEntryDialog(item); }), "編集: " + label),
          labelled(ui.button("削除", "quiet sm danger", null, function () { removeOne(item); }), "削除: " + label)
        ])
      ]);
    }
    function labelled(btn, text) { btn.setAttribute("aria-label", text); return btn; }

    // ---- 追加・編集 ----
    function openEntryDialog(item) {
      var editing = !!item;
      var reading = textInput(ui, "ud-reading", item ? item.reading : "", "例: こうえん");
      var surface = textInput(ui, "ud-surface", item && item.pos !== SNIPPET ? item.surface : "", "例: 公園");
      var snippetText = el("textarea", { id: "ud-snippet", "class": "field", spellcheck: "false", rows: "4" });
      snippetText.value = item && item.pos === SNIPPET ? item.surface : "";
      var pos = el("select", { id: "ud-pos", "class": "field" });
      (ud.posOptions.length ? ud.posOptions : ["名詞"]).forEach(function (p) { pos.appendChild(el("option", { value: p, text: p })); });
      pos.value = item ? item.pos : "名詞";
      var comment = textInput(ui, "ud-comment", item ? item.comment : "", "");
      var error = el("p", { "class": "ud-error", role: "alert" });
      var surfaceField = field(ui, "ud-surface", "単語", surface, "変換結果として出したい表記です。");
      var snippetField = field(ui, "ud-snippet", "定型文", snippetText, "読みを入力して変換すると、この文章（改行を含められます）が候補に出ます。");
      function syncKind() {
        var s = pos.value === SNIPPET;
        surfaceField.hidden = s;
        snippetField.hidden = !s;
      }
      pos.addEventListener("change", syncKind);
      syncKind();
      var cancel = ui.button("キャンセル", "", null, function () { dlg.close("cancel"); });
      var save = ui.button(editing ? "保存" : "登録", "primary", null, null);
      var busy = false;
      function submit() {
        if (busy) return;
        var problem = readingProblem(reading.value);
        reading.setAttribute("aria-invalid", problem ? "true" : "false");
        if (problem) { error.textContent = problem; reading.focus(); return; }
        var snippet = pos.value === SNIPPET;
        var text = snippet ? snippetText.value : surface.value;
        if (text.trim() === "") { error.textContent = snippet ? "定型文を入力してください" : "単語を入力してください"; (snippet ? snippetText : surface).focus(); return; }
        error.textContent = "";
        var entry = { reading: reading.value, surface: text, pos: pos.value, comment: comment.value };
        busy = true;
        save.disabled = true;
        var req = editing
          ? ui.api("/api/userdict/update", { target: { reading: item.reading, surface: item.surface, file: item.file }, entry: entry })
          : ui.api("/api/userdict/add", { entry: entry });
        req.then(function () {
          delete ud.selected[item ? keyOf(item) : ""];
          dlg.close("saved");
          ui.toast(editing ? "保存しました" : "登録しました", false);
          return load(editing);
        }, function (e) {
          busy = false;
          save.disabled = false;
          error.textContent = e.message;
        });
      }
      save.addEventListener("click", submit);
      var form = el("form", { "class": "ud-form", novalidate: "" }, [
        field(ui, "ud-reading", "読み", reading, "ひらがなで入力します（カタカナは自動で直します）。"),
        surfaceField, snippetField,
        field(ui, "ud-pos", "品詞", pos, "迷ったら「名詞」のままにします。"),
        field(ui, "ud-comment", "コメント（省略できます）", comment, ""),
        error
      ]);
      onEnter(form, submit);
      var dlg = makeDialog(ui, editing ? "単語を編集" : "単語を追加", [form], [cancel, save]);
      dlg.showModal();
      reading.focus();
    }

    // ---- 削除 ----
    function removeItems(items) {
      return ui.api("/api/userdict/remove", { items: items }).then(function (r) {
        items.forEach(function (i) { delete ud.selected[keyOf(i)]; });
        ui.toast(r.removed + " 件を削除しました", false);
        return load(true);
      }, function (e) { ui.toast(e.message, true); });
    }
    function removeOne(item) {
      confirmDialog(ui, "この語を削除しますか？", "「" + item.surface.split("\n")[0] + "」（読み: " + item.reading + "）をユーザー辞書から削除します。", "削除する").then(function (ok) {
        if (ok) removeItems([{ reading: item.reading, surface: item.surface, file: item.file }]);
      });
    }
    function removeSelected() {
      var items = Object.keys(ud.selected).map(function (k) { return ud.selected[k]; });
      if (items.length === 0) return;
      confirmDialog(ui, items.length + " 件を削除しますか？", "選択した " + items.length + " 件の語をユーザー辞書から削除します。この操作は取り消せません。", "削除する").then(function (ok) {
        if (ok) removeItems(items);
      });
    }

    // ---- 取り込み ----
    function openImportDialog() {
      var fileInput = el("input", { type: "file", accept: ".txt,.tsv,.csv,.dic,.plist,text/plain", hidden: true });
      var fileName = el("span", { "class": "ud-file-name", text: "ファイルが選ばれていません" });
      var pick = ui.button("ファイルを選ぶ…", "", "file", function () { fileInput.click(); });
      var policy = el("select", { id: "ud-dup", "class": "field" }, [
        el("option", { value: "skip", text: "既存を残す（取り込まない）" }),
        el("option", { value: "overwrite", text: "新しい内容で上書きする" })
      ]);
      var error = el("p", { "class": "ud-error", role: "alert" });
      var result = el("div", { "class": "ud-result", hidden: true, role: "status" });
      var close = ui.button("閉じる", "", null, function () { dlg.close("close"); });
      var run = ui.button("取り込む", "primary", "download", null);
      run.disabled = true;
      var chosen = null;
      var busy = false;
      fileInput.addEventListener("change", function () {
        error.textContent = "";
        result.hidden = true;
        chosen = fileInput.files && fileInput.files[0] ? fileInput.files[0] : null;
        if (chosen && chosen.size > MAX_IMPORT_BYTES) { error.textContent = "ファイルが大きすぎます（上限 5 MB）。"; chosen = null; }
        if (chosen && chosen.size === 0) { error.textContent = "ファイルが空です。"; chosen = null; }
        fileName.textContent = chosen ? chosen.name : "ファイルが選ばれていません";
        run.disabled = !chosen;
      });
      run.addEventListener("click", function () {
        if (!chosen || busy) return;
        busy = true;
        run.disabled = true;
        error.textContent = "";
        readBase64(chosen).then(function (b64) {
          return ui.api("/api/userdict/import", { data_base64: b64, on_duplicate: policy.value });
        }).then(function (r) {
          busy = false;
          showResult(result, r);
          chosen = null;
          fileName.textContent = "取り込みました。続けて別のファイルを選べます";
          fileInput.value = "";
          return load(true);
        }, function (e) {
          busy = false;
          run.disabled = !chosen;
          error.textContent = e.message;
        });
      });
      var body = [
        el("p", { text: "Mozc（Google 日本語入力）の TSV のほか、MS-IME・ATOK・macOS の辞書の書き出しも取り込めます。形式は自動で判定します。5 MB まで。" }),
        el("div", { "class": "ud-form" }, [
          el("div", { "class": "ud-file-line" }, [pick, fileName, fileInput]),
          field(ui, "ud-dup", "同じ読みと単語がすでにあるとき", policy, ""),
          error, result
        ])
      ];
      var dlg = makeDialog(ui, "辞書を取り込む", body, [close, run]);
      dlg.showModal();
      pick.focus();
    }
    function readBase64(file) {
      return new Promise(function (resolve, reject) {
        var reader = new FileReader();
        reader.onload = function () {
          var s = String(reader.result || "");
          var at = s.indexOf(",");
          resolve(at >= 0 ? s.slice(at + 1) : "");
        };
        reader.onerror = function () { reject(new Error("ファイルを読み込めませんでした")); };
        reader.readAsDataURL(file);
      });
    }
    function showResult(box, r) {
      box.textContent = "";
      box.hidden = false;
      var chips = el("div", { "class": "chips" }, [
        ui.badge(r.imported > 0 ? "ok" : "neutral", "取り込み " + r.imported + " 件", "check", false),
        r.replaced > 0 ? ui.badge("info", "上書き " + r.replaced + " 件", "refresh", false) : null,
        ui.badge(r.skipped > 0 ? "warn" : "neutral", "スキップ " + r.skipped + " 件", r.skipped > 0 ? "alert" : "minus", false)
      ]);
      box.appendChild(chips);
      box.appendChild(el("p", { "class": "ud-help", text: "読み取った項目: " + r.total + " 件" }));
      if (r.skipped_reasons.length > 0) {
        var reasons = el("ul", {});
        r.skipped_reasons.forEach(function (x) {
          reasons.appendChild(el("li", { text: x.message + "：" + x.count + " 件" + (r.skipped_lines.length ? "" : "（最初は " + x.first_line + " 行目）") }));
        });
        box.appendChild(reasons);
      }
      if (r.skipped_lines.length > 0) {
        var lines = el("ul", { "class": "ud-skips", tabindex: "0", "aria-label": "スキップした行" });
        r.skipped_lines.forEach(function (x) {
          lines.appendChild(el("li", {}, [el("span", { "class": "ud-ln", text: x.line + " 行目" }), el("span", { text: x.message })]));
        });
        box.appendChild(lines);
        if (r.skipped > r.skipped_lines.length) box.appendChild(el("p", { "class": "ud-help", text: "ほか " + (r.skipped - r.skipped_lines.length) + " 件（先頭の " + r.skipped_lines.length + " 件だけ表示しています）" }));
      }
    }

    // ---- 書き出し ----
    function openExportDialog() {
      ui.api("/api/userdict/export").then(function (r) {
        var preview = el("textarea", { "class": "ud-preview", readonly: "", rows: "8", spellcheck: "false", "aria-label": "書き出す内容" });
        preview.value = r.text;
        var close = ui.button("閉じる", "", null, function () { dlg.close("close"); });
        var copy = ui.button("コピー", "", "copy", function () {
          var done = function () { ui.toast("コピーしました", false); };
          var fail = function () { preview.focus(); preview.select(); ui.toast("自動でコピーできませんでした。選択済みなので Ctrl+C を押してください", true); };
          if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(r.text).then(done, fail); else fail();
        });
        var saveBtn = ui.button("ファイルに保存", "primary", "save", function () { download(r.text); });
        saveBtn.disabled = r.count === 0;
        copy.disabled = r.count === 0;
        var dlg = makeDialog(ui, "辞書を書き出す", [
          el("p", { text: r.count + " 件の語を Mozc の TSV（UTF-8）として書き出します。Google 日本語入力・Mozc・Nagi に取り込めます。" }),
          preview
        ], [close, copy, saveBtn], true);
        dlg.showModal();
        preview.scrollTop = 0;
        saveBtn.focus();
      }, function (e) { ui.toast(e.message, true); });
    }
    function download(text) {
      try {
        var blob = new Blob([text], { type: "text/tab-separated-values;charset=utf-8" });
        var url = URL.createObjectURL(blob);
        var a = el("a", { href: url, download: "nagi-user-dict.tsv", hidden: true });
        document.body.appendChild(a);
        a.click();
        window.setTimeout(function () { URL.revokeObjectURL(url); if (a.parentNode) a.parentNode.removeChild(a); }, 1000);
        ui.toast("書き出しました（ダウンロードを確認してください）", false);
      } catch (e) {
        ui.toast("ファイルに保存できませんでした。「コピー」を使ってください", true);
      }
    }

    // ---- 組み立て ----
    var search = el("input", { type: "text", "class": "field ud-search", placeholder: "読み・単語・コメントで検索", "aria-label": "辞書を検索", autocomplete: "off", spellcheck: "false" });
    search.value = ud.query;
    var timer = 0;
    search.addEventListener("input", function () {
      window.clearTimeout(timer);
      timer = window.setTimeout(function () { ud.query = search.value; load(false); }, 250);
    });
    var sortSel = el("select", { "class": "field", "aria-label": "並べ替え" });
    SORTS.forEach(function (s) { sortSel.appendChild(el("option", { value: s.id, text: s.label })); });
    sortSel.value = ud.sortId;
    sortSel.addEventListener("change", function () { ud.sortId = sortSel.value; load(false); });
    var tools = el("div", { "class": "ud-tools" }, [
      search, sortSel,
      ui.button("単語を追加", "primary", null, function () { openEntryDialog(null); }),
      ui.button("取り込み…", "", "download", openImportDialog),
      ui.button("書き出し…", "", "save", openExportDialog)
    ]);
    refs.list = el("div", { "class": "ud-listbody" });
    var section = ui.section("登録した語", "…", ui.card([tools, refs.list]));
    refs.metaEl = section.querySelector(".section-head .meta");

    var out = ui.frag([
      ui.hero("ユーザー辞書", "よく使う語や固有名詞を登録します。登録した語は変換の候補に出ます。Mozc 形式（TSV）で取り込み・書き出しができます。"),
      section
    ]);
    if (ud.data) draw();
    load(true);
    return out;
  }

  // ==== 学習と抑制語 ====
  function renderLearning(ui) {
    var el = ui.el;
    var refs = {};
    var state = { stats: null, suppressed: null };

    function fmtCount(n) { return String(n) + " 件"; }

    function loadStats() {
      return ui.api("/api/learning/stats").then(function (s) { state.stats = s; drawStats(); }, function (e) { ui.toast(e.message, true); });
    }
    function drawStats() {
      var s = state.stats;
      refs.stats.textContent = "";
      if (!s) return;
      function cell(label, value, small) {
        return el("div", {}, [el("dt", { text: label }), el("dd", { "class": small ? "small" : "", text: value })]);
      }
      refs.stats.appendChild(cell("学習した語", fmtCount(s.learned_entries)));
      refs.stats.appendChild(cell("ユーザー辞書", fmtCount(s.user_dict_entries)));
      refs.stats.appendChild(cell("抑制語", fmtCount(s.suppressed_entries)));
      refs.stats.appendChild(cell("最後に保存した日時", formatTime(s.last_saved_unix), true));
      if (refs.switchInput) {
        refs.switchInput.checked = s.enabled;
        refs.switchState.textContent = s.enabled ? "オン" : "オフ";
      }
    }

    function learningRow() {
      var item = ui.itemByKey("learning.enabled");
      var input = el("input", { type: "checkbox", id: "ud-learn", role: "switch", "class": "switch" });
      var state = el("span", { "class": "switch-state", "aria-hidden": "true" });
      refs.switchInput = input;
      refs.switchState = state;
      input.checked = item ? item.value === true : true;
      state.textContent = input.checked ? "オン" : "オフ";
      input.addEventListener("change", function () {
        var want = input.checked;
        state.textContent = want ? "オン" : "オフ";
        ui.api("/api/update", { changes: [{ key: "learning.enabled", value: { type: "bool", value: want } }] }).then(function (r) {
          ui.setScreen(r.screen);
          if (r.errors && r.errors.length) { ui.toast(r.errors[0].message, true); }
          else ui.toast(want ? "学習をオンにしました" : "学習をオフにしました", false);
          return loadStats();
        }, function (e) { ui.toast(e.message, true); input.checked = !want; state.textContent = input.checked ? "オン" : "オフ"; });
      });
      var row = el("div", { "class": "row clickable" }, [
        el("div", { "class": "row-main" }, [
          el("label", { "class": "row-label", "for": "ud-learn" }, [el("span", { text: "学習する" })]),
          el("p", { "class": "row-desc", text: "確定した変換結果を記録し、次回以降の候補の順位に反映します。パスワード欄などでは自動で止まります。" })
        ]),
        el("div", { "class": "row-ctl" }, [el("div", { "class": "ctl-line" }, [state, input])])
      ]);
      row.addEventListener("click", function (e) {
        if (e.target === input || (e.target.closest && e.target.closest("label, button, a"))) return;
        input.click();
      });
      return row;
    }

    function clearLearning() {
      confirmDialog(ui, "学習データを全削除しますか？", "これまでの変換の学習（よく使う変換・次語予測・「もしかして」の採用状況）がすべて消えます。この操作は取り消せません。ユーザー辞書と抑制語は消えません。", "削除する").then(function (ok) {
        if (!ok) return;
        ui.api("/api/clear_learning").then(function () {
          ui.toast("学習データを削除しました", false);
          return loadStats();
        }, function (e) { ui.toast(e.message, true); });
      });
    }

    // ---- 抑制語 ----
    function loadSuppressed() {
      return ui.api("/api/suppressed/list").then(function (r) { state.suppressed = r.items; drawSuppressed(); }, function (e) { ui.toast(e.message, true); });
    }
    function drawSuppressed() {
      var items = state.suppressed || [];
      refs.supMeta.textContent = items.length + " 件";
      refs.supList.textContent = "";
      if (items.length === 0) {
        refs.supList.appendChild(el("p", { "class": "ud-empty", text: "抑制語はありません。変換候補の中で出てほしくない表記を登録できます。" }));
        return;
      }
      items.forEach(function (it) {
        var rm = ui.button("解除", "quiet sm", null, function () {
          ui.api("/api/suppressed/remove", { reading: it.reading, surface: it.surface }).then(function () {
            ui.toast("解除しました", false);
            return Promise.all([loadSuppressed(), loadStats()]);
          }, function (e) { ui.toast(e.message, true); });
        });
        rm.setAttribute("aria-label", "解除: " + it.reading + " " + it.surface);
        refs.supList.appendChild(el("div", { "class": "ud-row" }, [
          el("div", { "class": "ud-main" }, [el("div", { "class": "ud-line" }, [el("span", { "class": "ud-word", text: it.surface }), el("span", { "class": "ud-reading", text: it.reading + " のとき出さない" })])]),
          el("div", { "class": "ud-actions" }, [rm])
        ]));
      });
    }
    function addSuppressed(readingInput, surfaceInput, error, button) {
      var problem = readingProblem(readingInput.value);
      readingInput.setAttribute("aria-invalid", problem ? "true" : "false");
      if (problem) { error.textContent = problem; readingInput.focus(); return; }
      if (surfaceInput.value.trim() === "") { error.textContent = "出さない表記を入力してください"; surfaceInput.focus(); return; }
      error.textContent = "";
      button.disabled = true;
      ui.api("/api/suppressed/add", { reading: katakanaToHiragana(readingInput.value.trim()), surface: surfaceInput.value.trim() }).then(function () {
        button.disabled = false;
        readingInput.value = "";
        surfaceInput.value = "";
        ui.toast("登録しました。この読みでは候補に出なくなります", false);
        readingInput.focus();
        return Promise.all([loadSuppressed(), loadStats()]);
      }, function (e) { button.disabled = false; error.textContent = e.message; });
    }

    // ---- 曖昧入力 ----
    function settingText(key) {
      var item = ui.itemByKey(key);
      if (!item) return "";
      if (item.kind.type === "bool") return item.value === true ? "オン" : "オフ";
      if (item.kind.type === "choice") {
        var label = "";
        item.kind.options.forEach(function (o) { if (o.value === item.value) label = o.label; });
        return label;
      }
      return item.value === null ? "自動" : String(item.value);
    }
    function fuzzyRow(key, label, desc) {
      var value = settingText(key);
      return el("div", { "class": "kv" }, [
        el("div", {}, [el("div", { "class": "kv-label", text: label }), el("p", { "class": "row-desc", text: desc })]),
        value ? ui.badge(value === "オフ" ? "neutral" : "info", value, value === "オフ" ? "minus" : "check", false) : null
      ]);
    }

    // ---- 組み立て ----
    refs.stats = el("dl", { "class": "ud-stat" });
    var clear = ui.button("学習データを全削除…", "danger", "trash", clearLearning);
    var learnCard = ui.card([
      learningRow(),
      refs.stats,
      el("div", { "class": "card-pad stack" }, [
        el("p", { "class": "muted small", text: "ここに出るのは件数と時刻だけです。学習した語や入力した内容は表示しません。" }),
        el("div", { "class": "buttons" }, [clear])
      ])
    ]);

    var supReading = textInput(ui, "ud-sup-reading", "", "例: こうえん");
    var supSurface = textInput(ui, "ud-sup-surface", "", "例: 講演");
    var supError = el("p", { "class": "ud-error", role: "alert" });
    var supAdd = ui.button("追加", "primary", null, function () { addSuppressed(supReading, supSurface, supError, supAdd); });
    var supForm = el("form", { "class": "card-pad stack", novalidate: "" }, [
      el("p", { "class": "muted", text: "「この読みのとき、この表記を候補に出さない」を登録します。毎回出てくる不要な表記を消したいときに使います（読みそのままの表記は残ります）。" }),
      el("div", { "class": "ud-addline" }, [field(ui, "ud-sup-reading", "読み", supReading, ""), field(ui, "ud-sup-surface", "出さない表記", supSurface, ""), supAdd]),
      supError
    ]);
    onEnter(supForm, function () { addSuppressed(supReading, supSurface, supError, supAdd); });
    refs.supList = el("div", { "class": "ud-listbody" });
    var supCard = ui.card([supForm, refs.supList]);
    var supSection = ui.section("抑制語", "…", supCard);
    refs.supMeta = supSection.querySelector(".section-head .meta");

    var fuzzyCard = ui.card([
      el("div", { "class": "card-pad stack" }, [
        el("p", { text: "キーの打ち間違い（隣のキーの押し間違い・打ち忘れ・打ち順の入れ替え・濁点の付け忘れなど）を、「もしかして」の候補として変換候補に混ぜます。入力中の文字は打った通りのままで、自動では置き換えません。" }),
        el("p", { "class": "muted", text: "「確定した直後の打ち間違いを見直す」をオンにすると、文節を確定して次を打ち始めたとき、直前の文節が打ち間違いだった可能性を控えめに知らせます（確定した内容は保存しません）。" })
      ]),
      fuzzyRow("input.fuzzy.enabled", "タイプミスの補正候補（曖昧入力）", "「もしかして」候補を出すか"),
      fuzzyRow("input.fuzzy.strength", "補正の強さ", "強いほど拾える代わりに、不要な候補も増えます"),
      fuzzyRow("input.fuzzy.retro_review", "確定した直後の打ち間違いを見直す（遡及再評価）", "確定した直前の文節の修正候補を出すか"),
      el("div", { "class": "card-pad" }, [el("div", { "class": "buttons" }, [ui.button("入力の設定を開く", "", "sliders", function () { openCategoryOf(ui, "input.fuzzy.enabled"); })])])
    ]);

    var out = ui.frag([
      ui.hero("学習と抑制語", "変換の学習データと、候補に出したくない語を管理します。"),
      ui.section("学習", "", learnCard),
      supSection,
      ui.section("曖昧入力（もしかして候補）", "", fuzzyCard)
    ]);
    loadStats();
    loadSuppressed();
    return out;
  }

  window.NagiPages = window.NagiPages || [];
  window.NagiPages.push({ id: "userdict", title: "ユーザー辞書", icon: "file", render: renderUserDict });
  window.NagiPages.push({ id: "learning", title: "学習と抑制語", icon: "eye", render: renderLearning });
})();
