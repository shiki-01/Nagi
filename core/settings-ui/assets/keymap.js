"use strict";
// キー割当ページ（設定画面のページの 1 つ。app.js が window.NagiPages から読んで描画する）。
// 一覧・検証・保存・リセット・インポート/エクスポートはすべてコア（/api/keymap/*）が行う。
// ここにあるキーの知識は「押されたキーを、コアが読めるキー仕様（Ctrl+Shift+J / F8 / Henkan …）へ書き換える」ことだけ。
// 文字列は textContent / DOM API だけで入れる（HTML 文字列の組み立ては使わない）。
(function () {
  // ---- キー仕様（コアの keys.rs / keymap.rs の表記）への正規化。DOM に触れない純粋な関数 ----
  var MOD_KEYS = { Control: 1, Shift: 1, Alt: 1, Meta: 1, AltGraph: 1, OS: 1, Hyper: 1, Super: 1 };
  var NAMED_KEYS = {
    " ": "Space", Enter: "Enter", Escape: "Escape", Backspace: "BackSpace", Delete: "Delete", Tab: "Tab",
    ArrowLeft: "Left", ArrowRight: "Right", ArrowUp: "Up", ArrowDown: "Down",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown"
  };
  // 日本語キーボードの特殊キー（KeyboardEvent.key の名前）
  var JP_KEYS = {
    Alphanumeric: "Eisu", Eisu: "Eisu", Katakana: "Katakana", Hiragana: "Hiragana", KanaMode: "Kana",
    HiraganaKatakana: "Kana", ZenkakuHankaku: "Zenkaku", Zenkaku: "Zenkaku", Hankaku: "Zenkaku",
    Convert: "Henkan", NonConvert: "Muhenkan"
  };
  // IME が処理中で key が "Process" になるときは code で見分ける
  var JP_CODES = { Convert: "Henkan", NonConvert: "Muhenkan", KanaMode: "Kana" };

  function modifiersOf(e) {
    // AltGr は Ctrl+Alt として届くが、文字を打つためのものなので修飾としては数えない
    var altGr = !!(e.getModifierState && e.getModifierState("AltGraph"));
    return { ctrl: !!e.ctrlKey && !altGr, shift: !!e.shiftKey, alt: !!e.altKey && !altGr, meta: !!e.metaKey };
  }
  function modsText(m) {
    var parts = [];
    if (m.ctrl) parts.push("Ctrl");
    if (m.shift) parts.push("Shift");
    if (m.alt) parts.push("Alt");
    if (m.meta) parts.push("Meta");
    return parts;
  }
  // 押されたキー → {kind, spec?, held?, message?}
  //   pending: 修飾キーだけ（held = 押されている修飾）／ ok: spec が決まった／ unsupported: 取り込めない（message）
  //   cancel: 修飾なしの Esc（取り込みをやめる）／ pass: Tab（フォーカス移動に使うので取り込まない）
  function specFromEvent(e) {
    var m = modifiersOf(e);
    var key = e.key;
    if (MOD_KEYS[key]) return { kind: "pending", held: modsText(m) };
    var plain = !m.ctrl && !m.alt && !m.meta;
    if (key === "Escape" && plain && !m.shift) return { kind: "cancel" };
    if (key === "Tab" && plain) return { kind: "pass" };
    var name = null;
    if (JP_KEYS[key]) name = JP_KEYS[key];
    else if (key === "Process" || key === "Unidentified" || key === "Dead" || key === "Compose") {
      name = JP_CODES[e.code] || null;
      if (!name) {
        var cm = /^Key([A-Z])$/.exec(e.code || "") || /^Digit([0-9])$/.exec(e.code || "");
        if (cm) name = cm[1].toLowerCase();
      }
    } else if (NAMED_KEYS[key]) name = NAMED_KEYS[key];
    else if (/^F([1-9]|1[0-2])$/.test(key)) name = key;
    else if (/^F[0-9]+$/.test(key)) return { kind: "unsupported", message: "F13 以降のキーは割り当てできません。" };
    else if (key && key.length === 1) {
      if (/[A-Za-z]/.test(key)) name = key.toLowerCase();
      else if (key === "+") return { kind: "unsupported", message: "「+」は修飾キーの区切りと同じ記号のため、割り当てできません。" };
      else if (key.charCodeAt(0) >= 0x21 && key.charCodeAt(0) <= 0x7e) name = key;
      else return { kind: "unsupported", message: "このキーは割り当てできません（英数字・記号・ファンクションキー・変換/無変換などに対応しています）。" };
    }
    if (!name) return { kind: "unsupported", message: "このキーは割り当てできません（英数字・記号・ファンクションキー・変換/無変換などに対応しています）。" };
    return { kind: "ok", spec: modsText(m).concat([name]).join("+") };
  }

  // 表示用: キー仕様 → キートップの名前の並び
  var DISPLAY = {
    BackSpace: "Backspace", Escape: "Esc", Left: "←", Right: "→", Up: "↑", Down: "↓",
    Henkan: "変換", Muhenkan: "無変換", Kana: "かな", Eisu: "英数", Zenkaku: "半角/全角", Hiragana: "ひらがな", Katakana: "カタカナ",
    Meta: "Win/Cmd"
  };
  function displayParts(spec) {
    return String(spec).split("+").filter(function (p) { return p !== ""; }).map(function (p) {
      if (DISPLAY[p]) return DISPLAY[p];
      return p.length === 1 ? p.toUpperCase() : p;
    });
  }
  // 画面外からの検査用（Playwright）
  window.NagiKeymapKeys = { specFromEvent: specFromEvent, displayParts: displayParts };

  // ---- 文脈・ラベル ----
  var CONTEXTS = [
    { id: "common", title: "共通", editable: false,
      note: "入力モードの切り替えなど、どの状態でも効くキーです。モード切替はプリセットだけが持ち、個別の上書きはできません（切替キーを誤って潰すと日本語入力に戻れなくなるため）。" },
    { id: "direct", title: "未入力", editable: false,
      note: "何も入力していないときのキーです。再変換・確定の取り消し・スペースの入力など。文字キーによる入力の開始とモード切替はプリセットだけが持ち、個別の上書きはできません（文字が打てなくなるのを防ぐため）。" },
    { id: "composing", title: "入力中", editable: true, note: "" },
    { id: "converting", title: "変換中", editable: true, note: "" }
  ];
  function contextById(id) {
    for (var i = 0; i < CONTEXTS.length; i++) if (CONTEXTS[i].id === id) return CONTEXTS[i];
    return CONTEXTS[2];
  }
  // コアの assignable 一覧に無い（上書きできない）Action の表示名
  var EXTRA_LABELS = {
    "SetMode:Alphanumeric": "英数モードにする", "SetMode:Hiragana": "ひらがなモードにする", "SetMode:Katakana": "カタカナモードにする",
    "SetMode:HankakuKatakana": "半角カナモードにする", "SetMode:ZenkakuAlnum": "全角英数モードにする", "SetMode:HankakuAlnum": "半角英数モードにする",
    Reconvert: "再変換", UndoCommit: "確定を取り消す（確定直後のみ）", Insert: "文字を入力する", InsertText: "文字を入力する"
  };
  var SOURCES = {
    "default": { label: "既定", cls: "default" },
    preset: { label: "プリセット", cls: "preset" },
    user: { label: "ユーザー", cls: "user" }
  };
  var MODE_NOTES = { kana: "かな系のモードのみ", passthrough: "英数系のモードのみ" };

  // ---- 状態 ----
  var UI = null;
  var S = {
    data: null, error: "", ctx: "composing", query: "", userOnly: false,
    transferText: "", transferBase: null, transferValid: null, merge: false
  };
  var refs = {};
  var loadSeq = 0;

  function pref(name) { return UI ? UI.loadPref("keymap." + name) : null; }
  function setPref(name, value) { if (UI) UI.savePref("keymap." + name, value); }

  function ensureIcons() {
    var add = {
      pencil: ["M4 20h4L19 9l-4-4L4 16z", "M13.5 6.5l4 4"],
      plus: ["M12 5v14M5 12h14"],
      undo: ["M9 14L4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 0 11H11"],
      upload: ["M12 16V5M7.5 9.5L12 5l4.5 4.5", "M5 19h14"],
      search: ["M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14z", "M20 20l-4-4"],
      block: ["M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z", "M5.6 5.6l12.8 12.8"],
      lock: ["M6 11h12v9H6z", "M8.5 11V8a3.5 3.5 0 0 1 7 0v3"]
    };
    Object.keys(add).forEach(function (k) { if (!UI.icons[k]) UI.icons[k] = add[k]; });
  }

  // ---- 通信 ----
  function post(path, body) { return UI.api("/api/keymap/" + path, body || {}); }

  function bindings() { return S.data ? S.data.bindings : []; }
  function userCount(ctxId) {
    return bindings().filter(function (b) { return b.source === "user" && (!ctxId || b.context === ctxId); }).length;
  }
  function actionLabel(name) {
    var found = null;
    if (S.data) S.data.actions.forEach(function (a) { if (a.name === name) found = a.label; });
    return found || EXTRA_LABELS[name] || name;
  }

  function reload() {
    var seq = ++loadSeq;
    return post("get", {}).then(function (data) {
      if (seq !== loadSeq) return;
      S.data = data;
      S.error = "";
      paintAll();
    }, function (e) {
      if (seq !== loadSeq) return;
      S.error = e.message;
      paintAll();
    });
  }
  function adopt(result) {
    // 保存系の応答は、保存後の一覧を持っている（再取得は要らない）
    S.data = result.keymap;
    S.error = "";
    paintAll();
    refreshTransferIfClean();
  }

  // ---- 確認ダイアログ（ページの共通ダイアログは学習データ専用なので、ここで作る） ----
  var dialogSeq = 0;
  function openDialog(build) {
    var id = "kc-dlg-" + (++dialogSeq);
    var dlg = UI.el("dialog", { "aria-labelledby": id + "-t" });
    var body = UI.el("div", { "class": "dialog-body kc-dialog-body" });
    dlg.appendChild(body);
    build(body, id, function (value) { dlg.close(value); });
    document.body.appendChild(dlg);
    return new Promise(function (resolve) {
      dlg.addEventListener("close", function () {
        var value = dlg.returnValue;
        if (dlg.parentNode) dlg.parentNode.removeChild(dlg);
        resolve(value);
      });
      dlg.showModal();
    });
  }
  function confirmBox(opts) {
    return openDialog(function (body, id, close) {
      body.appendChild(UI.el("h2", { id: id + "-t", text: opts.title }));
      (opts.lines || []).forEach(function (t) { body.appendChild(UI.el("p", { text: t })); });
      var cancel = UI.button("キャンセル", "", null, function () { close("cancel"); });
      cancel.setAttribute("autofocus", "");
      var ok = UI.button(opts.ok || "実行する", opts.danger ? "danger" : "primary", null, function () { close("ok"); });
      body.appendChild(UI.el("div", { "class": "buttons end" }, [cancel, ok]));
    }).then(function (v) { return v === "ok"; });
  }

  // ---- 描画: 全体 ----
  function render(ui) {
    UI = ui;
    ensureIcons();
    var saved = pref("ctx");
    if (!S.data && saved && contextById(saved).id === saved) S.ctx = saved;
    var root = UI.el("div", { "class": "kc-page" });
    refs = { root: root };
    root.appendChild(UI.hero("キー割当", "変換や確定に使うキーを確認し、好みに合わせて変更します。変更は保存するとすぐに反映されます。"));
    ["preset", "bindings", "transfer", "reset"].forEach(function (name) {
      refs[name] = UI.el("div", { "class": "kc-slot" });
      root.appendChild(refs[name]);
    });
    paintAll();
    reload();
    return root;
  }
  function paintAll() {
    if (!refs.root) return;
    paintPreset();
    paintBindings();
    paintTransfer();
    paintReset();
  }
  function replace(slot, node) {
    slot.textContent = "";
    slot.appendChild(node);
  }
  function errorCard() {
    return UI.card([
      UI.el("div", { "class": "card-pad stack" }, [
        UI.el("p", { "class": "row-error", role: "alert", text: S.error }),
        UI.el("div", { "class": "buttons" }, [UI.button("再試行", "primary", "refresh", function () { reload(); })])
      ])
    ]);
  }

  // ---- プリセット ----
  function paintPreset() {
    var body;
    if (!S.data) {
      body = S.error ? errorCard() : UI.el("div", { "class": "skeleton", "aria-hidden": "true" });
      replace(refs.preset, UI.section("プリセット", "", body));
      return;
    }
    var presets = S.data.presets;
    var current = S.data.preset;
    var select = UI.el("select", { id: "kc-preset", "class": "field", "aria-describedby": "kc-preset-desc" });
    presets.forEach(function (p) { select.appendChild(UI.el("option", { value: p.name, text: p.label })); });
    select.value = current;
    var desc = UI.el("p", { "class": "row-desc", id: "kc-preset-desc" });
    function describe(name) {
      var info = presets.filter(function (p) { return p.name === name; })[0];
      desc.textContent = info ? info.description : "";
    }
    describe(current);
    select.addEventListener("change", function () {
      var next = select.value;
      select.value = current;
      describe(current);
      changePreset(next);
    });
    var n = userCount();
    var row = UI.el("div", { "class": "row wide" }, [
      UI.el("div", { "class": "row-main" }, [
        UI.el("label", { "class": "row-label", "for": "kc-preset" }, [UI.el("span", { text: "キー割当のプリセット" })]),
        desc,
        UI.el("p", { "class": "row-desc", text: n > 0 ? "あなたの割り当て " + n + " 件は、どのプリセットでも常に優先されます。" : "あなたが個別に割り当てたキーは、どのプリセットでも常に優先されます。" })
      ]),
      UI.el("div", { "class": "row-ctl" }, [select])
    ]);
    replace(refs.preset, UI.section("プリセット", "", UI.card([row])));
  }
  function changePreset(next) {
    var info = S.data.presets.filter(function (p) { return p.name === next; })[0];
    if (!info) return;
    post("get", { preset: next }).then(function (preview) {
      // 切り替えで内容が変わる割当の数（キーごとに、Action か有無が変わるもの）
      var index = function (list) {
        var m = {};
        list.forEach(function (b) { m[b.context + "\u0000" + b.key + (b.when_mode ? "\u0000" + b.when_mode : "")] = b.action; });
        return m;
      };
      var before = index(S.data.bindings);
      var after = index(preview.bindings);
      var keys = {};
      Object.keys(before).concat(Object.keys(after)).forEach(function (k) { keys[k] = 1; });
      var changed = Object.keys(keys).filter(function (k) { return before[k] !== after[k]; }).length;
      var lines = ["「" + info.label + "」に切り替えます。" + info.description];
      lines.push(changed > 0 ? "キー割当のうち " + changed + " 件が変わります。" : "表示中のキー割当に変わるものはありません。");
      if (userCount() > 0) lines.push("あなたが個別に割り当てた " + userCount() + " 件はそのまま残り、プリセットより優先されます。");
      return confirmBox({ title: "プリセットを切り替えますか？", lines: lines, ok: "切り替える" });
    }).then(function (ok) {
      if (!ok) return null;
      return UI.api("/api/update", { changes: [{ key: "keymap.preset", value: { type: "text", value: next } }] }).then(function (r) {
        if (r.errors && r.errors.length) { UI.toast(r.errors[0].message, true); return null; }
        UI.setScreen(r.screen);
        UI.toast("プリセットを切り替えました", false);
        return reload();
      });
    }).then(null, function (e) { UI.toast(e.message, true); });
  }

  // ---- 割り当ての一覧 ----
  function paintBindings() {
    var ctx = contextById(S.ctx);
    var wrap = UI.el("div", { "class": "kc-bindings" });
    wrap.appendChild(tabsBar());
    if (!S.data) {
      wrap.appendChild(S.error ? errorCard() : UI.el("div", { "class": "skeleton", "aria-hidden": "true" }));
      replace(refs.bindings, UI.section("割り当て", "", wrap));
      return;
    }
    wrap.appendChild(toolbar(ctx));
    if (ctx.note) wrap.appendChild(UI.el("div", { "class": "kc-note" }, [UI.icon("lock"), UI.el("p", { text: ctx.note })]));
    var panel = UI.el("div", { id: "kc-panel", role: "tabpanel", "aria-labelledby": "kc-tab-" + ctx.id });
    panel.appendChild(table(ctx));
    wrap.appendChild(panel);
    replace(refs.bindings, UI.section("割り当て", "", wrap));
  }
  function tabsBar() {
    var list = UI.el("div", { "class": "kc-tabs", role: "tablist", "aria-label": "キーを使う場面" });
    CONTEXTS.forEach(function (c) {
      var n = S.data ? userCount(c.id) : 0;
      var tab = UI.el("button", {
        type: "button", "class": "kc-tab", role: "tab", id: "kc-tab-" + c.id, "aria-selected": c.id === S.ctx ? "true" : "false",
        "aria-controls": "kc-panel", tabindex: c.id === S.ctx ? "0" : "-1", "data-ctx": c.id
      }, [UI.el("span", { text: c.title })]);
      if (n > 0) tab.appendChild(UI.el("span", { "class": "kc-count", "aria-label": "ユーザーの割り当て " + n + " 件", text: String(n) }));
      tab.addEventListener("click", function () { selectContext(c.id, false); });
      list.appendChild(tab);
    });
    list.addEventListener("keydown", function (e) {
      var ids = CONTEXTS.map(function (c) { return c.id; });
      var at = ids.indexOf(S.ctx);
      var next = e.key === "ArrowRight" ? at + 1 : e.key === "ArrowLeft" ? at - 1 : e.key === "Home" ? 0 : e.key === "End" ? ids.length - 1 : -1;
      if (next < 0) return;
      e.preventDefault();
      selectContext(ids[(next + ids.length) % ids.length], true);
    });
    return list;
  }
  function selectContext(id, focus) {
    S.ctx = id;
    setPref("ctx", id);
    paintBindings();
    paintReset();
    if (focus) { var t = document.getElementById("kc-tab-" + id); if (t) t.focus(); }
  }
  function toolbar(ctx) {
    var search = UI.el("input", { type: "text", "class": "field kc-search", placeholder: "操作やキーで探す", "aria-label": "操作やキーで探す", autocomplete: "off", spellcheck: "false" });
    search.value = S.query;
    search.addEventListener("input", function () {
      S.query = search.value;
      refs.tablePaint(ctx);
    });
    var only = UI.el("input", { type: "checkbox", role: "switch", "class": "switch", id: "kc-only-user" });
    only.checked = S.userOnly;
    only.addEventListener("change", function () { S.userOnly = only.checked; paintBindings(); });
    var assign = UI.button("キーを割り当てる", "primary", "plus", function () { openAssign({ context: ctx.editable ? ctx.id : "composing" }); });
    if (!ctx.editable) { assign.disabled = true; assign.title = "この場面のキーは変更できません"; }
    refs.tablePaint = function () {
      var panel = document.getElementById("kc-panel");
      if (panel) { panel.textContent = ""; panel.appendChild(table(ctx)); }
    };
    return UI.el("div", { "class": "kc-toolbar" }, [
      UI.el("div", { "class": "kc-search-box" }, [UI.icon("search"), search]),
      UI.el("label", { "class": "switch-line kc-only", "for": "kc-only-user" }, [UI.el("span", { "class": "switch-line-text", text: "ユーザーの割り当てだけ表示" }), only]),
      UI.el("span", { "class": "spacer" }),
      assign
    ]);
  }

  // 1 つの文脈の表の行（Action ごと。キーが 1 つも無い操作も、編集できる場面では載せる）
  function rowsFor(ctx) {
    var byAction = {};
    var order = [];
    bindings().forEach(function (b) {
      if (b.context !== ctx.id) return;
      if (!byAction[b.action]) { byAction[b.action] = []; order.push(b.action); }
      byAction[b.action].push(b);
    });
    var rows = [];
    var seen = {};
    if (ctx.editable) {
      S.data.actions.forEach(function (a) {
        if (a.contexts.indexOf(ctx.id) < 0) return;
        seen[a.name] = 1;
        rows.push({ action: a.name, keys: byAction[a.name] || [] });
      });
    }
    order.forEach(function (name) { if (!seen[name]) rows.push({ action: name, keys: byAction[name] }); });
    return rows;
  }
  function matches(row, q) {
    if (!q) return true;
    var hay = [row.action, actionLabel(row.action)];
    row.keys.forEach(function (b) { hay.push(b.key, displayParts(b.key).join(" ")); });
    return hay.join("\n").toLowerCase().indexOf(q) >= 0;
  }
  function table(ctx) {
    var q = S.query.trim().toLowerCase();
    var rows = rowsFor(ctx).map(function (r) {
      if (S.userOnly) r = { action: r.action, keys: r.keys.filter(function (b) { return b.source === "user"; }) };
      return r;
    }).filter(function (r) {
      if (S.userOnly && r.keys.length === 0) return false;
      if (!q) return true;
      return matches(r, q);
    });
    var card = UI.card([]);
    if (rows.length === 0) {
      card.appendChild(UI.el("div", { "class": "kc-empty", text: S.userOnly ? "この場面にユーザーの割り当てはありません。" : q ? "一致する操作やキーがありません。" : "割り当てはありません。" }));
      return card;
    }
    rows.forEach(function (r) { card.appendChild(rowNode(ctx, r)); });
    return card;
  }
  function rowNode(ctx, row) {
    var main = UI.el("div", { "class": "kc-main" }, [
      UI.el("p", { "class": "kc-label", text: actionLabel(row.action) }),
      UI.el("p", { "class": "kc-id", text: row.action })
    ]);
    var chips = UI.el("div", { "class": "kc-chips" });
    row.keys.forEach(function (b) { chips.appendChild(chip(ctx, row, b)); });
    if (row.keys.length === 0) chips.appendChild(UI.el("span", { "class": "kc-none", text: "未割り当て" }));
    var ops = UI.el("div", { "class": "kc-ops" });
    if (ctx.editable) {
      var add = UI.button("キーを追加", "quiet sm", "plus", function () { openAssign({ context: ctx.id, action: row.action }); });
      add.setAttribute("aria-label", actionLabel(row.action) + "にキーを追加");
      ops.appendChild(add);
      var mine = row.keys.filter(function (b) { return b.source === "user"; });
      if (mine.length > 0) {
        var reset = UI.button("既定に戻す", "quiet sm", "undo", function () { resetRow(row, mine); });
        reset.setAttribute("aria-label", actionLabel(row.action) + "のユーザーの割り当てを既定に戻す");
        ops.appendChild(reset);
      }
    }
    return UI.el("div", { "class": "kc-row", "data-action": row.action }, [main, chips, ops]);
  }
  function chip(ctx, row, b) {
    var src = SOURCES[b.source] || SOURCES["default"];
    var caps = UI.el("span", { "class": "kc-caps", title: b.key });
    displayParts(b.key).forEach(function (p) { caps.appendChild(UI.el("kbd", { "class": "kc-cap", text: p })); });
    var node = UI.el("span", { "class": "kc-chip " + src.cls, "data-key": b.key }, [caps, UI.el("span", { "class": "kc-src", text: src.label })]);
    if (b.when_mode && MODE_NOTES[b.when_mode]) node.appendChild(UI.el("span", { "class": "kc-src mode", text: MODE_NOTES[b.when_mode] }));
    var name = displayParts(b.key).join("+");
    if (ctx.editable) {
      if (b.source === "user") {
        var edit = chipButton("pencil", "変更", name + " の割り当てを変更", function () { openAssign({ context: ctx.id, action: b.action, key: b.key, oldKey: b.key }); });
        var del = chipButton("x", "解除", name + " の割り当てを解除（既定・プリセットの割当があればそれに戻ります）", function () { removeBinding(ctx.id, b); });
        node.appendChild(edit);
        node.appendChild(del);
      } else {
        var change = chipButton("pencil", "このキーの操作を変える", name + " に割り当てる操作を変える", function () { openAssign({ context: ctx.id, action: b.action, key: b.key, lockKey: true }); });
        var off = chipButton("block", "無効にする", name + " を無効にする（何もしない・キーは消費）", function () { disableBinding(ctx.id, b); });
        node.appendChild(change);
        node.appendChild(off);
      }
    }
    return node;
  }
  function chipButton(iconName, title, label, onClick) {
    var b = UI.el("button", { type: "button", "class": "kc-chip-btn", title: title, "aria-label": label }, [UI.icon(iconName)]);
    b.addEventListener("click", onClick);
    return b;
  }

  // ---- 1 件の保存・解除 ----
  function setBinding(context, key, action) {
    var body = { context: context, key: key };
    if (action) body.action = action;
    return post("set", body);
  }
  function removeBinding(context, b) {
    setBinding(context, b.key, null).then(function (r) {
      adopt(r);
      UI.toast("解除しました", false);
    }, function (e) { UI.toast(e.message, true); });
  }
  function disableBinding(context, b) {
    confirmBox({
      title: "このキーを無効にしますか？",
      lines: [displayParts(b.key).join(" + ") + " を「何もしない」に割り当てます。このキーは押されても消費され、" + actionLabel(b.action) + "は実行されません。「ユーザー」の割り当てとして残るので、あとから解除して元に戻せます。"],
      ok: "無効にする"
    }).then(function (ok) {
      if (!ok) return null;
      return setBinding(context, b.key, "NoOp").then(function (r) { adopt(r); UI.toast("無効にしました", false); });
    }).then(null, function (e) { UI.toast(e.message, true); });
  }
  function resetRow(row, mine) {
    confirmBox({
      title: "この行を既定に戻しますか？",
      lines: [actionLabel(row.action) + "のユーザーの割り当て（" + mine.map(function (b) { return displayParts(b.key).join("+"); }).join("、") + "）を解除します。既定・プリセットの割当があれば、そのキーはそれに戻ります。"],
      ok: "戻す"
    }).then(function (ok) {
      if (!ok) return null;
      // 1 件ずつ解除する（最後の応答が最新の一覧）
      var chain = Promise.resolve(null);
      mine.forEach(function (b) { chain = chain.then(function () { return setBinding(b.context, b.key, null); }); });
      return chain.then(function (r) { if (r) { adopt(r); UI.toast("既定に戻しました", false); } });
    }).then(null, function (e) { UI.toast(e.message, true); reload(); });
  }

  // ---- 割り当てダイアログ ----
  // opts: {context, action?, key?, oldKey?, lockKey?}。oldKey があれば「変更」（新しいキーを保存し、古いユーザー割当を解除）
  function openAssign(opts) {
    var state = { context: opts.context, action: opts.action || "", key: opts.key || "", result: null, seq: 0, timer: 0, busy: false };
    return openDialog(function (body, id, close) {
      body.appendChild(UI.el("h2", { id: id + "-t", text: opts.oldKey ? "割り当てを変更" : opts.lockKey ? "操作を変更" : "キーを割り当てる" }));

      // 場面（入力中 / 変換中）
      var ctxSel = UI.el("select", { "class": "field", id: id + "-ctx" });
      CONTEXTS.filter(function (c) { return c.editable; }).forEach(function (c) { ctxSel.appendChild(UI.el("option", { value: c.id, text: c.title })); });
      ctxSel.value = state.context;
      // 操作
      var actSel = UI.el("select", { "class": "field kc-wide", id: id + "-act" });
      function fillActions() {
        actSel.textContent = "";
        actSel.appendChild(UI.el("option", { value: "", text: "（操作を選んでください）" }));
        S.data.actions.forEach(function (a) {
          if (a.contexts.indexOf(state.context) < 0) return;
          actSel.appendChild(UI.el("option", { value: a.name, text: a.label + "（" + a.name + "）" }));
        });
        // 一覧に無い Action（NoOp にした既定の割当など）が現在の値のときも選べるようにする
        if (state.action && !Array.prototype.some.call(actSel.options, function (o) { return o.value === state.action; })) {
          actSel.appendChild(UI.el("option", { value: state.action, text: actionLabel(state.action) + "（" + state.action + "）" }));
        }
        actSel.value = state.action;
      }
      fillActions();
      if (opts.oldKey || opts.lockKey) ctxSel.disabled = true;

      // キー入力（押して取り込む）と手入力
      var captureText = UI.el("span", { "class": "kc-capture-text" });
      var capture = UI.el("div", { "class": "kc-capture", tabindex: "0", role: "group", "aria-label": "割り当てるキー。ここにフォーカスしてキーを押すと取り込みます", id: id + "-cap" }, [captureText]);
      var manual = UI.el("input", { type: "text", "class": "field", id: id + "-man", autocomplete: "off", spellcheck: "false", placeholder: "例: Ctrl+Shift+J / F8 / Henkan" });
      var hint = UI.el("p", { "class": "row-desc kc-hint" });
      var capturing = false;
      function showCapture(held) {
        captureText.textContent = "";
        capture.classList.toggle("active", capturing);
        if (capturing && held && held.length) {
          held.forEach(function (h) { captureText.appendChild(UI.el("kbd", { "class": "kc-cap", text: displayParts(h).join("") })); });
          captureText.appendChild(UI.el("span", { "class": "kc-capture-hint", text: "続けてキーを押す…" }));
          return;
        }
        if (state.key) {
          displayParts(state.key).forEach(function (p) { captureText.appendChild(UI.el("kbd", { "class": "kc-cap", text: p })); });
          if (capturing) captureText.appendChild(UI.el("span", { "class": "kc-capture-hint", text: "別のキーを押すと置き換わります" }));
        } else {
          captureText.appendChild(UI.el("span", { "class": "kc-capture-hint", text: capturing ? "キーを押してください（Esc で取り込みをやめる）" : "ここをクリックして、割り当てるキーを押す" }));
        }
      }
      if (opts.lockKey) {
        capture.setAttribute("aria-disabled", "true");
        capture.classList.add("locked");
        capture.removeAttribute("tabindex");
        manual.disabled = true;
      } else {
        capture.addEventListener("focus", function () { capturing = true; showCapture(); });
        capture.addEventListener("blur", function () { capturing = false; showCapture(); });
        capture.addEventListener("keydown", function (e) {
          var r = specFromEvent(e);
          if (r.kind === "pass") return; // Tab はフォーカス移動に使う
          e.preventDefault();
          e.stopPropagation();
          if (e.repeat) return;
          if (r.kind === "cancel") { capture.blur(); hint.textContent = ""; return; }
          if (r.kind === "pending") { showCapture(r.held); hint.textContent = ""; return; }
          if (r.kind === "unsupported") { hint.textContent = r.message; showCapture(); return; }
          hint.textContent = "";
          state.key = r.spec;
          manual.value = r.spec;
          showCapture();
          changed();
        });
        capture.addEventListener("keyup", function (e) { if (capturing) { e.preventDefault(); showCapture(); } });
        capture.addEventListener("click", function () { capture.focus(); });
        manual.addEventListener("input", function () { state.key = manual.value.trim(); hint.textContent = ""; showCapture(); changed(); });
      }
      manual.value = state.key;
      showCapture();

      var issues = UI.el("div", { "class": "kc-issues", "aria-live": "polite" });
      var same = UI.el("p", { "class": "row-desc kc-existing", hidden: true });
      var save = UI.button("割り当てる", "primary", null, function () { submit(); });
      var cancel = UI.button("キャンセル", "", null, function () { close("cancel"); });

      ctxSel.addEventListener("change", function () { state.context = ctxSel.value; state.action = ""; fillActions(); changed(); });
      actSel.addEventListener("change", function () { state.action = actSel.value; changed(); });

      function field(label, forId, control, extra) {
        return UI.el("div", { "class": "kc-field" }, [UI.el("label", { "for": forId, text: label }), control, extra || null]);
      }
      body.appendChild(UI.el("div", { "class": "kc-form" }, [
        field("使う場面", id + "-ctx", ctxSel),
        field("操作", id + "-act", actSel),
        field("キー", id + "-cap", capture, hint),
        field("キーを手で入力", id + "-man", manual, UI.el("p", { "class": "row-desc", text: "Win キー・Alt+F4・Ctrl+Alt+Delete など、OS が先に処理するキーは押しても取り込めません。そのようなキーはここに書けますが、アプリに届かないため動作しないことがあります。変換・無変換は Henkan・Muhenkan と書きます。" })),
        same,
        issues
      ]));
      body.appendChild(UI.el("div", { "class": "buttons end" }, [cancel, save]));

      // 検証（TOML 断片を作ってコアに検証させる。入力が止まってから）
      function fragment() {
        return "[keymap." + state.context + "]\n" + JSON.stringify(state.key) + " = " + JSON.stringify(state.action) + "\n";
      }
      function changed() {
        state.result = null;
        window.clearTimeout(state.timer);
        showExisting();
        paintIssues();
        updateSave();
        if (!state.key || !state.action) return;
        var seq = ++state.seq;
        state.timer = window.setTimeout(function () {
          post("validate", { toml: fragment() }).then(function (r) {
            if (seq !== state.seq) return;
            state.result = r;
            paintIssues();
            updateSave();
          }, function (e) {
            if (seq !== state.seq) return;
            state.result = { errors: [{ message: e.message, line: 0 }], warnings: [] };
            paintIssues();
            updateSave();
          });
        }, 180);
      }
      function showExisting() {
        // 同じ場面の同じキーに、いま別の割当があるか（置き換わることを伝える。拒否ではない）
        var found = null;
        if (state.key) {
          var want = state.key.toLowerCase().replace(/control\+/g, "ctrl+");
          bindings().forEach(function (b) {
            if (b.context === state.context && b.key.toLowerCase() === want && b.action !== state.action) found = b;
          });
        }
        if (found && !(opts.oldKey && found.key === opts.oldKey && found.action === opts.action)) {
          same.hidden = false;
          same.textContent = "このキーは今「" + actionLabel(found.action) + "」（" + SOURCES[found.source].label + "）に割り当てられています。割り当てると、この操作に置き換わります。";
        } else {
          same.hidden = true;
          same.textContent = "";
        }
      }
      function paintIssues() {
        issues.textContent = "";
        var r = state.result;
        if (!r) return;
        r.errors.forEach(function (i) { issues.appendChild(issueNode(i, "ng")); });
        r.warnings.forEach(function (i) { issues.appendChild(issueNode(i, "warn")); });
      }
      function issueNode(i, kind) {
        return UI.el("div", { "class": "kc-issue " + kind }, [UI.icon(kind === "ng" ? "x" : "alert"), UI.el("p", { text: i.message })]);
      }
      function updateSave() {
        var ready = !!state.key && !!state.action && !!state.result && state.result.errors.length === 0 && !state.busy;
        save.disabled = !ready;
        var warn = state.result && state.result.warnings.length > 0;
        save.querySelector("span").textContent = warn ? "それでも割り当てる" : "割り当てる";
      }
      function submit() {
        if (save.disabled) return;
        state.busy = true;
        updateSave();
        var chain = Promise.resolve(null);
        // 変更: 古いユーザー割当を先に解除してから新しいキーを保存する（同じキーなら、保存で書き直される）
        if (opts.oldKey && opts.oldKey !== state.key) chain = setBinding(opts.context, opts.oldKey, null);
        chain.then(function () { return setBinding(state.context, state.key, state.action); }).then(function (r) {
          adopt(r);
          var n = r.warnings.length;
          UI.toast(n > 0 ? "割り当てました（注意 " + n + " 件）" : "割り当てました", false);
          close("ok");
        }, function (e) {
          state.busy = false;
          state.result = { errors: [{ message: e.message, line: 0 }], warnings: [] };
          paintIssues();
          updateSave();
          reload();
        });
      }
      updateSave();
      if (state.key && state.action) changed();
      if (!opts.lockKey && !state.key) window.setTimeout(function () { capture.focus(); }, 0);
    });
  }

  // ---- インポート / エクスポート ----
  function paintTransfer() {
    if (!refs.transfer) return;
    if (!S.data) { refs.transfer.textContent = ""; return; }
    var area = UI.el("textarea", { "class": "report-text kc-toml", rows: "10", spellcheck: "false", "aria-label": "キー割当の TOML", id: "kc-toml", placeholder: "[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"" });
    area.value = S.transferText;
    var status = UI.el("div", { "class": "kc-issues", "aria-live": "polite" });
    var mergeBox = UI.el("input", { type: "checkbox", role: "switch", "class": "switch", id: "kc-merge" });
    mergeBox.checked = S.merge;
    var apply = UI.button("適用", "primary", "check", function () { doImport(); });
    var timer = 0;
    var seq = 0;

    function paintStatus(r, text) {
      status.textContent = "";
      if (!text.trim()) { apply.disabled = true; return; }
      if (!r) { apply.disabled = true; return; }
      r.errors.forEach(function (i) {
        status.appendChild(UI.el("div", { "class": "kc-issue ng" }, [UI.icon("x"), UI.el("p", { text: (i.line > 0 ? i.line + " 行目: " : "") + i.message })]));
      });
      r.warnings.forEach(function (i) {
        status.appendChild(UI.el("div", { "class": "kc-issue warn" }, [UI.icon("alert"), UI.el("p", { text: (i.line > 0 ? i.line + " 行目: " : "") + i.message })]));
      });
      if (r.errors.length === 0 && r.warnings.length === 0) {
        status.appendChild(UI.el("div", { "class": "kc-issue ok" }, [UI.icon("check"), UI.el("p", { text: "問題は見つかりませんでした。" })]));
      }
      apply.disabled = r.errors.length > 0;
    }
    function validateNow() {
      var text = area.value;
      S.transferText = text;
      S.transferValid = null;
      window.clearTimeout(timer);
      var my = ++seq;
      if (!text.trim()) { paintStatus(null, text); return; }
      apply.disabled = true;
      timer = window.setTimeout(function () {
        post("validate", { toml: text }).then(function (r) {
          if (my !== seq) return;
          S.transferValid = r;
          paintStatus(r, text);
        }, function (e) {
          if (my !== seq) return;
          status.textContent = "";
          status.appendChild(UI.el("div", { "class": "kc-issue ng" }, [UI.icon("x"), UI.el("p", { text: e.message })]));
          apply.disabled = true;
        });
      }, 300);
    }
    function setText(text, base) {
      area.value = text;
      S.transferText = text;
      S.transferBase = base;
      validateNow();
    }
    refs.setTransferText = setText;
    area.addEventListener("input", validateNow);
    mergeBox.addEventListener("change", function () { S.merge = mergeBox.checked; });

    function loadExport() {
      return post("export", {}).then(function (r) {
        setText(r.toml, r.toml);
      }, function (e) { UI.toast(e.message, true); });
    }
    function doImport() {
      var text = area.value;
      var merge = mergeBox.checked;
      confirmBox({
        title: merge ? "キー割当を追加しますか？" : "キー割当を置き換えますか？",
        lines: merge
          ? ["入力欄の割り当てを、いまのユーザーの割り当てに追加します（同じキーは入力欄の内容で上書きします）。"]
          : ["いまのユーザーの割り当て " + userCount() + " 件を、入力欄の内容で置き換えます。入力欄に無い割り当ては解除されます。"],
        ok: "適用する"
      }).then(function (ok) {
        if (!ok) return null;
        return post("import", { toml: text, merge: merge }).then(function (r) {
          adopt(r);
          UI.toast("キー割当を適用しました", false);
          return loadExport();
        });
      }).then(null, function (e) { UI.toast(e.message, true); });
    }
    function copy() {
      var text = area.value;
      var done = function () { UI.toast("コピーしました", false); };
      var fallback = function () {
        area.focus();
        area.select();
        var ok = false;
        try { ok = document.execCommand("copy"); } catch (e) { ok = false; }
        if (ok) done(); else UI.toast("コピーできませんでした。選択した文字を Ctrl+C でコピーしてください。", true);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, fallback);
      else fallback();
    }
    function saveFile() {
      var blob = new Blob([area.value], { type: "text/plain;charset=utf-8" });
      var url = URL.createObjectURL(blob);
      var a = UI.el("a", { href: url, download: "nagi-keymap.toml" });
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      window.setTimeout(function () { URL.revokeObjectURL(url); }, 4000);
    }
    var file = UI.el("input", { type: "file", accept: ".toml,.txt,text/plain", "class": "sr-only", tabindex: "-1", "aria-hidden": "true" });
    file.addEventListener("change", function () {
      var f = file.files && file.files[0];
      if (!f) return;
      if (f.size > 60 * 1024) { UI.toast("ファイルが大きすぎます（60KB まで）", true); file.value = ""; return; }
      f.text().then(function (t) { setText(t, S.transferBase); file.value = ""; }, function () { UI.toast("ファイルを読み込めませんでした", true); });
    });

    var card = UI.card([
      UI.el("div", { "class": "card-pad stack" }, [
        UI.el("p", { "class": "muted", text: "ユーザーの割り当てを config.toml の [keymap.*] と同じ書式で書き出し・読み込みします。読み込むときは、問題のある行を行番号つきで確認してから適用します（問題のある行があると適用できません）。" }),
        UI.el("div", { "class": "buttons" }, [
          UI.button("現在の設定を書き出す", "", "refresh", loadExport),
          UI.button("コピー", "", "copy", copy),
          UI.button("ファイルに保存", "", "save", saveFile),
          UI.button("ファイルを読み込む", "", "upload", function () { file.click(); }),
          file
        ]),
        area,
        status,
        UI.el("div", { "class": "kc-apply" }, [
          UI.el("label", { "class": "switch-line", "for": "kc-merge" }, [UI.el("span", { "class": "switch-line-text", text: "いまの割り当てを残して追加する" }), mergeBox]),
          apply
        ])
      ])
    ]);
    replace(refs.transfer, UI.section("インポート / エクスポート", "", card));
    if (S.transferBase === null) loadExport();
    else validateNow();
  }
  // 入力欄が最後の書き出しのまま（手を入れていない）なら、保存後の内容に更新する
  function refreshTransferIfClean() {
    if (S.transferBase !== null && S.transferText === S.transferBase && refs.setTransferText) {
      post("export", {}).then(function (r) { if (refs.setTransferText) refs.setTransferText(r.toml, r.toml); }, function () {});
    }
  }

  // ---- リセット ----
  function paintReset() {
    if (!refs.reset) return;
    if (!S.data) { refs.reset.textContent = ""; return; }
    var ctx = contextById(S.ctx);
    var nCtx = ctx.editable ? userCount(ctx.id) : 0;
    var nAll = userCount();
    var btnCtx = UI.button(ctx.editable ? "「" + ctx.title + "」のユーザーの割り当てを消す" : "この場面は変更できません", "", "undo", function () { resetContext(ctx); });
    btnCtx.disabled = nCtx === 0;
    var btnAll = UI.button("すべてのユーザーの割り当てを消す", "danger", "trash", function () { resetAll(); });
    btnAll.disabled = nAll === 0;
    var card = UI.card([
      UI.el("div", { "class": "card-pad stack" }, [
        UI.el("p", { "class": "muted", text: "行ごとの「既定に戻す」は、各行の右側にあります。ここでは場面ごと・全体のユーザーの割り当てをまとめて消します。消した後は、選んでいるプリセットと既定の割当に戻ります。プリセットの選択は変わりません。" }),
        UI.el("div", { "class": "buttons" }, [btnCtx, btnAll]),
        UI.el("p", { "class": "row-desc", text: "ユーザーの割り当て: " + nAll + " 件" + (ctx.editable ? "（「" + ctx.title + "」" + nCtx + " 件）" : "") })
      ])
    ]);
    replace(refs.reset, UI.section("リセット", "", card));
  }
  function resetContext(ctx) {
    confirmBox({
      title: "「" + ctx.title + "」の割り当てを消しますか？",
      lines: ["「" + ctx.title + "」のユーザーの割り当て " + userCount(ctx.id) + " 件をすべて解除します。この操作は取り消せません（先にエクスポートしておくと復元できます）。"],
      ok: "消す", danger: true
    }).then(function (ok) {
      if (!ok) return null;
      return post("reset", { scope: "context", context: ctx.id }).then(function (r) { adopt(r); UI.toast("消しました", false); });
    }).then(null, function (e) { UI.toast(e.message, true); });
  }
  function resetAll() {
    confirmBox({
      title: "すべてのユーザーの割り当てを消しますか？",
      lines: ["入力中・変換中のユーザーの割り当て " + userCount() + " 件をすべて解除します。プリセットの選択は変わりません。この操作は取り消せません（先にエクスポートしておくと復元できます）。"],
      ok: "すべて消す", danger: true
    }).then(function (ok) {
      if (!ok) return null;
      return post("reset", { scope: "all" }).then(function (r) { adopt(r); UI.toast("すべて消しました", false); });
    }).then(null, function (e) { UI.toast(e.message, true); });
  }

  (window.NagiPages = window.NagiPages || []).push({ id: "keymap", title: "キー割当", icon: "keyboard", render: render });
})();
