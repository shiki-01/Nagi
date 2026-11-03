<script lang="ts">
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { parseDraft } from '$lib/keyboard/json-sync'
  import { assetsUsage, formatKB } from '$lib/keyboard/look-assets'
  import { LAYOUT_FILE_MAX_BYTES } from '$lib/keyboard/types'
  import { parseLayoutJSON } from '$lib/keyboard/validate'
  import type { ValidationError } from '$lib/keyboard/validate'
  import { compatWarnings } from '$lib/keyboard/compat'
  import { buildLayoutLinks } from '$lib/keyboard/layout-url'
  import { REPO_URL } from '$lib/site'
  import { onMount } from 'svelte'
  import ErrorList from './ErrorList.svelte'
  import QrCode from './QrCode.svelte'

  let { store }: { store: EditorStore } = $props()

  const json = $derived(JSON.stringify(store.layout, null, 2) + '\n')
  // 書き出しをブロックするのは error だけ（warning は iOS がそのプロパティだけ無視するので書き出せる）
  const invalid = $derived(store.errors.length > 0)
  const warningCount = $derived(store.warnings.length)
  const assetBytes = $derived(assetsUsage(store.layout.assets && typeof store.layout.assets === 'object' ? (store.layout.assets as Record<string, unknown>) : {}))

  let pasted = $state('')
  let importErrors = $state<ValidationError[]>([])
  let message = $state('')
  let outputEl = $state<HTMLTextAreaElement>()

  // 現在のレイアウトの JSON を直接編集できる（store.layout が唯一の正。編集は読める形なら即座に反映し、フォームも追従する）。
  // 構文エラーなど反映できない間は、入力中の文字列を保ち、直前に反映できた内容が書き出し対象のまま。
  let draft = $state('')
  let draftError = $state('')
  let editing = $state(false)
  let lastId = ''
  let timer: ReturnType<typeof setTimeout> | undefined

  $effect(() => {
    const id = store.state.currentId
    if (id !== lastId) {
      lastId = id
      draft = json
      draftError = ''
    } else if (!editing && !draftError) draft = json
  })

  function applyDraft() {
    clearTimeout(timer)
    const r = parseDraft(draft)
    if (!r.ok) {
      draftError = r.error
      return
    }
    draftError = ''
    if (JSON.stringify(r.layout) !== JSON.stringify(store.layout)) store.replaceFromJson(r.layout)
  }
  function ondraft(e: Event & { currentTarget: HTMLTextAreaElement }) {
    draft = e.currentTarget.value
    clearTimeout(timer)
    timer = setTimeout(applyDraft, 300)
  }
  function onblur() {
    applyDraft()
    editing = false
  }

  // アプリへ渡すリンク（nagi://layout は「アプリで開く」、https の共有ページは コピー・QR 用。どちらも同じレイアウト）。
  // 上限超え・assets ありは作れず、理由を表示する。検証エラー中は作らない
  const links = $derived(invalid || draftError ? undefined : buildLayoutLinks(store.layout))
  const linkOk = $derived(links?.ok ? links : undefined)
  const linkReason = $derived(invalid || draftError ? '検証エラーを直すと、リンクを作れます。' : links && !links.ok ? links.message : '')
  const compat = $derived(compatWarnings(store.layout))
  let showQr = $state(false)
  let showGuide = $state(false)
  let isMobile = $state(false)
  onMount(() => {
    isMobile = /Android|iPhone|iPad|iPod/i.test(navigator.userAgent)
  })

  function openInApp() {
    if (!linkOk) return
    showGuide = true
    message = 'Nagi アプリを開きます。開かないときは、下の案内を確認してください。'
    window.location.href = linkOk.appUrl
  }

  async function copyLink() {
    if (!linkOk) return
    try {
      await navigator.clipboard.writeText(linkOk.shareUrl)
      message = 'リンクをコピーしました。スマホのブラウザで開くと、Nagi アプリに取り込めます。'
    } catch {
      message = 'コピーできませんでした。'
    }
  }

  function download() {
    const blob = new Blob([json], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `${store.layout.id}.json`
    document.body.append(a)
    a.click()
    a.remove()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
    message = `${store.layout.id}.json を書き出しました。`
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(json)
      message = 'JSON をクリップボードにコピーしました。'
    } catch {
      outputEl?.select()
      message = document.execCommand?.('copy') ? 'JSON をコピーしました。' : 'コピーできませんでした。テキストを選択して手動でコピーしてください。'
    }
  }

  function importText(text: string, source: string) {
    const known = store.state.layouts.map((l) => l.id)
    // 読み込むレイアウト自身の id は layout.target として有効。
    const result = parseLayoutJSON(text, { knownLayoutIds: known })
    if (!result.ok) {
      importErrors = result.errors
      message = `${source}を読み込めませんでした。内容を直して再度お試しください。`
      return false
    }
    importErrors = []
    const added = store.add(result.layout)
    message = `「${added.name}」を読み込みました${added.id !== result.layout.id ? `（id が重複していたため ${added.id} に変更）` : ''}。`
    return true
  }

  async function onfile(e: Event & { currentTarget: HTMLInputElement }) {
    const file = e.currentTarget.files?.[0]
    if (!file) return
    if (file.size > LAYOUT_FILE_MAX_BYTES) {
      importErrors = [{ path: '', message: `ファイルが大きすぎます（${LAYOUT_FILE_MAX_BYTES / 1024 / 1024}MB まで）` }]
      return
    }
    importText(await file.text(), file.name)
    e.currentTarget.value = ''
  }
</script>

<section aria-labelledby="json-title" class="panel">
  <h3 id="json-title">JSON の書き出しと読み込み</h3>

  <div class="block">
    <h4>書き出し（現在のレイアウト）</h4>
    {#if invalid}
      <p class="warn">検証エラーがあるため書き出せません。上のエラーを直してください。</p>
    {:else if warningCount > 0}
      <p class="hint">警告が {warningCount} 件あります（無視される値）。書き出しはできます。</p>
    {/if}
    {#if assetBytes > 0}
      <p class="hint">
        画像（assets）を含みます（画像 {formatKB(assetBytes)}、JSON 全体で約 {formatKB(new Blob([json]).size)}）。URL スキームでは渡せません。ダウンロードしたファイルかコピーした
        JSON でお渡しください。
      </p>
    {/if}
    <div class="btns">
      <button type="button" class="btn primary" disabled={invalid || !!draftError} onclick={download}>JSON をダウンロード</button>
      <button type="button" class="btn" disabled={invalid || !!draftError} onclick={copy}>コピー</button>
    </div>
    <div class="share" aria-labelledby="share-title">
      <h4 id="share-title">アプリに渡す（Android）</h4>
      <p class="hint">
        {isMobile ? 'このスマホの Nagi アプリで開けます。' : 'パソコンで作ったときは、QR コードをスマホのカメラで読み取ると渡せます。'}
        上限は 1MB で、画像（assets）を含むものは渡せません。
      </p>
      <div class="btns">
        <button type="button" class="btn" class:primary={isMobile} disabled={!linkOk} onclick={openInApp}>アプリで開く</button>
        <button type="button" class="btn" disabled={!linkOk} onclick={copyLink}>リンクをコピー</button>
        <button type="button" class="btn" class:primary={!isMobile} disabled={!linkOk} aria-expanded={showQr} onclick={() => (showQr = !showQr)}>
          {showQr ? 'QR を隠す' : 'QR を表示'}
        </button>
      </div>
      {#if !linkOk && linkReason}
        <p class="warn" role="status">{linkReason}</p>
      {/if}
      {#if showQr && linkOk}
        <QrCode text={linkOk.shareUrl} />
      {/if}
      {#if showGuide}
        <p class="hint">
          アプリが開かないときは、Nagi（Android 版）が入っていないか、このブラウザがアプリを開けません。
          <a href="{REPO_URL}/releases">配布ページ</a>からインストールするか（手順は<a href="/support#layout">サポート</a>）、
          リンクをコピーして Chrome で開いてください。
        </p>
      {/if}
    </div>
    {#if compat.length > 0}
      <div class="compat" role="note">
        <p class="warn"><strong>アプリがまだ反映しない指定があります。</strong>共有や書き出しはできますが、見た目どおりに動かない場合があります。</p>
        <ul>
          {#each compat as w (w.platform)}
            <li>{w.platformLabel}: {w.features.map((f) => `${f.name}（${f.label}）`).join('、')}</li>
          {/each}
        </ul>
      </div>
    {/if}
    <label>
      <span class="lbl">JSON（直接編集できます。編集は即座にフォームとプレビューへ反映されます）</span>
      <textarea
        bind:this={outputEl}
        rows="8"
        value={draft}
        spellcheck="false"
        aria-invalid={draftError ? 'true' : undefined}
        aria-describedby={draftError ? 'json-draft-error' : undefined}
        onfocus={() => (editing = true)}
        oninput={ondraft}
        {onblur}
      ></textarea>
    </label>
    {#if draftError}
      <p class="warn" id="json-draft-error" role="alert">
        {draftError}。反映されていません（直前の内容のままです）。
        <button type="button" class="btn small" onclick={() => { draft = json; draftError = '' }}>編集前に戻す</button>
      </p>
    {/if}
  </div>

  <div class="block">
    <h4>読み込み</h4>
    <p class="hint">読み込んだレイアウトは一覧に追加されます（現在のものは上書きしません）。</p>
    <label class="file">
      <span>ファイルから (.json)</span>
      <input type="file" accept="application/json,.json" onchange={onfile} />
    </label>
    <label>
      <span class="lbl">貼り付けから</span>
      <textarea bind:value={pasted} rows="5" placeholder={'{ "schema": 2, "id": "...", ... }'} spellcheck="false"></textarea>
    </label>
    <button
      type="button"
      class="btn"
      disabled={pasted.trim() === ''}
      onclick={() => {
        if (importText(pasted, '貼り付けた内容')) pasted = ''
      }}>貼り付けた JSON を読み込む</button
    >
    <ErrorList errors={importErrors} title="読み込みエラー" />
  </div>

  <p class="status" role="status" aria-live="polite">{message}</p>
</section>

<style>
  .panel {
    display: grid;
    gap: 16px;
  }
  h3 {
    margin: 0;
    font-size: 1.0625rem;
  }
  h4 {
    margin: 0 0 6px;
    font-size: 0.9375rem;
  }
  .block {
    display: grid;
    gap: 8px;
  }
  .btns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 0.8125rem;
    line-height: 1.5;
  }
  .hint,
  .lbl {
    color: var(--muted);
    font-size: 0.875rem;
    margin: 0;
  }
  .warn {
    margin: 0;
    color: var(--danger);
    font-size: 0.875rem;
  }
  .share {
    display: grid;
    gap: 8px;
    padding-top: 4px;
  }
  .compat ul {
    margin: 4px 0 0;
    padding-left: 1.25em;
    font-size: 0.875rem;
  }
  .file {
    display: grid;
    gap: 4px;
    font-size: 0.875rem;
  }
  .status {
    min-height: 1.5em;
    margin: 0;
    color: var(--ok);
    font-size: 0.875rem;
  }
</style>
