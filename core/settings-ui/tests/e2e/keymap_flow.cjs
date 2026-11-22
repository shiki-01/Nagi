#!/usr/bin/env node
// キー割当ページの操作テスト（Playwright + chromium。マウスの実クリックとキー入力）。CI には組み込まない手動用。
//   使い方（core/ で）:  cargo build -p nagi-settings-ui --bin nagi-settings -p nagi-server --bin imserver
//                       node settings-ui/tests/e2e/keymap_flow.cjs [スクリーンショットの出力先]
//   playwright は node_modules に無ければ、グローバルの場所（NODE_PATH / /opt/node22/lib/node_modules）から読む。
//   見る点: 各ステップで「ユーザーの目に見える反応」があること（ダイアログが開く・フォーカス枠・取り込んだキーの表示・
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
  const shot = async (n) => { await page.waitForTimeout(400); return page.screenshot({ path: path.join(outDir, v.name + "-" + n + ".png") }); };
  await page.goto(url);
  await page.click('.nav-item[data-page="keymap"]');
  await page.waitForSelector(".kc-row");
  await shot("01-list");

  // 1) 場面タブ（変換中）を実クリック
  await mouseClick(page, page.locator("#kc-tab-converting"), "「変換中」タブ");
  check((await page.locator("#kc-tab-converting").getAttribute("aria-selected")) === "true", "タブが選択状態になる");
  await mouseClick(page, page.locator("#kc-tab-composing"), "「入力中」タブ");

  // 2) 行の「キーを追加」→ モーダルのダイアログ
  const row = page.locator('.kc-row[data-action="ToKatakana"]');
  await row.waitFor();
  await mouseClick(page, row.locator(".kc-add"), "「キーを追加」");
  const dlg = page.locator("dialog[open]");
  await dlg.waitFor();
  const title = await dlg.locator("h2").innerText();
  check(/『.+』のキーを割り当てる/.test(title), "見出し: " + title);
  check(await row.evaluate((r) => r.classList.contains("editing")), "操作中の行が強調される");
  const dbox = await dlg.boundingBox();
  check(dbox && dbox.x >= 0 && dbox.y >= 0 && dbox.x + dbox.width <= v.w && dbox.y + dbox.height <= v.h + 1, "ダイアログが画面内に収まる");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-capture")), "開くとキー入力欄に自動でフォーカスが入る");
  check(/割り当てるキーを押してください/.test(await page.locator(".kc-capture").innerText()), "案内文: 割り当てるキーを押してください");
  check(await dlg.locator(".kc-save").isDisabled(), "取り込み前は「割り当てる」が無効");
  check(/キーを押す/.test(await dlg.locator(".kc-next").innerText()), "押せない理由が表示される");
  await shot("02-dialog-open");

  // 3) キーを押す（取り込み → 検証が終わるまで「割り当てる」は無効）
  await page.keyboard.press("Control+Shift+KeyY");
  check((await page.locator(".kc-capture .kc-cap").allInnerTexts()).join("|") === "Ctrl|Shift|Y", "押したキーがキーチップで即座に出る");
  await page.waitForFunction(() => !document.querySelector(".kc-save").disabled, null, { timeout: 4000 });
  check(true, "検証のあとに「割り当てる」が有効になる");
  await shot("03-captured");

  // 4) 実クリックで確定 → トースト・行のハイライト・ダイアログが閉じる
  await mouseClick(page, dlg.locator(".kc-save"), "「割り当てる」");
  await page.waitForSelector(".toast");
  check(/保存しました/.test(await page.locator(".toast").last().innerText()), "トースト: 保存しました");
  check((await page.locator("dialog[open]").count()) === 0, "ダイアログが閉じる");
  check(await page.locator('.kc-row[data-action="ToKatakana"]').evaluate((r) => r.classList.contains("flash")), "保存した行がハイライトされる");
  const chip = page.locator('.kc-row[data-action="ToKatakana"] .kc-chip[data-key="Ctrl+Shift+y" i]');
  check((await chip.count()) === 1, "新しいキーがチップとして並ぶ");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-chip")), "保存後、新しいキーのチップにフォーカスが移る");
  await shot("04-saved");

  // 5) キーチップ（ボタン）を実クリック → 変更/解除のダイアログ。Esc でダイアログだけ閉じ、フォーカスが戻る
  await page.waitForTimeout(3800);
  check(!(await page.locator('.kc-row[data-action="ToKatakana"]').evaluate((r) => r.classList.contains("flash"))), "ハイライトは数秒で消える");
  await mouseClick(page, chip, "キーチップ");
  await dlg.waitFor();
  check(/変更する/.test(await dlg.locator("h2").innerText()), "チップを押すと変更のダイアログが開く");
  check(await dlg.getByRole("button", { name: /このキーを解除/ }).count() === 1, "「このキーを解除」がある");
  await shot("05-chip-dialog");
  await page.keyboard.press("Escape");
  check((await page.locator("dialog[open]").count()) === 0, "Esc でダイアログが閉じる");
  check((await page.locator(".kc-row").count()) > 0 && (await page.locator("#main").count()) === 1, "設定画面は閉じない");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-chip") && document.activeElement.getAttribute("data-key").toLowerCase() === "ctrl+shift+y"), "閉じるとフォーカスがチップへ戻る");

  // 6) 「キー」ラベルを押しても入力欄にフォーカスが入る。Tab はダイアログ内で循環する
  await mouseClick(page, row.locator(".kc-add"), "「キーを追加」");
  await dlg.waitFor();
  await page.locator(".kc-save").focus().catch(() => {});
  await page.evaluate(() => document.querySelector(".kc-cancel").focus());
  await mouseClick(page, page.locator('dialog .kc-field > label[for$="-cap"]'), "「キー」ラベル");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-capture")), "「キー」ラベルを押すと入力欄にフォーカスが入る");
  await page.evaluate(() => document.querySelector(".kc-cancel").focus());
  for (let i = 0; i < 12; i++) {
    await page.keyboard.press("Tab");
    if (!(await page.evaluate(() => !!document.activeElement.closest("dialog")))) break;
  }
  check(await page.evaluate(() => !!document.activeElement.closest("dialog")), "Tab を繰り返してもフォーカスがダイアログの外へ出ない");
  await page.keyboard.press("Shift+Tab");
  check(await page.evaluate(() => !!document.activeElement.closest("dialog")), "Shift+Tab でもダイアログの中にいる");
  await mouseClick(page, page.locator("dialog .kc-capture"), "キー入力欄");
  check(/割り当てるキーを押してください/.test(await page.locator(".kc-capture").innerText()), "入力欄を押すと取り込み待ちの表示になる");
  // IME 処理中（key が Process）でも、物理キー（code）から取り込む
  const viaCode = await page.evaluate(() => ["F8", "ArrowLeft", "Space", "Enter"].map((code) => window.NagiKeymapKeys.specFromEvent({ key: "Process", code, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false }).spec).join(","));
  check(viaCode === "F8,Left,Space,Enter", "IME 処理中でも F8・矢印・Space・Enter を取り込める: " + viaCode);
  await page.keyboard.press("Escape");
  check((await page.locator("dialog[open]").count()) === 0, "取り込み中の Esc でもダイアログだけが閉じる");
  check((await page.locator(".kc-row").count()) > 0, "設定画面は閉じない");
  check(await page.evaluate(() => document.activeElement && document.activeElement.classList.contains("kc-add")), "閉じると「キーを追加」へフォーカスが戻る");

  // 7) チップから解除
  await mouseClick(page, chip, "キーチップ");
  await mouseClick(page, dlg.getByRole("button", { name: /このキーを解除/ }), "「このキーを解除」");
  await page.waitForFunction(() => !document.querySelector('.kc-chip[data-key="Ctrl+Shift+y" i]') && !document.querySelector("dialog[open]"));
  check(/解除しました/.test(await page.locator(".toast").last().innerText()), "トースト: 解除しました");
  await shot("06-removed");

  // 8) キーボードだけで一巡（Enter で開く → Ctrl+Shift+U を押す → 保存ボタンへ → Enter）
  await row.locator(".kc-add").focus();
  await page.keyboard.press("Enter");
  await dlg.waitFor();
  await page.keyboard.press("Control+Shift+KeyU");
  await page.waitForFunction(() => !document.querySelector(".kc-save").disabled, null, { timeout: 4000 });
  await page.locator(".kc-save").focus();
  await page.keyboard.press("Enter");
  await page.waitForSelector('.kc-row[data-action="ToKatakana"] .kc-chip[data-key="Ctrl+Shift+u" i]');
  check(await page.evaluate(() => document.activeElement && document.activeElement.getAttribute("data-key").toLowerCase() === "ctrl+shift+u"), "保存後、新しいキーのチップにフォーカスが移る（キーボード）");
  await page.keyboard.press("Enter");
  await dlg.waitFor();
  await page.getByRole("button", { name: /このキーを解除/ }).focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => !document.querySelector('.kc-chip[data-key="Ctrl+Shift+u" i]'));
  check(true, "キーボードだけで追加・変更ダイアログ・解除ができる");
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
