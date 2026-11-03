// レイアウト JSON と共有リンクの相互変換（純粋関数）。
// 形式は Android の LayoutLink.kt・iOS の Shared/LayoutURL.swift と同じ:
//   nagi://layout?data=<base64url(UTF-8 の JSON)>   （アプリで開く）
//   https://nagi.shiki-01.com/layout#data=<同>      （共有ページ。フラグメントはサーバーに送られない。Android は App Links でアプリが直接開く）
// base64url はパディングなし。data の文字数の上限は 1MB。assets（画像）を含むレイアウトはリンクにできない。

import { SITE_URL } from '$lib/site'

/** data（base64url 文字列）の文字数の上限。Android の LayoutLink.MAX_DATA_CHARS・iOS の maxDataChars と同じ。 */
export const MAX_DATA_CHARS = 1024 * 1024
/** URL 全体で data 以外に許す長さ（受け側と同じ）。 */
const URL_EXTRA = 4096

export const APP_LINK_PREFIX = 'nagi://layout?data='
export const SHARE_PAGE_URL = `${SITE_URL}/layout`

export type LinkResult =
  | { ok: true; data: string; appUrl: string; shareUrl: string }
  | { ok: false; reason: 'assets' | 'tooLarge' | 'invalid'; message: string }

export type DecodeResult = { ok: true; json: string } | { ok: false; message: string }

/** assets が 1 つ以上あるか（Android の hasAssets と同じ判定: null 以外で、オブジェクトなら空でないもの）。 */
export function hasAssets(layout: unknown): boolean {
  if (!layout || typeof layout !== 'object') return false
  const a = (layout as { assets?: unknown }).assets
  if (a === undefined || a === null) return false
  if (typeof a === 'object' && !Array.isArray(a)) return Object.keys(a).length > 0
  return true
}

export function toBase64Url(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let bin = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) bin += String.fromCharCode(...bytes.subarray(i, i + chunk))
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export function fromBase64Url(data: string): string | undefined {
  let b64 = data.replace(/-/g, '+').replace(/_/g, '/')
  while (b64.length % 4 !== 0) b64 += '='
  try {
    const bin = atob(b64)
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0))
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    return undefined
  }
}

/** レイアウト（オブジェクトまたは JSON 文字列）からリンクを作る。作れない理由は日本語の message で返す。 */
export function buildLayoutLinks(layout: unknown): LinkResult {
  let obj: unknown = layout
  let json: string
  if (typeof layout === 'string') {
    try {
      obj = JSON.parse(layout)
    } catch {
      return { ok: false, reason: 'invalid', message: 'JSON として読み取れないため、リンクにできません。' }
    }
    json = layout
  } else {
    json = JSON.stringify(layout)
  }
  if (hasAssets(obj)) {
    return { ok: false, reason: 'assets', message: '画像（assets）を含むレイアウトはリンクにできません。JSON をダウンロードして、ファイルで渡してください。' }
  }
  const data = toBase64Url(json)
  if (data.length > MAX_DATA_CHARS) {
    return {
      ok: false,
      reason: 'tooLarge',
      message: `レイアウトが大きすぎてリンクにできません（${Math.round(data.length / 1024)}KB。上限 ${MAX_DATA_CHARS / 1024}KB）。JSON をダウンロードして、ファイルで渡してください。`
    }
  }
  return { ok: true, data, appUrl: `${APP_LINK_PREFIX}${data}`, shareUrl: `${SHARE_PAGE_URL}#data=${data}` }
}

/** data の文字列（base64url）から JSON 文字列に戻す。 */
export function decodeLayoutData(data: string): DecodeResult {
  if (!data) return { ok: false, message: 'data がありません。' }
  if (data.length > MAX_DATA_CHARS) return { ok: false, message: `data が大きすぎます（${MAX_DATA_CHARS / 1024}KB まで）。` }
  const json = fromBase64Url(data)
  if (json === undefined) return { ok: false, message: 'data を base64url の UTF-8 として読み取れません。' }
  return { ok: true, json }
}

function paramOf(source: string, name: string): string | undefined {
  for (const part of source.split('&')) {
    const eq = part.indexOf('=')
    const k = eq >= 0 ? part.slice(0, eq) : part
    if (k !== name) continue
    const raw = eq >= 0 ? part.slice(eq + 1) : ''
    try {
      // base64 の + がクエリで空白に読まれた場合を戻す
      return decodeURIComponent(raw).replace(/ /g, '+')
    } catch {
      return undefined
    }
  }
  return undefined
}

/** フラグメント（`#data=…` または `data=…`）またはクエリ（`?data=…`）から data を取り出す。無ければ undefined。 */
export function extractData(fragmentOrQuery: string): string | undefined {
  const s = fragmentOrQuery.replace(/^[#?]/, '')
  return paramOf(s, 'data')
}

/** nagi://layout?data=… / https://nagi.shiki-01.com/layout#data=… のどちらからでも JSON を復元する（往復テスト・検証用）。 */
export function parseLayoutLink(url: string): DecodeResult {
  const u = url.trim()
  if (u.length > MAX_DATA_CHARS + URL_EXTRA) return { ok: false, message: `リンクが大きすぎます（${MAX_DATA_CHARS / 1024}KB まで）。` }
  const m = /^([a-z][a-z0-9+.-]*):\/\/([^/?#]*)([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i.exec(u)
  if (!m) return { ok: false, message: '対応していない URL です。' }
  const [, scheme, host, path, query = '', fragment = ''] = m
  let data: string | undefined
  if (scheme.toLowerCase() === 'nagi' && host.toLowerCase() === 'layout') {
    data = paramOf(query, 'data')
  } else if (scheme.toLowerCase() === 'https' && host.toLowerCase() === new URL(SITE_URL).host && path.replace(/\/+$/, '') === '/layout') {
    data = paramOf(fragment, 'data') ?? paramOf(query, 'data')
  } else {
    return { ok: false, message: '対応していない URL です。' }
  }
  if (!data) return { ok: false, message: 'URL に data がありません。' }
  return decodeLayoutData(data)
}
