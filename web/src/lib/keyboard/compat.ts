// 編集中のレイアウトが使っている機能のうち、アプリ側（Android / iOS）がまだ反映しないものの判定（純粋関数）。
// Android 側の一覧は frontend-android の LayoutCompat.kt（UNSUPPORTED）と同じにする。
// iOS は docs/ios-keyboard-layout.md「見た目の指定」「実装の段階」のとおり、look 系・toolbar・assets は描画に反映済みで、
// 未対応は visible（v1 では描画に使わない）だけ。

export type Platform = 'android' | 'ios'

export interface CompatFeature {
  /** JSON のプロパティ名 */
  name: string
  /** 日本語の説明 */
  label: string
}

export const FEATURES: readonly CompatFeature[] = [
  { name: 'look', label: '見た目' },
  { name: 'roles', label: '役割ごとの見た目' },
  { name: 'rules', label: '状態の見た目' },
  { name: 'states', label: 'キーの状態ごとの見た目' },
  { name: 'metrics', label: '余白' },
  { name: 'background', label: '背景' },
  { name: 'assets', label: '画像' },
  { name: 'toolbar', label: 'ツールバー' },
  { name: 'visible', label: '表示条件' }
]

/** プラットフォームごとの未対応の機能名。 */
export const UNSUPPORTED: Readonly<Record<Platform, readonly string[]>> = {
  android: ['look', 'roles', 'rules', 'states', 'metrics', 'background', 'assets', 'toolbar', 'visible'],
  ios: ['visible']
}

export const PLATFORM_LABELS: Readonly<Record<Platform, string>> = { android: 'Android', ios: 'iOS' }

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

/** レイアウトが実際に使っている機能名を FEATURES の順で返す（null・空の assets は数えない）。 */
export function usedFeatures(layout: unknown): string[] {
  if (!isObj(layout)) return []
  const found = new Set<string>()
  const scan = (o: Record<string, unknown>) => {
    for (const f of FEATURES) {
      const v = o[f.name]
      if (v === undefined || v === null) continue
      if (f.name === 'assets' && isObj(v) && Object.keys(v).length === 0) continue
      found.add(f.name)
    }
  }
  scan(layout)
  if (Array.isArray(layout.rows)) {
    for (const row of layout.rows) {
      if (!isObj(row)) continue
      scan(row)
      if (Array.isArray(row.keys)) for (const k of row.keys) if (isObj(k)) scan(k)
    }
  }
  return FEATURES.map((f) => f.name).filter((n) => found.has(n))
}

export interface CompatWarning {
  platform: Platform
  platformLabel: string
  /** 使っていて、そのプラットフォームが反映しない機能。 */
  features: CompatFeature[]
}

/** プラットフォームごとの警告。未対応の機能を使っていないプラットフォームは含めない。 */
export function compatWarnings(layout: unknown): CompatWarning[] {
  const used = usedFeatures(layout)
  const out: CompatWarning[] = []
  for (const platform of ['android', 'ios'] as const) {
    const features = FEATURES.filter((f) => used.includes(f.name) && UNSUPPORTED[platform].includes(f.name))
    if (features.length > 0) out.push({ platform, platformLabel: PLATFORM_LABELS[platform], features })
  }
  return out
}
