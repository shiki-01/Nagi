# 実機（Windows / macOS）テストチェックリスト

このセッション（クラウドのLinuxサンドボックス）で実装した機能は、実データ（Mozc辞書・zenzモデル）も
実OSも無い環境で開発した。GitHub Actions は ubuntu/windows/macos の3OSでビルド・テストしているが、
`--features llama`（実LLM推論）は既定でCIに含めていないため、以下は実機でしか確認できない。

各項目の実装根拠・設計判断は `docs/decisions.md` の該当G番号を参照。

## 0. 準備

```sh
git clone https://github.com/shiki-01/nagi.git
cd nagi/core
cargo xtask fetch-dict          # Mozc OSS辞書を取得（SHA-256検証あり）
cargo xtask build-dict          # data/dict/nagi-dict.bin を生成
```

LLM（ビームサーチ・reconvertの品質確認等）も試す場合、追加で:

```sh
cargo xtask fetch-model         # zenz-v3.1-small を取得（SHA-256検証あり）
```

`--features llama` でビルドするには cmake と LLVM/clang（`LIBCLANG_PATH`）が必要（Windowsは
LLVM本体のインストールが必要な場合がある。macOSは `xcode-select --install` で足りるはず）。

## 1. ビルド・テストの基本確認（実機で初めて走らせる意味がある）

```sh
cargo fmt --all --check
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CIはubuntu/windows/macosの3OSで既に通っているはずだが、**実データ（`data/dict/nagi-dict.bin`）が
無い環境で走るCIでは、実データ依存のテストは自動でskipされている**（`dictionary_backed_conversion_end_to_end`
等、テスト内で `data_dir()` が `None` なら早期return）。実機で辞書を取得した状態で同じコマンドを
実行すると、これらのテストが初めて実際に走る。

LLMも含めて確認する場合:

```sh
cargo test -p nagi-infer -p nagi-server --features llama
```

## 2. ライブ変換（G-25・G-26・G-27）

`imserver` は標準入出力でJSON-RPCを受け付ける。手元で対話的に試す一番簡単な方法は、
別ターミナルで `imserver` を起動し、標準入力に手でJSON-RPC行を打つか、簡単なスクリプトで送る:

```sh
cargo run -p nagi-server --bin imserver -- --data-dir ../data --user-dir /tmp/nagi-test
```

（`--user-dir` を指定して実ユーザーの学習データ・設定を汚さないこと。LLMも試すなら
`cargo run --release -p nagi-server --features llama --bin imserver -- --data-dir ../data --user-dir /tmp/nagi-test`）

確認したいこと:

1. `initialize` → `process_key` でかな2文字以上を打った後、**何も送らずに**約228ms
   （既定の `live_conversion_delay_ms`）待つと、`update_preedit` 通知が自発的に出力されること。
   1文字だけでは出ないこと（既定 `live_conversion_min_key_length = 2`）。
2. ライブ変換の変換結果が、同じ読みをSpaceで変換した結果と比べて違和感が無いか（実データでの
   変換品質。辞書のみ/LLM併用の両方で確認できると良い）。
3. 高速に連続してキーを打ったとき（1文字ごとにデバウンススレッドが1本立つ設計）、CPU負荷・
   メモリが問題にならないか（タスクマネージャー/アクティビティモニタで `imserver` プロセスを見る）。
4. 60文字を超える長い読みを一度に打ったとき、ライブ変換の反映が遅く感じないか（`composing_chunk_len`
   でチャンク分割しているが、「編集されたチャンクのみ再変換」という最適化はまだ無いため、
   長文では毎回チャンク全体を再変換している）。

## 3. ビームサーチ（文全体N-best、G-30）— `--features llama` 推奨

`conversion.initial_view = "sentence"`（既定）で、同音異義語を含む文を打ってSpaceを1回押す:

1. 候補窓がその場で開き、複数の文全体の変換結果が並ぶこと（`conversion.initial_view = "segment"`
   に設定すると従来どおり候補窓が出ないことも確認できると良い）。
2. **1番目の候補が、LLM併用時の通常のSpace変換（1-best）と同じ表記になっていること**
   （辞書コスト順ではなくLLM再採点済みの結果を1番目に揃える設計にした。ここが辞書コスト順に
   戻っていたら品質の後退なので要報告）。
3. 2番目以降の候補（辞書コスト順、LLM再採点なし）の並びが不自然に感じられないか。
4. 候補を選んだ状態で矢印キー（MoveFocus）を押すと、その候補の文節分割に展開されて複数文節の
   通常編集に戻ること。

## 4. 確定取消 Ctrl+Backspace（G-28）

**フロント（macOS/Windows）が無いため、ホストアプリのテキストを実際に削除する動作はまだ無い。**
`imserver` に直接JSON-RPCを送って、エンジンの機構だけを確認する:

1. かな入力→Enterで確定
2. 直後に `Ctrl+Backspace`（`keysym` は BackSpace=0xff08、`modifiers.ctrl=true`）を送る
3. 応答に `delete_before`（削除すべきUTF-16長）が入り、`preedit` が確定前の状態に戻ること
4. 確定後に他のキー（何でも良い）を1回挟んでから `Ctrl+Backspace` を送ると、何も起きず
   素通し（`consumed: false`）になること

## 5. reconvert（G-29）

`reconvert` RPCに `{"session_id": "s", "text": "<確定済みの表記>"}` を送る:

1. 一般的な単語・文で、辞書の逆引きによる読みの推定がどの程度使えるか（辞書のみ、LLM補完なし）
2. 辞書に無い表記（固有名詞・当て字等）を渡したときにクラッシュしないこと（`reading` が
   正しくなくても編集を続けられればOK）
3. **`reconvert` を初めて呼んだ直後のメモリ増加量**（逆引き索引を辞書全体から構築する）。
   **2026-09-28にWindows実機で実測済み: 約270MB**（見積もりの「数十MB」は誤りだった。
   `docs/decisions.md` G-29 追記参照）。`minimal`/`light` プロファイルでは使用後に自動解放
   するよう対応済み（`Dictionary::release_reverse_index`）。macOSでも同様に、タスクマネージャー
   相当（アクティビティモニタ）で `imserver` のメモリを `reconvert` 前後・解放後で比較し、
   同じ傾向になるか確認してほしい。

## 6. アップデート署名検証（G-31）

`config/update-public-key.txt` の公開鍵は本番用（2026-10-22 に差し替え済み）。配布フローは
`macos-release.yml`（H-37）。リリース前に実機で試せるのは暗号ロジック自体の単体テスト:

```sh
cargo test -p nagi-server update_check
```

ed25519の実装（`ed25519-dalek`）はOS依存のCライブラリを使わない純Rustのはずなので、Win/Macでも
問題なく通るはずだが、実際に確認してほしい。

## 7. その他

- 標準IME（macOS標準／MS-IME／Mozc）との挙動照合（ゴールデンテストの期待値は設計書から書いたもので、
  標準IMEとの実際の照合はまだ行っていない）
- レイテンシベンチ（p95）: 設計書の目標値（キー入力→ひらがな反映 p95 < 16ms 等）を実機で計測できると良い
- `cargo xtask fetch-model` でモデルを取得した状態で `cargo run --release -p nagi-server --features llama --bin nagi-eval -- --data-dir ../data` を実行すると、精度・レイテンシの計測結果が出る（G-11の数値は評価環境（クラウドサンドボックス）でのもので、実機での再計測が無い）

## 8. Windows フロント（TSF、H-39〜H-110）— 0.1.14 の実機で主要機能を確認済み（2026-11-20）。残りは下の未確認一覧

> **実機確認の状況（2026-11-20、ユーザー報告・0.1.14）**: 次の項目は Windows 実機で動作を確認した（根拠はユーザー報告。ログ・計測値は未取得）:
> 導入（`install.ps1`）、設定 GUI、候補の重複・位置、プロファイル判定、キー応答、モード切替、ライブ変換、右クリックメニュー、imserver の寿命管理、スタートメニューのショートカット。
>
> **未確認の一覧（実機）**: 自動更新（H-95。取得・署名検証・UAC 1 回での適用・トースト）、更新時の旧エンジン停止（`imserver.exe.old` が残らないこと）、`uninstall.ps1`、標準ユーザーでの動作、VC++ 再頒布可能パッケージなしの環境、ARM64（H-106）、SmartScreen の出方、曖昧入力の Windows 実機（§9）。
> 2026-11-21 に追加した H-102・H-104〜H-108 も実機未確認（§8.5 の候補の先頭位置、§8.8）: 最初の 1 文字の候補位置、文書切替・キャレット移動での確定、`ITfFnConfigure`（言語のオプションから設定 GUI）、テーマ追従、UILess、モード切替のキャレット付近表示、ARM64 機での導入・入力。
> 加えて、下の個別項目のうち `[ ]` のものは個別には未確認（上の「確認した」項目に含まれる範囲は個別の `[x]` に反映した）。

> **E2E による自動検証（2026-11）**: `windows-e2e` ワークフロー（windows-latest）で、SendInput → TSF → IME → 入力欄の経路を通した
> E2E 17 件が成功している（[`docs/ui-testing.md`](ui-testing.md)）。この E2E は標準の EDIT コントロールをホストにした CI 上の検証で、
> 実アプリ（ブラウザ・Office・チャット等）での確認の代わりにはならない。自動化済みの項目には「E2E 済」と注記した。
> なお E2E が `key_id` のリピート回数不一致による 1 キー 2 回評価の不具合を検出し、修正している（下記 8.3 の `OnTestKeyDown` の項目）

`frontend-windows/`（`nagi-windows`）と `frontend-windows-tsf/`（`nagi-windows-tsf`）は Linux 上で fmt / clippy / test と
`x86_64-pc-windows-gnu` の `cargo check` / `clippy` が通り、CI の E2E（標準 EDIT ホスト上、上記）でも動いたが、
実アプリでの Windows 実機検証は 0.1.14 で主要機能のみ実施した（上記。ブラウザ・Office・UWP などの個別確認は未了）。
以下は Windows 10 22H2 以降 / 11 (x64) の実機で確認する項目。確認したら結果と日付を書き込むこと。
検証の進め方・コマンド・記録方法は [`docs/windows-verification.md`](windows-verification.md)。

### 8.1 ビルド・単体テスト
- [ ] `cd core && cargo test -p nagi-windows -p nagi-windows-tsf` が Windows ホストで通る
- [ ] `cargo build --release -p nagi-windows-tsf` で DLL（cdylib）ができる（MSVC ターゲット）
- [ ] `nagi-windows` の `process.rs`（`ProcessTransport`）のテストが Windows で通る（Linux では `cat` / `true` を使う `cfg(unix)` テスト。Windows 側は未検証）
- [ ] 子プロセス起動時に `CREATE_NO_WINDOW` でコンソールウィンドウが出ない

### 8.2 登録・読み込み
- [x] 管理者権限の `regsvr32 nagi_windows_tsf.dll` が成功し、設定の「言語と地域」に入力方式として出る（E2E 済: 登録・Activate。実機: 0.1.14 の導入で入力方式として使えた。ユーザー報告 2026-11-20）
- [ ] `regsvr32 /u` で登録が消える（レジストリに残骸がない）
- [ ] ホストアプリ（メモ帳・ブラウザ・VS Code・Office）で DLL が読み込まれ、アンロード（`DllCanUnloadNow`）でホストが落ちない（E2E 済: 標準 EDIT ホスト。実アプリのうち入力できたものは 0.1.14 で動作したが、アプリごとの確認は未記録）
- [x] DLL と同じディレクトリの imserver が起動する（配置は `%ProgramFiles%\Nagi`、H-42）。データは `%APPDATA%\Nagi\data`（E2E 済: imserver の自動起動。実機: 0.1.14 で確認、ユーザー報告 2026-11-20）

### 8.3 キー入力・composition
- [ ] ローマ字入力でひらがなが composition として表示され、Enter で確定、Esc で取り消せる（E2E 済: 標準 EDIT ホスト上。実アプリでは未確認）
- [ ] Space 変換 → 文節移動（←/→）→ 確定が通る（E2E 済: Space 変換・候補選択・Esc。文節移動は未カバー）
- [ ] `OnTestKeyDown` だけが呼ばれて `OnKeyDown` が来ない場合の挙動（エンジン状態が先に進む問題）の発生頻度（E2E で、test と実キーの lParam のリピート回数が異なり結果の引き継ぎが外れる不具合を検出・修正済み。実アプリでの発生頻度は未確認）
- [ ] 入力 250ms のタイムアウト（実測なしの暫定値）が妥当か。重い変換でもキーを取りこぼさない
- [ ] エンジン未接続時の素通し（composition 中だけ食う）で入力が壊れない。imserver を強制終了しても自動再起動し、`restore_session` で preedit が復元される
  - 確認方法: `verify.ps1 crash`（連続 6 回で minimal へ入るかは `verify.ps1 crash -Repeat 6`）。手順は `docs/windows-verification.md`「堅牢性・環境の簡単な検証」
- [ ] スリープ復帰後の再起動
  - 確認方法: `verify.ps1 sleep` → スリープ → `verify.ps1 sleep -After`（`[engine] restart requested (resume: …)` の検出経路と二重起動なしを判定）
- [x] ~~ライブ変換・LLM 結果の差し替えが「次のキー入力まで画面に出ない」~~ → H-41 で隠しウィンドウ + PostMessage により解消（実機の確認は 8.5b）
- [ ] キャレット位置の設定（`ShiftEnd`）。確定後にカーソルが文末に来る
- [x] **右クリックメニュー（H-96。0.1.14 の実機で確認、ユーザー報告 2026-11-20）**: 入力インジケーター（「あ」）を右クリックすると、ひらがな / 英数・状態の行（グレー）・設定を開く・フォルダ・verify・（更新があれば）更新を適用 が出る。1 回の右クリックでメニューは 1 つだけ（3 重の `OnClick` で重ならない）。項目を選ぶと動作し、外側をクリック / Esc で閉じる。閉じた後、入力中のアプリのフォーカスが失われない。tsf.log に `[langbar] popup menu shown entries=N` と `selected id=N` / `dismissed`
- [x] **imserver の数（H-96。0.1.14 の実機で確認、ユーザー報告 2026-11-20）**: タスクマネージャー（詳細）の imserver が、Nagi に切り替えて入力したアプリの数だけに限られる（入力しないアプリ・バックグラウンドの WebView2 等では増えない）。入力したアプリを閉じると対応する imserver が消える（ホストを強制終了しても消える）。`verify.ps1 status` で孤児（親が存在しない）の WARN が出ない
- [ ] 更新を適用した後（`apply-update.ps1`）、`imserver.exe.old` のプロセスが残らない（旧エンジンは止められ、新しい exe で自動再起動する）

### 8.4 キー割り当て
- [ ] 半角/全角（`VK_KANJI` / `VK_OEM_AUTO` / `VK_OEM_ENLW`）でモードが切り替わる
- [ ] 変換（`VK_CONVERT`）・無変換（`VK_NONCONVERT`）・カタカナひらがな（`VK_KANA`）の動作
- [ ] `VK_IME_ON` → HIRAGANA、`VK_IME_OFF` → EISU_TOGGLE の割り当てが OS 側の IME オン/オフと食い違わない
- [ ] `VK_OEM_BACKTAB` が実際にどのキーに当たるか（資料が乏しい。HIRAGANA_KATAKANA に割り当てた判断が妥当か）
- [ ] JIS / US 配列の両方。AltGr 配列で `@` などの記号が失われない（Ctrl+Alt 併用時に text を残す判断）
- [ ] 修飾キー単独の押下・離上（`flags_changed`）。押下を食ったキーの離上を食わない挙動

### 8.5 OS 非依存ロジック（H-40。Linux の単体テスト済み。実機の値・見た目で確認する）
- [ ] 候補ウィンドウの位置（**H-102 で決め方が変わった**: 先頭 → 全体 → 選択範囲 → 直前に測れた矩形 → マウス位置。下の H-98・H-102 の項目を優先して確認する）: キャレット行の下に出る。画面下端（タスクバーの上）では上に反転する。複数モニター・DPI 違い・縦に長い候補で作業領域からはみ出さない
- [ ] 候補ウィンドウの寸法: 文字幅の計測（`TextMeasure`）と行の高さ・余白が見た目で窮屈でない（DPI 倍率の掛け方）。ページ表示「2/5」の位置
- [ ] ページング: 候補が `page_size` を超えたとき、選択に追従してページが替わる。数字キーで選べる（キーの解釈はコア）
- [ ] 出所ラベル（学習・AI・辞書・もしかして等）の表示
- [ ] キャレット矩形が取れないアプリでマウス位置に出る（`anchor_rect`）
- [ ] 候補ウィンドウが入力中の文字列（composition）の**左端**に出る（キャレット位置ではない）。複数行にまたがる長い入力でも、最初の行の下に出る（H-98）。メモ帳・ブラウザ・Office で確認
  - 確認方法: `tsf.log` の `[cand] anchor source=start|range|caret|mouse`（由来が変わったときだけ出る）。`start` が出ていれば先頭の点範囲で測れている。アプリによって `range` / `caret` / `mouse` に落ちる場合は、そのアプリ名と見え方を記録する
- [ ] 最初の 1 文字から候補ウィンドウが入力位置に出る（マウス位置に出ない）（H-102）。メモ帳・ブラウザ・Office で確認。ちらつき（マウス位置 → 入力位置への移動）が見えない。`tsf.log` の `[cand] anchor remeasure source=…` が最初の 1 文字で出る頻度。非同期セッションが走らないアプリで保留した候補が出ない場合は要再検討
- [ ] 学習した語が候補に重複して並ばない（読み「だいたい」など、学習・AI・辞書の出所が 1 行に併記される）（H-98）
- [ ] キー応答のタイムアウトが実機で起きるか: `tsf.log` の `[key] engine error kind=timeout composing=… elapsed_ms=…` を確認する（1 秒あたり最大 1 行。起きるなら頻度と状況を記録し、`KEY_TIMEOUT` 250ms の要否を判断する）
- [ ] `set_environment`: AC/電池の切替、バッテリー節約の切替で `get_status` のプロファイルが変わる。電池の無いデスクトップ PC で電池駆動と誤判定されない
  - 確認方法: `verify.ps1 env battery` / `saver`（検証専用の `debug-env.json` で擬似。`tsf.log` の `[env] sent … source=override` と言語バーの「プロファイル: …」を見る。終了時に `env clear`）。実際の電源切替は従来どおり実機で
- [ ] メモリ逼迫の推定閾値（Warning: 利用可能 <600MB または総メモリの 4%〈上限 1000MB〉、Critical: <250MB または 2%〈上限 500MB〉。改善は閾値の 125% を上回ってから。H-99）が妥当か（実測なし）。搭載 16GB・利用可能 1.3GB で light に落ちない。メモリを圧迫したときだけ light/minimal に落ち、境目で解放と再読み込みを繰り返さない（imserver ログの `プロファイル A → B（理由）`・`モデル解放`・`モデル再読み込み` で確認）
  - 確認方法: 擬似は `verify.ps1 env memory-warning` / `memory-critical`（閾値そのものの妥当性は実メモリで別途確認）
- [ ] サーマルの推定（周波数 / 上限の比 85%/60%/40%）。バッテリー節約中や省電力プランで誤って軽いプロファイルに落ちない（節約が有効または不明なら推定しない仕様）
  - 確認方法: 擬似は `verify.ps1 env thermal-serious`（推定の閾値そのものは検証できない）
- [ ] `set_context`: 入力開始時にキャレット直前のテキストが送られ、次語候補・文脈が効く。取得できないアプリで前回の文脈が維持される。パスワード欄（`InputScope` の `IS_PASSWORD` = 31 が `windows` クレートの定数と一致するか）で周辺テキストを読まず `secure_input: true`、学習・ログが止まる
  - 確認方法: `verify.ps1 password`（ブラウザ（Edge / Chrome）のパスワード欄。`tsf.log` の `[ctx] secure_input=true app=msedge.exe` が出て、メモ帳の通常欄では出ない）。WinForms 等の標準 EDIT の ES_PASSWORD は Windows が IME を無効にするため判定できないのが正常（H-94）
- [ ] 表示属性: 通常 = 細い実線、注目文節・選択 = 太い実線、確定済み = 点線、スタイル変換 = 破線、保留ローマ字 = 点線。ホストアプリが属性を無視する場合の見え方（MS-IME 風に未変換 = 点線へ変える要否）
- [ ] 通知の文言（エンジン停止・minimal 移行・復元失敗・update_available）と 30 秒間引き、モード表示（あ / A）

### 8.5b OS 層（H-41。型検査のみ。一部のみ実機確認済み）
- [ ] 候補ウィンドウ: Space 変換で表示され、**フォーカスを奪わない**（入力中のアプリのキャレットが消えない・タイトルバーが非アクティブにならない）。選択行のハイライト、番号、出所ラベル、ページ「2/5」が崩れない。DPI 100/150/200% と、DPI の違う複数モニター（Per-Monitor 対応ホストと非対応ホストの両方）で大きさ・位置が合う。タスクバー付近で上に反転する
- [ ] キャレット矩形（`GetTextExt`）: メモ帳・ブラウザ・VS Code・Office・ターミナル・UWP で候補ウィンドウが composition の直下に出る。取れないアプリでマウス位置に出る
- [ ] 非同期通知の即時反映: ライブ変換・LLM 結果の差し替えが次のキーを待たずに画面に出る。差し替え直後のキー入力と順序が崩れない（`TF_ES_ASYNCDONTCARE` の編集セッションと同期セッションの順序）。通知が別ウィンドウ（別スレッド）の入力に混ざらない
- [ ] スリープ復帰: 復帰後にエンジンが再起動し、`restore_session` で preedit が復元される（メッセージ専用ウィンドウの `WM_POWERBROADCAST` が届くか、届かなくてもタイマーの検出が働くか）。二重に再起動しない
- [ ] 環境の報告: AC/電池・バッテリー節約の切り替えが約 30 秒以内（電源通知が届けば即時）に `get_status` のプロファイルへ反映される。`MhzLimit` を使うサーマル推定がアイドル時に誤って軽いプロファイルへ落とさない
- [ ] `set_context`: 文字入力の開始時だけ文書を読む（矢印・ショートカット・修飾キーで読まない）。パスワード欄（`InputScope` が `IS_PASSWORD`。定数 31 はコンパイル時に一致を確認済み）で周辺テキストを読まず `secure_input: true` を送る。アプリ ID に実行ファイル名（`chrome.exe` 等）が入る
- [ ] 表示属性: 通常 = 細い実線、注目文節 = 太い実線、確定済み・保留 = 点線、スタイル変換 = 破線がホストに反映される。確定後に下線が残らない。属性を無視するアプリでも入力は壊れない。`IEnumTfDisplayAttributeInfo` の終端（`S_FALSE`）で列挙するアプリが固まらない。アクティブ化されないプロセスで imserver が起動しない
- [ ] 入力モード: タスクトレイの表示が「あ / A」で切り替わる。英数キー・半角/全角キー・トレイのクリックの往復でモードが反転し続けない（`SetMode` の送りすぎ。入力中の文字列が意図せず確定されない）。英数 = IME 閉のとき、アプリ側が IME オフ扱いで困らない。Activate 直後・エンジン再起動後に OS とコアのモードが揃う
- [ ] 再変換: 確定済みテキストを選択して変換キー / かなキーで composition になる（コアの `reconvert_selection`）。Office など `ITfFnReconversion` を呼ぶアプリで動く。待ちきれないとき（初回の辞書の逆引き索引）の通知と取り消し。選択が空・200 文字超のとき何もしない
- [ ] 通知トースト: エンジン停止・minimal 移行・復元失敗・`update_available` が右下に約 4 秒出てフォーカスを奪わない。同文言は 30 秒間引かれる。背面のプロセスの通知が出ない
- [ ] アイコン: `regsvr32` で DLL と同じディレクトリに `nagi.ico` が作られ、設定の「言語と地域」とタスクトレイに「N」のアイコンが出る。`regsvr32 /u` で削除される。書き込めないディレクトリ（Program Files 以外の権限）での挙動

### 8.5c H-43 で追加した OS 層（型検査のみ。一部のみ実機確認済み）
- [ ] 言語バーのボタン: 従来の言語バー（設定の「言語バー」を有効にした環境、またはデスクトップ言語バー）に「あ / A」のボタンが出る。`ITfLangBarItemMgr` を `ITfThreadMgr` の `QueryInterface` で得られる（`LangBar::new` が失敗しない）。ホスト切り替え・Deactivate でボタンが残らない（`RemoveItem`）。トレイの「あ / A」と二重に出ないか
- [ ] **0.1.11 実機の結果**: 左クリックのモード切替は動作。**右クリックでメニューは表示されなかった**（`tsf.log` に InitMenu の記録なし）。H-94 で `[langbar] InitMenu called entries=N` / `OnClick called click=…` / `OnMenuSelect` / `GetInfo` / `GetStatus` / `Show` を記録するようにした。次の確認: 右クリック後の tsf.log に `InitMenu called` が出るか（出ない = OS が呼んでいない。dwStyle は根拠が無いので未変更）。メニューが無くてもスタートメニューの「Nagi」フォルダのショートカットで設定・ログ・verify を開ける
- [ ] ボタンの操作: 左クリックでひらがな ⇄ 英数が切り替わり、表示（アイコン・文字・ツールチップ）が更新される。右クリックでメニュー（ひらがな / 英数・現在にチェック・状態の行・「設定…」「設定ファイル（config.toml）を直接開く」ほか開く項目 6 行）が出る（`BTN_BUTTON` で `InitMenu` が呼ばれるか）。メニューの選択でモードが変わる。入力中の文字列が意図せず確定されるのは `SetMode` の仕様どおりか
- [ ] ボタンの表示とトレイ・英数キー・半角/全角キーでの切り替えの同期（往復して反転し続けない）。エンジン停止中（接続前）のメニュー（「変換エンジンに接続していません」）と、開いたときの待ち（`get_status` の同期呼び出し、最大 250ms）が気にならないか
- [ ] `GetIcon` の `HICON` を言語バーが破棄するか（長時間・多数回のモード切り替えで GDI オブジェクトが増え続けないか。タスクマネージャーの GDI オブジェクト数）。DPI 150/200% でアイコンが滲まない
- [ ] 「設定ファイル（config.toml）を直接開く」（旧「設定を開く（config.toml）…」）: `config.toml`（`get_status` の `config_path`）がメモ帳で開く。ファイルが無いときの作成の確認。`config_path` が無いとき項目が灰色
- [ ] 右クリックメニューの状態行が短い（H-103）: 「プロファイル: balanced（推論 48ms）」のように理由が短い表記で、メニュー幅が横に長すぎない。メモリ逼迫などで切り替わった後も「（メモリ逼迫）」等の短い表記。詳細な理由は `tsf.log` の `[engine] notify … profile_changed` で確認できる
- [ ] 右クリックメニューの開く項目（H-93。`ITfMenu` の平坦な項目 5 行）: 「設定フォルダを開く」でエクスプローラーが `%APPDATA%\Nagi` を開く。「ユーザー辞書フォルダを開く」が `user_dict` があればそれ、無ければ `%APPDATA%\Nagi` を開く。「ログフォルダを開く」が `%LOCALAPPDATA%\Nagi\logs` を開く。「状態を確認（verify.ps1）」が新しいコンソールで `verify.ps1 status` を実行し、終了後もコンソールが閉じない（`-NoExit`。ExecutionPolicy で止まらない）。対象のフォルダ・`verify.ps1` が無い環境では該当項目が灰色（フォルダは作られない）。開けなかったときは `tsf.log` に `[menu] open failed kind=… code=…` が出る。開く操作の前後で入力中の文字列・モードが変わらない
- [ ] **設定画面（H-97。`nagi-settings.exe`。起動と基本動作は 0.1.14 で確認済み（H-101）。全項目の確認は未了）**:
  - 右クリック → 「設定…」で既定のブラウザに設定ページが開く（黒いコンソール窓が出ない。タスクマネージャーに `nagi-settings.exe` と、その子の `imserver.exe` が 1 つずつ）。`nagi-settings.exe` が DLL と同じディレクトリに無いと「設定…」が灰色。起動に失敗したら `tsf.log` に `[menu] open failed kind=settings_gui code=…`。スタートメニューの「Nagi 設定」でも同じ画面が開く
  - 項目がカテゴリごとに並び、「シンプル / 詳細」で項目数が変わる。iOS 専用の項目（濁点の循環順・タッチ座標の利用・グライド入力）が出ない。ライト/ダーク（Windows の色設定）に追従する。Tab キーだけで操作でき、トグル・入力欄のラベルが読み上げられる
  - 項目を変えると（トグル・数値・選択・テキスト）すぐ `%APPDATA%\Nagi\config.toml` に反映される（メモ帳で開いて確認。ファイル内のコメントと他の項目が保たれる）。変更が実際の入力にも反映される（例: ライブ変換のオン/オフ。入力方式を切り替え直さずに）。「既定に戻す」で明示指定が消える
  - `requires_restart` の項目（例: 更新の自動確認）に「変更を反映するには入力方式を切り替え直すかアプリを開き直してください」のバッジが出る
  - 範囲外の値（例: ライブ変換の待ち時間に 99999）で、その項目の下にコアのメッセージが出て、`config.toml` は変わらない。`config.toml` を壊した状態で開くと、画面上部にエラーが出て値は書き込まれない
  - 「config.toml を直接開く」でメモ帳、「ユーザー辞書フォルダを開く」でエクスプローラーが開く。「学習データを全削除…」で確認ダイアログが出て、キャンセルでは消えない・「削除する」で `learning.tsv` の内容が消える
  - タブを閉じると数秒後に `nagi-settings.exe` と子の `imserver.exe` が消える。再読み込み（F5）では終了しない。15 分放置で終了する。`nagi-settings.exe` をタスクマネージャーで強制終了しても子の `imserver.exe` が残らない（`--parent-pid`）。2 つ同時に開いても別ポートで動く
  - **トークンなしのアクセスの拒否**（PowerShell。設定画面を開いたまま）: ポートを調べる `$p = (Get-Process nagi-settings | Select-Object -First 1).Id; $port = (Get-NetTCPConnection -OwningProcess $p -State Listen).LocalPort`。次はどれも 403 になる: `Invoke-WebRequest "http://127.0.0.1:$port/" -UseBasicParsing`（トークンなし。`-SkipHttpErrorCheck` が使える PowerShell 7 か、5.1 なら try/catch で `$_.Exception.Response.StatusCode`）、`Invoke-WebRequest "http://127.0.0.1:$port/?t=wrong" -UseBasicParsing`、`Invoke-WebRequest "http://127.0.0.1:$port/api/settings" -Method Post -ContentType application/json -Body '{}' -UseBasicParsing`（ヘッダーなし）、`Invoke-WebRequest "http://127.0.0.1:$port/" -Headers @{Host='evil.example'} -UseBasicParsing`（5.1 は Host を上書きできないので `curl.exe -H "Host: evil.example" http://127.0.0.1:$port/` で確認）。ブラウザのアドレスバーのトークンつき URL は 200。別のサイトのページから `fetch("http://127.0.0.1:<port>/api/settings", {method:"POST"})` が失敗する（CORS ヘッダーなし）
  - Edge / Chrome / Firefox で表示が崩れず、DevTools のコンソールに CSP 違反が出ない。`<dialog>` の確認が出る。日本語が文字化けしない
  - 設定画面を開いたまま `install.ps1`（更新）を実行しても、`nagi-settings.exe` が `.old` に退避されて置き換わる。`uninstall.ps1` が開いている設定画面を止めて削除できる
- [ ] `GetReconversion`: `ITfFnReconversion` を呼ぶアプリ（Office 等）で再変換の候補一覧が出て、選ぶと範囲が置き換わりキャレットが末尾に来る。複数文節の範囲で、選んだ候補以外の部分（残りの文節）が失われない。アプリが文書ロックを持つ最中に呼んだ場合に失敗して固まらない。呼んだ後にコアが変換中のまま残らない（直後の入力が通常どおり）。選択が空・200 文字超・入力中は失敗する。待ちきれないとき（初回の逆引き索引）の通知
- [ ] `THIRD_PARTY_LICENSES_WINDOWS.md`（112 crates。設定画面 `nagi-settings-ui` の依存を含む）が実際の Windows のビルド（MSVC・`--features llama`）の依存と一致する（`cargo tree --target x86_64-pc-windows-msvc` を Windows 上で実行して比較。llama.cpp のビルド用の crate が漏れていないか）。zip の `THIRD_PARTY_LICENSES.md` が Windows 用になっている

### 8.5d トレイのアイコンが出ない場合の確認と tsf.log（H-54。一部のみ実機確認済み）
- [ ] Windows 11 / 10 のタスクバー右端の入力インジケーターに「あ / A」（藍の角アイコン）が出る。出ない場合の確認の順序:
  1. `%LOCALAPPDATA%\Nagi\logs\tsf.log` を開く（フロントの診断ログ。入力内容は書かない。256KB で `tsf.log.1` へ回る）。`[tsf] Activate` が無ければ、Nagi が選ばれていない・DLL が読み込まれていない（`tasklist /m nagi_windows_tsf.dll`、`regsvr32` の登録、設定の「言語と地域」で Nagi が有効か）
  2. `[langbar] AddItem ok (GUID_LBI_INPUTMODE)` があるか。`AddItem failed hr=0x…` / `ITfLangBarItemMgr unavailable` ならその HRESULT を記録する。`[tsf] Activate done langbar=false` は登録失敗
  3. `AddItem ok` なのにトレイに出ないなら、アイコン側（`GetIcon` のサイズ・描画）か、シェルがこの項目を採用していない。`GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT` / `IMMERSIVESUPPORT` / `UIELEMENTENABLED` が登録されているか（`regsvr32` を管理者で再実行）、サインアウトして入れ直す
  4. 設定 > 時刻と言語 > 言語と地域 > 日本語 > 言語のオプション > キーボード で Nagi が見えるか。「入力インジケーター」をオフにしていないか（設定 > 個人用設定 > タスクバー > システムトレイ アイコン）
- [ ] 右クリックでメニュー（ひらがな / 英数・状態・「設定…」「設定ファイル（config.toml）を直接開く」）、左クリックでひらがな ⇄ 英数。エンジン起動に失敗している環境（`imserver.exe` が無い・初期化失敗）でもアイコンとメニューは出る（メニューは「変換エンジンに接続していません」）
- [ ] tsf.log の内容: `[engine] location exe=… exists=…`（imserver の置き場）、`starting` / `started …` / `launch failed: spawn failed|initialize timeout|initialize failed|…` / `stopped (crash or start failure)` / `entering minimal profile`。複数のホストプロセスが同時に書いても行が壊れない

### 8.5e imserver の起動失敗（「起動に N 回失敗したので辞書のみで起動します」）の診断（H-56。実機で一度も動かしていない）
- [ ] 修正後の版で、通常のホスト（メモ帳・エクスプローラー・Office 等）から Nagi を選んでも、起動失敗のトーストが出ずに LLM 付きで起動する（`tsf.log` に `started … profile=…` が出て `entering minimal profile` が無い）
- [ ] `%LOCALAPPDATA%\Nagi\logs\imserver-<ホストpid>.log` が作られ、`[launch] imserver launch minimal=false` の後ろに `imserver: プロファイル …` などの stderr が出ている（入力内容が出ていない）。pid は `tsf.log` の `pid=` と一致する
- [ ] それでも失敗する場合: `tsf.log` の `exited … phase=… exit_code=…` と `normal (LLM) launch failures=…` の一覧、`imserver-<pid>.log` を採取する。`docs/windows-verification.md` の「imserver の起動失敗の切り分け」の PowerShell（通常と `--no-llm` の比較、5 秒生存確認）とイベントビューアーも実施する
- [ ] ログの掃除: 21 個以上の `imserver-*.log` があるとき、古いものから消えて 20 個になる（現在のホストのファイルは消えない）。7 日より古いものは消える
- [ ] stdout に JSON 以外の行が混ざる環境では `dropped non-JSON line … len=… first=… total=…` が tsf.log に出る（内容は出ない）

### 8.6 設計の再検討事項
- imserver の寿命管理（H-96）の実機確認: ホストプロセスごとの imserver の数がタスクマネージャーで Nagi を使ったアプリだけに限られること、ホストを閉じる・強制終了すると imserver も消えること（Job Object と `--parent-pid`。tsf.log の `[engine] job assigned pid=…` / imserver ログの親終了の行）、フォーカスが当たった時点で起動する体感（最初の 1 文字が素通しにならないか）、更新後に `.old` のプロセスが残らないこと。`verify.ps1 status` の imserver の欄（パス・親 PID・孤児）で確認する。
- ホストプロセスごとに imserver を起動する方式のメモリ・起動時間。名前付きパイプ + 常駐ブローカーへの変更要否（H-39）。**判断に必要な実測項目と設計案は decisions.md H-43 §4**（ホスト 1/5/10 個のメモリ合計、`initialize` の所要、パイプ往復の増分、`learning.tsv` の取りこぼし、AppContainer からの接続可否、再接続の所要）

### 8.7 配布・インストーラ・SmartScreen・更新（H-42。Linux では PowerShell の構文と `SHA256SUMS` の検証までしか確認していない）
- [ ] `windows-release.yml` を `-f publish=false` で実行して通る（windows-latest での MSVC ビルド、`LIBCLANG_PATH`、llama.cpp のビルド、DLL の CRT 静的リンク、`bundle-windows`、`sign-update`）。インストーラのスモークテスト（regsvr32 の登録と解除）の結果を Actions のログで確認する（失敗は警告扱い）
- [ ] 展開した zip で `powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1`: `SHA256SUMS` の検証、UAC の確認、`C:\Program Files\Nagi` への配置、`regsvr32` の登録、「アプリと機能」の項目、辞書・モデルの取得（`%APPDATA%\Nagi\data`、約 150MB）が通る。標準ユーザー（UAC で別アカウントの資格情報を入力）の場合に、入力方式が使うアカウントに登録されるか
- [ ] Program Files の DLL が UWP / 保護されたアプリ（設定アプリ・Edge の一部・Microsoft Store アプリ）でも読み込まれる（`ALL APPLICATION PACKAGES` の読み取り権限）
- [ ] 更新（上書きインストール）: 入力方式の使用中に再実行して `.old` への退避が通り、サインアウト / 再起動後に新しい版にそろう。`.old` が次回に片付く。動作中の imserver（旧版）が混在して問題を起こさない
- [ ] `uninstall.ps1`: 登録が消え（`regsvr32 /u`）、ファイルが消える。使用中のファイルが再起動後に消える。`-RemoveUserData` の確認（`yes`）と `-Force`。既定ではユーザーデータが残る。「アプリと機能」の項目が消える
- [ ] `irm https://nagi.shiki-01.com/install.ps1 | iex`（PowerShell 5.1 と 7）: latest.json の取得、SHA-256 検証、SmartScreen が出ないこと。`NAGI_SKIP_DATA=1`。prerelease の間は 404 になる（想定どおり）。公開前に `web.yml` の取り込み（`static/updates/windows/latest.json`）を確認する
- [ ] ブラウザで取得した zip の SmartScreen / Defender の警告の出方（Edge・Chrome）と、`docs/windows-install.md` の回避手順（zip のブロック解除、「詳細情報」→「実行」）で実際に進める。`.ps1` の MOTW 付きでも `-ExecutionPolicy Bypass` で実行できる
- [ ] Authenticode 署名（Secrets を登録した場合）: `signtool verify /pa` が通り、署名した DLL / exe が読み込まれる。タイムスタンプの取得
- [ ] 更新の通知: 旧版の imserver が `/updates/windows/latest.json` を取得し、新版があれば右下のトーストが出る（文言「…取得中です。取得できたらスタートメニューの「Nagi を更新」から適用できます」→ 取得後「新しい版 X をダウンロードしました。スタートメニューの「Nagi を更新」から適用できます」）。`%APPDATA%\Nagi\update_check.json` による 24 時間の間引き
- [ ] 自動更新（H-95。取得・署名検証・UAC 1 回での適用。**実機未確認**）。確認用に旧版（例: 0.1.12）を入れた状態で、新しい版 X の `Nagi-windows-X.zip` と `latest.json`（`windows-release.yml` の成果物。署名は zip に対するものなので `download_url` を書き換えても有効）を 1 つのフォルダに置き、`download_url` を `http://localhost:8000/Nagi-windows-X.zip` に書き換えて `python -m http.server 8000` で配る。`%APPDATA%\Nagi\config.toml` に `[update]` `check_url = "http://localhost:8000/latest.json"` を書く（prerelease は web の `latest.json` に載らないため、本番の URL では試せない。`prerelease=false` で公開すれば本番の URL でも確認できる）。`%APPDATA%\Nagi\update_check.json` を消して（24 時間の間引きを外す）、メモ帳などで Nagi を使って imserver を起動する。
  - [ ] `%LOCALAPPDATA%\Nagi\update\` に `Nagi-windows-X.zip` と `ready.json` ができる（`.tmp` が残らない）。再度 imserver を起動しても再取得しない（ログに取得の記録が増えない）。トースト「新しい版 X をダウンロードしました…」
  - [ ] 入力インジケーターの右クリックに「更新を適用（X）…」が出る。取得前・適用後は出ない
  - [ ] スタートメニューの「Nagi を更新」または右クリックの項目で、UAC が 1 回出て、検証 → 展開 → 配置 → 完了メッセージ「サインアウトまたは再起動で全アプリが新しい版になります」。`C:\Program Files\Nagi\VERSION` が X、「アプリと機能」の版が X、`staging` と `update\` の `ready.json`・zip が消える。サインアウト後に新しい版（`Get-Process imserver | Select Path`）
  - [ ] 失敗系: zip を `update\` に置いたまま 1 バイト書き換えて適用 → 「更新を検証できませんでした」で中止し、何も置き換わらない。`ready.json` の `version` を現在以下にして適用 → 「更新がありません / 新しくありません」で中止。旧い署名済み zip を `Nagi-windows-<大きい版>.zip` の名前で置き `ready.json` の版も大きくして適用 → 「zip の版が一致しません」で中止。UAC で「いいえ」→ エラーで終わり、何も変わらない
  - [ ] **NG のときに出すログ**: `%LOCALAPPDATA%\Nagi\logs\imserver-*.log`（取得の失敗は「更新の取得に失敗しました」）、`%LOCALAPPDATA%\Nagi\logs\tsf.log`（`[menu]` の行）、`apply-update.ps1` を実行した PowerShell の画面の全文（`Start-Process` の失敗や検証の理由）、`%LOCALAPPDATA%\Nagi\update\ready.json`、`dir "C:\Program Files\Nagi"`
- [ ] VC++ 再頒布可能パッケージが無い環境（クリーンな Windows）で、DLL（静的 CRT）は読み込まれ、imserver は警告のうえ起動しない / する（imserver は動的 CRT）
- [ ] ARM64 Windows（エミュレーション）で x64 の DLL がホストに読み込まれるか（現状は未対応の扱い。`install.ps1` は ARM64 を拒否する）

### 8.8 文書フォーカス・テーマ・UILess・モード表示・ARM64（H-104〜H-108・H-106。Linux では単体テストと型検査のみ。実機未確認）
- [ ] 文書切替で確定（H-104）: Chrome で入力中（未確定）に別タブの入力欄をクリックすると、元の欄で確定される（未確定のまま残らない・別の欄に入らない）
- [ ] キャレット移動で確定（H-104）: メモ帳で入力中（未確定）に前の位置をクリックすると確定される（MS-IME と同じ）
- [ ] 勝手に確定しない（H-104）: 入力中の矢印キー・Space 変換・候補切替・文節移動・再変換で、意図せず確定されない
- [ ] 二重 `focus_out` が無い（H-104）: Alt-Tab でアプリを切り替えても `tsf.log` に `focus_out` が連続して出ない（`FocusTracker` による冪等化）。背景アプリの文書変化で `focus_in` が送られていないか（残課題）
- [ ] 言語の設定 → Nagi の「オプション」から設定 GUI が開く（`ITfFnConfigure`。入力方式が未アクティブでも開く）。オプションの項目が出ない場合は `ITfFunctionProvider` が `ITfFnConfigure` を返していない（H-104 の残課題）
- [ ] テーマ追従（H-105）: ライト / ダーク / ハイコントラスト（設定 → アクセシビリティ → コントラスト テーマ）を切り替えると、候補ウィンドウとトーストの色が**再起動なしで**変わる。ライトの選択行（#0066CC）の白文字が読める。HC では補助文字もウィンドウ文字色
- [ ] UILess（H-107）: UILess に対応するアプリ（`ITfUIElementMgr` を使うアプリ）で自前の候補ウィンドウが出ず、アプリ側の候補で選択・確定・取り消しできる。`tsf.log` に `[ui] BeginUIElement show=false` が出る。UILess でないアプリでは従来どおり出る
- [ ] モード切替の表示（H-108）: 英数 / 半角全角キー・言語バーのボタン・メニューでモードを変えると、キャレット付近に「あ / A」が 1 秒出る。**出ない**こと: フォーカス切替直後、エンジン再起動後、他の IME から切り替えた直後、UILess 中。言語バー操作で出ない場合はコンパートメント通知が非同期（H-108 の残課題）
- [ ] ARM64（H-106）: ARM64 の Windows 11 で `Nagi-windows-arm64-<ver>.zip` の `install.ps1` が通り、ARM64 ネイティブのアプリ（メモ帳など）で入力できる。x64 版の zip を ARM64 機で実行すると拒否される（`ARCH` 不一致）。**x64 エミュレーションのアプリでは Nagi が出ない**（ARM64X 未対応。既知の制約）。自動更新が `latest-arm64.json` を見る

## 9. 曖昧入力 H-88・H-89（macOS / Windows。コア `0.1.10` 以降）— 未検証

曖昧入力はデスクトップでは既定 OFF。評価の数値は `docs/decisions.md` H-88・H-89。iOS の項目は `docs/ios-device-checklist.md` §5.8。

### 9.0 準備

- 実ユーザーのデータを汚さないよう、`--user-dir` か `NAGI_CONFIG_DIR` を使う（§0）。macOS は `~/Library/Application Support/com.shiki.nagi/`、Windows は `%APPDATA%\Nagi\` が既定の置き場。
- `config.toml` に次を書く（変更は再読込される。反映されない場合は入力ソースを切り替えて戻す）:
  ```toml
  [input.fuzzy]
  enabled = true
  strength = "normal"   # 9.5 で gentle / aggressive も試す
  ```
- フロントを使わず `imserver` を直接動かす場合は `docs/progress.md` の起動コマンドを参照し、JSON-RPC の呼び出し順は `core/server/tests/suppress.rs`・`tests/fuzzy_stats.rs`・`tests/retro_review.rs` を参照。

### 9.1 もしかして候補

- [ ] 濁点・小書きの付け忘れ（`ありかとう`・`おねかいします`）で、Space 変換の候補に出所「もしかして」（macOS は候補ウィンドウのラベル、Windows は候補ウィンドウの表示）が出る。選ぶと補正後の読みで確定する。
- [ ] 辞書にある普通の語・文を打っても「もしかして」が出ない（20 文で 0〜2 件）。
- [ ] ローマ字入力の打ち間違い（`kamji`→感じ）が出る。

### 9.2 誤り辞書

- [ ] 同梱リスト: `ありがとごさいます`・`やぱり`・`びょいん` で、先頭の「もしかして」に正しい変換が出る（`config.toml` で `misspellings = false` にすると出ない）。
- [ ] 利用者の追記: `<user-dir>/misspellings.tsv` に `てすとよみ<TAB>てすとただしい` の形式（UTF-8、タブ区切り。3〜24 文字のひらがな）を書き、再読込後に出る。不正な行（タブ無し・ひらがな以外）を書くと行番号つきの警告通知（`misspellings_warning`）が出て、他の行は有効のまま。

### 9.3 抑制語（フロントの UI は未実装。imserver の JSON-RPC で確認）

- [ ] `suppress_candidate{reading, surface}` を送ると、その読みの Space 変換候補・文全体の候補からその表記が消える。読みそのまま（ひらがな）の候補は消えない。`list_suppressed` に載り、`<user-dir>/suppressed.tsv` に保存される。`unsuppress_candidate` で戻る。
- [ ] `suppressed.tsv` を手で編集して再読込しても反映される。学習の全削除では**消えない**。

### 9.4 個人化と統計

- [ ] 「もしかして」を何度か採用/無視したあと、`get_fuzzy_stats` が種類ごとの `shown`/`adopted`/`margin_scale` を返す。`<user-dir>/fuzzy_stats.json` に**読みや表記が含まれない**。`personalize = false` で増えない。学習の全削除（`clear_learning`）でファイルが消える。
- [ ] secure input（パスワード欄）では件数が増えない。

### 9.5 強さ

- [ ] `gentle` / `normal` / `aggressive` で 9.1 を繰り返し、出る数と誤表示が gentle < normal < aggressive の順になる（誤表示は aggressive でも 20 文で数件まで）。

### 9.6 確定後の見直し（`retro_review`。フロントの表示は未実装）

- [ ] `retro_review = true` にして `すうせい`（辞書にない語の連なり）を変換・確定し、続けて `する` と打つと、`retro_review` 通知（`candidates` に「しゅうせい」など）が届く。`accept_retro_review{index}` が置換テキストを返す。確定を `UndoCommit`・カーソル移動・フォーカス喪失で挟むと通知が出ない。**期待:** クリーンな文では通知がほぼ出ない。出ない場合は結果を記録（評価では正解を含められた率は 10〜25%）。

### 9.7 全体

- [ ] 上記の間、入力の体感の遅さ・クラッシュ・`imserver` の再起動が無い。Windows は §8 の項目も併せて確認する。
