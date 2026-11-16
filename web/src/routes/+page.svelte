<script lang="ts">
  import Meta from '$lib/components/Meta.svelte'
  import CodeExample from '$lib/components/site/CodeExample.svelte'
  import Minamo from '$lib/components/site/Minamo.svelte'
  import { RELEASES_URL, REPO_URL } from '$lib/site'

  // 特徴。右側の見本（spec）は下のマークアップで key ごとに描く
  const features = [
    {
      key: 'neural',
      title: '文脈を読んで、文ごと変換する',
      body: '小型の言語モデル（zenz-v3.1）が文全体を見て変換します。辞書とビタビ探索も併用し、モデルが無い・使えないときは辞書だけで動きます。'
    },
    {
      key: 'live',
      title: '打つそばから変わる',
      body: 'ライブ変換では、入力中の読みを少しずつ変換して表示します。打った読みはそのまま保たれるので、いつでも文節単位で直せます。'
    },
    {
      key: 'range',
      title: '文節と範囲を、そのまま操作する',
      body: '入力中の文字列を、最初から文節の列として扱います。注目文節の移動、区切りの伸縮、複数文節の選択、先頭の文節だけの確定ができます。'
    },
    {
      key: 'fuzzy',
      title: '打ち間違いには「もしかして」',
      body: '隣のキーへの打ち間違いを補った候補を「もしかして」として混ぜます。打った読みを勝手に置き換えることはしません。英単語の綴りにも対応します。'
    },
    {
      key: 'learn',
      title: '覚えるのも、端末の中',
      body: '確定した候補は端末内に学習し、次から上位に出します。ユーザー辞書は Mozc の TSV と互換で、候補には出どころ（学習・LLM・辞書など）が付きます。'
    },
    {
      key: 'profile',
      title: '端末の余力に合わせる',
      body: 'メモリや処理能力から 4 段階のプロファイルを自動で選び、実行中にも切り替えます。余力が小さいときは、辞書だけの軽い経路で動きます。'
    }
  ] as const

  const figures = [
    { value: '76.4', unit: '%', label: '変換の正解率（辞書 + モデル）', sub: '辞書のみでは約 66%。Mozc の評価セット 609 件' },
    { value: '32', unit: 'ms', label: '文全体の変換にかかる時間（中央値）', sub: '95 パーセンタイルは 56 ms' },
    { value: '70', unit: 'MB', label: '既定の変換モデルの大きさ（約）', sub: 'zenz-v3.1 small。iOS のキーボード拡張でも動く大きさ' },
    { value: '0', unit: '件', label: '外部へ送信する入力内容', sub: '変換・学習・辞書の処理はすべて端末内' }
  ]

  // stage: 0 未定 / 1 予定 / 2 開発 / 3 検証 / 4 配布
  const stages = ['予定', '開発', '検証', '配布']
  // 12 キーの配列（カスタムキーボードの紹介用。実際の既定配列とは細部が異なる）
  const kana = ['あ', 'か', 'さ', 'た', 'な', 'は', 'ま', 'や', 'ら', '小ﾞﾟ', 'わ', '、。?!']
  const macInstall = 'curl -fsSL https://nagi.shiki-01.com/install.sh | sh'
</script>

<Meta
  title="Nagi"
  path="/"
  description="Nagi（凪）は、端末内で動く小型の言語モデルでかな漢字変換を行う日本語入力システムです。入力内容を外部に送信しません。macOS 版と Android 版を配布中、iOS 版は TestFlight で検証中。"
/>

<main>
  <Minamo>
    {#snippet intro()}
      <p class="eyebrow">日本語入力システム</p>
      <p class="lead">
        Nagi（凪）は、小さなローカル言語モデルでかな漢字変換を行う日本語 IME です。変換も学習も端末の中で完結し、入力した文章をサーバーへ送りません。
      </p>
    {/snippet}
    {#snippet title()}
      <h1 id="hero-title"><span>端末の中だけで考える、</span><span>ニューラルかな漢字変換。</span></h1>
    {/snippet}
    {#snippet actions()}
      <p class="cta">
        <a class="btn primary" href="#status">入手方法を見る</a>
        <a class="btn" href={REPO_URL} rel="noopener">ソースコード</a>
      </p>
      <ul class="avail">
        <li><span>macOS</span><b>配布中</b></li>
        <li><span>Android</span><b>配布中</b></li>
        <li><span>iOS</span><b>検証中</b></li>
        <li><span>Windows</span><b>開発中</b></li>
      </ul>
    {/snippet}
  </Minamo>

  <section class="wrap block" aria-labelledby="features">
    <header class="block-head">
      <p class="eyebrow">特徴</p>
      <h2 id="features"><span>読みは保ち、</span><span>表記だけを映す</span></h2>
      <p class="block-lead">
        Nagi の内部では、打った読み（ひらがな）が常に元の情報で、漢字まじりの表記はその写しです。だから途中で何度でも、文節ごとに直せます。
      </p>
    </header>
    <ol class="ledger">
      {#each features as f (f.key)}
        <li>
          <div class="txt">
            <h3>{f.title}</h3>
            <p>{f.body}</p>
          </div>
          <div class="spec" aria-hidden="true">
            {#if f.key === 'neural'}
              <p class="rd">きしゃのきしゃがきしゃした</p>
              <p class="sf">貴社の記者が帰社した</p>
            {:else if f.key === 'live'}
              <p class="rd">きょうはいいてんき<i class="caret"></i></p>
              <p class="sf"><span class="dotted">今日はいい天気</span></p>
            {:else if f.key === 'range'}
              <p class="rd">あしたのかいぎのしりょうをおくります</p>
              <p class="sf segs"><span>明日の</span><span class="sel">会議の</span><span class="sel">資料を</span><span>送ります</span></p>
              <p class="keys"><span><kbd>←</kbd><kbd>→</kbd>注目文節</span><span><kbd>Shift</kbd>区切り</span><span><kbd>Ctrl</kbd><kbd>Shift</kbd>範囲</span></p>
            {:else if f.key === 'fuzzy'}
              <p class="rd mono">kamji</p>
              <p class="sf">感じ<span class="tag">もしかして</span></p>
              <p class="rd mono second">adress</p>
              <p class="sf latin">address<span class="tag">もしかして</span></p>
            {:else if f.key === 'learn'}
              <p class="rd">かいぎは</p>
              <p class="sf rows">
                <span class="row on"><b>会議は</b><em>学習</em></span>
                <span class="row"><b>懐疑は</b><em>辞書</em></span>
                <span class="row"><b>回帰は</b><em>辞書</em></span>
              </p>
            {:else}
              <p class="levels">
                <span><b>quality</b>大きいモデル</span>
                <span class="now"><b>balanced</b>既定</span>
                <span><b>light</b>小さいモデル</span>
                <span><b>minimal</b>辞書のみ</span>
              </p>
            {/if}
          </div>
        </li>
      {/each}
    </ol>
    <p class="note">見本はイメージです。使える機能は OS によって異なります。</p>
  </section>

  <section class="wrap block" aria-labelledby="figures">
    <header class="block-head">
      <p class="eyebrow">計測値</p>
      <h2 id="figures"><span>小さなモデルで、</span><span>手元で足りる</span></h2>
    </header>
    <dl class="figures">
      {#each figures as fg (fg.label)}
        <div>
          <dt>{fg.label}</dt>
          <dd><span class="num">{fg.value}</span><span class="unit">{fg.unit}</span></dd>
          <dd class="sub">{fg.sub}</dd>
        </div>
      {/each}
    </dl>
    <p class="note">
      正解率と時間は、開発機（Linux・CPU のみ）で辞書と zenz-v3.1 small を使って計測した値です。端末や設定によって変わります。
    </p>
  </section>

  <section class="wrap block" aria-labelledby="status">
    <header class="block-head">
      <p class="eyebrow">対応状況と入手</p>
      <h2 id="status">いまの状況</h2>
      <p class="block-lead">2026 年 11 月時点の状況です。いずれも開発中の版で、機能や手順は変わることがあります。</p>
    </header>
    <ul class="status">
      <li id="macos">
        <div class="pf">
          <h3>macOS</h3>
          <p class="req">macOS 13 以降</p>
        </div>
        {@render track(4)}
        <div class="pf-body">
          <p><strong>配布中</strong>（0.1.10）。ターミナルで次の 1 行を実行すると、署名・公証済みのアプリを入れ、辞書とモデルを取得します。</p>
          <CodeExample code={macInstall} />
          <p class="small">管理者権限は使いません。更新は署名を検証してから適用します。</p>
        </div>
      </li>
      <li id="android">
        <div class="pf">
          <h3>Android</h3>
          <p class="req">Android 8.0 以降</p>
        </div>
        {@render track(4)}
        <div class="pf-body">
          <p>
            <strong>APK を配布中</strong>。かな 12 キー・フリック・変換に対応し、端末内のモデルで変換します。<a href={RELEASES_URL} rel="noopener"
              >GitHub の配布ページ</a
            >にある <code>android-v</code> で始まる版の APK を入れてください。
          </p>
        </div>
      </li>
      <li id="testflight">
        <div class="pf">
          <h3>iOS</h3>
          <p class="req">iOS 16.1 以降</p>
        </div>
        {@render track(3)}
        <div class="pf-body">
          <p>
            <strong>TestFlight で検証中</strong>。かな 12 キー・フリック・QWERTY・変換・部分変換・絵文字と、<a href="/custom-keyboard"
              >レイアウトの取り込み</a
            >が動いています。
          </p>
          <!-- TODO: 外部テストの公開リンク（TestFlight Public Link）が決まったらここに設置する。 -->
          <p class="small">TestFlight の公開リンクは準備中です。用意でき次第、ここに掲載します。</p>
        </div>
      </li>
      <li id="windows">
        <div class="pf">
          <h3>Windows</h3>
          <p class="req">Windows 10 22H2 以降 / 11（x64）</p>
        </div>
        {@render track(2)}
        <div class="pf-body">
          <p><strong>開発中</strong>。実装は一通り済んでいますが、実機での検証が終わっていないため、まだ公開していません。</p>
        </div>
      </li>
      <li>
        <div class="pf">
          <h3>Linux</h3>
        </div>
        {@render track(0)}
        <div class="pf-body">
          <p><strong>未定</strong>。将来の検討対象です。</p>
        </div>
      </li>
    </ul>
    <p class="note">辞書（Mozc）とモデル（zenz）はアプリに同梱せず、配布元から別途取得します。使い方は<a href="/support">サポート</a>をご覧ください。</p>
  </section>

  {#snippet track(stage: number)}
    <ol class="track" aria-label="進み具合">
      {#each stages as st, i (st)}
        <li class:on={i < stage} class:now={i === stage - 1} aria-current={i === stage - 1 ? 'step' : undefined}>{st}</li>
      {/each}
    </ol>
  {/snippet}

  <section class="sea" aria-labelledby="privacy-h">
    <div class="wrap">
      <p class="eyebrow">プライバシー</p>
      <h2 id="privacy-h"><span>入力した文章は、</span><span>端末の外へ</span><span>出ません。</span></h2>

      <div class="inout" aria-hidden="true">
        <div class="device">
          <p class="cap">端末の中</p>
          <p class="flow"><span>入力</span><i></i><span>変換</span><i></i><span>学習</span><i></i><span>辞書</span></p>
        </div>
        <div class="wire">
          <p>モデル・辞書の取得</p>
          <p>更新の確認（無効にできます）</p>
        </div>
        <div class="outside">
          <p class="cap">外</p>
          <p class="none">入力内容は含みません</p>
        </div>
      </div>

      <ul class="sea-list">
        <li>入力した文章は端末の外に送信しません。変換は端末内のモデルと辞書だけで行います。</li>
        <li>
          ログは端末内にのみ保存します。App Store 配布版では入力内容を残しません。テスト配布（TestFlight）の期間中のみ、診断ログに入力内容を含めることがあります（パスワード欄を除く。設定で無効化・消去できます）。
        </li>
        <li>学習データは端末内に保存し、ワンコマンドで全削除できるようにする方針です。</li>
      </ul>
      <p class="sea-more">
        詳しくは<a href="/privacy">プライバシーポリシー</a>をご覧ください。使い方で困ったときは<a href="/support">サポート</a>をどうぞ。
      </p>

      <div class="fullaccess">
        <div>
          <h3 id="fullaccess">iOS の「フルアクセス」について</h3>
          <p>
            iOS のキーボードは、設定で「フルアクセスを許可」にしないと、本体アプリが取得した辞書・モデルのファイルを読めません。
            Nagi が許可をお願いする理由はこれだけで、<strong>辞書・モデルの読み込みのためです</strong>。
          </p>
          <ul>
            <li>キーボードから通信は行いません。入力内容を外部へ送信しません。</li>
            <li>
              許可しない場合の動作は端末により異なる場合があり、辞書・モデルを利用できないことがあります（入力した文字は失われません）。
            </li>
            <li>辞書・モデルの取得や更新の確認は、キーボードではなく本体アプリが行います。</li>
          </ul>
        </div>
        <div class="settings" aria-hidden="true">
          <p class="path">設定 › 一般 › キーボード › Nagi</p>
          <div class="row"><span>Nagi</span><span class="toggle on"></span></div>
          <div class="row"><span>フルアクセスを許可</span><span class="toggle on"></span></div>
          <p class="why">用途: 共有領域の辞書・モデルを読む</p>
        </div>
      </div>
    </div>
  </section>

  <section class="wrap block split" aria-labelledby="ck">
    <div>
      <p class="eyebrow">カスタムキーボード</p>
      <h2 id="ck">キーボードの配列を<br />自分で作る</h2>
      <p>
        モバイル版のキーボードは、配列とキーの動作を JSON で定義できます。<a href="/custom-keyboard">編集ページ</a
        >では、ブラウザだけでキーの配置・種類（文字入力・フリック・ショートカットなど）・幅・見た目を設定して書き出せます。
        データはブラウザ内にのみ保存され、サーバーには送信されません。
      </p>
      <p class="note">
        作ったレイアウトは、iOS 版では本体アプリの「レイアウト」タブ、Android 版ではリンクや QR コードから取り込めます（Android
        は見た目の細かい指定とツールバーに未対応です）。JSON の書式は<a href="/reference">リファレンス</a>にあります。
      </p>
      <p><a class="btn" href="/custom-keyboard">編集ページを開く</a></p>
    </div>
    <div class="kb" aria-hidden="true">
      <span class="k fn">☆123</span>
      {#each kana.slice(0, 3) as k (k)}<span class="k">{k}</span>{/each}
      <span class="k fn">⌫</span>
      <span class="k fn">ABC</span>
      {#each kana.slice(3, 6) as k (k)}
        {#if k === 'な'}
          <span class="k flick">
            <b>な</b><i class="u">ぬ</i><i class="l">に</i><i class="r">ね</i><i class="d">の</i>
          </span>
        {:else}
          <span class="k">{k}</span>
        {/if}
      {/each}
      <span class="k fn">空白</span>
      <span class="k fn">あいう</span>
      {#each kana.slice(6, 9) as k (k)}<span class="k">{k}</span>{/each}
      <span class="k accent tall">改行</span>
      <span class="k fn"
        ><svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.1"
          ><circle cx="8" cy="8" r="6.5" /><ellipse cx="8" cy="8" rx="3" ry="6.5" /><path d="M1.5 8h13M2.6 4.6h10.8M2.6 11.4h10.8" /></svg
        ></span
      >
      {#each kana.slice(9, 12) as k (k)}<span class="k small">{k}</span>{/each}
    </div>
  </section>
</main>

<style>
  .wrap {
    max-width: var(--wrap);
    margin: 0 auto;
    padding: 0 var(--gutter);
    box-sizing: border-box;
  }
  h1,
  h2 {
    font-family: var(--font-serif);
    font-weight: 600;
    letter-spacing: 0.04em;
  }
  .note {
    color: var(--muted);
    font-size: 0.875rem;
  }

  /* ---- 冒頭（水面）に渡す中身 ---- */
  .lead {
    max-width: 27em;
    margin: 18px 0 0;
    font-size: 0.9375rem;
    line-height: 2;
  }
  /* 見出しは縦書き。右の行から読む */
  h1 {
    writing-mode: vertical-rl;
    margin: 0;
    font-size: clamp(1.5rem, 3.1vw, 2.25rem);
    line-height: 1.8;
    letter-spacing: 0.1em;
    white-space: nowrap;
  }
  h1 span {
    display: block;
  }
  h1 span + span {
    padding-top: 1.4em;
  }
  .cta {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 0 0 20px;
  }
  .cta .btn {
    min-height: 44px;
    padding: 6px 18px;
  }
  .avail {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--line);
    font-size: 0.8125rem;
  }
  .avail li {
    display: flex;
    justify-content: space-between;
    padding: 7px 0;
    border-bottom: 1px solid var(--line);
  }
  .avail span {
    font-family: var(--font-mono);
    letter-spacing: 0.02em;
  }
  .avail b {
    font-weight: 400;
    color: var(--muted);
  }

  /* ---- 各節の共通 ---- */
  .block {
    padding-top: clamp(72px, 10vw, 128px);
  }
  .block-head {
    display: grid;
    gap: 14px 48px;
    margin-bottom: 36px;
  }
  @media (min-width: 860px) {
    .block-head {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      align-items: end;
    }
    .block-head .eyebrow,
    .block-head:not(:has(.block-lead)) h2 {
      grid-column: 1 / -1;
    }
  }
  h2 {
    font-size: clamp(1.625rem, 4vw, 2.375rem);
    line-height: 1.5;
    margin: 0;
    text-wrap: balance;
  }
  h2 span {
    display: inline-block;
  }
  .block-lead {
    margin: 0;
    color: var(--muted);
    font-size: 0.9375rem;
  }
  .split h2 {
    margin: 12px 0 20px;
  }

  /* ---- 特徴: 左に説明、右に「読み／水平線／表記」の見本 ---- */
  .ledger {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--line-strong);
  }
  .ledger > li {
    display: grid;
    gap: 18px 48px;
    padding: 28px 0 30px;
    border-bottom: 1px solid var(--line);
  }
  @media (min-width: 860px) {
    .ledger > li {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      align-items: center;
    }
  }
  .ledger h3 {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: 1.25rem;
    line-height: 1.6;
    letter-spacing: 0.03em;
    margin: 0 0 8px;
  }
  .ledger .txt p {
    margin: 0;
    max-width: 32em;
    font-size: 0.9375rem;
    color: var(--muted);
  }
  .spec {
    min-width: 0;
    font-family: var(--font-serif);
    overflow: hidden;
  }
  .spec p {
    margin: 0;
  }
  .rd {
    padding-bottom: 6px;
    border-bottom: 1px solid var(--fg);
    font-size: 0.875rem;
    letter-spacing: 0.14em;
    color: var(--muted);
    white-space: nowrap;
  }
  .rd.mono {
    font-family: var(--font-mono);
    letter-spacing: 0.1em;
  }
  .rd.second {
    margin-top: 14px;
  }
  .sf {
    padding-top: 6px;
    font-size: clamp(1.25rem, 3.4vw, 1.75rem);
    font-weight: 600;
    line-height: 1.6;
    letter-spacing: 0.04em;
    white-space: nowrap;
    background: linear-gradient(var(--water), transparent);
  }
  .sf.latin {
    font-family: var(--font-mono);
    font-weight: 400;
    font-size: 1.25rem;
    letter-spacing: 0.02em;
  }
  .spec .caret {
    display: inline-block;
    width: 1.5px;
    height: 1.1em;
    margin-left: 2px;
    background: var(--fg);
    vertical-align: -0.18em;
  }
  .dotted {
    text-decoration: underline dotted 1.5px;
    text-underline-offset: 0.28em;
  }
  .segs span {
    display: inline-block;
    margin-right: 0.16em;
    line-height: 1.35;
    box-shadow: 0 1.5px 0 var(--fg);
  }
  .segs .sel {
    box-shadow: 0 4px 0 var(--accent);
    background: var(--accent-soft);
  }
  .keys {
    margin-top: 12px !important;
    font-family: var(--font-sans);
    font-size: 0.75rem;
    color: var(--muted);
    white-space: nowrap;
  }
  kbd {
    display: inline-block;
    min-width: 1.4em;
    margin: 0 3px 0 0;
    padding: 0 5px;
    border: 1px solid var(--line-strong);
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    line-height: 1.7;
    text-align: center;
    color: var(--fg);
  }
  .keys span {
    display: inline-block;
    margin-right: 16px;
  }
  .keys kbd:last-of-type {
    margin-right: 6px;
  }
  .tag,
  .rows em {
    font-family: var(--font-sans);
    font-style: normal;
    font-weight: 400;
    font-size: 0.6875rem;
    letter-spacing: 0.08em;
    color: var(--accent);
  }
  .tag {
    margin-left: 12px;
    vertical-align: 0.3em;
  }
  .rows {
    display: grid;
    max-width: 15em;
    font-size: 1.0625rem !important;
    font-weight: 400 !important;
    background: none !important;
  }
  .rows .row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 3px 10px;
    border-bottom: 1px solid var(--line);
  }
  .rows .row.on {
    background: var(--accent-soft);
  }
  .rows b {
    font-weight: 400;
  }
  .rows .row:not(.on) em {
    color: var(--muted);
  }
  /* プロファイル: 水位のように 4 段で示す */
  .levels {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    align-items: end;
    gap: 6px;
    font-family: var(--font-sans);
    font-size: 0.6875rem;
    color: var(--muted);
    border-bottom: 1px solid var(--fg);
  }
  .levels span {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 2px;
    padding: 0 8px 8px;
    box-sizing: border-box;
    background: var(--water);
    white-space: nowrap;
  }
  .levels span:nth-child(1) {
    height: 104px;
  }
  .levels span:nth-child(2) {
    height: 82px;
  }
  .levels span:nth-child(3) {
    height: 60px;
  }
  .levels span:nth-child(4) {
    height: 42px;
  }
  .levels b {
    font-family: var(--font-mono);
    font-weight: 400;
    font-size: 0.75rem;
    color: var(--fg);
  }
  .levels .now {
    background: var(--accent-soft);
    box-shadow: inset 0 3px 0 var(--accent);
  }

  /* ---- 計測値 ---- */
  .figures {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 0;
    border-top: 1px solid var(--line-strong);
  }
  @media (min-width: 860px) {
    .figures {
      grid-template-columns: repeat(4, minmax(0, 1fr));
    }
  }
  .figures > div {
    display: flex;
    flex-direction: column;
    padding: 18px 16px 22px 0;
    border-bottom: 1px solid var(--line);
  }
  .figures > div + div {
    padding-left: 16px;
    border-left: 1px solid var(--line);
  }
  @media (max-width: 859px) {
    .figures > div:nth-child(odd) {
      padding-left: 0;
      border-left: 0;
    }
  }
  .figures dt {
    order: 2;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .figures dd {
    margin: 0;
  }
  .figures .num {
    font-family: var(--font-serif);
    font-size: clamp(2.5rem, 7vw, 4.25rem);
    line-height: 1.2;
    letter-spacing: 0.01em;
    font-variant-numeric: lining-nums;
  }
  .figures .unit {
    margin-left: 6px;
    font-family: var(--font-mono);
    font-size: 0.875rem;
    color: var(--muted);
  }
  .figures .sub {
    order: 3;
    margin-top: 4px;
    font-size: 0.75rem;
    line-height: 1.7;
    color: var(--muted);
  }

  /* ---- 対応状況: 水平線の上を進む日で段階を示す ---- */
  .status {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    border-top: 1px solid var(--line-strong);
  }
  .status > li {
    display: grid;
    gap: 14px 40px;
    padding: 26px 0;
    border-bottom: 1px solid var(--line);
    align-items: start;
    scroll-margin-top: 72px;
  }
  @media (min-width: 860px) {
    .status > li {
      grid-template-columns: 12em 13em minmax(0, 1fr);
    }
  }
  .status h3 {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: 1.5rem;
    line-height: 1.4;
    letter-spacing: 0.02em;
    margin: 0;
  }
  .req {
    margin: 2px 0 0;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .pf-body {
    min-width: 0;
  }
  .pf-body p {
    margin: 0;
    font-size: 0.9375rem;
  }
  .pf-body .small {
    margin-top: 8px;
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .pf-body :global(.example) {
    margin: 12px 0 0;
  }
  .track {
    list-style: none;
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    margin: 0;
    padding: 12px 0 0;
    max-width: 15em;
  }
  .track li {
    position: relative;
    padding-top: 9px;
    border-top: 1px solid var(--line);
    font-size: 0.6875rem;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .track li.on {
    border-top-color: var(--fg);
    color: var(--fg);
  }
  /* 通過した段階は線上の小さな印、現在の段階は水平線に掛かる日 */
  .track li.on::before {
    content: '';
    position: absolute;
    left: 0;
    top: -3px;
    width: 1px;
    height: 5px;
    background: var(--fg);
  }
  .track li.now::before {
    left: 0;
    top: -11px;
    width: 20px;
    height: 10px;
    border-radius: 10px 10px 0 0;
    background: var(--sun);
  }
  .track li.now {
    font-weight: 600;
  }

  /* ---- プライバシー: 夜の海の帯 ---- */
  .sea {
    margin-top: clamp(80px, 11vw, 136px);
    padding: clamp(64px, 9vw, 104px) 0 clamp(72px, 10vw, 112px);
    background: var(--sea);
    color: var(--sea-fg);
    --focus: #f3d98f;
  }
  .sea a {
    color: var(--sea-fg);
  }
  .sea .eyebrow {
    color: var(--sea-muted);
  }
  .sea .eyebrow::before {
    background: #f3d98f;
    box-shadow: 0 1px 0 var(--sea-fg);
  }
  .sea h2 span {
    display: inline-block;
  }
  .sea h2 {
    margin-top: 14px;
    font-size: clamp(1.875rem, 5.4vw, 3.25rem);
    line-height: 1.5;
  }
  /* 端末の中と外の図 */
  .inout {
    display: grid;
    margin-top: 44px;
    font-size: 0.8125rem;
    color: var(--sea-muted);
  }
  .inout p {
    margin: 0;
  }
  .device,
  .outside {
    padding: 16px 18px 18px;
    border: 1px solid var(--sea-line);
  }
  .device {
    border-color: var(--sea-fg);
  }
  .cap {
    font-size: 0.6875rem;
    letter-spacing: 0.14em;
  }
  .flow {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px !important;
    font-family: var(--font-serif);
    font-size: 1.125rem;
    color: var(--sea-fg);
    white-space: nowrap;
  }
  .flow i {
    flex: 1;
    max-width: 48px;
    height: 1px;
    background: var(--sea-muted);
  }
  .wire {
    display: grid;
    align-content: center;
    gap: 4px;
    padding: 14px 18px;
    border-left: 1px dashed var(--sea-muted);
    margin-left: 28px;
  }
  .none {
    margin-top: 10px !important;
    font-family: var(--font-serif);
    font-size: 1.125rem;
    color: var(--sea-fg);
  }
  @media (min-width: 860px) {
    .inout {
      grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr) minmax(0, 0.85fr);
    }
    .wire {
      margin: 0;
      border-left: 0;
      padding: 0 20px;
      text-align: center;
      background: linear-gradient(var(--sea-muted), var(--sea-muted)) center / 100% 1px no-repeat;
      background-image: repeating-linear-gradient(90deg, var(--sea-muted) 0 6px, transparent 6px 12px);
      background-size: 100% 1px;
      background-position: center;
      background-repeat: no-repeat;
      align-content: space-between;
      padding-block: 10px;
    }
  }
  .sea-list {
    list-style: none;
    padding: 0;
    margin: 40px 0 0;
    display: grid;
    gap: 0 40px;
    counter-reset: item;
  }
  @media (min-width: 760px) {
    .sea-list {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  .sea-list li {
    padding: 18px 0 20px;
    border-top: 1px solid var(--sea-line);
    font-size: 0.9375rem;
    color: var(--sea-muted);
  }
  .sea-more {
    font-size: 0.9375rem;
    margin: 24px 0 0;
  }
  .fullaccess {
    display: grid;
    gap: 32px 56px;
    margin-top: 72px;
    padding-top: 40px;
    border-top: 1px solid var(--sea-line);
    align-items: start;
  }
  @media (min-width: 860px) {
    .fullaccess {
      grid-template-columns: minmax(0, 1fr) 320px;
    }
  }
  .fullaccess h3 {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: 1.375rem;
    margin: 0 0 12px;
  }
  .fullaccess p,
  .fullaccess li {
    font-size: 0.9375rem;
    color: var(--sea-muted);
  }
  .fullaccess strong {
    color: var(--sea-fg);
    text-decoration: underline 2px #f3d98f;
    text-underline-offset: 0.3em;
  }
  .fullaccess ul {
    padding-left: 1.2em;
  }
  .settings {
    border: 1px solid var(--sea-line);
    padding: 14px 16px;
    font-size: 0.875rem;
  }
  .settings .path {
    margin: 0 0 10px;
    font-size: 0.75rem;
    color: var(--sea-muted);
  }
  .settings .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 0;
    border-top: 1px solid var(--sea-line);
  }
  .toggle {
    width: 38px;
    height: 22px;
    border-radius: 11px;
    background: var(--sea-line);
    position: relative;
  }
  .toggle.on {
    background: #f3d98f;
  }
  .toggle::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--sea);
  }
  .toggle.on::after {
    left: 19px;
  }
  .settings .why {
    margin: 8px 0 0;
    font-size: 0.75rem;
    color: var(--sea-muted);
  }

  /* ---- 2 段組の節 ---- */
  .split {
    display: grid;
    gap: 32px 64px;
    align-items: center;
  }
  @media (min-width: 860px) {
    .split {
      grid-template-columns: minmax(0, 1fr) 360px;
    }
  }
  .split p {
    max-width: 38em;
  }

  /* ---- 12 キーの見本 ---- */
  .kb {
    display: grid;
    grid-template-columns: 1fr 1.2fr 1.2fr 1.2fr 1fr;
    grid-auto-rows: 46px;
    gap: 6px;
    padding: 8px;
    background: var(--kb-bg);
    border-radius: 4px;
  }
  .k {
    position: relative;
    display: grid;
    place-items: center;
    background: var(--key-bg);
    border-radius: 6px;
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.18);
    font-size: 1.0625rem;
  }
  .k.fn {
    background: var(--key-fn-bg);
    font-size: 0.75rem;
  }
  .k.small {
    font-size: 0.8125rem;
  }
  .k.accent {
    background: var(--key-accent-bg);
    color: var(--key-accent-fg);
    font-size: 0.75rem;
  }
  .k.tall {
    grid-row: span 2;
  }
  .flick {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    z-index: 1;
  }
  .flick i {
    position: absolute;
    font-style: normal;
    font-size: 0.6875rem;
    color: var(--muted);
  }
  .flick .u {
    top: 1px;
  }
  .flick .d {
    bottom: 1px;
  }
  .flick .l {
    left: 6px;
  }
  .flick .r {
    right: 6px;
  }
</style>
