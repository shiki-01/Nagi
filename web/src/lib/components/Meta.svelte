<script lang="ts">
  import { SITE_NAME, SITE_URL } from '$lib/site'

  let { title, description, path }: { title: string; description: string; path: string } = $props()
  const full = $derived(title === SITE_NAME ? title : `${title} - ${SITE_NAME}`)
  const OG_ALT = 'Nagi（凪）。水平線の上に読み「ゆうがたのうみはなぎだった」、下に表記「夕方の海は凪だった」。端末の中だけで考える、ニューラルかな漢字変換。'
  const url = $derived(SITE_URL + (path === '/' ? '/' : path))
</script>

<svelte:head>
  <title>{full}</title>
  <meta name="description" content={description} />
  <link rel="canonical" href={url} />
  <meta property="og:type" content="website" />
  <meta property="og:site_name" content={SITE_NAME} />
  <meta property="og:locale" content="ja_JP" />
  <meta property="og:title" content={full} />
  <meta property="og:description" content={description} />
  <meta property="og:url" content={url} />
  <meta property="og:image" content={`${SITE_URL}/og.png`} />
  <meta property="og:image:type" content="image/png" />
  <meta property="og:image:width" content="1200" />
  <meta property="og:image:height" content="630" />
  <meta property="og:image:alt" content={OG_ALT} />
  <meta name="twitter:card" content="summary_large_image" />
  <meta name="twitter:image" content={`${SITE_URL}/og.png`} />
  <meta name="twitter:image:alt" content={OG_ALT} />
  <meta name="twitter:title" content={full} />
  <meta name="twitter:description" content={description} />
</svelte:head>
