<!--
  トップの冒頭「水面」。水平線の上（空）に打った読みを、下（水面）にその変換結果を置く。
  「読みが真実で、表記はその射影」という Nagi の内部表現を、水面に映る像として見せる。
  読みを 1 文字ずつ入力（ライブ変換で下の表記が追従）→ Space で文節に分かれる → 注目文節の候補 → Enter で確定、を繰り返す。
  実際のエンジンは動かさず、決め打ちの台本を再生するだけ。動きを減らす設定では変換後の状態を止めて表示する。
  eyebrow・title（大きな見出し）・lead（導入文）・actions（導線）は呼び出し側から渡す。
  日と水面の映りがゆっくり呼吸するように明滅する（動きを減らす設定では止まる）。
-->
<script lang="ts">
  import { onMount, type Snippet } from 'svelte'

  let { eyebrow, title, lead, actions }: { eyebrow: Snippet; title: Snippet; lead: Snippet; actions: Snippet } = $props()

  type Source = '学習' | 'LLM' | '辞書' | 'かな'
  type Script = {
    reading: string
    /** ライブ変換の途中経過。[読みの文字数, その時点までの表記]。間の読みは未変換のまま後ろに付く */
    live: [number, string][]
    segments: string[]
    focus: number
    candidates: { text: string; source: Source }[]
  }

  const scripts: Script[] = [
    {
      reading: 'ゆうがたのうみはなぎだった',
      live: [
        [4, '夕方'],
        [5, '夕方の'],
        [7, '夕方の海'],
        [8, '夕方の海は'],
        [10, '夕方の海は凪'],
        [13, '夕方の海は凪だった']
      ],
      segments: ['夕方の', '海は', '凪だった'],
      focus: 2,
      candidates: [
        { text: '凪だった', source: 'LLM' },
        { text: '薙だった', source: '辞書' },
        { text: 'なぎだった', source: 'かな' }
      ]
    },
    {
      reading: 'きしゃのきしゃがきしゃした',
      live: [
        [3, '汽車'],
        [4, '汽車の'],
        [7, '貴社の記者'],
        [8, '貴社の記者が'],
        [11, '貴社の記者が帰社'],
        [13, '貴社の記者が帰社した']
      ],
      segments: ['貴社の', '記者が', '帰社した'],
      focus: 1,
      candidates: [
        { text: '記者が', source: 'LLM' },
        { text: '汽車が', source: '辞書' },
        { text: '貴社が', source: '辞書' }
      ]
    },
    {
      reading: 'かいぎはごごからです',
      live: [
        [3, '会議'],
        [4, '会議は'],
        [6, '会議は午後'],
        [8, '会議は午後から'],
        [10, '会議は午後からです']
      ],
      segments: ['会議は', '午後からです'],
      focus: 0,
      candidates: [
        { text: '会議は', source: '学習' },
        { text: '懐疑は', source: '辞書' },
        { text: '回帰は', source: '辞書' }
      ]
    }
  ]

  type Phase = 'typing' | 'converted' | 'committed'
  let index = $state(0)
  let typed = $state(scripts[0].reading.length)
  let phase = $state<Phase>('converted')

  const s = $derived(scripts[index])
  /** ライブ変換中の表記: 最後の経過点までの表記 + まだ変換されていない読み */
  const liveText = $derived.by(() => {
    let done = 0
    let text = ''
    for (const [n, t] of s.live) {
      if (n <= typed) {
        done = n
        text = t
      }
    }
    return text + s.reading.slice(done, typed)
  })
  // 状態機械の状態名（Direct / Composing / Converting）をそのまま出す
  const status = $derived(
    phase === 'typing'
      ? { name: 'Composing', note: 'ライブ変換' }
      : phase === 'converted'
        ? { name: 'Converting', note: `文節 ${s.focus + 1} / ${s.segments.length}` }
        : { name: 'Direct', note: '確定' }
  )

  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return
    let timer: ReturnType<typeof setTimeout>
    let alive = true
    const wait = (ms: number) => new Promise<void>((r) => (timer = setTimeout(r, ms)))
    ;(async () => {
      // 初期表示は 1 本目の変換後。少し見せてから動き出す
      await wait(3200)
      while (alive) {
        phase = 'committed'
        await wait(1500)
        index = (index + 1) % scripts.length
        typed = 0
        phase = 'typing'
        for (let i = 1; i <= scripts[index].reading.length && alive; i++) {
          typed = i
          await wait(110 + Math.random() * 80)
        }
        await wait(900)
        phase = 'converted'
        await wait(3600)
      }
    })()
    return () => {
      alive = false
      clearTimeout(timer)
    }
  })
</script>

<section class="minamo" aria-labelledby="hero-title">
  <div class="sky">
    <div class="copy">
      {@render eyebrow()}
      {@render title()}
      {@render lead()}
      {@render actions()}
    </div>
    <p class="kana" class:gone={phase === 'committed'} aria-hidden="true">
      <span class="label">よみ</span>
      <span class="chars">{s.reading.slice(0, typed)}</span>{#if phase === 'typing'}<span class="caret"></span>{/if}
    </p>
  </div>

  <div class="horizon" aria-hidden="true">
    <span class="sun"></span>
    <span class="glow"></span>
    <!-- 日の下のなみなみ（アプリアイコンの波）。ゆっくり横へ流れる -->
    <svg class="waves" viewBox="-120 0 240 60" focusable="false">
      <path class="wave" d="M-200 14q20-10 40 0t40 0t40 0t40 0t40 0t40 0t40 0t40 0t40 0t40 0" />
      <path class="wave faint" d="M-180 40q20-10 40 0t40 0t40 0t40 0t40 0t40 0t40 0t40 0t40 0" />
    </svg>
  </div>

  <div class="water">
    <div class="shore">
      <figure class="conv">
        <div aria-hidden="true">
          <p class="surface">
            <span class="label">表記</span>
            {#if phase === 'typing'}
              <span class="live">{liveText}</span>
            {:else if phase === 'converted'}
              {#each s.segments as seg, i (i)}
                <span class="seg" class:focus={i === s.focus}
                  >{seg}{#if i === s.focus}
                    <span class="cands">
                      {#each s.candidates as c, j (c.text)}
                        <span class="cand" class:selected={j === 0}>
                          <span class="n">{j + 1}</span>
                          <span class="t">{c.text}</span>
                          <span class="src">{c.source}</span>
                        </span>
                      {/each}
                    </span>
                  {/if}</span
                >
              {/each}
            {:else}
              <span class="done">{s.segments.join('')}</span>
            {/if}
          </p>
          <p class="state"><span class="st">{status.name}</span>{status.note}</p>
        </div>
        <figcaption>
          変換の様子（イメージ）。水平線の上が打った読み、下がその表記です。太い下線の文節が候補の対象で、候補の右に出どころ（学習・LLM・辞書など）が付きます。
        </figcaption>
      </figure>
    </div>
  </div>
</section>

<style>
  .minamo {
    --sun-r: clamp(40px, 7vw, 84px);
    --sun-x: 80%;
    position: relative;
    overflow: hidden;
  }

  /* ---- 空: 日のあたりだけがほのかに明るい ---- */
  .sky {
    position: relative;
    background: radial-gradient(ellipse 60% 70% at var(--sun-x) 100%, color-mix(in srgb, var(--sun) 26%, transparent), transparent 70%);
  }
  .sky > * {
    position: relative;
  }
  .copy {
    max-width: var(--wrap);
    margin: 0 auto;
    padding: clamp(40px, 5vw, 64px) var(--gutter) 0;
    box-sizing: border-box;
  }
  .kana {
    max-width: var(--wrap);
    margin: 0 auto;
    padding: clamp(28px, 3.5vw, 40px) var(--gutter) 14px;
    box-sizing: border-box;
    min-height: 1.9em;
    font-family: var(--font-serif);
    font-size: clamp(1.0625rem, 2.5vw, 1.875rem);
    line-height: 1.9;
    letter-spacing: 0.14em;
    color: var(--muted);
    transition: opacity 0.9s ease;
  }
  .kana.gone {
    opacity: 0;
  }
  .label {
    display: inline-block;
    width: 3.4em;
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    letter-spacing: 0.1em;
    color: var(--muted);
    vertical-align: middle;
  }
  .caret {
    display: inline-block;
    width: 1.5px;
    height: 1.15em;
    margin-left: 1px;
    background: var(--fg);
    vertical-align: -0.2em;
    animation: blink 1s steps(2, start) infinite;
  }
  @keyframes blink {
    to {
      visibility: hidden;
    }
  }

  /* ---- 水平線と日 ---- */
  .horizon {
    position: relative;
    z-index: 1; /* 水面の背景より手前に、日の映りと波を出す */
    height: 0;
    border-top: 1px solid var(--fg);
  }
  .sun,
  .glow {
    position: absolute;
    left: var(--sun-x);
    width: calc(var(--sun-r) * 2);
    margin-left: calc(var(--sun-r) * -1);
  }
  .sun {
    bottom: 0;
    height: var(--sun-r);
    border-radius: var(--sun-r) var(--sun-r) 0 0;
    background: var(--sun);
    animation: breathe 9s ease-in-out infinite;
    transform-origin: 50% 100%;
  }
  /* 水面に映る日。横縞に割れて、下へ向かって消える */
  .glow {
    top: 1px;
    height: calc(var(--sun-r) * 1.7);
    background: var(--sun);
    border-radius: 0 0 50% 50% / 0 0 100% 100%;
    opacity: 0.55;
    -webkit-mask-image: repeating-linear-gradient(#000 0 3px, transparent 3px 9px), linear-gradient(#000, transparent 92%);
    mask-image: repeating-linear-gradient(#000 0 3px, transparent 3px 9px), linear-gradient(#000, transparent 92%);
    -webkit-mask-composite: source-in;
    mask-composite: intersect;
    animation: shimmer 9s ease-in-out infinite;
  }
  .waves {
    position: absolute;
    left: var(--sun-x);
    top: calc(var(--sun-r) * 0.35);
    width: calc(var(--sun-r) * 6);
    height: calc(var(--sun-r) * 1.5);
    margin-left: calc(var(--sun-r) * -3);
    overflow: hidden;
    -webkit-mask-image: linear-gradient(90deg, transparent, #000 25%, #000 75%, transparent);
    mask-image: linear-gradient(90deg, transparent, #000 25%, #000 75%, transparent);
  }
  .wave {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-linecap: round;
    opacity: 0.75;
    animation: drift 7s linear infinite;
  }
  .wave.faint {
    opacity: 0.4;
    animation-duration: 10s;
    animation-direction: reverse;
  }
  @keyframes drift {
    to {
      transform: translateX(40px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sun,
    .glow,
    .wave {
      animation: none;
    }
  }
  @keyframes breathe {
    50% {
      transform: scale(1.025);
      filter: brightness(1.04);
    }
  }
  @keyframes shimmer {
    50% {
      opacity: 0.4;
      transform: translateX(3px);
    }
  }

  /* ---- 水面 ---- */
  .water {
    position: relative;
    background: linear-gradient(var(--water), var(--bg));
  }
  .shore {
    position: relative;
    max-width: var(--wrap);
    margin: 0 auto;
    padding: 0 var(--gutter) clamp(48px, 7vw, 88px);
    box-sizing: border-box;
  }
  .conv {
    position: relative;
    z-index: 1;
    margin: 0;
    min-width: 0;
  }
  .surface {
    margin: 0;
    padding: 10px 0 0;
    min-height: 1.7em;
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: clamp(1.75rem, 5.4vw, 3.5rem);
    line-height: 1.7;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }
  .surface .label {
    font-weight: 400;
    width: 3.4em;
  }
  /* 入力中（ライブ変換）は細い点線の下線、変換中は文節ごとの下線、注目文節は太い下線 */
  .live {
    text-decoration: underline dotted 1.5px;
    text-underline-offset: 0.28em;
  }
  .seg {
    position: relative;
    display: inline-block;
    margin-right: 0.14em;
    line-height: 1.35;
    box-shadow: 0 1.5px 0 var(--fg);
  }
  .seg.focus {
    box-shadow: 0 4px 0 var(--accent);
  }
  .cands {
    position: absolute;
    left: -1px;
    top: calc(100% + 12px);
    z-index: 2;
    display: grid;
    min-width: 12.5em;
    border: 1px solid var(--fg);
    background: var(--surface);
    font-family: var(--font-sans);
    font-weight: 400;
    font-size: 0.9375rem;
    letter-spacing: 0;
    line-height: 1.4;
  }
  .cand {
    display: grid;
    grid-template-columns: 1.6em 1fr auto;
    align-items: baseline;
    gap: 6px;
    padding: 6px 10px;
  }
  .cand + .cand {
    border-top: 1px solid var(--line);
  }
  .cand.selected {
    background: var(--accent-soft);
  }
  .n {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--muted);
  }
  .t {
    font-family: var(--font-serif);
    font-size: 1.0625rem;
  }
  .src {
    font-size: 0.6875rem;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .cand.selected .src {
    color: var(--accent);
  }
  .state {
    margin: 150px 0 0;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .st {
    display: inline-block;
    min-width: 6.4em;
    margin-right: 10px;
    padding: 1px 6px;
    border: 1px solid var(--line-strong);
    color: var(--fg);
    text-align: center;
  }
  figcaption {
    margin-top: 10px;
    max-width: 44em;
    font-size: 0.75rem;
    line-height: 1.7;
    color: var(--muted);
  }

  @media (max-width: 899px) {
    .minamo {
      --sun-x: 84%;
    }
    .waves {
      width: calc(var(--sun-r) * 3.4);
      margin-left: calc(var(--sun-r) * -1.7);
    }
    .label {
      display: block;
      width: auto;
      margin-bottom: -4px;
    }
    .kana {
      padding-bottom: 10px;
      font-size: 0.9375rem;
      letter-spacing: 0.06em;
    }
    .state {
      margin-top: 138px;
    }
  }
</style>
