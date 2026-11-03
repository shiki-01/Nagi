<script lang="ts">
  import { onMount } from 'svelte'
  import Meta from '$lib/components/Meta.svelte'
  import { decodeLayoutData, extractData, APP_LINK_PREFIX } from '$lib/keyboard/layout-url'
  import { REPO_URL } from '$lib/site'

  // 共有リンク（https://nagi.shiki-01.com/layout#data=<base64url>）の受け皿。静的サイトなので、フラグメントはブラウザの中だけで読む
  // （サーバーには送られない）。Android で Nagi が App Links として認証されていれば、このページを経ずにアプリが直接開く。
  type Info = { name: string; id: string; mode: string; keys: number; json: string; data: string }
  const MODES: Record<string, string> = { kana: 'かな', alpha: '英字', number: '数字', symbol: '記号', emoji: '絵文字', custom: 'カスタム' }

  let loaded = $state(false)
  let info = $state<Info | undefined>()
  let problem = $state('')
  let copied = $state('')

  onMount(() => {
    const data = extractData(location.hash) ?? extractData(location.search)
    if (data) {
      const r = decodeLayoutData(data)
      if (!r.ok) problem = r.message
      else {
        try {
          const l = JSON.parse(r.json) as { id?: unknown; name?: unknown; mode?: unknown; rows?: unknown }
          if (!l || typeof l !== 'object' || !Array.isArray(l.rows) || typeof l.name !== 'string') throw new Error()
          const keys = l.rows.reduce((n: number, row: { keys?: unknown[] }) => n + (Array.isArray(row?.keys) ? row.keys.length : 0), 0)
          info = {
            name: l.name,
            id: typeof l.id === 'string' ? l.id : '',
            mode: typeof l.mode === 'string' ? (MODES[l.mode] ?? l.mode) : '',
            keys,
            json: r.json,
            data
          }
        } catch {
          problem = 'レイアウトとして読み取れませんでした。'
        }
      }
    }
    loaded = true
  })

  async function copyJson() {
    if (!info) return
    try {
      await navigator.clipboard.writeText(JSON.stringify(JSON.parse(info.json), null, 2) + '\n')
      copied = 'JSON をコピーしました。'
    } catch {
      copied = 'コピーできませんでした。'
    }
  }
</script>

<Meta title="レイアウトを Nagi で開く" path="/layout" description="共有されたキーボードのレイアウトを、Nagi アプリで開きます。" />
<svelte:head><meta name="robots" content="noindex" /></svelte:head>

<main class="doc">
  <header class="doc-head">
    <p class="eyebrow">レイアウトの共有</p>
    <h1>レイアウトを Nagi で開く</h1>
  </header>

  <div class="prose">
    {#if !loaded}
      <p>読み込み中です…</p>
    {:else if info}
      <dl class="meta">
        <dt>名前</dt>
        <dd>{info.name}</dd>
        {#if info.mode}<dt>モード</dt><dd>{info.mode}</dd>{/if}
        <dt>キー数</dt>
        <dd>{info.keys}</dd>
      </dl>
      <p>
        <a class="btn primary" href="{APP_LINK_PREFIX}{info.data}">Nagi で開く</a>
        <button type="button" class="btn" onclick={copyJson}>JSON をコピー</button>
      </p>
      <p class="status" role="status" aria-live="polite">{copied}</p>
      <p class="note">
        心当たりのないリンクは開かないでください。Nagi アプリは、取り込む前に必ず内容を確認する画面を出します。
        この内容はあなたの端末の中だけで読み取っており、サーバーには送られません。
      </p>
      <h2>ボタンを押しても開かないとき</h2>
      <ul>
        <li>
          Nagi（Android 版）が入っていない場合は、<a href="{REPO_URL}/releases">配布ページ</a>からインストールしてください。
          手順は<a href="/support#layout">サポート</a>にあります。
        </li>
        <li>アプリ内ブラウザ（SNS やメッセージアプリ）ではアプリを開けないことがあります。リンクをコピーして Chrome などで開いてください。</li>
        <li>パソコンで開いている場合は、スマホで開くか、「JSON をコピー」した内容を Nagi の「レイアウト」タブに貼り付けてください。</li>
      </ul>
    {:else}
      <p>
        {problem ? `このリンクは読み取れませんでした（${problem}）。` : 'このページは、共有されたレイアウトのリンクから開くと、内容を表示します。'}
      </p>
      <p>
        レイアウトは<a href="/custom-keyboard">編集ページ</a>で作れます。作ったあとに「アプリで開く」または QR コードで、スマホの Nagi に渡せます。
      </p>
    {/if}
  </div>
</main>

<style>
  .meta {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 0 0 16px;
  }
  .meta dt {
    color: var(--muted);
  }
  .meta dd {
    margin: 0;
  }
  .status {
    min-height: 1.5em;
    color: var(--ok);
    font-size: 0.875rem;
  }
</style>
