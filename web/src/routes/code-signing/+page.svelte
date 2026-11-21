<script lang="ts">
  import Meta from '$lib/components/Meta.svelte'
  import { ISSUES_URL, PRIVACY_PATH, RELEASES_URL, REPO_URL } from '$lib/site'
</script>

<Meta
  title="コード署名ポリシー"
  path="/code-signing"
  description="Nagi の Windows 版配布物に施すコード署名の方針。署名の対象・署名を承認する人・ビルドの経路を示します。Code signing policy for Nagi."
/>

<main class="doc">
  <header class="doc-head">
    <p class="eyebrow">配布</p>
    <h1>コード署名ポリシー</h1>
    <p class="lead">対象: Nagi の Windows 版の配布物（セットアップ用 .exe・zip と、その中の .exe / .dll）</p>
  </header>

  <div class="prose">
    <p>
      Free code signing provided by <a href="https://about.signpath.io/" rel="noopener">SignPath.io</a>,
      certificate by <a href="https://signpath.org/" rel="noopener">SignPath Foundation</a>.
    </p>
    <p>
      Windows 版の配布物の署名には、SignPath Foundation の証明書を使います（署名者の表示は「SignPath Foundation」になります）。
      署名の準備が整うまでの版は、署名なしで配布しています。
    </p>

    <h2>1. 署名の対象</h2>
    <ul>
      <li>
        <a href={REPO_URL} rel="noopener">公開リポジトリ</a>のソースから、GitHub Actions
        （<code>.github/workflows/windows-release.yml</code>）でビルドしたものだけに署名します。
        開発者の手元でビルドしたものには署名しません。
      </li>
      <li>
        署名するのは、このプロジェクトのソースからビルドした Nagi 自身の実行ファイル（<code>imserver.exe</code>・<code>nagi-settings.exe</code>
        など）と DLL（<code>nagi_windows_tsf.dll</code> など）、およびセットアップ用 .exe です。
      </li>
      <li>
        第三者のバイナリ（Microsoft Visual C++ 再頒布可能パッケージなど）には署名しません。辞書・変換モデルは配布物に含めず、
        利用者の端末で別途取得します。
      </li>
      <li>署名した配布物は <a href={RELEASES_URL} rel="noopener">GitHub Releases</a> で公開します。</li>
    </ul>

    <h2>2. 役割</h2>
    <ul>
      <li>コミット・レビュー（Committers and reviewers）: <a href="https://github.com/shiki-01" rel="noopener">shiki-01</a></li>
      <li>署名の承認（Approvers）: <a href="https://github.com/shiki-01" rel="noopener">shiki-01</a></li>
    </ul>
    <p>
      外部からの変更（pull request）は、上記のレビュー担当が内容を確認してから取り込みます。
      署名の要求はリリースのたびに承認担当が SignPath 上で承認します。関係するアカウントはすべて多要素認証を有効にしています。
    </p>

    <h2>3. プライバシー</h2>
    <p>
      This program will not transfer any information to other networked systems unless specifically requested
      by the user or the person installing or operating it.
    </p>
    <p>
      Nagi は入力した文章・学習データを端末の外へ送信しません。ネットワークを使うのは次の場合だけです。
    </p>
    <ul>
      <li>辞書・変換モデルの取得（導入時、または利用者が取得を指示したとき）</li>
      <li>アップデートの確認（<code>nagi.shiki-01.com</code> の更新情報を取得します。設定で無効にできます）</li>
    </ul>
    <p>詳しくは<a href={PRIVACY_PATH}>プライバシーポリシー</a>を参照してください。</p>

    <h2>4. 問い合わせ</h2>
    <p>
      署名された配布物に問題を見つけた場合は、<a href={ISSUES_URL} rel="noopener">GitHub の Issues</a> で報告してください。
    </p>

    <h2 lang="en">Code signing policy (English)</h2>
    <div lang="en">
      <p>
        Free code signing provided by SignPath.io, certificate by SignPath Foundation.
      </p>
      <ul>
        <li>
          Only binaries built by GitHub Actions (<code>.github/workflows/windows-release.yml</code>) from the
          <a href={REPO_URL} rel="noopener">public repository</a> are signed. Locally built binaries are never signed.
        </li>
        <li>
          Signed files are Nagi's own executables and DLLs built from this project's source, and the setup .exe.
          Third-party binaries are not signed. Dictionaries and conversion models are not bundled; they are downloaded
          on the user's machine.
        </li>
        <li>Committers, reviewers and approvers: <a href="https://github.com/shiki-01" rel="noopener">shiki-01</a>.</li>
        <li>Every signing request is manually approved by an approver. All team accounts use multi-factor authentication.</li>
        <li>
          Privacy: This program will not transfer any information to other networked systems unless specifically
          requested by the user or the person installing or operating it. Network access is limited to downloading
          dictionaries / models and checking for updates (which can be disabled). Typed text and learning data never
          leave the device.
        </li>
      </ul>
    </div>
  </div>
</main>
