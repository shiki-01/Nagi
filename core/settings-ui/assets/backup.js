"use strict";
// 「バックアップと移行」ページ。app.js の追加ページの仕組み（window.NagiPages）に登録する。
// バックアップの検証・差分・適用・自動バックアップ・復元はすべてコア（imserver）が行い、ここは表示と操作だけを持つ。
// バックアップには入力内容（辞書・学習）が含まれるので、画面に出すのは件数・項目名・設定の差分だけ
// （辞書の語・学習の中身は表示しない）。文字列は textContent / DOM API だけで入れる。
(function () {
  var MAX_FILE_BYTES = 20 * 1024 * 1024;
  var ITEMS = [
    { id: "settings", label: "設定", desc: "config.toml の内容です。キー割当（[keymap.*]）も含みます。", mode: "置き換え" },
    { id: "user_dict", label: "ユーザー辞書", desc: "登録した語とスニペット（user_dict フォルダの辞書ファイル）です。", mode: "追加" },
    { id: "suppressed", label: "抑制語", desc: "「この読みのときこの表記を出さない」の登録です。", mode: "追加" },
    { id: "learning", label: "学習データ", desc: "変換の学習結果（よく使う変換・次語予測）です。サイズが大きくなることがあります。", mode: "置き換え" }
  ];
  function itemInfo(id) {
    for (var i = 0; i < ITEMS.length; i++) if (ITEMS[i].id === id) return ITEMS[i];
    return { id: id, label: id, desc: "", mode: "" };
  }
  var PLATFORM_NAMES = { windows: "Windows", macos: "macOS", linux: "Linux", ios: "iOS", android: "Android" };
  function platformName(p) { return PLATFORM_NAMES[p] || p || "不明"; }

  function pad(n) { return n < 10 ? "0" + n : String(n); }
  // ダウンロードの既定のファイル名（この PC の時刻。nagi-backup-20261122-193045.json）
  function localFileName() {
    var d = new Date();
    return "nagi-backup-" + d.getFullYear() + pad(d.getMonth() + 1) + pad(d.getDate()) + "-" + pad(d.getHours()) + pad(d.getMinutes()) + pad(d.getSeconds()) + ".json";
  }
  function formatTime(iso) {
    if (!iso) return "不明";
    var d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    try { return d.toLocaleString("ja-JP", { dateStyle: "medium", timeStyle: "short" }); } catch (e) { return d.toString(); }
  }
  function formatSize(bytes) {
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1024 * 1024) return Math.max(1, Math.round(bytes / 1024)) + " KB";
    return (bytes / (1024 * 1024)).toFixed(1) + " MB";
  }
  function byteLength(text) {
    try { return new Blob([text]).size; } catch (e) { return text.length; }
  }

  // ---- ダイアログ（ユーザー辞書ページと同じ見た目。.ud-dialog は userdict.css） ----
  function makeDialog(ui, title, bodyNodes, buttons) {
    var el = ui.el;
    var dlg = el("dialog", { "class": "ud-dialog", "aria-label": title });
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
  function confirmDialog(ui, title, nodes, okLabel, okClass) {
    return new Promise(function (resolve) {
      var cancel = ui.button("キャンセル", "", null, function () { dlg.close("cancel"); });
      cancel.setAttribute("autofocus", "");
      var ok = ui.button(okLabel, okClass || "primary", null, function () { dlg.close("ok"); });
      var dlg = makeDialog(ui, title, nodes, [cancel, ok]);
      dlg.addEventListener("close", function () { resolve(dlg.returnValue === "ok"); });
      dlg.showModal();
    });
  }
  // 自動でコピーできないときに、全文を選択済みで見せる
  function copyFallback(ui, text) {
    var area = ui.el("textarea", { "class": "report-text", readonly: "", rows: "12", spellcheck: "false", "aria-label": "バックアップの内容" });
    area.value = text;
    var close = ui.button("閉じる", "primary", null, function () { dlg.close("ok"); });
    close.setAttribute("autofocus", "");
    var dlg = makeDialog(ui, "内容を選択しました", [
      ui.el("p", { text: "自動でコピーできませんでした。下の内容を選択済みです。Ctrl+C でコピーして、テキストファイルなどに保存してください。" }),
      area
    ], [close]);
    dlg.showModal();
    area.focus();
    area.select();
  }

  function renderBackup(ui) {
    var el = ui.el;
    var refs = { exportBoxes: {} };
    var st = { stats: null, backups: null, backupDir: "", keep: 5, parsed: null, fileName: "", fileSize: 0, preview: null, picked: {}, busy: false, last: null };

    function countOf(id) {
      var s = st.stats;
      if (!s) return null;
      if (id === "user_dict") return s.user_dict_entries;
      if (id === "suppressed") return s.suppressed_entries;
      if (id === "learning") return s.learned_entries;
      return null;
    }
    function itemNoun(id) { return id === "user_dict" ? " 語" : " 件"; }

    // ================= 書き出し =================
    function exportRow(item) {
      var id = "bk-ex-" + item.id;
      var box = el("input", { type: "checkbox", id: id, "data-item": item.id });
      box.checked = item.id !== "learning";
      box.addEventListener("change", updateExportButtons);
      refs.exportBoxes[item.id] = box;
      var side = el("div", { "class": "bk-side" });
      refs["count-" + item.id] = side;
      if (item.id === "learning") side.appendChild(ui.badge("warn", "入力の履歴を含む", "alert", false));
      var title = el("label", { "class": "bk-title", "for": id, text: item.label });
      var desc = el("p", { "class": "bk-desc", text: item.desc });
      var body = [title, desc];
      if (item.id === "learning") body.push(el("p", { "class": "bk-desc bk-caution", text: "入力した内容の履歴が含まれます。ファイルの保管・受け渡しに注意してください。" }));
      return el("div", { "class": "bk-row" }, [el("div", { "class": "bk-check" }, [box]), el("div", { "class": "bk-main" }, body), side]);
    }
    function drawCounts() {
      ITEMS.forEach(function (item) {
        var side = refs["count-" + item.id];
        if (!side) return;
        var n = countOf(item.id);
        var old = side.querySelector(".bk-count");
        if (old) old.parentNode.removeChild(old);
        if (n !== null) side.insertBefore(el("span", { "class": "bk-count", text: n + itemNoun(item.id) }), side.firstChild);
      });
    }
    function selectedForExport() {
      var out = [];
      ITEMS.forEach(function (item) {
        var box = refs.exportBoxes[item.id];
        if (box && box.checked) out.push(item.id);
      });
      return out;
    }
    function updateExportButtons() {
      var none = selectedForExport().length === 0 || st.busy;
      refs.exportBtn.disabled = none;
      refs.copyBtn.disabled = none;
    }
    function showExportNote(text, isError, warnings) {
      refs.exportNote.textContent = "";
      refs.exportNote.className = "note-line bk-note" + (isError ? " error" : "");
      if (!text) return;
      refs.exportNote.appendChild(el("span", { text: text }));
      (warnings || []).forEach(function (w) { refs.exportNote.appendChild(el("span", { "class": "bk-warnline", text: w })); });
    }
    function doExport(how) {
      var include = selectedForExport();
      if (include.length === 0) { ui.toast("書き出す項目を選んでください", true); return; }
      st.busy = true;
      updateExportButtons();
      ui.api("/api/backup/export", { include: include }).then(function (r) {
        var text = JSON.stringify(r.backup, null, 2);
        var name = localFileName();
        if (how === "copy") {
          var done = function () { ui.toast("バックアップの内容をコピーしました", false); showExportNote("内容をクリップボードにコピーしました（" + formatSize(byteLength(text)) + "）。", false, r.warnings); };
          if (navigator.clipboard && navigator.clipboard.writeText) {
            navigator.clipboard.writeText(text).then(done, function () { copyFallback(ui, text); showExportNote("", false); });
          } else {
            copyFallback(ui, text);
          }
        } else {
          var url = null;
          try {
            url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
            var a = el("a", { href: url, download: name, hidden: "" });
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            window.setTimeout(function () { URL.revokeObjectURL(url); }, 30000);
            showExportNote(name + " を書き出しました（" + formatSize(byteLength(text)) + "）。保存されない場合は「内容をコピー」を使ってください。", false, r.warnings);
            ui.toast("バックアップを書き出しました", false);
          } catch (e) {
            showExportNote("ファイルとして書き出せませんでした。「内容をコピー」を使ってください。", true, r.warnings);
          }
        }
      }, function (e) {
        showExportNote(e.message, true, []);
        ui.toast(e.message, true);
      }).then(function () { st.busy = false; updateExportButtons(); });
    }

    // ================= 読み込み =================
    function pickFile() { refs.file.click(); }
    function resetImport() {
      st.parsed = null; st.preview = null; st.picked = {}; st.fileName = "";
      drawImport();
    }
    function onFile() {
      var f = refs.file.files && refs.file.files[0];
      refs.file.value = "";
      if (!f) return;
      st.last = null;
      if (f.size > MAX_FILE_BYTES) { importError("ファイルが大きすぎます（上限 20 MB）。"); return; }
      st.fileName = f.name;
      st.fileSize = f.size;
      var read = f.text ? f.text() : new Promise(function (resolve, reject) {
        var fr = new FileReader();
        fr.onload = function () { resolve(String(fr.result)); };
        fr.onerror = function () { reject(new Error("read")); };
        fr.readAsText(f);
      });
      read.then(function (text) {
        var data;
        try { data = JSON.parse(text); } catch (e) { importError("JSON として読めません。Nagi が書き出したバックアップファイルを選んでください。"); return; }
        var problem = shapeProblem(data);
        if (problem) { importError(problem); return; }
        st.parsed = data;
        st.preview = null;
        refs.importNote.textContent = "";
        refs.importNote.className = "note-line bk-note";
        refs.importBusy = true;
        drawImport();
        ui.api("/api/backup/preview", { backup: data }).then(function (p) {
          st.preview = p;
          st.picked = {};
          p.items.forEach(function (it) { st.picked[it.kind] = it.present && it.applicable; });
          refs.importBusy = false;
          drawImport();
          ui.announce("プレビューを表示しました");
        }, function (e) { refs.importBusy = false; importError(e.message); });
      }, function () { importError("ファイルを読み込めませんでした。"); });
    }
    function shapeProblem(data) {
      if (!data || typeof data !== "object" || Array.isArray(data)) return "Nagi のバックアップファイルではありません。";
      if (typeof data.version !== "number" || !Array.isArray(data.entries)) return "Nagi のバックアップファイルではありません（版または項目がありません）。";
      for (var i = 0; i < data.entries.length; i++) {
        var e = data.entries[i];
        if (!e || typeof e.kind !== "string" || typeof e.name !== "string" || typeof e.text !== "string") return "バックアップの形式が正しくありません（" + (i + 1) + " 番目の項目）。";
      }
      return "";
    }
    function importError(message) {
      st.parsed = null; st.preview = null; st.picked = {};
      refs.importBusy = false;
      drawImport();
      refs.importNote.textContent = message;
      refs.importNote.className = "note-line bk-note error";
    }

    function chosenKinds() {
      var out = [];
      if (!st.preview) return out;
      st.preview.items.forEach(function (it) { if (st.picked[it.kind] && it.applicable) out.push(it.kind); });
      return out;
    }
    function badgeLine(kind, text) {
      return el("p", { "class": "bk-issue " + kind }, [ui.badge(kind === "ng" ? "ng" : "warn", kind === "ng" ? "エラー" : "注意", kind === "ng" ? "x" : "alert", false), el("span", { text: text })]);
    }
    function sameSummary(it) {
      if (it.kind === "settings") {
        if (it.changed === 0 && it.added === 0 && it.removed === 0) return "現在の設定と同じです（" + it.count + " 項目）。";
        var parts = [];
        if (it.changed) parts.push("変更 " + it.changed);
        if (it.added) parts.push("追加 " + it.added);
        if (it.removed) parts.push("既定に戻る " + it.removed);
        if (it.same) parts.push("変更なし " + it.same);
        return parts.join(" ・ ") + "（バックアップの設定 " + it.count + " 項目 / 現在の指定 " + it.current + " 項目）";
      }
      if (it.kind === "learning") return "バックアップ " + it.count + " 件。適用すると、現在の " + it.current + " 件はバックアップの内容に置き換わります。";
      var noun = it.kind === "user_dict" ? "語" : "件";
      if (it.count === 0) return "バックアップに含まれる " + noun + "はありません。";
      return "バックアップ " + it.count + " " + noun + "のうち、新しく追加 " + it.added + " ・ すでにある " + it.same + "（現在の登録を優先）。現在 " + it.current + " " + noun + "。";
    }
    function diffList(it) {
      if (!it.changes || it.changes.length === 0) return null;
      var total = it.changed + it.added + it.removed;
      var list = el("ul", { "class": "bk-diff" });
      it.changes.forEach(function (c) {
        var beforeNone = c.before === null;
        var afterNone = c.after === null;
        var before = beforeNone ? "（既定）" : c.before;
        var after = afterNone ? "（既定に戻る）" : c.after;
        list.appendChild(el("li", {}, [
          el("span", { "class": "bk-diff-key", text: c.label }),
          el("span", { "class": "bk-diff-val" }, [el("span", { "class": "bk-from" + (beforeNone ? " none" : ""), text: before }), el("span", { "class": "bk-arrow", "aria-hidden": "true", text: "→" }), el("span", { "class": "bk-to", text: after })])
        ]));
      });
      var more = total - it.changes.length;
      if (more > 0) list.appendChild(el("li", { "class": "bk-more", text: "ほか " + more + " 件" }));
      var det = el("details", { "class": "bk-details" }, [el("summary", { text: "変更の内容を見る（" + total + " 件）" }), list]);
      return det;
    }
    function previewRow(it) {
      var info = itemInfo(it.kind);
      var id = "bk-im-" + it.kind;
      var box = el("input", { type: "checkbox", id: id, "data-item": it.kind });
      box.disabled = !it.applicable;
      box.checked = !!st.picked[it.kind] && it.applicable;
      box.addEventListener("change", function () { st.picked[it.kind] = box.checked; updateApply(); });
      var main = el("div", { "class": "bk-main" });
      var head = el("div", { "class": "bk-headline" }, [el("label", { "class": "bk-title", "for": id, text: info.label })]);
      if (it.present) head.appendChild(el("span", { "class": "tag bk-mode", text: info.mode === "追加" ? "新しいものを追加" : "置き換え" }));
      main.appendChild(head);
      if (!it.present) {
        main.appendChild(el("p", { "class": "bk-desc", text: "このバックアップには含まれていません。" }));
      } else {
        if (it.applicable) main.appendChild(el("p", { "class": "bk-desc", text: sameSummary(it) }));
        it.errors.forEach(function (e) { main.appendChild(badgeLine("ng", (e.line > 0 ? e.line + " 行目: " : "") + e.message)); });
        it.warnings.forEach(function (w) { main.appendChild(badgeLine("warn", w)); });
        var d = it.applicable ? diffList(it) : null;
        if (d) main.appendChild(d);
      }
      return el("div", { "class": "bk-row" + (it.present ? "" : " off") }, [el("div", { "class": "bk-check" }, [box]), main]);
    }
    function drawImport() {
      var box = refs.importBox;
      box.textContent = "";
      if (refs.importBusy) {
        box.hidden = false;
        box.appendChild(el("div", { "class": "bk-working" }, [ui.badge("info", "確認中", "spinner", true), el("span", { text: "バックアップの内容を確認しています…" })]));
        return;
      }
      var p = st.preview;
      if (!p) { box.hidden = true; return; }
      box.hidden = false;
      var meta = el("div", { "class": "bk-meta" }, [
        el("div", { "class": "bk-metaline" }, [el("span", { "class": "bk-file", text: st.fileName }), el("span", { "class": "bk-dim", text: " ・ " + formatSize(st.fileSize) })]),
        el("div", { "class": "bk-metaline bk-dim" }, [
          el("span", { text: formatTime(p.created_at) + " に作成" }),
          el("span", { text: " ・ Nagi " + (p.app_version || "不明") }),
          el("span", { text: " ・ " + platformName(p.platform) })
        ])
      ]);
      var head = el("div", { "class": "bk-previewhead" }, [meta, p.same_platform ? ui.badge("ok", "同じ OS", "check", false) : ui.badge("warn", "別の OS", "alert", false)]);
      var kids = [head];
      if (p.warnings.length) {
        var w = el("div", { "class": "bk-issues" });
        p.warnings.forEach(function (t) { w.appendChild(badgeLine("warn", t)); });
        kids.push(w);
      }
      var list = el("div", { "class": "bk-list" });
      p.items.forEach(function (it) { list.appendChild(previewRow(it)); });
      kids.push(list);
      refs.applyBtn = ui.button("選んだ項目を適用…", "primary", "download", askApply);
      refs.cancelBtn = ui.button("読み込みをやめる", "quiet", null, resetImport);
      kids.push(el("div", { "class": "bk-actions" }, [refs.applyBtn, refs.cancelBtn]));
      kids.forEach(function (k) { box.appendChild(k); });
      updateApply();
    }
    function updateApply() {
      if (refs.applyBtn) refs.applyBtn.disabled = st.busy || chosenKinds().length === 0;
    }

    function askApply() {
      var kinds = chosenKinds();
      if (kinds.length === 0) return;
      var lines = [];
      kinds.forEach(function (k) {
        if (k === "settings") lines.push("設定（キー割当を含む）: 現在の設定を上書きします。");
        else if (k === "user_dict") lines.push("ユーザー辞書: 新しい語だけを追加します（今ある語は消えません）。");
        else if (k === "suppressed") lines.push("抑制語: 新しいものだけを追加します（今あるものは消えません）。");
        else if (k === "learning") lines.push("学習データ: 現在の学習データをバックアップの内容に置き換えます。");
      });
      var list = el("ul", { "class": "bk-confirm-list" });
      lines.forEach(function (t) { list.appendChild(el("li", { text: t })); });
      confirmDialog(ui, "バックアップを適用しますか？", [
        el("p", { text: "現在の設定を上書きします。適用前のバックアップを自動で保存します。" }),
        list,
        el("p", { "class": "muted small", text: "途中で失敗したときは、自動で元に戻します。適用したあとも「元に戻す」からいつでも戻せます。" })
      ], "適用する", "primary").then(function (ok) { if (ok) doApply(kinds); });
    }
    function refreshScreen() {
      return ui.api("/api/settings").then(function (s) { ui.setScreen(s); }, function () { /* 設定の再読み込みに失敗しても、適用の結果は表示する */ });
    }
    function doApply(kinds) {
      st.busy = true;
      updateApply();
      ui.api("/api/backup/apply", { backup: st.parsed, apply: kinds }).then(function (r) {
        st.last = { kind: "apply", result: r };
        ui.toast("バックアップを適用しました", false);
        ui.announce("バックアップを適用しました");
        st.parsed = null; st.preview = null; st.picked = {};
        drawImport();
        drawResult();
        return Promise.all([refreshScreen(), loadStats(), loadBackups()]);
      }, function (e) {
        st.last = { kind: "error", message: e.message };
        ui.toast(e.message, true);
        drawResult();
        return loadBackups();
      }).then(function () { st.busy = false; updateApply(); });
    }
    function drawResult() {
      var box = refs.resultBox;
      box.textContent = "";
      var l = st.last;
      if (!l) { box.hidden = true; return; }
      box.hidden = false;
      if (l.kind === "error") {
        box.appendChild(el("div", { "class": "result-head" }, [ui.badge("ng", "失敗", "x", false), el("span", { text: "適用できませんでした" })]));
        box.appendChild(el("p", { "class": "result-text", text: l.message }));
        return;
      }
      var r = l.result;
      var title = l.kind === "restore" ? "元に戻しました" : "バックアップを適用しました";
      box.appendChild(el("div", { "class": "result-head" }, [ui.badge("ok", "完了", "check", false), el("span", { text: title })]));
      var ul = el("ul", { "class": "bk-resultlist" });
      r.applied.forEach(function (a) {
        var info = itemInfo(a.kind);
        var text = info.label + ": ";
        if (a.kind === "settings") text += a.count + " 項目";
        else if (a.kind === "user_dict") text += l.kind === "restore" ? a.count + " ファイル" : "新しく " + a.count + " 語を追加";
        else if (a.kind === "suppressed") text += a.count + " 件" + (l.kind === "restore" ? "" : "（追加後の合計）");
        else text += a.count + " 件";
        ul.appendChild(el("li", { text: text }));
      });
      box.appendChild(ul);
      box.appendChild(el("p", { "class": "result-text", text: "この操作の前の状態を自動で保存しました（" + r.auto_backup + "）。下の「元に戻す」で戻せます。" }));
      (r.warnings || []).forEach(function (w) { box.appendChild(badgeLine("warn", w)); });
    }

    // ================= 自動バックアップと元に戻す =================
    function loadBackups() {
      return ui.api("/api/backup/list").then(function (r) {
        st.backups = r.backups; st.backupDir = r.dir || ""; st.keep = r.keep;
        drawBackups();
      }, function (e) { ui.toast(e.message, true); });
    }
    // 件数（辞書・抑制語はファイルの内容から、学習は統計から。取れなくても件数が出ないだけ）
    function loadStats() {
      var stats = { learned_entries: null, user_dict_entries: null, suppressed_entries: null };
      return Promise.all([
        ui.api("/api/learning/stats").then(function (s) { stats.learned_entries = s.learned_entries; }, function () {}),
        ui.api("/api/userdict/list", { limit: 1 }).then(function (r) { stats.user_dict_entries = r.total; }, function () {}),
        ui.api("/api/suppressed/list").then(function (r) { stats.suppressed_entries = r.items.length; }, function () {})
      ]).then(function () { st.stats = stats; drawCounts(); });
    }
    function drawBackups() {
      var box = refs.backupList;
      box.textContent = "";
      var list = st.backups || [];
      refs.undoBtn.disabled = list.length === 0;
      if (refs.dirLine) refs.dirLine.textContent = st.backupDir ? "保存先: " + st.backupDir : "";
      if (list.length === 0) {
        box.appendChild(el("p", { "class": "bk-empty", text: "まだありません。バックアップを適用すると、その直前の状態がここに自動で保存されます（最新 " + st.keep + " 世代）。" }));
        return;
      }
      list.forEach(function (b, i) {
        var tags = el("div", { "class": "bk-tags" });
        b.kinds.forEach(function (k) { tags.appendChild(el("span", { "class": "tag bk-kind", text: itemInfo(k).label })); });
        var restore = ui.button("この状態に戻す…", "quiet sm", null, function () { askRestore(b); });
        restore.setAttribute("aria-label", formatTime(b.created_at) + " の状態に戻す");
        box.appendChild(el("div", { "class": "bk-row" }, [
          el("div", { "class": "bk-main" }, [
            el("div", { "class": "bk-headline" }, [el("span", { "class": "bk-title", text: formatTime(b.created_at) }), i === 0 ? el("span", { "class": "tag bk-latest", text: "最新" }) : null]),
            el("p", { "class": "bk-desc", text: "Nagi " + (b.app_version || "不明") + " ・ " + formatSize(b.size) }),
            tags
          ]),
          el("div", { "class": "bk-side" }, [restore])
        ]));
      });
    }
    function askRestore(b) {
      var target = b || (st.backups && st.backups[0]);
      if (!target) return;
      var kinds = target.kinds.map(function (k) { return itemInfo(k).label; }).join("・");
      confirmDialog(ui, "この状態に戻しますか？", [
        el("p", { text: formatTime(target.created_at) + " に保存した自動バックアップ（" + kinds + "）の状態に戻します。" }),
        el("p", { "class": "muted small", text: "今の状態も、戻す前に自動で保存します。もう一度「元に戻す」を押すと、この操作を取り消せます。" })
      ], "元に戻す", "primary").then(function (ok) { if (ok) doRestore(target.name); });
    }
    function doRestore(name) {
      st.busy = true;
      refs.undoBtn.disabled = true;
      ui.api("/api/backup/restore", { name: name }).then(function (r) {
        st.last = { kind: "restore", result: r };
        ui.toast("元に戻しました", false);
        ui.announce("元に戻しました");
        drawResult();
        return Promise.all([refreshScreen(), loadStats(), loadBackups()]);
      }, function (e) {
        st.last = { kind: "error", message: e.message };
        ui.toast(e.message, true);
        drawResult();
      }).then(function () { st.busy = false; drawBackups(); updateApply(); });
    }

    // ================= 組み立て =================
    refs.exportBtn = ui.button("ファイルに書き出す", "primary", "download", function () { doExport("file"); });
    refs.copyBtn = ui.button("内容をコピー", "", "copy", function () { doExport("copy"); });
    refs.exportNote = el("p", { "class": "note-line bk-note", role: "status", "aria-live": "polite" });
    var exportCard = ui.card([
      el("div", { "class": "bk-list" }, ITEMS.map(exportRow)),
      el("div", { "class": "card-pad stack" }, [
        el("p", { "class": "muted small", text: "1 つの JSON ファイル（nagi-backup-日時.json）にまとめます。中身は読める文字だけで、別の PC の Nagi に読み込めます。この PC のパスを含む設定（モデルのファイルなど）は書き出しません。" }),
        el("div", { "class": "buttons" }, [refs.exportBtn, refs.copyBtn]),
        refs.exportNote
      ])
    ]);

    refs.file = el("input", { type: "file", accept: ".json,application/json", "class": "sr-only", "aria-label": "バックアップファイル", tabindex: "-1" });
    refs.file.addEventListener("change", onFile);
    refs.importNote = el("p", { "class": "note-line bk-note", role: "status", "aria-live": "polite" });
    refs.importBox = el("div", { "class": "bk-preview", hidden: "" });
    refs.resultBox = el("div", { "class": "result bk-result", role: "status", "aria-live": "polite", hidden: "" });
    var importCard = ui.card([
      el("div", { "class": "card-pad stack" }, [
        el("p", { "class": "muted", text: "書き出したバックアップファイルを選ぶと、適用する前に中身と現在との違いを確認できます。上限は 20 MB です。" }),
        el("div", { "class": "buttons" }, [ui.button("バックアップファイルを選ぶ…", "", "file", pickFile), refs.file]),
        refs.importNote
      ]),
      refs.importBox,
      refs.resultBox
    ]);

    refs.undoBtn = ui.button("直前の状態に戻す…", "", "refresh", function () { askRestore(null); });
    refs.undoBtn.disabled = true;
    refs.backupList = el("div", { "class": "bk-list" });
    var backupsCard = ui.card([
      el("div", { "class": "card-pad stack" }, [
        el("p", { "class": "muted", text: "バックアップを適用する直前に、今の状態を自動で保存します。新しい順に " + st.keep + " 世代まで残り、古いものは自動で消えます。" }),
        el("div", { "class": "buttons" }, [refs.undoBtn]),
        el("p", { "class": "bk-dir small", id: "bk-dir" })
      ]),
      refs.backupList
    ]);
    refs.dirLine = backupsCard.querySelector("#bk-dir");

    var out = ui.frag([
      ui.hero("バックアップと移行", "設定・キー割当・辞書・学習データを 1 つのファイルにまとめて保存し、別の PC への移行や、元に戻すために使います。"),
      ui.section("書き出し", "", exportCard),
      ui.section("読み込み", "", importCard),
      ui.section("元に戻す", "", backupsCard)
    ]);
    updateExportButtons();
    drawCounts();
    loadStats();
    loadBackups();
    return out;
  }

  window.NagiPages = window.NagiPages || [];
  window.NagiPages.push({ id: "backup", title: "バックアップと移行", icon: "save", group: "system", order: 55, render: renderBackup });
})();
