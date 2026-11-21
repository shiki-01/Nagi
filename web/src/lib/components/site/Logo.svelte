<!--
  サイトのロゴ。アイコン（太陽・水平線・波）とワードマーク（Nagi ｜ 凪）。
  ヘッダーとフッターで同じものを使う。size は基準の文字サイズ（アイコンはその 1.6 倍）。
-->
<script lang="ts">
  let { size = '1.1875rem' }: { size?: string } = $props()
</script>

<span class="logo" style:--s={size}>
  <picture>
    <source srcset="/icon-dark.svg" media="(prefers-color-scheme: dark)" />
    <img src="/icon.svg" alt="" width="64" height="64" />
  </picture>
  <span class="name">Nagi</span>
  <span class="kanji" aria-hidden="true">凪</span>
</span>

<style>
  .logo {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--s) * 0.55);
    color: var(--fg);
    line-height: 1.2;
  }
  img {
    display: block;
    width: calc(var(--s) * 1.6);
    height: calc(var(--s) * 1.6);
    border-radius: calc(var(--s) * 0.32);
  }
  .name {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: var(--s);
    letter-spacing: 0.04em;
  }
  .kanji {
    font-family: var(--font-serif);
    color: var(--muted);
    font-size: calc(var(--s) * 0.74);
    padding-left: calc(var(--s) * 0.55);
    border-left: 1px solid var(--line);
  }
</style>
