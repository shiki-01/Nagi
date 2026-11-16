// OGP 画像とアイコンの PNG を static/ に作り直す。
// 使い方: PW_CHROMIUM=/path/to/chrome npm run assets
//   og.png             … static/og.svg を 1200×630 に描画（明朝体は実行環境の Noto Serif JP / ヒラギノ明朝などを使う）
//   icon-192/512.png   … static/icon.svg（角丸）
//   icon-maskable-512  … 角丸なしの全面版（Android のマスク用。図柄を 80% に縮めて安全領域に収める）
//   apple-touch-icon   … 180px。iOS が角を丸めるので角丸なしの全面版
import { chromium } from 'playwright-core'
import { readFileSync } from 'node:fs'

const read = (name) => readFileSync(new URL(`../static/${name}`, import.meta.url), 'utf8')
const browser = await chromium.launch({ executablePath: process.env.PW_CHROMIUM, args: ['--no-sandbox'] })
const page = await browser.newPage()

async function render(svg, size, out, background = 'transparent') {
  const [w, h] = size
  await page.setViewportSize({ width: w, height: h })
  await page.setContent(
    `<style>html,body{margin:0;background:${background}}svg{display:block;width:${w}px;height:${h}px}</style>${svg}`
  )
  await page.evaluate(() => document.fonts.ready)
  await page.screenshot({ path: new URL(`../static/${out}`, import.meta.url).pathname, omitBackground: background === 'transparent' })
  console.log(out, `${w}x${h}`)
}

const icon = read('icon.svg')
// 角丸を外した全面版
const full = icon.replace('rx="224"', 'rx="0"')
// マスク用: 背景は全面、図柄だけ中央へ 80% に縮める
const maskable = full.replace(/(<rect[^>]*\/>)([\s\S]*)<\/svg>/, '$1<g transform="translate(102.4 102.4) scale(.8)">$2</g></svg>')

await render(read('og.svg'), [1200, 630], 'og.png', '#eef1f5')
await render(icon, [192, 192], 'icon-192.png')
await render(icon, [512, 512], 'icon-512.png')
await render(maskable, [512, 512], 'icon-maskable-512.png', '#2b43b0')
await render(full, [180, 180], 'apple-touch-icon.png', '#2b43b0')
await browser.close()
