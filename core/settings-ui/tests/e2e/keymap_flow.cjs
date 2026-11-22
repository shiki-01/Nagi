#!/usr/bin/env node
// キー割当ページの操作テスト（Playwright + chromium。マウスの実クリックとキー入力）。CI には組み込まない手動用。
//   使い方（core/ で）:  cargo build -p nagi-settings-ui --bin nagi-settings -p nagi-server --bin imserver
//                       node settings-ui/tests/e2e/keymap_flow.cjs [スクリーンショットの出力先]
//   playwright は node_modules に無ければ、グローバルの場所（NODE_PATH / /opt/node22/lib/node_modules）から読む。
//   見る点: 各ステップで「ユーザーの目に見える反応」があること（編集パネルが開く・フォーカス枠・取り込んだキーの表示・
//           トースト・行のハイライト）。画面内に収まっているか（getBoundingClientRect）も確かめる。
"use strict";
const fs = require("fs");
const os = require("os");
const path = require("path");
const cp = require("child_process");

function loadPlaywright() {
  for (const p of ["playwright", "/opt/node22/lib/node_modules/playwright", path.join(process.env.NODE_PATH || "", "playwright")]) {
    try { return require(p); } catch (e) { /* 次へ */ }
  }
  throw new Error("playwright が見つかりません（npm i -g playwright）");
}
const { chromium } = loadPlaywright();

const root = path.resolve(__dirname, "../../..");
const bin = (n) => path.join(root, "target/debug", n + (process.platform === "win32" ? ".exe" : ""));
const outDir = process.argv[2] || fs.mkdtempSync(path.join(os.tmpdir(), "nagi-km-shots-"));
fs.mkdirSync(outDir, { recursive: true });

let failures = 0;
function check(cond, msg) {
  console.log((cond ? "  ok   " : "  FAIL ") + msg);
  if (!cond) failures++;
}

async function startServer() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nagi-km-e2e-"));
  const urlFile = path.join(dir, "url.txt");
  const child = cp.spawn(bin("nagi-settings"), [
    "--web", "--no-browser", "--url-file", urlFile, "--imserver", bin("imserver"),
    "--user-dir", dir, "--data-dir", path.join(dir, "data")
  ], { stdio: "ignore" });
  for (let i = 0; i < 100 && !fs.existsSync(urlFile); i++) await new Promise((r) => setTimeout(r, 100));
  if (!fs.existsSync(urlFile)) throw new Error("設定画面が起動しませんでした");
  return { child, url: fs.readFileSync(urlFile, "utf8").trim() };
}

// 要素の中央を実マウスでクリックする（要素が画面内にあることも確かめる）
async function mouseClick(page, locator, what) {
  await locator.scrollIntoViewIfNeeded();
  const box = await locator.boundingBox();
  if (!box) throw new Error(what + " が見つかりません");
  const vp = page.viewportSize();
  check(box.x >= 0 && box.y >= 0 && box.x + box.width <= vp.width && box.y + box.height <= vp.height + 1, what + " が画面内にある");
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
}

async function flow(browser, url, v) {
  console.log("== " + v.name);
  const ctx = await browser.newContext({
    viewport: { width: v.w, height: v.h }, deviceScaleFactor: v.scale || 1,
    colorScheme: v.dark ? "dark" : "light", forcedColors: v.forced ? "active" : "none"
  });
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  const shot = (n) => page.screenshot({ path: path.join(outDir, v.name + "-" + n + ".png") });
  await page.goto(url);
  await page.click('.nav-item[data-page="keymap"]');
  await page.waitForSelector(".kc-row");
  await shot("01-list");

  // 1) 場面タブ（変換中）を実クリック
  await mouseClick(page, page.locator("#kc-tab-converting"), "「変換中」タブ");
  check((await page.locator("#kc-tab-converting").getAttribute("aria-selected")) === "true", "タブが選択状態になる");
  await mouseClick(page, page.locator("#kc-tab-composing"), "「入力中」タブ");

  // 2) 行の「キーを追加」→ その行の下に編集パネル
  const row = page.locator('.kc-row[data-action="ToKatakana"]');
  await row.waitFor();
  await mouseClick(page, row.locator(".kc-add"), "「キーを追加」");
  const editor = page.locator(".kc-editor");
  await editor.waitFor();
  const title = await editor.locator("h3").innerText();
  check(/『.+』のキーを割り当てる/.test(title), "見出し: " + title);
  check(await row.evaluate((r) => r.classList.contains("editing")), "操作中の行が強調される");
  check(await row.evaluate((r) => r.nextElementSibling && r.nextElementSibling.classList.contains("kc-editor")), "編集パネルは行のすぐ下に開く");
  const ebox = await editor.boundingBox();
  check(ebox && ebox.y >= 0 && ebox.y + Math.min(ebox.height, 200) <= v.h, "編集パネルが画面内に見えている");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-capture")), "キー入力欄に自動でフォーカスが入る");
  check(/割り当てるキーを押してください/.test(await page.locator(".kc-capture").innerText()), "案内文: 割り当てるキーを押してください");
  check(await editor.locator(".kc-save").isDisabled(), "取り込み前は「割り当てる」が無効");
  await shot("02-editor-open");

  // 3) キーを押す（IME オンの Process を模した合成イベントも）
  await page.keyboard.press("Control+Shift+KeyY");
  check((await page.locator(".kc-capture .kc-cap").allInnerTexts()).join("|") === "Ctrl|Shift|Y", "押したキーがキーチップで即座に出る");
  await page.waitForFunction(() => !document.querySelector(".kc-save").disabled, null, { timeout: 4000 });
  check(true, "取り込み後に「割り当てる」が有効になる");
  await shot("03-captured");

  // 4) 実クリックで確定 → トースト・行のハイライト・パネルが閉じる
  await mouseClick(page, editor.locator(".kc-save"), "「割り当てる」");
  await page.waitForSelector(".toast");
  check(/保存しました/.test(await page.locator(".toast").last().innerText()), "トースト: 保存しました");
  check((await page.locator(".kc-editor").count()) === 0, "編集パネルが閉じる");
  check(await page.locator('.kc-row[data-action="ToKatakana"]').evaluate((r) => r.classList.contains("flash")), "保存した行がハイライトされる");
  const chip = page.locator('.kc-row[data-action="ToKatakana"] .kc-chip[data-key="Ctrl+Shift+y" i]');
  check((await chip.count()) === 1, "新しいキーがチップとして並ぶ");
  await shot("04-saved");

  // 5) キーチップ（ボタン）を実クリック → 変更/解除の編集パネル。Esc で閉じてフォーカスが戻る
  await page.waitForTimeout(3800);
  check(!(await page.locator('.kc-row[data-action="ToKatakana"]').evaluate((r) => r.classList.contains("flash"))), "ハイライトは数秒で消える");
  await mouseClick(page, chip, "キーチップ");
  await editor.waitFor();
  check(/変更する/.test(await editor.locator("h3").innerText()), "チップを押すと変更パネルが開く");
  check(await editor.getByRole("button", { name: /このキーを解除/ }).count() === 1, "「このキーを解除」がある");
  await shot("05-chip-editor");
  await page.keyboard.press("Escape");
  check((await page.locator(".kc-editor").count()) === 0, "Esc で編集パネルが閉じる（設定画面は閉じない）");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-chip")), "閉じるとフォーカスがチップへ戻る");

  // 6) 「キー」ラベルを押しても入力欄にフォーカスが入る
  await mouseClick(page, row.locator(".kc-add"), "「キーを追加」");
  await mouseClick(page, page.locator('.kc-editor .kc-field > label[for$="-cap"]'), "「キー」ラベル");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-capture")), "「キー」ラベルを押すと入力欄にフォーカスが入る");
  // 取り込み枠を押しても取り込み待ちになる
  await page.locator("#kc-panel").click({ position: { x: 5, y: 5 } });
  await mouseClick(page, page.locator(".kc-editor .kc-capture"), "キー入力欄");
  check(/割り当てるキーを押してください/.test(await page.locator(".kc-capture").innerText()), "入力欄を押すと取り込み待ちの表示になる");
  await page.keyboard.press("Escape");

  // 7) チップから解除
  await mouseClick(page, chip, "キーチップ");
  await mouseClick(page, editor.getByRole("button", { name: /このキーを解除/ }), "「このキーを解除」");
  await page.waitForFunction(() => !document.querySelector('.kc-chip[data-key="Ctrl+Shift+y" i]'));
  check(/解除しました/.test(await page.locator(".toast").last().innerText()), "トースト: 解除しました");
  await shot("06-removed");

  // 8) キーボードだけで一巡（Enter で開く → Ctrl+Shift+U を押す → 保存ボタンへ Tab → Enter）
  await row.locator(".kc-add").focus();
  await page.keyboard.press("Enter");
  await editor.waitFor();
  await page.keyboard.press("Control+Shift+KeyU");
  await page.waitForFunction(() => !document.querySelector(".kc-save").disabled, null, { timeout: 4000 });
  await page.locator(".kc-save").focus();
  await page.keyboard.press("Enter");
  await page.waitForSelector('.kc-row[data-action="ToKatakana"] .kc-chip[data-key="Ctrl+Shift+u" i]');
  check(await page.evaluate(() => document.activeElement && document.activeElement.getAttribute("data-key").toLowerCase() === "ctrl+shift+u"), "保存後、新しいキーのチップにフォーカスが移る");
  await page.keyboard.press("Enter");
  await editor.waitFor();
  await page.keyboard.press("Tab"); // 解除ボタンなど、パネル内をたどれる
  await page.getByRole("button", { name: /このキーを解除/ }).focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => !document.querySelector('.kc-chip[data-key="Ctrl+Shift+u" i]'));
  check(true, "キーボードだけで追加・変更パネル・解除ができる");
  // 窓を最大にしなくても入る幅か（横スクロールが無い）
  check(await page.evaluate(() => document.getElementById("main").scrollWidth <= document.getElementById("main").clientWidth + 1), "横スクロールが出ない");
  check(errors.length === 0, "コンソールにエラーが無い" + (errors.length ? ": " + errors.join(" / ") : ""));
  await ctx.close();
}

(async () => {
  const srv = await startServer();
  const browser = await chromium.launch();
  const variants = [
    { name: "light-1280", w: 1280, h: 800 },
    { name: "light-840", w: 840, h: 600 },
    { name: "dark-840-x2", w: 840, h: 600, dark: true, scale: 2 },
    { name: "forced-840", w: 840, h: 600, forced: true }
  ];
  try {
    for (const v of variants) await flow(browser, srv.url, v);
  } finally {
    await browser.close();
    srv.child.kill();
  }
  console.log(failures === 0 ? "ALL OK  shots: " + outDir : failures + " 件の失敗  shots: " + outDir);
  process.exit(failures === 0 ? 0 : 1);
})().catch((e) => { console.error(e); process.exit(2); });
