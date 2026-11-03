# Android: サイトのエディタで作ったレイアウトを読み込んで使う（企画。A・B・C は実装済み。§8）

- 版: 2026-11-03（企画のみ。実装は後回し）
- 関連: `docs/ios-keyboard-layout.md`（schema 2）、`docs/decisions.md` H-76〜H-78、`docs/android-release.md`
- 対象: サイト（`nagi.shiki-01.com/custom-keyboard`）の編集 GUI で作ったレイアウトを、Android の本体アプリへ渡して使う流れ

## 1. 現状（調査結果）

| 項目 | 状況 |
| --- | --- |
| Android: 取り込み | 実装済み。`LayoutManagerActivity` が、ファイル（SAF）・テキスト貼り付け・`nagi://layout?data=…` のリンクから取り込む。検証（`LayoutParser`）→ 確認ダイアログ → 保存。`nagi://layout` の intent-filter も登録済み（H-77 以降の実装） |
| Android: 検証・描画 | schema 2 の検証と、JSON 駆動の描画（行・キー・フリック・toggle・longPress・shortcut・layout・action）を実装済み。`look` 系・`toolbar`・`visible`・Iconify は未対応（構文は受け付けるが無視する） |
| サイトのエディタ（出力） | **JSON のダウンロードとクリップボードへのコピーだけ**（`web/src/lib/components/JsonPanel.svelte`）。`nagi://layout` のリンクを作る機能が無い。リンクの仕様は `docs/ios-keyboard-layout.md` と `web/src/routes/reference` に書かれているだけで、エディタには実装が無い |
| サイトのエディタ（入力） | JSON の貼り付け・読み込みで編集を再開できる（`json-sync.ts`）。端末のアプリ側から「編集に戻す」経路は無い |
| 渡し方の実態 | パソコンで作った JSON を、端末へファイル転送（Drive・USB・メール）してから、アプリで取り込む手作業になる |

結論: Android 側の受け口は揃っているが、**サイトから端末へ「ワンタップで渡す」入口が無い**。また、サイトで作れる機能のうち、Android がまだ描画しないものがあり、作った見た目どおりに動かない場合がある。

## 2. 目標（完了条件）

1. サイトでレイアウトを作り、**スマホのブラウザから 1 タップで** Nagi に取り込める（確認ダイアログ → 保存 → そのまま使うかの確認）。
2. パソコンで作ったものは、**QR コードまたはリンクの共有**でスマホへ渡せる。
3. 取り込み後は、すぐに使える（かな / 英字への割り当てを、その場で選べる）。
4. Android が未対応の機能を含むレイアウトは、取り込み時と、サイトのエディタ上で分かる（黙って崩れない）。

## 3. 方式の選択肢

| 案 | 内容 | 長所 | 短所 |
| --- | --- | --- | --- |
| A. 「アプリで開く」リンク（`nagi://layout?data=…`） | エディタに、JSON から `nagi://layout` のリンクを作り、開く・コピー・共有するボタンを足す | Android 側は実装済み。サイト側の追加だけ。サーバー不要 | 上限 1MB（`assets` を含むものは渡せない）。アプリ未導入の端末では開けず、無反応になりやすい。ブラウザによってはカスタムスキームを開かない |
| B. https の App Links | `https://nagi.shiki-01.com/layout#data=…` を、アプリが直接開く（`/.well-known/assetlinks.json` をサイトに置き、署名鍵の SHA-256 を登録）。アプリ未導入ならそのままサイトの案内ページが開く | 通常のリンクとして共有・QR 化できる。未導入時の導線も作れる。メッセージアプリからも開ける | `assetlinks.json` の用意と、署名鍵の指紋の管理が要る。検証に端末での確認が要る |
| C. QR コード | A または B のリンクを QR にして表示する。スマホのカメラで読むと開く | パソコン→スマホの受け渡しが楽 | リンクの上限に依存。大きなレイアウトは QR にならない |
| D. 共有コード（短い ID）をサーバーに預ける | 短いコードでレイアウトを預かり、アプリがコードで取りに行く | 大きなレイアウトも渡せる | サーバーの保管が要る（現状、サイトは静的）。プライバシーと運用の負担が大きい。**採用しない** |
| E. アプリ内からサイトを開く | 本体アプリのレイアウト画面から「エディタを開く」（現状あり）。編集済みの JSON をアプリから渡して再編集する | 往復ができる | 渡し方が A/B と同じ仕組みに依存 |

**推奨: A（まず作る）→ B（App Links に拡張）→ C（QR）。** D は採らない。
理由: サーバーを増やさずに、「リンク 1 つ」で渡せるようにでき、段階的に作れる。

## 4. 作業項目（実装の順）

### 4-1. サイト（`web/`）
1. **リンクの生成**: エディタの JSON から `nagi://layout?data=<base64url>` を作る関数（`LayoutURL` 相当。iOS の `Shared/LayoutURL.swift` と同じ形式。上限 1MB、`assets` を含むものは作れない旨を表示）。単体テストを追加。
2. **エディタのボタン**: 「アプリで開く」「リンクをコピー」「QR を表示」を `JsonPanel` に追加。スマホのブラウザなら「アプリで開く」、パソコンなら QR を主にする。アプリが無い場合の案内（インストール手順へのリンク）を出す。
3. **互換表示**: 編集中のレイアウトが使っている機能のうち、Android / iOS が未対応のもの（§5）を一覧で警告する（保存や共有は止めない）。
4. **（B のとき）共有ページ** `/layout`: `#data=…`（フラグメント。サーバーに送られない）を読み、「Nagi で開く」ボタンと、アプリ未導入の案内を出す。`static/.well-known/assetlinks.json` を追加（パッケージ名 `com.shiki01.nagi`、リリース署名鍵の SHA-256 指紋）。

### 4-2. Android（`frontend-android/`）
1. **受け口の確認**: `nagi://layout` は実装済み。B のときは、`https://nagi.shiki-01.com/layout` の intent-filter（`android:autoVerify="true"`）を `LayoutManagerActivity` に追加し、フラグメントの `data` を読む。
2. **取り込み後の導線**: 保存に成功したあと、「かな入力で使う」「英字入力で使う」「あとで」を選ぶダイアログを出す（いまは管理画面で割り当てる必要がある）。
3. **未対応機能の通知**: 取り込み時の確認ダイアログに、「この端末では次の指定は反映されません: 見た目（look）、ツールバー、…」を出す（`LayoutParser` が検出した未対応の項目を返す）。
4. **（任意）アプリ内 QR 読み取り**: カメラ権限が要るため、標準のカメラアプリで足りる間は見送る。

### 4-3. 描画の対応範囲（別途）
サイトで作れるのに Android が描画しないもの: `look` / `roles` / `rules` / `states` / `metrics` / `background` / `assets`（キーの見た目）、`toolbar`、`visible`、Iconify のアイコン、クリップボード履歴（`history`）。優先度は、見た目（`look` 系）> `toolbar` > Iconify の順を案とする（`docs/ios-keyboard-layout.md` の実装の段階に合わせる）。

## 5. 確認が必要な点（決定事項）

- 署名鍵の SHA-256 指紋を `assetlinks.json` に置くことの可否（公開情報。問題ないはずだが、鍵を変えると更新が必要）。
- カスタムスキームを開けないブラウザ（アプリ内ブラウザなど）でのフォールバックの出し方。
- 大きなレイアウト（上限超え・`assets` あり）は、ファイルで渡す手順の案内にとどめるか。
- iOS と共通の `LayoutURL` の仕様を、サイトと両アプリで 1 か所に定義するか（`docs/ios-keyboard-layout.md` に追記する案）。

## 6. 検証

- サイト: リンク生成・復元の単体テスト、プリセット全件で往復（生成→`LayoutParser`）が通ること。
- Android: 実機で、スマホのブラウザからリンクを開く → 確認ダイアログ → 取り込み → 割り当て → キーボードで反映、までの手順。
- App Links（B）: 検証が通るか（`adb shell pm get-app-links`）と、未導入端末でサイトの案内が出るか。

## 7. 規模の目安

- A（サイトのリンク生成 + ボタン、Android の導線とダイアログ）: 小〜中
- B（App Links、共有ページ）: 中
- C（QR）: 小（ライブラリは、サイトの既存の依存に合わせて選ぶ）
- 描画の対応範囲の拡大（4-3）: 大（別計画）

## 8. 実装状況（2026-11-03）

| 方式 | 状況 |
| --- | --- |
| A. `nagi://layout` のリンク | **実装済み**。`web/src/lib/keyboard/layout-url.ts`（生成・復元。上限 1MB、`assets` ありは作らず理由を返す。往復テストは `layout-url.test.ts`）。`JsonPanel` に「アプリで開く」「リンクをコピー」「QR を表示」。Android 側の受け口は従来どおり |
| B. https の App Links | **実装済み（実機での検証は未了）**。`LayoutManagerActivity` に `https://nagi.shiki-01.com/layout`（`autoVerify`）の intent-filter。`LayoutLink.parse` がフラグメント / クエリの `data` を受け、`nagi://` と同じ経路（確認ダイアログ → 取り込み → 割り当て）へ。共有ページ `web/src/routes/layout/+page.svelte`（フラグメントをブラウザ内だけで読む）。`android-apk.yml` が release 鍵のときだけ `assetlinks.json` を生成して Release に添付し、`web.yml` が最新の公開リリースから `/.well-known/assetlinks.json` に取り込む |
| C. QR | **実装済み**（サイト側。npm `qrcode`、白地に黒の SVG。共有ページの https リンクを QR にする）。データが QR の容量を超えるときは理由を表示 |
| 未対応機能の通知 | Android: `LayoutCompat.unsupported` を `LayoutImporter.inspect` の `LayoutCandidate.unsupported` に入れ、確認ダイアログに表示。サイト: `compat.ts`（Android / iOS 別の警告。純粋関数） |

### 残り

- アプリ内の QR 読み取り（4-2 の 4）: 標準のカメラアプリで足りる間は見送り。
- 描画の対応範囲の拡大（4-3）。Android が対応した項目は、`LayoutCompat.UNSUPPORTED`（Kotlin）と `compat.ts` の `UNSUPPORTED.android` から外すこと（2 か所を揃える）。
- 初回リリースの手順: release 鍵で署名した公開リリース（prerelease でない `android-v*`）を作る → `web` ワークフローを手動実行（assetlinks.json が公開される）。
- **assetlinks の検証結果の確認**: 公開後に `https://nagi.shiki-01.com/.well-known/assetlinks.json` を開いて指紋を確認し、実機で `adb shell pm get-app-links com.shiki01.nagi` を実行する（`nagi.shiki-01.com` が `verified` なら成功。`none` / `1024` などのときは、`adb shell pm verify-app-links --re-verify com.shiki01.nagi` のあとにもう一度確認する）。署名鍵を変えたら assetlinks.json も更新される（次のリリースで再生成）。
- 実機の確認: スマホのブラウザで共有リンクを開く → 確認ダイアログ（未対応の指定の一覧を含む）→ 取り込み → かな / 英字への割り当て。
