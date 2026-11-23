#!/usr/bin/env node
// バックアップと移行ページの操作テスト（Playwright + chromium。実クリックとファイル選択・ダウンロード）。CI には組み込まない手動用。
//   使い方（core/ で）:  cargo build -p nagi-settings-ui --bin nagi-settings -p nagi-server --bin imserver
//                       node settings-ui/tests/e2e/backup_flow.cjs [スクリーンショットの出力先]
//   流れ: 書き出し（ダウンロード）→ 設定・辞書を変更 → ファイルを選んで読み込み → プレビュー → 適用 → 元に戻す。
//   1280x800 と 840x600、ライト・ダークで繰り返す。不正なファイル・大きすぎるファイルの扱いも確かめる。
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
const outDir = process.argv[2] || fs.mkdtempSync(path.join(os.tmpdir(), "nagi-bk-shots-"));
fs.mkdirSync(outDir, { recursive: true });

let failures = 0;
function check(cond, msg) {
  console.log((cond ? "  ok   " : "  FAIL ") + msg);
  if (!cond) failures++;
}

const CONFIG_A = "# 私の設定\n[conversion]\nsync_budget_ms = 200\nlive_conversion = false\n\n[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n";
const CONFIG_B = "[conversion]\nsync_budget_ms = 120\nuse_llm = false\n";

async function startServer() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nagi-bk-e2e-"));
  fs.writeFileSync(path.join(dir, "config.toml"), CONFIG_A);
  fs.mkdirSync(path.join(dir, "user_dict"));
  fs.writeFileSync(path.join(dir, "user_dict", "mine.tsv"), "こうえん\t公苑\t名詞\t旧\nひなた\t日向\t地名\t\nさくら\t桜\t名詞\t\n");
  fs.writeFileSync(path.join(dir, "suppressed.tsv"), "あい\t愛\nかお\t顔\n");
  const urlFile = path.join(dir, "url.txt");
  const child = cp.spawn(bin("nagi-settings"), [
    "--web", "--no-browser", "--url-file", urlFile, "--imserver", bin("imserver"),
    "--user-dir", dir, "--data-dir", path.join(dir, "data")
  ], { stdio: "ignore" });
  for (let i = 0; i < 100 && !fs.existsSync(urlFile); i++) await new Promise((r) => setTimeout(r, 100));
  if (!fs.existsSync(urlFile)) throw new Error("設定画面が起動しませんでした");
  return { child, dir, url: fs.readFileSync(urlFile, "utf8").trim() };
}

async function mouseClick(page, locator, what) {
  await locator.scrollIntoViewIfNeeded();
  const box = await locator.boundingBox();
  if (!box) throw new Error(what + " が見つかりません");
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
}

async function flow(browser, v) {
  console.log("== " + v.name);
  const srv = await startServer();
  const ctx = await browser.newContext({
    viewport: { width: v.w, height: v.h }, deviceScaleFactor: 1,
    colorScheme: v.dark ? "dark" : "light", acceptDownloads: true
  });
  await ctx.grantPermissions(["clipboard-read", "clipboard-write"], { origin: new URL(srv.url).origin });
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  const shot = async (n, full) => { await page.waitForTimeout(450); return page.screenshot({ path: path.join(outDir, v.name + "-" + n + ".png"), fullPage: !!full }); };
  const token = () => page.evaluate(() => document.querySelector('meta[name="nagi-token"]').content);
  const post = (p, body) => page.evaluate(async ([p, body, t]) => {
    const r = await fetch(p, { method: "POST", headers: { "Content-Type": "application/json", "X-Nagi-Token": t }, body: JSON.stringify(body) });
    return { status: r.status, json: await r.json().catch(() => null) };
  }, [p, body, null]).catch(() => null);
  const apiPost = async (p, body) => { const t = await token(); return page.evaluate(async ([p, body, t]) => {
    const r = await fetch(p, { method: "POST", headers: { "Content-Type": "application/json", "X-Nagi-Token": t }, body: JSON.stringify(body) });
    return { status: r.status, json: await r.json().catch(() => null) };
  }, [p, body, t]); };
  void post;

  await page.goto(srv.url);
  await page.waitForSelector('.nav-item[data-page="backup"]');
  // サイドバー: 「システム」グループの、データ・ファイルの次
  const order = await page.$$eval(".nav-item", (els) => els.map((e) => e.getAttribute("data-page")));
  const iData = order.indexOf("data"), iBackup = order.indexOf("backup");
  check(iBackup === iData + 1, "サイドバーでデータ・ファイルの次に「バックアップと移行」がある");
  await page.click('.nav-item[data-page="backup"]');
  await page.waitForSelector(".bk-row");
  await page.waitForSelector(".bk-count");
  await shot("01-page");
  check((await page.locator("#bk-ex-learning").isChecked()) === false, "学習データは既定でオフ");
  check((await page.locator("#bk-ex-settings").isChecked()) === true, "設定は既定でオン");
  check(/入力した内容の履歴が含まれます/.test(await page.locator(".bk-caution").innerText()), "学習データに入力履歴の注意が出る");
  check((await page.locator(".bk-count").first().innerText()).length > 0, "件数が出る");

  // 1) 書き出し（ダウンロード）
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    mouseClick(page, page.getByRole("button", { name: "ファイルに書き出す" }), "「ファイルに書き出す」")
  ]);
  check(/^nagi-backup-\d{8}-\d{6}\.json$/.test(download.suggestedFilename()), "ファイル名: " + download.suggestedFilename());
  const saved = path.join(srv.dir, "saved-backup.json");
  await download.saveAs(saved);
  const backup = JSON.parse(fs.readFileSync(saved, "utf8"));
  check(backup.version === 1 && backup.entries.length >= 3, "スキーマ: version 1, entries " + backup.entries.length);
  check(!backup.entries.some((e) => e.kind === "learning"), "学習データは含まれない");
  await page.waitForSelector(".bk-note span");
  check(/を書き出しました/.test(await page.locator("#page .bk-note").first().innerText()), "書き出しの完了が表示される");
  await shot("02-exported");

  // 2) 学習データを選ぶと注意の色・コピー動作（クリップボード）
  await page.locator("#bk-ex-learning").check();
  await mouseClick(page, page.getByRole("button", { name: "内容をコピー" }), "「内容をコピー」");
  await page.waitForTimeout(500);
  const clip = await page.evaluate(() => navigator.clipboard.readText());
  const clipped = JSON.parse(clip);
  check(clipped.entries.some((e) => e.kind === "learning") || clipped.included.indexOf("learning") >= 0, "コピーした内容に学習データの項目が含まれる（選んだので）");
  await shot("03-copied");
  await page.locator("#bk-ex-learning").uncheck();

  // 3) 変更する（設定を書き換え、辞書に語を足し、抑制語を足す）
  fs.writeFileSync(path.join(srv.dir, "config.toml"), CONFIG_B);
  let r = await apiPost("/api/userdict/add", { entry: { reading: "あるく", surface: "歩く" } });
  check(r.status === 200, "辞書に語を追加（変更）");
  r = await apiPost("/api/suppressed/add", { reading: "さけ", surface: "酒" });
  check(r.status === 200, "抑制語を追加（変更）");
  await page.waitForTimeout(1200);

  // 4) 読み込み: 不正なファイル → エラー表示
  const bad = path.join(srv.dir, "bad.json");
  fs.writeFileSync(bad, "これは JSON ではありません");
  await page.setInputFiles('input[type="file"]', bad);
  await page.waitForFunction(() => /JSON として読めません/.test(document.querySelector("#page").innerText));
  check(true, "JSON でないファイルはエラーを表示する");
  const wrong = path.join(srv.dir, "wrong.json");
  fs.writeFileSync(wrong, JSON.stringify({ hello: 1 }));
  await page.setInputFiles('input[type="file"]', wrong);
  await page.waitForFunction(() => /Nagi のバックアップファイルではありません/.test(document.querySelector("#page").innerText));
  check(true, "別形式の JSON はエラーを表示する");
  const huge = path.join(srv.dir, "huge.json");
  fs.writeFileSync(huge, Buffer.alloc(21 * 1024 * 1024, 32));
  await page.setInputFiles('input[type="file"]', huge);
  await page.waitForFunction(() => /大きすぎます/.test(document.querySelector("#page").innerText));
  check(true, "20 MB を超えるファイルは拒否する");
  fs.unlinkSync(huge);
  await shot("04-import-error");

  // 5) 正しいファイル → プレビュー
  await page.setInputFiles('input[type="file"]', saved);
  await page.waitForSelector(".bk-preview .bk-row");
  await page.waitForSelector(".bk-actions");
  const previewText = await page.locator(".bk-preview").innerText();
  check(/設定/.test(previewText) && /ユーザー辞書/.test(previewText) && /抑制語/.test(previewText), "プレビューに 3 項目");
  check(/変更 1/.test(previewText), "設定の差分の要約（変更 1）: " + (previewText.match(/変更 \d[^\n]*/) || [""])[0]);
  check(/すでにある 3/.test(previewText), "辞書の重複件数（すでにある 3）");
  check(/学習データ/.test(previewText) && /含まれていません/.test(previewText), "学習データは含まれていないと表示");
  check(await page.locator("#bk-im-learning").isDisabled(), "含まれない項目は選べない");
  await mouseClick(page, page.locator(".bk-details summary").first(), "「変更の内容を見る」");
  await page.waitForSelector(".bk-diff");
  await shot("05-preview", true);
  const diffText = await page.locator(".bk-diff").innerText();
  check(/200/.test(diffText) && /120/.test(diffText), "差分に前後の値が出る");
  check(/キー割当（composing）Ctrl\+j/.test(diffText), "キー割当の差分が読める表記: " + (diffText.match(/キー割当[^\n]*/) || [""])[0]);

  // 6) 適用: 辞書は選ばない → 確認ダイアログ
  await page.locator("#bk-im-user_dict").uncheck();
  await mouseClick(page, page.getByRole("button", { name: /選んだ項目を適用/ }), "「選んだ項目を適用」");
  const dlg = page.locator("dialog[open]");
  await dlg.waitFor();
  const dtext = await dlg.innerText();
  check(/現在の設定を上書きします。適用前のバックアップを自動で保存します。/.test(dtext), "確認ダイアログの文言");
  check(!/ユーザー辞書:/.test(dtext), "選ばなかった項目はダイアログに出ない");
  const dbox = await dlg.boundingBox();
  check(dbox && dbox.x >= 0 && dbox.y >= 0 && dbox.x + dbox.width <= v.w && dbox.y + dbox.height <= v.h + 1, "ダイアログが画面内に収まる");
  await shot("06-confirm");
  await mouseClick(page, dlg.getByRole("button", { name: "適用する" }), "「適用する」");
  await page.waitForSelector(".bk-result:not([hidden]) .badge.ok");
  check(fs.readFileSync(path.join(srv.dir, "config.toml"), "utf8") === CONFIG_A, "設定がバックアップの内容に置き換わった");
  const sup = await apiPost("/api/suppressed/list", {});
  check(sup.json.items.length === 3, "抑制語は追加（和集合: 既存 3 件のまま）");
  const ud = await apiPost("/api/userdict/list", {});
  check(ud.json.total === 4, "選ばなかった辞書は変わらない（4 語）");
  check(/元に戻す/.test(await page.locator(".bk-result").innerText()), "結果に自動バックアップの案内");
  await page.waitForSelector(".bk-row .bk-latest");
  check((await page.locator(".bk-latest").count()) === 1, "自動バックアップが 1 件一覧に出る");
  await shot("07-applied", true);

  // 7) 元に戻す
  await mouseClick(page, page.getByRole("button", { name: /直前の状態に戻す/ }), "「直前の状態に戻す」");
  const dlg2 = page.locator("dialog[open]");
  await dlg2.waitFor();
  await shot("08-undo-confirm");
  await mouseClick(page, dlg2.getByRole("button", { name: "元に戻す" }), "ダイアログの「元に戻す」");
  await page.waitForFunction(() => /元に戻しました/.test(document.querySelector(".bk-result").innerText));
  check(fs.readFileSync(path.join(srv.dir, "config.toml"), "utf8") === CONFIG_B, "設定が適用前（変更後）の状態に戻った");
  const sup2 = await apiPost("/api/suppressed/list", {});
  check(sup2.json.items.length === 3, "抑制語も適用前の状態");
  await page.waitForFunction(() => document.querySelectorAll("#page .bk-row .tag.bk-kind").length >= 2);
  await shot("09-undone", true);

  // 8) 画面のはみ出し（横スクロールがない）
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  check(overflow <= 0, "横方向にはみ出さない（" + overflow + "px）");
  check(errors.length === 0, "コンソールエラーなし" + (errors.length ? ": " + errors.join(" | ") : ""));

  await ctx.close();
  srv.child.kill();
  fs.rmSync(srv.dir, { recursive: true, force: true });
}

(async () => {
  const browser = await chromium.launch();
  const variants = [
    { name: "d1280-light", w: 1280, h: 800, dark: false },
    { name: "d1280-dark", w: 1280, h: 800, dark: true },
    { name: "d840-light", w: 840, h: 600, dark: false },
    { name: "d840-dark", w: 840, h: 600, dark: true }
  ];
  for (const v of variants) await flow(browser, v);
  await browser.close();
  console.log(failures === 0 ? "すべて成功" : failures + " 件の失敗");
  console.log("スクリーンショット: " + outDir);
  process.exit(failures === 0 ? 0 : 1);
})().catch((e) => { console.error(e); process.exit(2); });
