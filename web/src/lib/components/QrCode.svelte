<script lang="ts">
  import QRCode from 'qrcode'

  let { text, size = 240 }: { text: string; size?: number } = $props()

  // 白地に黒（ダーク/ライトどちらのテーマでも読み取れるよう、背景は常に白・余白あり）。
  // 誤り訂正は L（データ量を優先）。入りきらないときは error に理由を出す。
  let svg = $state('')
  let error = $state('')

  $effect(() => {
    const t = text
    let cancelled = false
    QRCode.toString(t, { type: 'svg', margin: 2, errorCorrectionLevel: 'L', color: { dark: '#000000', light: '#ffffff' } }).then(
      (s) => {
        if (cancelled) return
        svg = s
        error = ''
      },
      () => {
        if (cancelled) return
        svg = ''
        error = 'レイアウトが大きく、QR コードにできません。リンクをコピーするか、JSON のファイルで渡してください。'
      }
    )
    return () => {
      cancelled = true
    }
  })
</script>

{#if error}
  <p class="err" role="alert">{error}</p>
{:else if svg}
  <div class="qr" style:width="{size}px" style:height="{size}px" role="img" aria-label="レイアウトを開く QR コード">
    <!-- eslint-disable-next-line svelte/no-at-html-tags -- qrcode ライブラリが生成した SVG のみ -->
    {@html svg}
  </div>
{/if}

<style>
  .qr {
    background: #fff;
    border-radius: 8px;
    max-width: 100%;
  }
  .qr :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
  .err {
    margin: 0;
    color: var(--danger);
    font-size: 0.875rem;
  }
</style>
