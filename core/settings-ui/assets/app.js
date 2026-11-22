"use strict";
// Nagi 設定画面。項目はすべてコアの記述子（/api/settings）から組み立てる。
// 文字列は textContent / DOM API だけで入れる（HTML 文字列の組み立ては使わない）。
(function () {
  var NS = "nagi.settings.ui.";
  var token = document.querySelector('meta[name="nagi-token"]').content;
  var RESTART_HINT = "変更を反映するには入力方式を切り替え直すかアプリを開き直してください";
  var DISCONNECTED = "設定画面のサーバーに接続できません。この画面を閉じて、もう一度「設定…」から開いてください。";

  // ホスト（WebView2）との取り決め: window.chrome.webview.postMessage({type: "close-request" | "open-external", ...})。
  // ブラウザ（--web）では host が無く、何も送らない
  var host = window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === "function" ? window.chrome.webview : null;
  function postHost(message) {
    if (!host) return false;
    try { host.postMessage(message); return true; } catch (e) { return false; }
  }

  // ---- 永続化（利用者ごとの小さな記憶。使えなくても動く） ----
  function loadPref(name) {
    try { return window.localStorage.getItem(NS + name); } catch (e) { return null; }
  }
  function savePref(name, value) {
    try { window.localStorage.setItem(NS + name, value); } catch (e) { /* 保存できなくてもよい */ }
  }

  // ---- 状態 ----
  var screen = null;          // /api/settings の応答
  var itemByKey = {};
  var detail = loadPref("detail") === "1";
  var page = loadPref("page") || "";
  var errors = {};
  var rowRefs = {};
  var idCounter = 0;
  var upd = { busy: false, view: null };
  var updUI = null;
  var diag = { status: null, timer: 0, savedPath: "", ui: null };

  var mainEl = document.getElementById("main");
  var pageEl = document.getElementById("page");
  var navEl = document.getElementById("nav");
  var bannerEl = document.getElementById("banner");
  var toastsEl = document.getElementById("toasts");
  var srLive = document.getElementById("sr-live");
  var detailToggle = document.getElementById("detail-toggle");

  // ---- 小さな DOM 補助 ----
  function el(tag, attrs, children) {
    var node = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      var v = attrs[k];
      if (v === null || v === undefined || v === false) return;
      if (k === "text") node.textContent = v;
      else if (v === true) node.setAttribute(k, "");
      else node.setAttribute(k, v);
    });
    (children || []).forEach(function (c) { if (c) node.appendChild(c); });
    return node;
  }
  var SVG_NS = "http://www.w3.org/2000/svg";
  function svgEl(tag, attrs) {
    var node = document.createElementNS(SVG_NS, tag);
    Object.keys(attrs || {}).forEach(function (k) { node.setAttribute(k, attrs[k]); });
    return node;
  }
  var ICONS = {
    keyboard: ["M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2z", "M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"],
    convert: ["M5 8h13l-3.5-3.5M19 16H6l3.5 3.5"],
    list: ["M5 6h14M5 12h14M5 18h9"],
    bell: ["M6 9a6 6 0 0 1 12 0c0 6 2.5 8 2.5 8h-17S6 15 6 9z", "M10.3 21a2 2 0 0 0 3.4 0"],
    monitor: ["M3 5h18v11H3z", "M8 20h8M12 16v4"],
    gauge: ["M4.5 17a8 8 0 1 1 15 0", "M12 13l4-4M12 17h.01"],
    book: ["M4 5a2 2 0 0 1 2-2h13v15H6a2 2 0 0 0-2 2z", "M4 20a2 2 0 0 0 2 2h13v-4"],
    sparkle: ["M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z", "M18.5 16l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7z"],
    download: ["M12 4v11M7.5 10.5L12 15l4.5-4.5", "M5 19h14"],
    sliders: ["M4 7h9M17 7h3M4 17h3M11 17h9", "M15 4v6M9 14v6"],
    refresh: ["M20 11a8 8 0 0 0-14.7-3.2M4.5 3.5v4.3h4.3", "M4 13a8 8 0 0 0 14.7 3.2M19.5 20.5v-4.3h-4.3"],
    folder: ["M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"],
    pulse: ["M3 12h4l3-8 4 16 3-8h4"],
    check: ["M5 12.5l4.5 4.5L19 7"],
    alert: ["M12 4l9 16H3z", "M12 10v4M12 17h.01"],
    x: ["M6 6l12 12M18 6L6 18"],
    minus: ["M6 12h12"],
    eye: ["M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z", "M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z"],
    spinner: ["M12 3a9 9 0 1 0 9 9"],
    info: ["M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z", "M12 11v5M12 8h.01"],
    external: ["M14 4h6v6M20 4l-9 9", "M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"],
    copy: ["M9 9h11v11H9z", "M5 15V4h10"],
    save: ["M5 4h11l3 3v13H5z", "M8 4v5h7V4M8 20v-6h8v6"],
    play: ["M7 5l12 7-12 7z"],
    stop: ["M7 7h10v10H7z"],
    trash: ["M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"],
    file: ["M6 3h8l4 4v14H6z", "M14 3v5h4"]
  };
  function icon(name, cls) {
    var s = svgEl("svg", { viewBox: "0 0 24 24", "class": "icon" + (cls ? " " + cls : ""), "aria-hidden": "true", focusable: "false" });
    (ICONS[name] || ICONS.info).forEach(function (d) { s.appendChild(svgEl("path", { d: d })); });
    return s;
  }
  function categoryIcon(name) {
    if (name.indexOf("モード") >= 0 || name.indexOf("プロファイル") >= 0) return "gauge";
    if (name.indexOf("学習") >= 0) return "book";
    if (name.indexOf("予測") >= 0) return "sparkle";
    if (name.indexOf("アップデート") >= 0 || name.indexOf("更新") >= 0) return "download";
    if (name.indexOf("入力") >= 0) return "keyboard";
    if (name.indexOf("変換") >= 0) return "convert";
    if (name.indexOf("候補") >= 0) return "list";
    if (name.indexOf("通知") >= 0) return "bell";
    if (name.indexOf("表示") >= 0 || name.indexOf("Windows") >= 0) return "monitor";
    return "sliders";
  }

  function announce(text) {
    srLive.textContent = "";
    window.setTimeout(function () { srLive.textContent = text; }, 30);
  }

  // ---- 通信 ----
  function api(path, body) {
    return fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Nagi-Token": token },
      body: JSON.stringify(body || {}),
      cache: "no-store",
      referrerPolicy: "no-referrer"
    }).then(function (res) {
      return res.json().catch(function () { return null; }).then(function (data) {
        if (!res.ok) throw new Error((data && data.error) || "エラーが発生しました（" + res.status + "）");
        return data;
      });
    }, function () { throw new Error(DISCONNECTED); });
  }

  function showBanner(message) {
    bannerEl.textContent = message || "";
    bannerEl.hidden = !message;
  }

  function toast(message, isError) {
    var node = el("div", { "class": "toast" + (isError ? " error" : "") }, [icon(isError ? "alert" : "check"), el("span", { text: message })]);
    toastsEl.appendChild(node);
    while (toastsEl.children.length > 3) toastsEl.removeChild(toastsEl.firstChild);
    window.setTimeout(function () {
      node.classList.add("leaving");
      window.setTimeout(function () { if (node.parentNode) node.parentNode.removeChild(node); }, 260);
    }, isError ? 5200 : 2600);
  }

  // ---- ページの一覧 ----
  var FIXED = [
    { id: "update", title: "更新", icon: "refresh" },
    { id: "data", title: "データ・ファイル", icon: "folder" },
    { id: "diag", title: "動作確認", icon: "pulse" }
  ];

  function visibleItems(group) {
    return group.items.filter(function (i) { return detail || i.level === "simple"; });
  }
  function visibleGroups() {
    return screen ? screen.groups.filter(function (g) { return visibleItems(g).length > 0; }) : [];
  }
  function pageList() {
    var list = [];
    if (screen) visibleGroups().forEach(function (g) { list.push({ id: "cat:" + g.name, title: g.name, icon: categoryIcon(g.name), section: "settings" }); });
    else list.push({ id: "status", title: "設定", icon: "sliders", section: "settings" });
    FIXED.forEach(function (f) { list.push({ id: f.id, title: f.title, icon: f.icon, section: "maint" }); });
    return list;
  }
  function findPage(id) {
    var list = pageList();
    for (var i = 0; i < list.length; i++) if (list[i].id === id) return list[i];
    return null;
  }

  // ---- サイドバー ----
  function renderNav() {
    navEl.textContent = "";
    var list = pageList();
    [["settings", "設定"], ["maint", "メンテナンス"]].forEach(function (sec, si) {
      var entries = list.filter(function (p) { return p.section === sec[0]; });
      if (entries.length === 0) return;
      var labelId = "nav-label-" + si;
      navEl.appendChild(el("p", { "class": "nav-label", id: labelId, text: sec[1] }));
      var ul = el("ul", { "class": "nav-list", "aria-labelledby": labelId });
      entries.forEach(function (p) {
        var btn = el("button", { type: "button", "class": "nav-item", "data-page": p.id }, [icon(p.icon), el("span", { "class": "nav-text", text: p.title })]);
        if (p.id === page) btn.setAttribute("aria-current", "page");
        btn.addEventListener("click", function () { navigate(p.id); });
        ul.appendChild(el("li", {}, [btn]));
      });
      navEl.appendChild(ul);
    });
  }
  navEl.addEventListener("keydown", function (e) {
    var keys = ["ArrowDown", "ArrowUp", "Home", "End"];
    if (keys.indexOf(e.key) < 0) return;
    var items = Array.prototype.slice.call(navEl.querySelectorAll(".nav-item"));
    var at = items.indexOf(document.activeElement);
    if (at < 0) return;
    var next = e.key === "ArrowDown" ? at + 1 : e.key === "ArrowUp" ? at - 1 : e.key === "Home" ? 0 : items.length - 1;
    if (next < 0 || next >= items.length) return;
    e.preventDefault();
    items[next].focus();
  });

  function markNav() {
    Array.prototype.forEach.call(navEl.querySelectorAll(".nav-item"), function (b) {
      if (b.getAttribute("data-page") === page) b.setAttribute("aria-current", "page");
      else b.removeAttribute("aria-current");
    });
  }

  function navigate(id) {
    var changed = id !== page;
    page = id;
    if (findPage(id) && id !== "status") savePref("page", id);
    markNav();
    renderPage(changed);
    var p = findPage(id);
    if (changed && p) announce(p.title + "のページ");
  }

  // ---- ページの共通部品 ----
  // ページ見出し（題と、必要なら 1 行の説明だけ。飾りは付けない）
  function hero(title, lead) {
    return el("header", { "class": "hero" }, [
      el("h1", { id: "page-title", text: title }),
      lead ? el("p", { "class": "lead", text: lead }) : null
    ]);
  }
  function section(title, meta, body) {
    var id = "sec-" + (++idCounter);
    var head = el("div", { "class": "section-head" }, [el("h2", { id: id, text: title })]);
    if (meta) head.appendChild(el("span", { "class": "meta", text: meta }));
    return el("section", { "class": "section", "aria-labelledby": id }, [head, body]);
  }
  function card(children, pad) {
    return el("div", { "class": "card" + (pad ? " card-pad" : "") }, children);
  }
  function badge(kind, label, iconName, running) {
    var b = el("span", { "class": "badge " + kind + (running ? " running" : "") }, [icon(iconName), el("span", { text: label })]);
    return b;
  }
  var BADGE_ICON = { ok: "check", warn: "alert", ng: "x", neutral: "minus", info: "info" };
  function statusBadge(kind, label, running, manual) {
    var name = running ? "spinner" : manual ? "eye" : BADGE_ICON[kind] || "info";
    return badge(kind, label, name, running);
  }
  function button(label, cls, iconName, onClick) {
    var b = el("button", { type: "button", "class": "btn" + (cls ? " " + cls : "") }, [iconName ? icon(iconName) : null, el("span", { text: label })]);
    if (onClick) b.addEventListener("click", onClick);
    return b;
  }
  function externalLink(url, label) {
    var a = el("a", { "class": "ext-link", href: url, target: "_blank", rel: "noopener noreferrer" }, [el("span", { text: label }), icon("external")]);
    a.addEventListener("click", function (e) {
      if (host) { e.preventDefault(); postHost({ type: "open-external", url: url }); }
    });
    return a;
  }

  // ---- ページの描画 ----
  function renderPage(changed) {
    var keepScroll = changed ? 0 : mainEl.scrollTop;
    rowRefs = {};
    updUI = null;
    diag.ui = null;
    pageEl.textContent = "";
    var body = el("div", { "class": "page-body" + (changed ? " enter" : "") });
    pageEl.appendChild(body);
    var p = findPage(page);
    if (!p) { page = pageList()[0].id; p = findPage(page); markNav(); }
    if (page === "status") body.appendChild(pageStatus());
    else if (page === "update") body.appendChild(pageUpdate());
    else if (page === "data") body.appendChild(pageData());
    else if (page === "diag") body.appendChild(pageDiag());
    else body.appendChild(pageCategory(page.slice(4)));
    document.title = p.title + " - Nagi 設定";
    mainEl.scrollTop = keepScroll;
  }
  function frag(nodes) {
    var f = document.createDocumentFragment();
    nodes.forEach(function (n) { if (n) f.appendChild(n); });
    return f;
  }

  // ---- 設定カテゴリ ----
  function pageStatus() {
    return frag([
      hero("設定", "設定を読み込めませんでした。"),
      section("状態", "", card([
        el("div", { "class": "card-pad stack" }, [
          el("p", { text: bannerEl.textContent || "設定を読み込んでいます…" }),
          el("div", { "class": "buttons" }, [button("再試行", "primary", "refresh", function () { load(); })])
        ])
      ]))
    ]);
  }

  function pageCategory(name) {
    var group = null;
    screen.groups.forEach(function (g) { if (g.name === name) group = g; });
    if (!group) return hero(name, "");
    var simple = group.items.filter(function (i) { return i.level === "simple"; });
    var deep = detail ? group.items.filter(function (i) { return i.level !== "simple"; }) : [];
    var out = [hero(name, "変更はすぐに保存されます。")];
    if (simple.length) out.push(deep.length ? section("基本", "", itemsCard(simple, false)) : itemsCard(simple, false));
    if (deep.length) out.push(section("詳細設定", "", itemsCard(deep, true)));
    return frag(out);
  }
  function itemsCard(items, isDetail) {
    var c = card([]);
    items.forEach(function (item) { c.appendChild(row(item, isDetail)); });
    return c;
  }

  function rangeText(item) {
    var k = item.kind;
    if (k.type !== "int" && k.type !== "float") return "";
    var text = "範囲 " + k.min + " から " + k.max;
    if (item.default !== null) text += "（既定 " + item.default + "）";
    return text;
  }
  function typed(kind, value) {
    if (value === null) return null;
    var t = kind.type === "choice" ? "text" : kind.type;
    return { type: t, value: value };
  }
  function valueText(item) {
    return item.value === null || item.value === undefined ? "" : String(item.value);
  }

  function control(item, id, descId, ref) {
    var k = item.kind;
    var input;
    if (k.type === "bool") {
      input = el("input", { type: "checkbox", id: id, role: "switch", "class": "switch" });
      input.checked = item.value === true;
      ref.state = el("span", { "class": "switch-state", "aria-hidden": "true" });
      input.addEventListener("change", function () { ref.state.textContent = input.checked ? "オン" : "オフ"; commit(item, input.checked); });
    } else if (k.type === "int" || k.type === "float") {
      input = el("input", { type: "number", id: id, "class": "field", min: String(k.min), max: String(k.max), step: String(k.step), inputmode: k.type === "int" ? "numeric" : "decimal" });
      input.value = valueText(item);
      if (item.value === null) input.placeholder = "自動";
      input.setAttribute("data-committed", input.value);
      input.addEventListener("change", function () {
        if (input.value.trim() === "") { commit(item, null); return; }
        var n = Number(input.value);
        if (!isFinite(n) || (k.type === "int" && !Number.isInteger(n))) {
          errors[item.key] = k.type === "int" ? "整数で入力してください" : "数値で入力してください";
          syncRows();
          return;
        }
        commit(item, n);
      });
    } else if (k.type === "choice") {
      input = el("select", { id: id, "class": "field" });
      if (item.value === null) input.appendChild(el("option", { value: "", text: "（自動）" }));
      k.options.forEach(function (o) { input.appendChild(el("option", { value: o.value, text: o.label })); });
      input.value = valueText(item);
      input.addEventListener("change", function () { commit(item, input.value === "" ? null : input.value); });
    } else {
      input = el("input", { type: "text", id: id, "class": "field", autocomplete: "off", spellcheck: "false" });
      input.value = valueText(item);
      input.setAttribute("data-committed", input.value);
      input.addEventListener("change", function () { commit(item, input.value === "" ? null : input.value); });
    }
    input.setAttribute("data-key", item.key);
    input.setAttribute("aria-describedby", descId);
    ref.input = input;
    return input;
  }

  function row(item, isDetail) {
    var n = ++idCounter;
    var id = "s" + n;
    var descId = id + "-desc";
    var ref = { item: item };
    rowRefs[item.key] = ref;
    var label = el("label", { "class": "row-label", "for": id }, [el("span", { text: item.label })]);
    if (isDetail) label.appendChild(el("span", { "class": "tag detail", text: "詳細" }));
    if (item.requires_restart) label.appendChild(el("span", { "class": "tag restart", text: "再起動が必要" }));
    var main = el("div", { "class": "row-main" }, [label, el("p", { "class": "row-desc", id: descId, text: item.description })]);
    var range = rangeText(item);
    if (range) main.appendChild(el("p", { "class": "row-desc", text: range }));
    if (item.requires_restart) main.appendChild(el("p", { "class": "row-hint", text: RESTART_HINT }));
    ref.err = el("p", { "class": "row-error", role: "alert", hidden: true });
    main.appendChild(ref.err);
    ref.resetSlot = el("div", { "class": "reset-slot" });
    var input = control(item, id, descId, ref);
    var ctl = el("div", { "class": "row-ctl" }, [ref.resetSlot, el("div", { "class": "ctl-line" }, [ref.state || null, input])]);
    var isBool = item.kind.type === "bool";
    ref.row = el("div", { "class": "row" + (isBool ? " clickable" : " wide"), "data-row": item.key }, [main, ctl]);
    // スイッチの行は、行のどこを押しても切り替わる（Windows の設定と同じ）。ラベルは for= で、ボタンは自分で処理する
    if (isBool) ref.row.addEventListener("click", function (e) {
      if (e.target === input || (e.target.closest && e.target.closest("label, button, a"))) return;
      input.click();
    });
    syncRow(ref, item, true);
    return ref.row;
  }

  // 保存後の更新は、要素を作り直さず値だけ差し替える（スイッチの動き・フォーカス・スクロールを保つ）
  function syncRow(ref, item, force) {
    ref.item = item;
    var input = ref.input;
    if (input.type === "checkbox") {
      input.checked = item.value === true;
      if (ref.state) ref.state.textContent = input.checked ? "オン" : "オフ";
    } else if (input.tagName === "SELECT") {
      // 値が未指定（自動）のときだけ「（自動）」の選択肢を持つ
      if (item.value === null && !input.querySelector('option[value=""]')) input.insertBefore(el("option", { value: "", text: "（自動）" }), input.firstChild);
      input.value = valueText(item);
    } else if (force || input.value === input.getAttribute("data-committed")) {
      input.value = valueText(item);
      input.setAttribute("data-committed", input.value);
    }
    if (input.type === "number") input.placeholder = item.value === null ? "自動" : "";
    ref.resetSlot.textContent = "";
    if (item.explicit) {
      var reset = button("既定に戻す", "quiet sm", null, function () { commit(ref.item, null); });
      reset.setAttribute("aria-label", item.label + "を既定に戻す");
      ref.resetSlot.appendChild(reset);
    }
    var message = errors[item.key];
    ref.err.textContent = message || "";
    ref.err.hidden = !message;
    if (message) input.setAttribute("aria-invalid", "true"); else input.removeAttribute("aria-invalid");
  }
  function syncRows() {
    Object.keys(rowRefs).forEach(function (key) {
      var item = itemByKey[key] || rowRefs[key].item;
      var ref = rowRefs[key];
      syncRow(ref, item, false);
    });
  }

  function setScreen(s) {
    screen = s;
    itemByKey = {};
    s.groups.forEach(function (g) { g.items.forEach(function (i) { itemByKey[i.key] = i; }); });
    if (s.app_version) document.getElementById("app-version").textContent = "v" + s.app_version;
    showBanner(s.error || "");
  }

  function commit(item, value) {
    showBanner("");
    var wasReset = value === null;
    return api("/api/update", { changes: [{ key: item.key, value: typed(item.kind, value) }] }).then(function (r) {
      errors = {};
      r.errors.forEach(function (e) { errors[e.key] = e.message; });
      setScreen(r.screen);
      // 値を直したときは、その欄は必ず最新の値にする
      var ref = rowRefs[item.key];
      if (ref) syncRow(ref, itemByKey[item.key], true);
      syncRows();
      if (r.applied.length > 0) toast(wasReset ? "既定に戻しました" : "保存しました", false);
    }, function (e) {
      errors = {};
      errors[item.key] = e.message;
      syncRows();
    });
  }

  // ---- 更新 ----
  function pageUpdate() {
    var ui = {};
    updUI = ui;
    var version = (screen && screen.app_version) || document.getElementById("app-version").textContent.replace(/^v/, "");
    ui.checkBtn = button("更新を確認", "primary", "refresh", function () { checkUpdate(); });
    ui.applyBtn = button("更新を適用…", "primary", "play", function () { applyUpdate(); });
    ui.notes = el("span", { "class": "notes" });
    ui.result = el("div", { "class": "result", role: "status", "aria-live": "polite" });
    var buttons = el("div", { "class": "card-pad" }, [el("div", { "class": "buttons" }, [ui.checkBtn, ui.applyBtn, ui.notes])]);
    var out = frag([
      hero("更新", "Nagi を最新の状態に保ちます。"),
      section("バージョン", "", card([
        el("div", { "class": "kv" }, [el("span", { "class": "kv-label", text: "現在の版" }), el("span", { "class": "kv-value", text: version ? "v" + version : "" })])
      ])),
      section("更新の確認", "", card([buttons, ui.result]))
    ]);
    renderUpdate();
    return out;
  }

  function renderUpdate() {
    var ui = updUI;
    if (!ui) return;
    ui.checkBtn.disabled = upd.busy;
    var v = upd.view;
    ui.applyBtn.hidden = !(v && v.can_apply);
    // 適用できるときは、適用が主要な操作
    ui.checkBtn.classList.toggle("primary", !(v && v.can_apply));
    ui.applyBtn.disabled = upd.busy;
    ui.notes.textContent = "";
    if (v && v.notes_url && v.notes_url.indexOf("https:") === 0) ui.notes.appendChild(externalLink(v.notes_url, "リリースノート"));
    ui.result.textContent = "";
    if (upd.busy) {
      ui.result.hidden = false;
      ui.result.appendChild(el("div", { "class": "result-head" }, [statusBadge("info", "確認中", true), el("span", { text: "更新を確認しています" })]));
    } else if (v) {
      ui.result.hidden = false;
      ui.result.appendChild(el("div", { "class": "result-head" }, [statusBadge(v.badge, v.label, false), el("span", { text: v.title })]));
      ui.result.appendChild(el("p", { "class": "result-text", text: v.text }));
    } else {
      ui.result.hidden = true;
    }
  }

  function checkUpdate() {
    if (upd.busy) return Promise.resolve();
    upd.busy = true;
    renderUpdate();
    return api("/api/update/check").then(function (r) {
      upd.view = r.view || { badge: "neutral", label: "お知らせ", title: "更新の状態", text: "", can_apply: r.status === "staged", notes_url: r.notes_url };
    }, function (e) {
      upd.view = { badge: "ng", label: "失敗", title: "更新に失敗しました", text: "更新を確認できませんでした: " + e.message, can_apply: false, notes_url: null };
    }).then(function () { upd.busy = false; renderUpdate(); });
  }

  function applyUpdate() {
    if (upd.busy) return;
    upd.busy = true;
    renderUpdate();
    var prev = upd.view;
    api("/api/update/apply").then(function () {
      upd.view = { badge: "info", label: "適用中", title: "更新を適用しています", text: "更新の適用を開始しました。新しく開いたコンソールの案内に従ってください（管理者の確認が出ます）。", can_apply: false, notes_url: prev && prev.notes_url };
    }, function (e) {
      upd.view = { badge: "ng", label: "失敗", title: "更新に失敗しました", text: "更新を適用できませんでした: " + e.message, can_apply: !!(prev && prev.can_apply), notes_url: prev && prev.notes_url };
    }).then(function () { upd.busy = false; renderUpdate(); });
  }

  // ---- データ・ファイル ----
  function openTarget(target) {
    api("/api/open", { target: target }).then(function () {
      toast("開きました", false);
    }, function (e) { toast(e.message, true); });
  }
  function pageData() {
    var path = screen && screen.config_path;
    var openBtn = function (label, target, iconName) {
      var b = button(label, "", iconName, function () { openTarget(target); });
      b.disabled = !path;
      return b;
    };
    var clearBtn = button("学習データを全削除…", "danger", "trash", function () {
      var dialog = document.getElementById("confirm");
      dialog.returnValue = "";
      dialog.showModal();
    });
    return frag([
      hero("データ・ファイル", "設定ファイルの場所と、変換の学習データを管理します。"),
      section("設定ファイル", "", card([
        el("div", { "class": "card-pad stack" }, [
          el("p", { "class": "path", text: path || "（設定ファイルの場所は未定です）" }),
          el("div", { "class": "buttons" }, [openBtn("config.toml を開く", "config", "file"), openBtn("設定フォルダを開く", "config_dir", "folder"), openBtn("ユーザー辞書フォルダを開く", "user_dict", "folder")])
        ])
      ])),
      section("学習データ", "", card([
        el("div", { "class": "card-pad stack" }, [
          el("p", { "class": "muted", text: "変換の学習結果（よく使う変換・次語予測）をすべて削除します。元に戻せません。ユーザー辞書は消えません。" }),
          el("div", { "class": "buttons" }, [clearBtn])
        ])
      ]))
    ]);
  }

  var confirmDialog = document.getElementById("confirm");
  document.getElementById("confirm-cancel").addEventListener("click", function () { confirmDialog.close("cancel"); });
  document.getElementById("confirm-ok").addEventListener("click", function () { confirmDialog.close("ok"); });
  confirmDialog.addEventListener("close", function () {
    if (confirmDialog.returnValue !== "ok") return;
    api("/api/clear_learning").then(function () {
      toast("学習データを削除しました", false);
    }, function (e) { toast(e.message, true); });
  });
  var copyDialog = document.getElementById("copy-dialog");
  document.getElementById("copy-close").addEventListener("click", function () { copyDialog.close(); });

  // ---- 動作確認（診断） ----
  var DIAG_GROUPS = [
    ["env", "環境"], ["install", "導入の状態"], ["data", "辞書・モデル"], ["tsf", "入力方式（TSF）"],
    ["engine", "変換エンジン"], ["input", "入力・変換"], ["update", "更新"], ["manual", "手動で確認する項目"]
  ];
  var DIAG_INTRO = "Nagi が正しく動く状態かを自動で確認します。確認の結果にあなたが入力した文章は含まれません（確認に使うのは固定のテスト文字列だけです）。すべて確認するには 1 分ほどかかることがあります。";

  function pageDiag() {
    var ui = { items: {} };
    diag.ui = ui;
    ui.runAll = button("すべて確認", "primary", "play", function () { diagStart(null); });
    ui.cancel = button("中止", "", "stop", function () {
      api("/api/diag/cancel").then(diagRefresh, function (e) { toast(e.message, true); });
    });
    ui.copy = button("レポートをコピー", "", "copy", copyReport);
    ui.save = button("レポートを保存", "", "save", saveReport);
    ui.progress = el("div", { "class": "progress", role: "status", "aria-live": "polite" });
    ui.bar = el("div", { "class": "bar", role: "progressbar", "aria-label": "確認の進み具合", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": "0" }, [el("div", { "class": "bar-fill" })]);
    ui.progressLine = el("p", { "class": "progress-line" });
    ui.progress.appendChild(ui.progressLine);
    ui.progress.appendChild(ui.bar);
    ui.chips = el("div", { "class": "chips" });
    ui.note = el("p", { "class": "note-line", role: "status", "aria-live": "polite" });
    var controls = el("div", { "class": "card-pad stack" }, [
      el("p", { "class": "muted", text: DIAG_INTRO }),
      el("div", { "class": "buttons" }, [ui.runAll, ui.cancel, el("span", { "class": "spacer" }), ui.copy, ui.save]),
      ui.chips,
      ui.note
    ]);
    var out = [
      hero("動作確認", "この PC での入力・変換・更新の動作を、ボタン一つで確認します。"),
      section("確認の実行", "", card([controls, ui.progress]))
    ];
    // 項目は ID の接頭辞でまとめる（ID の並びは Rust 側の一覧どおり）
    var groups = {};
    var items = (diag.status && diag.status.items) || [];
    items.forEach(function (it) {
      var prefix = it.id.split(".")[0];
      (groups[prefix] = groups[prefix] || []).push(it);
    });
    DIAG_GROUPS.forEach(function (g) {
      var list = groups[g[0]];
      if (!list) return;
      var c = card([]);
      list.forEach(function (it) {
        var node = diagItemNode(it);
        ui.items[it.id] = node;
        c.appendChild(node.root);
      });
      out.push(section(g[1], list.length + " 項目", c));
    });
    var f = frag(out);
    renderDiag();
    if (!diag.status) diagRefresh();
    return f;
  }

  function diagItemNode(it) {
    var n = { id: it.id };
    n.badgeSlot = el("span", { "class": "badge-slot" });
    n.title = el("h3", { text: it.title });
    n.time = el("span", { "class": "muted small" });
    n.btn = button(it.kind === "guide" ? "この項目の手順を表示" : "この項目だけ確認", "quiet sm", null, function () { diagStart(it.id); });
    n.btn.setAttribute("aria-label", it.title + (it.kind === "guide" ? "の手順を表示" : "だけ確認"));
    n.detail = el("p", { "class": "di-detail" });
    n.advice = el("p", { "class": "di-advice" });
    n.evidence = el("details", { "class": "di-evidence" });
    n.root = el("article", { "class": "diag-item", "data-id": it.id }, [
      el("div", { "class": "di-head" }, [n.badgeSlot, n.title, n.time, n.btn]), n.detail, n.advice, n.evidence
    ]);
    n.key = "";
    return n;
  }

  function syncDiagItem(n, it, busy) {
    var key = [it.label, it.badge, it.running, it.detail, it.advice, (it.evidence || []).join("\n")].join("\u0001");
    if (n.key !== key) {
      n.key = key;
      n.badgeSlot.textContent = "";
      n.badgeSlot.appendChild(statusBadge(it.badge, it.label, it.running, it.status === "manual"));
      n.detail.textContent = it.detail || "";
      n.detail.hidden = !it.detail;
      n.advice.textContent = "";
      n.advice.hidden = !it.advice;
      if (it.advice) {
        n.advice.appendChild(el("strong", { text: it.status === "manual" ? "手順" : "対処" }));
        n.advice.appendChild(document.createTextNode(it.advice));
      }
      var ev = it.evidence || [];
      var open = n.evidence.open;
      n.evidence.textContent = "";
      n.evidence.hidden = ev.length === 0;
      if (ev.length) {
        n.evidence.appendChild(el("summary", { text: "根拠（" + ev.length + " 行）" }));
        n.evidence.appendChild(el("pre", { text: ev.join("\n") }));
        n.evidence.open = open;
      }
    }
    n.time.textContent = it.elapsed_ms ? (it.elapsed_ms >= 1000 ? (it.elapsed_ms / 1000).toFixed(1) + " 秒" : it.elapsed_ms + " ms") : "";
    n.btn.disabled = busy;
  }

  var CHIP_DEFS = [["ng", "NG", "ng"], ["warn", "要確認", "warn"], ["ok", "OK", "ok"], ["skipped", "スキップ", "neutral"], ["manual", "手動確認", "info"]];

  function renderDiag() {
    var ui = diag.ui;
    if (!ui) return;
    var s = diag.status;
    if (!s) {
      ui.progress.hidden = true;
      ui.runAll.disabled = true; ui.cancel.disabled = true; ui.copy.disabled = true; ui.save.disabled = true;
      return;
    }
    ui.runAll.disabled = s.running;
    ui.cancel.disabled = !s.running || s.cancelling;
    ui.copy.disabled = !s.can_export;
    ui.save.disabled = !s.can_export;
    ui.progress.hidden = !s.running;
    if (s.running) {
      var pr = s.progress || { index: 0, total: 1 };
      var done = 0;
      s.items.forEach(function (it) { if (it.status) done += 1; });
      var total = s.mode === "all" ? pr.total || s.items.length : 1;
      var pct = s.mode === "all" ? Math.round((done / total) * 100) : 40;
      ui.progressLine.textContent = s.progress_line || "確認しています…";
      ui.bar.setAttribute("aria-valuenow", String(pct));
      ui.bar.firstChild.style.width = pct + "%";
    }
    ui.chips.textContent = "";
    if (s.summary && !s.running) {
      CHIP_DEFS.forEach(function (c) {
        ui.chips.appendChild(statusBadge(c[2], c[1] + " " + s.summary[c[0]], false, c[0] === "manual"));
      });
    }
    var note = "";
    var isError = false;
    if (s.note) {
      note = s.note.text;
      isError = s.note.is_error;
      if (isError) note = "エラー: " + note;
      // 件数はバッジで示すので、文章では繰り返さない（中止したことだけ残す）
      else if (s.summary && note.indexOf(s.summary.line) >= 0) note = note.replace(s.summary.line, "").trim();
    }
    if (diag.savedPath && s.can_export) note = (note ? note + "　" : "") + "保存先: " + diag.savedPath;
    ui.note.textContent = note;
    ui.note.className = "note-line" + (isError ? " error" : "");
    ui.note.hidden = !note;
    s.items.forEach(function (it) {
      var n = ui.items[it.id];
      if (n) syncDiagItem(n, it, s.running);
    });
  }

  function diagRefresh() {
    return api("/api/diag/status").then(function (s) {
      var wasRunning = diag.status && diag.status.running;
      diag.status = s;
      if (!diag.ui || !diag.ui.items[s.items[0].id]) {
        if (page === "diag") renderPage(false);
      } else {
        renderDiag();
      }
      if (s.running) schedulePoll();
      else if (wasRunning && s.summary) announce(s.summary.line);
    }, function (e) {
      toast(e.message, true);
    });
  }
  function schedulePoll() {
    if (diag.timer) return;
    diag.timer = window.setTimeout(function () { diag.timer = 0; diagRefresh(); }, 500);
  }
  function diagStart(itemId) {
    diag.savedPath = "";
    api("/api/diag/start", itemId ? { item: itemId } : {}).then(diagRefresh, function (e) {
      toast(e.message, true);
      diagRefresh();
    });
  }

  function writeClipboard(text) {
    if (navigator.clipboard && window.isSecureContext && typeof navigator.clipboard.writeText === "function") return navigator.clipboard.writeText(text);
    return Promise.reject(new Error("clipboard unavailable"));
  }
  function copyReport() {
    api("/api/diag/report").then(function (r) {
      return writeClipboard(r.text).then(function () {
        toast("レポートをコピーしました", false);
      }, function () {
        var area = document.getElementById("copy-text");
        area.value = r.text;
        copyDialog.showModal();
        area.focus();
        area.select();
        area.scrollTop = 0;
      });
    }, function (e) { toast(e.message, true); });
  }
  function saveReport() {
    api("/api/diag/save").then(function (r) {
      diag.savedPath = r.path;
      toast("レポートを保存しました", false);
      renderDiag();
    }, function (e) { toast(e.message, true); });
  }

  // ---- 起動・共通の操作 ----
  function load() {
    return api("/api/settings").then(function (s) {
      setScreen(s);
    }, function (e) {
      screen = null;
      showBanner(e.message);
    }).then(function () {
      if (screen && page === "status") page = loadPref("page") || "";
      renderNav();
      if (!findPage(page)) page = pageList()[0].id;
      markNav();
      renderPage(true);
    });
  }

  detailToggle.checked = detail;
  detailToggle.addEventListener("change", function () {
    detail = detailToggle.checked;
    savePref("detail", detail ? "1" : "0");
    errors = {};
    renderNav();
    if (!findPage(page)) page = pageList()[0].id;
    markNav();
    renderPage(false);
  });

  // Esc: ページ内のダイアログを閉じるのが先（<dialog> が自分で閉じる）。何も開いていないときだけホストへ閉じる要求を送る。
  // 入力欄の途中の編集は、Esc で元に戻す
  document.addEventListener("keydown", function (e) {
    if (e.key !== "Escape" || e.defaultPrevented || e.isComposing) return;
    if (document.querySelector("dialog[open]")) return;
    var a = document.activeElement;
    if (a && a.hasAttribute && a.hasAttribute("data-committed") && a.value !== a.getAttribute("data-committed")) {
      a.value = a.getAttribute("data-committed");
      e.preventDefault();
      return;
    }
    if (host) {
      e.preventDefault();
      postHost({ type: "close-request" });
    }
  });

  // 起動引数 --section update [--auto-check]（URL の断片）: 更新ページを開き、必要なら確認を自動実行する
  function startAction() {
    var hash = window.location.hash;
    if (hash !== "#update" && hash !== "#update-check") return;
    if (window.history && history.replaceState) history.replaceState(null, "", window.location.pathname + window.location.search);
    navigate("update");
    if (hash === "#update-check") checkUpdate();
  }

  // ページが閉じられたら、サーバー（と設定エンジン）を終了させる。再読み込みでは 5 秒以内に API が来て取り消される
  window.addEventListener("pagehide", function () {
    navigator.sendBeacon("/bye?t=" + encodeURIComponent(token), new Blob(["{}"], { type: "application/json" }));
  });
  window.addEventListener("pageshow", function (e) { if (e.persisted) load(); });

  // 最初の表示（読み込み中は骨組みだけ）
  renderNav();
  pageEl.appendChild(el("div", { "class": "skeleton", "aria-hidden": "true" }));
  load().then(startAction);
})();
