// displayText（app.js の純粋関数）の入出力テスト。`node tests/e2e/display_text.cjs`（依存なし）
"use strict";
const fs = require("fs");
const path = require("path");
const assert = require("assert");

// app.js から displayText の定義だけを取り出して評価する
const src = fs.readFileSync(path.join(__dirname, "../../assets/app.js"), "utf8");
const m = src.match(/function displayText\(s\) \{[\s\S]*?\n  \}\n/);
assert(m, "displayText が見つからない");
const displayText = new Function(m[0] + "\nreturn displayText;")();

const cases = [
  ["タスクバーのアイコン（Windows のみ）", "タスクバーのアイコン"],
  ["候補ウィンドウのテンプレート (Windows のみ)", "候補ウィンドウのテンプレート"],
  ["候補ウィンドウのテンプレート（ Windows のみ ）", "候補ウィンドウのテンプレート"],
  ["アイコン Windows のみ", "アイコン"],
  ["影を付けます（Windows のみ）。次に反映されます。", "影を付けます。次に反映されます。"],
  ["Windows の標準の動作に合わせます。", "Windows の標準の動作に合わせます。"],
  ["Windows のみで動く", "Windows のみで動く"],
  ["Windows の表示", "Windows の表示"],
  ["", ""]
];
cases.forEach(function (c) { assert.strictEqual(displayText(c[0]), c[1], JSON.stringify(c[0])); });
assert.strictEqual(displayText(null), null);
console.log("display_text: OK (" + cases.length + " cases)");
