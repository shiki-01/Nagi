import { describe, expect, it } from 'vitest'
import { compatWarnings, usedFeatures } from './compat'
import { buildLayoutLinks, decodeLayoutData, hasAssets, MAX_DATA_CHARS, parseLayoutLink, toBase64Url } from './layout-url'
import { presetResult } from './presets'
import { sampleLayout } from './ops'
import type { KeyboardLayout } from './types'

describe('layout-url', () => {
  it('全プリセットで生成→復元が一致する（nagi:// と https の両方）', () => {
    expect(presetResult.presets.length).toBeGreaterThan(0)
    for (const p of presetResult.presets) {
      const json = JSON.stringify(p)
      const r = buildLayoutLinks(p)
      if (hasAssets(p)) {
        expect(r.ok).toBe(false)
        continue
      }
      expect(r.ok, p.id).toBe(true)
      if (!r.ok) continue
      expect(r.appUrl.startsWith('nagi://layout?data=')).toBe(true)
      expect(r.shareUrl.startsWith('https://nagi.shiki-01.com/layout#data=')).toBe(true)
      for (const url of [r.appUrl, r.shareUrl]) {
        const back = parseLayoutLink(url)
        expect(back.ok).toBe(true)
        if (back.ok) expect(back.json).toBe(json)
      }
    }
  })

  it('パディングなしの base64url で、日本語・絵文字も往復する', () => {
    const text = '{"name":"かな😀"}'
    const data = toBase64Url(text)
    expect(data).not.toMatch(/[+/=]/)
    expect(decodeLayoutData(data)).toEqual({ ok: true, json: text })
  })

  it('文字列の JSON も受ける。読めない JSON は invalid', () => {
    const json = JSON.stringify(sampleLayout())
    const r = buildLayoutLinks(json)
    expect(r.ok && decodeLayoutData(r.data)).toEqual({ ok: true, json })
    const bad = buildLayoutLinks('{oops')
    expect(bad.ok === false && bad.reason).toBe('invalid')
  })

  it('assets を含むものは作らない（空の assets は作れる）', () => {
    const withAssets = { ...sampleLayout(), assets: { bg: 'data:image/png;base64,AAAA' } } as KeyboardLayout
    const r = buildLayoutLinks(withAssets)
    expect(r.ok).toBe(false)
    if (!r.ok) {
      expect(r.reason).toBe('assets')
      expect(r.message).toContain('assets')
    }
    expect(buildLayoutLinks({ ...sampleLayout(), assets: {} }).ok).toBe(true)
  })

  it('上限（data 1MB）を超えると作らない', () => {
    const big = { ...sampleLayout(), name: 'あ'.repeat(MAX_DATA_CHARS / 2) }
    const r = buildLayoutLinks(big)
    expect(r.ok).toBe(false)
    if (!r.ok) expect(r.reason).toBe('tooLarge')
  })

  it('復元の失敗: 欠落・不正・別ホスト', () => {
    expect(parseLayoutLink('nagi://layout').ok).toBe(false)
    expect(parseLayoutLink('nagi://layout?data=!!!!').ok).toBe(false)
    expect(parseLayoutLink('https://example.com/layout#data=AAAA').ok).toBe(false)
    expect(parseLayoutLink('https://nagi.shiki-01.com/layouts#data=AAAA').ok).toBe(false)
    expect(parseLayoutLink('not a url').ok).toBe(false)
    expect(decodeLayoutData('').ok).toBe(false)
    // UTF-8 でないバイト列
    expect(decodeLayoutData(btoa(String.fromCharCode(0xff, 0xfe, 0x41)).replace(/=+$/, '')).ok).toBe(false)
  })

  it('https のクエリ形式も読める', () => {
    const data = toBase64Url('{"a":1}')
    expect(parseLayoutLink(`https://nagi.shiki-01.com/layout?data=${data}`)).toEqual({ ok: true, json: '{"a":1}' })
  })
})

describe('compat', () => {
  it('機能を使っていなければ警告なし', () => {
    expect(usedFeatures(sampleLayout())).toEqual([])
    expect(compatWarnings(sampleLayout())).toEqual([])
  })

  it('全体・行・キー・toolbar の指定を拾う（null と空の assets は数えない）', () => {
    const l = {
      ...sampleLayout(),
      assets: {},
      background: null,
      metrics: { keySpacing: 2 },
      toolbar: { keys: [{ id: 'p', kind: 'action', action: 'paste' }] }
    } as unknown as KeyboardLayout
    l.rows[0].look = { fontSize: 20 }
    Object.assign(l.rows[0].keys[0], { states: { pressed: { fill: '#fff' } }, visible: { field: 'returnKeyType', in: ['search'] } })
    expect(usedFeatures(l)).toEqual(['look', 'states', 'metrics', 'toolbar', 'visible'])
  })

  it('Android と iOS で未対応の範囲が違う', () => {
    const l = sampleLayout()
    l.look = { fontSize: 20 }
    let w = compatWarnings(l)
    expect(w.map((x) => x.platform)).toEqual(['android'])
    expect(w[0].features.map((f) => f.name)).toEqual(['look'])
    Object.assign(l.rows[0].keys[0], { visible: { field: 'returnKeyType', in: ['search'] } })
    w = compatWarnings(l)
    expect(w.map((x) => x.platform)).toEqual(['android', 'ios'])
    expect(w[1].features.map((f) => f.name)).toEqual(['visible'])
  })
})
