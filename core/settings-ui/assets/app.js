"use strict";
// 設定画面。項目はすべてコアの記述子（/api/settings）から組み立てる。値は textContent / value でだけ入れる。
(function () {
  var token = document.querySelector('meta[name="nagi-token"]').content;
  var RESTART_MESSAGE = "変更を反映するには入力方式を切り替え直すかアプリを開き直してください";
  var DISCONNECTED = "設定画面のサーバーに接続できません。このタブを閉じて、もう一度「設定…」から開いてください。";

  var screen = null;
  var level = "simple";
  var errors = {};
  var groupsEl = document.getElementById("groups");
  var bannerEl = document.getElementById("banner");
  var resultEl = document.getElementById("action-result");

  function el(tag, attrs, children) {
    var node = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      if (k === "text") node.textContent = attrs[k];
      else node.setAttribute(k, attrs[k]);
    });
    (children || []).forEach(function (c) { node.appendChild(c); });
    return node;
  }

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

  function typed(kind, value) {
    if (value === null) return null;
    var t = kind.type === "choice" ? "text" : kind.type;
    return { type: t, value: value };
  }

  function commit(item, value) {
    showBanner("");
    return api("/api/update", { changes: [{ key: item.key, value: typed(item.kind, value) }] }).then(function (r) {
      errors = {};
      r.errors.forEach(function (e) { errors[e.key] = e.message; });
      screen = r.screen;
      render();
    }, function (e) {
      errors = {};
      errors[item.key] = e.message;
      render();
    });
  }

  function rangeText(item) {
    var k = item.kind;
    if (k.type !== "int" && k.type !== "float") return "";
    var text = "範囲 " + k.min + " から " + k.max;
    if (item.default !== null) text += "（既定 " + item.default + "）";
    return text;
  }

  function control(item, id, descId) {
    var k = item.kind;
    var input;
    if (k.type === "bool") {
      input = el("input", { type: "checkbox", id: id, role: "switch", "class": "switch" });
      input.checked = item.value === true;
      input.addEventListener("change", function () { commit(item, input.checked); });
    } else if (k.type === "int" || k.type === "float") {
      input = el("input", { type: "number", id: id, min: String(k.min), max: String(k.max), step: String(k.step) });
      if (item.value !== null) input.value = String(item.value);
      else input.placeholder = "自動";
      input.addEventListener("change", function () {
        if (input.value.trim() === "") { commit(item, null); return; }
        var n = Number(input.value);
        if (!isFinite(n) || (k.type === "int" && !Number.isInteger(n))) {
          errors = {};
          errors[item.key] = k.type === "int" ? "整数で入力してください" : "数値で入力してください";
          render();
          return;
        }
        commit(item, n);
      });
    } else if (k.type === "choice") {
      input = el("select", { id: id });
      if (item.value === null) input.appendChild(el("option", { value: "", text: "（自動）" }));
      k.options.forEach(function (o) { input.appendChild(el("option", { value: o.value, text: o.label })); });
      input.value = item.value === null ? "" : String(item.value);
      input.addEventListener("change", function () { commit(item, input.value === "" ? null : input.value); });
    } else {
      input = el("input", { type: "text", id: id, autocomplete: "off", spellcheck: "false" });
      if (item.value !== null) input.value = String(item.value);
      input.addEventListener("change", function () { commit(item, input.value === "" ? null : input.value); });
    }
    input.setAttribute("data-key", item.key);
    input.setAttribute("aria-describedby", descId);
    return input;
  }

  function row(item, n) {
    var id = "s" + n;
    var descId = id + "-desc";
    var head = el("div", { "class": "head" }, [el("label", { "class": "name", "for": id, text: item.label })]);
    var box = el("div", { "class": "control" });
    var unit = rangeText(item);
    if (unit) box.appendChild(el("span", { "class": "range", text: unit }));
    box.appendChild(control(item, id, descId));
    if (item.explicit) {
      var reset = el("button", { type: "button", "class": "small", text: "既定に戻す", "aria-label": item.label + "を既定に戻す" });
      reset.addEventListener("click", function () { commit(item, null); });
      box.appendChild(reset);
    }
    head.appendChild(box);
    var r = el("div", { "class": "row" }, [head, el("p", { "class": "desc", id: descId, text: item.description })]);
    if (item.requires_restart) r.appendChild(el("span", { "class": "badge", text: RESTART_MESSAGE }));
    if (errors[item.key]) r.appendChild(el("p", { "class": "error", role: "alert", text: errors[item.key] }));
    return r;
  }

  function render() {
    var active = document.activeElement;
    var activeKey = active && active.getAttribute ? active.getAttribute("data-key") : null;
    groupsEl.textContent = "";
    var n = 0;
    if (!screen) return;
    document.getElementById("config-path").textContent = screen.config_path || "（設定ファイルの場所は未定です）";
    document.getElementById("open-config").disabled = !screen.config_path;
    document.getElementById("open-user-dict").disabled = !screen.config_path;
    if (screen.error) showBanner(screen.error);
    screen.groups.forEach(function (g, gi) {
      var items = g.items.filter(function (i) { return level === "detail" || i.level === "simple"; });
      if (items.length === 0) return;
      var section = el("section", { "aria-labelledby": "g" + gi });
      section.appendChild(el("h2", { id: "g" + gi, text: g.name }));
      items.forEach(function (item) { section.appendChild(row(item, n++)); });
      groupsEl.appendChild(section);
    });
    if (activeKey) {
      var again = groupsEl.querySelector('[data-key="' + (window.CSS && CSS.escape ? CSS.escape(activeKey) : activeKey) + '"]');
      if (again) again.focus();
    }
  }

  function load() {
    return api("/api/settings").then(function (s) {
      screen = s;
      render();
    }, function (e) { showBanner(e.message); });
  }

  function act(target) {
    resultEl.textContent = "";
    api("/api/open", { target: target }).then(function () {
      resultEl.textContent = "開きました。";
    }, function (e) { resultEl.textContent = e.message; });
  }

  document.getElementById("open-config").addEventListener("click", function () { act("config"); });
  document.getElementById("open-user-dict").addEventListener("click", function () { act("user_dict"); });

  var dialog = document.getElementById("confirm");
  document.getElementById("clear-learning").addEventListener("click", function () {
    dialog.returnValue = "";
    dialog.showModal();
  });
  document.getElementById("confirm-cancel").addEventListener("click", function () { dialog.close("cancel"); });
  document.getElementById("confirm-ok").addEventListener("click", function () { dialog.close("ok"); });
  dialog.addEventListener("close", function () {
    if (dialog.returnValue !== "ok") return;
    resultEl.textContent = "";
    api("/api/clear_learning").then(function () {
      resultEl.textContent = "学習データを削除しました。";
    }, function (e) { resultEl.textContent = e.message; });
  });

  Array.prototype.forEach.call(document.querySelectorAll('input[name="level"]'), function (radio) {
    radio.addEventListener("change", function () {
      if (radio.checked) { level = radio.value; errors = {}; render(); }
    });
  });

  // ページが閉じられたら、サーバー（と設定エンジン）を終了させる。再読み込みでは 5 秒以内に API が来て取り消される
  window.addEventListener("pagehide", function () {
    navigator.sendBeacon("/bye?t=" + encodeURIComponent(token), new Blob(["{}"], { type: "application/json" }));
  });
  window.addEventListener("pageshow", function (e) { if (e.persisted) load(); });

  load();
})();
