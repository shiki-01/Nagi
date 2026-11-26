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

## 8. Windows フロント（TSF、H-39〜H-142）— 0.1.14（2026-11-20）・0.1.16・0.1.17（2026-11-21）・0.1.24・0.1.26〜0.1.28（2026-11-22）の実機で主要機能を確認済み。残りは下の未確認一覧

> **実機確認の状況（2026-11-20、ユーザー報告・0.1.14）**: 次の項目は Windows 実機で動作を確認した（根拠はユーザー報告。ログ・計測値は未取得）:
> 導入（`install.ps1`）、設定 GUI、候補の重複・位置、プロファイル判定、キー応答、モード切替、ライブ変換、右クリックメニュー、imserver の寿命管理、スタートメニューのショートカット。
>
> **追加で確認した（2026-11-21、ユーザー報告・0.1.16。ログ・計測値は未取得）**: 自動更新（0.1.15 の環境で右クリック「更新を確認…」→設定画面の更新ページ→新版 0.1.16 の検出・取得・「更新を適用…」→UAC 1 回→適用→`verify.ps1 status` で VERSION 0.1.16）、ネイティブ設定画面（H-104。ウィンドウが開く・見た目・項目の変更が config.toml に反映・シンプル/詳細の切替・データ・ファイルページ〈config.toml・フォルダを開く、学習全削除の確認ダイアログ〉・多重起動で既存ウィンドウが前面・閉じたあと imserver が残らない・Tab/Esc）、候補ウィンドウの位置（最初の 1 文字を含む。0.1.15 でも確認）。0.1.15（2026-11-20）: 右クリックメニューの幅（H-103）、`irm https://nagi.shiki-01.com/install.ps1 | iex` での導入、`uninstall.ps1`。
>
> **さらに確認した（2026-11-21、ユーザー報告・0.1.17。ログ・計測値は未取得。全体の動作のみで、§8.8 の細目は未記録）**: セットアップ用 .exe（`Nagi-setup-0.1.17.exe`、H-111。導入・UAC・辞書/モデル取得の別窓・「設定 → アプリ」に 1 件だけ出る・アンインストールとユーザーデータの確認）、文書切替・キャレット移動での確定（H-105）、ダークモード・ハイコントラスト追従（H-106）、モード切替のキャレット付近の「あ / A」表示（H-109）、UILess（全画面ゲームで候補がアプリ側に出る、H-108）。
>
> **未確認の一覧（実機）**: 標準ユーザーでの動作、VC++ 再頒布可能パッケージなしの環境、ARM64（H-107）、SmartScreen の出方（文言の記録）、曖昧入力の Windows 実機（§9）。自動更新の失敗系（zip 改竄・古い版・UAC 拒否）、更新時の旧エンジン停止（`imserver.exe.old` が残らないこと）の個別確認、設定画面の DPI 150% 以外の個別項目（別 DPI のモニター間移動・`--web`・設定画面を開いたままの更新）は個別には未記録。
> 0.1.17 の実機で未確認（2026-11-21）: ARM64（H-107。手元に実機が無いのでバグ報告待ち）、`ITfFnConfigure`（言語のオプションから設定 GUI。0.1.20 で `ThreadingModel=Both`・`GetFunction` を試したが「オプション」は出ず〈H-124〉、0.1.23 で登録を見直した〈H-129〉。**再起動後の確認待ち**。開き方は `docs/windows-install.md`）、§8.8 の個別の細目（下記）。
> 加えて、下の個別項目のうち `[ ]` のものは個別には未確認（上の「確認した」項目に含まれる範囲は個別の `[x]` に反映した）。

> **0.1.24 で確認した（2026-11-22、ユーザー報告。ログ・計測値は未取得）**: 診断 `nagi-settings --diag` が **NG 0・要確認 0・OK 23**、**WebView2 の設定画面は問題なし**（H-130・H-131）。0.1.19〜0.1.23 の個別の変更（H-118〜H-129。変換 UI のテンプレート、キー割当、候補位置、モード表示アイコン、起動不可ホスト、登録カテゴリ）は、個別には未記録（§8.9 の新しい項目）。**未解決**: 言語のオプションの「オプション」「削除」（0.1.20 でも出なかった。H-124・H-126・H-129 の登録は反映済みで、**再起動後の確認待ち**）。**未実施**: 0.1.18 の公開版への切替（`windows-promote.yml`）。

> **0.1.19〜0.1.28 で確認した（2026-11-22、ユーザー報告。ログ・計測値は未取得。細目は未記録）**: 診断 `nagi-settings --diag` は **NG 0・要確認 0・OK 23**（0.1.24・0.1.27・0.1.28）。F6〜F10 などのキー操作（H-120）、1 文字目の候補位置（H-123）、変換 UI のテンプレート（H-119）、タスクバーのアイコンの切替（H-115・H-127）、設定画面のダーク / ライト・ウィンドウの位置と大きさの保存（H-118・H-130）、設定画面のアニメーション（0.1.28。H-140）は**問題なし**。0.1.26 でユーザー辞書の書き出しも可。0.1.28 の確認項目 2-1〜2-4・3-2〜3-4 は問題なし（項目の中身は報告のまま未記録）、`--native` は開く（無視される。H-136）。キー割当の編集は 0.1.26 で反応が見えない不具合の報告 → 0.1.27 の作り直し → 0.1.28 でダイアログへ（H-137）。**0.1.29（キー割当の 3 択・`replace_keymap`、「Windows のみ」の削除）は確認待ち**。版ごとの記録は `docs/decisions.md` H-142 末尾の表。
>
> **未確認として残す**: **言語のオプションの「オプション」「削除」**（0.1.28 時点でも未解決。再起動前）、**WebView2 の無い環境でブラウザが開くこと**（H-136）、**ARM64**（H-107）、**別 DPI のモニター**、**SmartScreen の文言**、**Office の再変換**（H-43）。

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
- [ ] **設定画面（0.1.24 から既定は WebView2 + Web UI。H-130・H-131。`nagi-settings.exe`。0.1.24 の実機で「問題なし」と報告〈2026-11-22、ユーザー報告。ログ・計測値は未取得〉。以下の個別項目は未記録）**。経緯: H-97 のブラウザ版 → H-104 のネイティブ（Win32）→ H-130 で WebView2 → H-136 でネイティブ版を削除（フォールバックは既定のブラウザ）。ブラウザ版は `--web`
  - ウィンドウが開く: 右クリック → 「設定…」（スタートメニューの「Nagi 設定」も）で「Nagi 設定」ウィンドウが開く（ブラウザは開かない。黒いコンソール窓が出ない。ウィンドウが先に出て、最初は読み込み中 → 項目が並ぶ）。タスクマネージャーに `nagi-settings.exe`（と WebView2 の `msedgewebview2.exe` 群）と、その子の `imserver.exe` が 1 つずつ。`nagi-settings.exe` が無いと「設定…」が灰色。タイトルは `<ページ名> - Nagi 設定`。ウィンドウの位置・大きさ・最大化が次回も復元される（`%LOCALAPPDATA%\Nagi\settings-ui-state.json`）。タイトルバー・本体がシステムのライト/ダーク/ハイコントラストに追従する
  - **画面構成（H-131）**: 左のサイドバー（上にブランド〈ロゴ・「Nagi 設定 <版>」〉、「設定」の見出しの下にカテゴリ、「メンテナンス」の見出しの下に「更新」「データ・ファイル」「動作確認」、下端に「詳細設定を表示」のスイッチ）と、右のページ（見出し・カード）。カテゴリ・項目の表示は「詳細設定を表示」で変わる（カテゴリごと消えるものがある）。iOS 専用の項目（濁点の循環順・タッチ座標の利用・グライド入力）が出ない。Esc・×・Alt+F4 で閉じる（モーダルが開いていれば Esc はモーダルを閉じる）
  - 項目の変更が反映される: トグル・数値・選択・テキストを変えると「保存しました」の通知が出て `%APPDATA%\Nagi\config.toml` に反映される（メモ帳で開いて確認。コメントと他の項目が保たれる）。変更が実際の入力にも反映される（例: ライブ変換のオン/オフ）。「既定に戻す」で明示指定が消える。全角数字も入る。空欄で既定に戻る。範囲外の値・数字でない文字は項目の下に赤字が出て保存されず元に戻る。`config.toml` を壊した状態で開くと各ページにエラーが出る。`requires_restart` の項目に「（再起動後に反映）」が付く
  - **変換 UI（H-119）の項目**: 「テンプレート」（標準・コンパクト・ゆったり・ミニマル・くっきり）を変えると候補ウィンドウの見た目が変わり、詳細設定の項目（文字の大きさ・行の高さ・余白・角丸・影・番号・注釈・選択行・不透明度・配色・最大幅・ページ表示・間隔・下線、通知の時間と位置、モード表示の有無・時間・位置）を指定した部分だけがテンプレートを上書きする。1 ページの候補数は「変換」の `num_candidates`
  - 更新ページ・データ・ファイルページ・多重起動・閉じたあと（`nagi-settings.exe` と子の `imserver.exe` がすぐ消える。強制終了しても子が残らない）・キーボード操作・ナレーター・`install.ps1`（更新）や `uninstall.ps1` との併用は、旧ネイティブ版（0.1.16）で確認済みの項目と同じ。WebView2 版では個別に未記録
  - **WebView2 固有**: (1) 2 つ目の起動は既存のウィンドウを前面に出す（名前付きミューテックス）。(2) 固定ポート 47831〜（`netstat` で `nagi-settings.exe` が 127.0.0.1 だけで待ち受けている。外部から届かない）。(3) ページ内のリンク（リリースノートなど）は既定のブラウザで開き、ウィンドウ内では遷移しない。右クリックは編集系のメニューだけ。F12 で DevTools が開かない。(4) **WebView2 ランタイムが無い環境**（Windows 11 は標準。一部の Windows 10 は無い場合がある）では、自動で**既定のブラウザ**で同じ画面が開く（`%LOCALAPPDATA%\Nagi\logs\settings-ui.log` に「WebView2 を使えないため、既定のブラウザで開きます: <理由>」の 1 行。H-136）。ブラウザ版は多重起動を抑止せず、タブを閉じるか 15 分間操作が無いと終了する。`nagi-settings.exe --webview` は使えないとき開かずにエラー終了 — 未確認。 (5) 設定画面を開いたまま `install.ps1` を実行しても入れ替わる — 未確認
  - 起動方式: `--web`（ブラウザ版。トークンなしの 403 など）・`--webview`（WebView2 固定）がそれぞれ開く。`--native` を付けても無視され、通常どおり開く（`settings-ui.log` に 1 行の警告）— 未確認
- [ ] **WebView2 が無い環境でブラウザが開く（H-136）**: WebView2 ランタイムを使えない環境（ランタイムをアンインストールするか、WebView2 の初期化を失敗させた Windows 10 の VM など）で、右クリック → 「設定…」が**既定のブラウザ**で設定画面を開く。画面が WebView2 版と同じ内容で、項目の変更が `config.toml` に反映される。`settings-ui.log` に理由の 1 行が出る。タブを閉じると `nagi-settings.exe` と子の `imserver.exe` が消える。旧ネイティブ版（H-104・H-114・H-118・H-121）は H-136 で削除済み（0.1.16 では確認済みだった）
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
- [x] `uninstall.ps1`: 登録が消え（`regsvr32 /u`）、ファイルが消える。使用中のファイルが再起動後に消える。`-RemoveUserData` の確認（`yes`）と `-Force`。既定ではユーザーデータが残る。「アプリと機能」の項目が消える **実行できた（0.1.15、2026-11-20、ユーザー報告。個別の確認項目の詳細は未記録）。**
- [x] `irm https://nagi.shiki-01.com/install.ps1 | iex`（PowerShell 5.1 と 7）: latest.json の取得、SHA-256 検証、SmartScreen が出ないこと。`NAGI_SKIP_DATA=1`。prerelease の間は 404 になる（想定どおり）。公開前に `web.yml` の取り込み（`static/updates/windows/latest.json`）を確認する **実行できた（0.1.15、2026-11-20、ユーザー報告。個別の確認項目の詳細は未記録）。**
- [ ] ブラウザで取得した zip の SmartScreen / Defender の警告の出方（Edge・Chrome）と、`docs/windows-install.md` の回避手順（zip のブロック解除、「詳細情報」→「実行」）で実際に進める。`.ps1` の MOTW 付きでも `-ExecutionPolicy Bypass` で実行できる
- [ ] Authenticode 署名（Secrets を登録した場合）: `signtool verify /pa` が通り、署名した DLL / exe が読み込まれる。タイムスタンプの取得
- [ ] 更新の通知: 旧版の imserver が `/updates/windows/latest.json` を取得し、新版があれば右下のトーストが出る（文言「…取得中です。取得できたらスタートメニューの「Nagi を更新」から適用できます」→ 取得後「新しい版 X をダウンロードしました。スタートメニューの「Nagi を更新」から適用できます」）。`%APPDATA%\Nagi\update_check.json` による 24 時間の間引き
- [x] 自動更新（H-95。取得・署名検証・UAC 1 回での適用。**0.1.15→0.1.16 で確認済み、2026-11-21、ユーザー報告。失敗系と旧エンジン停止は個別には未記録**）。確認用に旧版（例: 0.1.12）を入れた状態で、新しい版 X の `Nagi-windows-X.zip` と `latest.json`（`windows-release.yml` の成果物。署名は zip に対するものなので `download_url` を書き換えても有効）を 1 つのフォルダに置き、`download_url` を `http://localhost:8000/Nagi-windows-X.zip` に書き換えて `python -m http.server 8000` で配る。`%APPDATA%\Nagi\config.toml` に `[update]` `check_url = "http://localhost:8000/latest.json"` を書く（prerelease は web の `latest.json` に載らないため、本番の URL では試せない。`prerelease=false` で公開すれば本番の URL でも確認できる）。`%APPDATA%\Nagi\update_check.json` を消して（24 時間の間引きを外す）、メモ帳などで Nagi を使って imserver を起動する。
  - [ ] `%LOCALAPPDATA%\Nagi\update\` に `Nagi-windows-X.zip` と `ready.json` ができる（`.tmp` が残らない）。再度 imserver を起動しても再取得しない（ログに取得の記録が増えない）。トースト「新しい版 X をダウンロードしました…」
  - [ ] 入力インジケーターの右クリックに「更新を適用（X）…」が出る。取得前・適用後は出ない
  - [ ] スタートメニューの「Nagi を更新」または右クリックの項目で、UAC が 1 回出て、検証 → 展開 → 配置 → 完了メッセージ「サインアウトまたは再起動で全アプリが新しい版になります」。`C:\Program Files\Nagi\VERSION` が X、「アプリと機能」の版が X、`staging` と `update\` の `ready.json`・zip が消える。サインアウト後に新しい版（`Get-Process imserver | Select Path`）
  - [ ] 失敗系: zip を `update\` に置いたまま 1 バイト書き換えて適用 → 「更新を検証できませんでした」で中止し、何も置き換わらない。`ready.json` の `version` を現在以下にして適用 → 「更新がありません / 新しくありません」で中止。旧い署名済み zip を `Nagi-windows-<大きい版>.zip` の名前で置き `ready.json` の版も大きくして適用 → 「zip の版が一致しません」で中止。UAC で「いいえ」→ エラーで終わり、何も変わらない
  - [ ] **NG のときに出すログ**: `%LOCALAPPDATA%\Nagi\logs\imserver-*.log`（取得の失敗は「更新の取得に失敗しました」）、`%LOCALAPPDATA%\Nagi\logs\tsf.log`（`[menu]` の行）、`apply-update.ps1` を実行した PowerShell の画面の全文（`Start-Process` の失敗や検証の理由）、`%LOCALAPPDATA%\Nagi\update\ready.json`、`dir "C:\Program Files\Nagi"`
- [ ] VC++ 再頒布可能パッケージが無い環境（クリーンな Windows）で、DLL（静的 CRT）は読み込まれ、imserver は警告のうえ起動しない / する（imserver は動的 CRT）
- [ ] ARM64 Windows（エミュレーション）で x64 の DLL がホストに読み込まれるか（現状は未対応の扱い。`install.ps1` は ARM64 を拒否する）

### 8.8 文書フォーカス・テーマ・UILess・モード表示・ARM64（H-105〜H-109・H-107。0.1.17 で全体の動作を一部確認済み。細目と ARM64・ITfFnConfigure は未確認）
- [x] 文書切替で確定（H-105）: Chrome で入力中（未確定）に別タブの入力欄をクリックすると、元の欄で確定される（未確定のまま残らない・別の欄に入らない） — 確認済み（0.1.17、2026-11-21、ユーザー報告）。
- [x] キャレット移動で確定（H-105）: メモ帳で入力中（未確定）に前の位置をクリックすると確定される（MS-IME と同じ） — 確認済み（0.1.17、2026-11-21、ユーザー報告）。
- [x] 勝手に確定しない（H-105）: 入力中の矢印キー・Space 変換・候補切替・文節移動・再変換で、意図せず確定されない — 確認済み（0.1.17、2026-11-21、ユーザー報告）（確定の動作は問題なしとの報告。個々の操作ごとの確認は未記録）。
- [ ] 二重 `focus_out` が無い（H-105）: Alt-Tab でアプリを切り替えても `tsf.log` に `focus_out` が連続して出ない（`FocusTracker` による冪等化）。背景アプリの文書変化で `focus_in` が送られていないか（残課題） — 未確認（0.1.17 でも `tsf.log` は未確認）。
- [ ] 言語の設定 → Nagi の「オプション」から設定 GUI が開く（`ITfFnConfigure`。入力方式が未アクティブでも開く）。オプションの項目が出ない場合は `ITfFunctionProvider` が `ITfFnConfigure` を返していない（H-105 の残課題） — **未解決**: 0.1.17 は場所が見つからず案内中、0.1.20（`ThreadingModel=Both`・`GetFunction`。H-124）では「オプション」が出なかった。0.1.23 で登録カテゴリ・Display Description を見直した（H-129）。0.1.24 でも「オプション」「削除」は出ず、**再起動後の確認待ち**。切り分けは診断 `tsf.language_options`（H-126）のレポートを貼る。開き方は `docs/windows-install.md`。
- [x] テーマ追従（H-106）: ライト / ダーク / ハイコントラスト（設定 → アクセシビリティ → コントラスト テーマ）を切り替えると、候補ウィンドウとトーストの色が**再起動なしで**変わる。ライトの選択行（#0066CC）の白文字が読める。HC では補助文字もウィンドウ文字色 — 確認済み（0.1.17、2026-11-21、ユーザー報告）（ダークモード・ハイコントラストへの追従。ライトの選択行の読みやすさ・HC の補助文字色など細目は未記録）。
- [x] UILess（H-108）: UILess に対応するアプリ（`ITfUIElementMgr` を使うアプリ）で自前の候補ウィンドウが出ず、アプリ側の候補で選択・確定・取り消しできる。`tsf.log` に `[ui] BeginUIElement show=false` が出る。UILess でないアプリでは従来どおり出る — 確認済み（0.1.17、2026-11-21、ユーザー報告）（全画面ゲームで候補がアプリ側に出る。`tsf.log` の `BeginUIElement show=false` と、UILess でないアプリでの従来表示は未記録）。
- [x] モード切替の表示（H-109）: 英数 / 半角全角キー・言語バーのボタン・メニューでモードを変えると、キャレット付近に「あ / A」が 1 秒出る。**出ない**こと: フォーカス切替直後、エンジン再起動後、他の IME から切り替えた直後、UILess 中。言語バー操作で出ない場合はコンパートメント通知が非同期（H-109 の残課題） — 確認済み（0.1.17、2026-11-21、ユーザー報告）（「あ / A」が出る。出ない条件〈フォーカス切替直後・エンジン再起動後等〉と言語バー操作は未記録）。
- [ ] ARM64（H-107）: ARM64 の Windows 11 で `Nagi-windows-arm64-<ver>.zip` の `install.ps1` が通り、ARM64 ネイティブのアプリ（メモ帳など）で入力できる。x64 版の zip を ARM64 機で実行すると拒否される（`ARCH` 不一致）。**x64 エミュレーションのアプリでは Nagi が出ない**（ARM64X 未対応。既知の制約）。自動更新が `latest-arm64.json` を見る — 手元に実機が無いためバグ報告待ち（H-113）。

### 8.9 ボタン一つで確認（`nagi-settings --diag`、H-116）と、H-114〜H-117 の実機確認

#### 診断の使い方

- **0.1.24 からは設定画面の「動作確認」ページ**（メンテナンス）で「すべて確認」を押すのが簡単（進捗が出て、完了後に「レポートをコピー」「レポートを保存」。H-131）。以下はコマンドラインから実行する場合。

- 実行: PowerShell で `& "$env:ProgramFiles\Nagi\nagi-settings.exe" --diag`。`nagi-settings.exe` は GUI サブシステムなので、画面で結果を見るには `... --diag > diag.txt` とリダイレクトする（進捗は標準エラー）。レポートは常にファイルにも保存される（既定 `%LOCALAPPDATA%\Nagi\logs`）。
- オプション: `--diag-item <id>`（1 項目だけ）、`--diag-out <dir>`（保存先）、`--diag-json`（JSON を標準出力）、`--no-network`（更新確認で実サーバーに通信しない）。終了コードは 0 = NG なし、1 = NG あり、2 = 引数の誤り。
- 1 分以上かかることがある（imserver の初期化・更新確認の待ち）。診断の imserver は一時フォルダを `--user-dir` にして動くので、実ユーザーの設定・学習は変わらない。診断に入力内容は含まれない。
- 結果は項目ごとに OK / NG / スキップ / 手動 / 判定不能。判定不能は PowerShell を実行できない環境の可能性（制限付き・グループポリシー）。報告にはレポートのテキストを貼る。

#### 診断項目と §8・§9 の対応

| 診断 ID | 種別 | 対応する項目 |
| --- | --- | --- |
| `env.arch` | 自動 | §8.8 ARM64（CPU と配布物の整合。実機が ARM64 のときだけ意味がある） |
| `env.user` | 自動 | §8 冒頭の未確認「標準ユーザー」（実行ユーザーと書き込み先） |
| `env.vcruntime` | 自動 | §8 冒頭の未確認「VC++ 再頒布可能パッケージなし」 |
| `env.display` | 自動 | §8.8 テーマ（DPI・ライト/ダーク・ハイコントラストの現状の記録） |
| `install.files` / `install.old_files` | 自動 | §8.7 導入・更新（ファイル構成、`.old` や旧 imserver の残り） |
| `install.signature` | 自動 | §8.7 SmartScreen・署名（状態の記録のみ） |
| `data.files` | 自動 | §8.7 辞書・モデルの取得 |
| `tsf.registration` / `tsf.user_profile` | 自動 | §8.2 登録・読み込み |
| `tsf.interfaces` / `tsf.function_provider` | 自動 | §8.8 `ITfFnConfigure`・再変換の COM 面（COM で QueryInterface） |
| `tsf.log` | 自動 | §8.5d・§8.5e（起動失敗・登録失敗の記録）、H-125 の起動不可ホスト（別集計）、H-128 の `[engine] exit … reason=`（H-129 で判定を補正） |
| `tsf.language_options` | 自動 | §8.8 `ITfFnConfigure`・言語のオプションの「オプション」「削除」（H-126。登録値・DLL の ACL・設定アプリの DLL 読み込み・Microsoft IME との差分・登録カテゴリ 6 種〈H-129〉を並べる。OK/NG ではなく見立てを出す） |
| `engine.start` / `engine.shutdown` | 自動 | §8.3・§8.5e（起動・応答・終了） |
| `engine.parent_exit` | 自動 | §8.5c 閉じたあと imserver が残らない（親プロセス消滅で終了） |
| `engine.memory` | 自動 | H-99 の閾値の材料（常駐メモリの測定） |
| `input.fuzzy` | 自動 | §9.1・§9.2（`ありかとう` は LLM なしでは出ないことがあり NG にしない） |
| `input.reconvert` | 自動 | §8.5c `GetReconversion` のうちエンジン側の再変換 |
| `update.check` | 自動 | §8.7 更新確認（実サーバー。`--no-network` で省略） |
| `update.offline` / `update.bad_signature` | 自動 | §8.7 自動更新の失敗系（オフライン・署名/チェックサム不正） |
| `update.state` | 自動 | §8.7 更新の保存先・適用スクリプトの状態 |
| `manual.language_options` | 手動 | §8.8 `ITfFnConfigure`（`tsf.log` に起動記録があれば自動で OK） |
| `manual.reconversion` | 手動 | §8.5c `GetReconversion`（実アプリ） |
| `manual.multi_dpi` | 手動 | §8 冒頭の未確認「別 DPI のモニター間移動」 |
| `manual.smartscreen` | 手動 | §8.7 SmartScreen の文言の記録 |
| `manual.apply_update` | 手動 | §8.7 更新の適用と UAC 拒否 |
| `manual.arm64_emulation` | 手動 | §8.8 ARM64（x64 エミュレーション下のアプリ。ARM64 でなければスキップ） |

自動 24 + 手動 6 の 30 項目（0.1.21 で `tsf.language_options` を追加。0.1.24 の実機で NG 0・要確認 0・OK 23）。自動で判定しない点（実アプリでの見た目・操作感・UAC の拒否・別モニター・SmartScreen の画面）は手動案内に残る。診断が OK でも §8.3 のキー入力の体感などは別に確認する。

#### H-114〜H-117 の実機確認（ユーザーが見る点）

- [ ] アイコン 16px の判別（H-115）: タスクバーの入力インジケーター・言語バーのアイコン（100% 表示）で、Nagi のロゴと「あ / A」の違いが判別できる。潰れて見えない場合は 24px 以下の小サイズ版の調整が必要。
- [ ] DPI 150% / 200% でアイコンが滲まない。入力方式の一覧（設定 → 時刻と言語 → 言語と地域）、`nagi-settings.exe` のエクスプローラー表示・タスクバー・Alt+Tab、スタートメニューのショートカット、「設定 → アプリ」、セットアップ .exe・アンインストーラが同じロゴ（カラー）になっている。`imserver.exe` のタスクマネージャーでのアイコンも確認する。
- [x] モノクロ切替（H-115）: `config.toml` に `[windows]` / `indicator_icon = "mono"` を書く（または設定画面）。最大約 5 秒後、モードを切り替えるとタスクバーのアイコンがモノクロになる（再起動不要）。タスクバーがダークなら白、ライトなら黒。システムのテーマ（設定 → 個人用設定 → 色 の「Windows モード」）を切り替えて追従する。`"color"` に戻すとカラー。入力方式の一覧・設定アプリのアイコンは mono でもカラーのまま。 **〔タスクバーのアイコンの切替は問題なし（ユーザー報告、2026-11-22）。入力方式の一覧のアイコンがカラーのままか、テーマ追従の個別は未記録。H-127 の「初回の色」は未確認〕**
- [ ] 設定画面の見た目（H-114）: サイドバーが左端に全高で出る・上部バーが無い・フッターに「詳細設定を表示」・ステータス・「閉じる」が並ぶ・説明文が本文と同じ大きさでグレー・グループ枠が無く節見出しと区切り線・入力欄の幅が揃う（240dp）。最小サイズ（760x440 dp 相当）まで縮めても入力欄とボタンが隠れない。DPI 100 / 150 / 200% で崩れない。
- [ ] 設定画面のナレーター: 各項目の「既定に戻す」が「{項目名}を既定に戻す」と読み上げられ、項目ごとに区別できる。
- [ ] 候補位置の補正（H-117）: ずれるアプリ（実行ファイル名を確認）について、`%APPDATA%\Nagi\window-offsets.tsv` に `<exe 名>`・Tab・`dx`・Tab・`dy`（dp。右・下が正）を書く。次にそのアプリへフォーカスを移したとき（再起動不要）に、候補ウィンドウとモード切替ポップアップの位置が動く。トーストは動かない。DPI 150% でも比例する。不正な行・巨大な値（±400 超）で候補が画面外へ飛ばない。ファイルを消すと補正なしに戻る。
- [ ] 候補保留のフォールバック（H-117）: キャレット位置を返さない（または非同期の測定が走らない）アプリで、最初の候補が出ないまま固まらず、約 150ms 後に最後の既知位置（無ければマウス位置）に出る。通常のアプリで、候補が先にマウス位置へ出てから動く現象が増えていない（150ms が短すぎないか）。
- [ ] メモリ計測ログ（H-117）: 入力を続けて 10 分以上経つと `%LOCALAPPDATA%\Nagi\logs\tsf.log` に `[mem] imserver ...`（ワーキングセット・ピーク・プライベートの MB）が出る。10 分に 1 回程度で、入力内容は含まれない。
- [x] **変換 UI のテンプレートと詳細項目（H-119）****変換 UI のテンプレートと詳細項目（H-119）**: 設定画面の「テンプレート」を 5 種（標準・コンパクト・ゆったり・ミニマル・くっきり）に切り替えると、候補ウィンドウの見た目（文字の大きさ・行の高さ・余白・角丸・影・番号・注釈・選択行・配色）が再起動なしで変わる。「詳細設定を表示」の項目（20 項目）を 1 つだけ変えると、その部分だけがテンプレートを上書きする。範囲外の値でエラーになり保存されない。トースト（時間・位置）とモード表示（有無・時間・位置〈キャレット付近または隅〉）、入力中の下線（実線・点線・破線。アプリによっては反映されない）。ライト/ダーク/ハイコントラストへの追従はどのテンプレートでも保たれる。ミニマルで注釈とページ表示が出ない。1 ページの候補数は `num_candidates`。 **〔0.1.20〜0.1.28 の実機で問題なし（ユーザー報告、2026-11-22。詳細 20 項目の個別は未記録）〕**
- [x] **キー割当（H-120。コア。macOS にも影響）****キー割当（H-120。コア。macOS にも影響）**: 入力中・変換中に F6 = ひらがな / F7 = カタカナ / F8 = 半角カナ / F9 = 全角英数 / F10 = 半角英数（F9・F10 の連打で小文字 → 大文字 → 先頭大文字）、無変換 = 循環（ひらがな → カタカナ → 半角カナ → 全角英数 → 半角英数）、PageUp / PageDown = 候補のページ送り。**未入力**: Space は半角のまま（`input.space_width = "half"`）、Shift+Space で全角スペース、`full` / `follow_mode` に変えると反転/追従、無変換で英数 ⇄ かな、カタカナキーでカタカナモード。Ctrl+F6〜F10 は表記切替にならない。`[keymap] preset` を `function_keys` / `ctrl_letters`（Ctrl+U/I/O/P/T/G）/ `henkan_keys`（無変換 = 英数、変換 = 変換）に変えると割当が変わる（個別の `[keymap.*]` が優先）。不明なプリセット名は警告つきで標準。カタカナキーでカタカナモードに入り、ローマ字がカタカナで入力できる（タスクバーの「あ / A」は変わらない — どのフロントも `extended_input_modes` を宣言していないため、既知の制約。H-120）。パスワード欄・停止アプリで Tab の前方一致予測に学習由来の候補が出ない。 **〔F6〜F10 などのキー操作は問題なし（ユーザー報告、2026-11-22）。プリセット切替・`space_width` 等の個別は未記録〕**
- [ ] **モード表示アイコン（H-122）**: タスクバー・言語バーの「あ / A」が実フォント（Yu Gothic UI。無ければ Meiryo UI → MS UI Gothic）の字形で出て、カラーは藍の角丸に生成り色の文字、モノクロは単色。16px・DPI 150/200% で潰れず滲まない。
- [x] **1 文字目の候補位置（H-123）****1 文字目の候補位置（H-123）**: 新しい入力欄で 1 文字目から候補（Space 後）が入力位置の近くに出て、マウスのあるモニター・位置には出ない。出たあとに大きく動かない（4dp 以内なら動かさない）。`tsf.log` に `[cand] anchor=composition|selection|last-known|gui-caret|focus-window|screen` が最初の表示と取得元の変化だけ出る（同じ内容は 10 秒以内に繰り返さない）。位置を返さないアプリでも約 0.4 秒以内に候補が出る（`focus-window` = ウィンドウの左上付近）。 **〔問題なし（ユーザー報告、2026-11-22）。`[cand] anchor=` の記録は未取得〕**
- [ ] **起動不可ホスト（H-125）**: `tsf.log` に `[host] exe=<ファイル名>` がホストごとに 1 回出る。imserver を起動できないホスト（os error 15623 など）で、`spawn blocked (os error 15623); giving up in this process` が 1 回だけ出て再試行の連続が止まり、トーストが出ない。そのホストでは変換が動かず素通しになる（既知の制約。ホスト名を報告してもらう）。5 分後に 1 回だけ再確認する。
- [ ] **エンジン停止の理由ログ・トースト（H-128）**: 更新（`install.ps1`・自動更新）の直後や imserver を強制終了したとき、トーストが出ず、`tsf.log` に停止の行の直後に `[engine] exit code=… reason=update|shutdown|crash|error|parent-exited|unknown …` が出る（入力内容を含まない）。続けて落ちたときだけ「辞書のみで動作します」の通知が出る。
- [ ] **インジケーターの初回の色（H-127）****インジケーターの初回の色（H-127）**: `indicator_icon = "mono"` にして Nagi を選び直す・サインインし直す直後の最初のアイコンからモノクロで出る（一瞬カラーにならない）。`%LOCALAPPDATA%\Nagi\indicator-style` に 1 行の値が書かれる。
- [ ] **登録カテゴリ・Display Description（H-129）**: 診断 `tsf.language_options` の登録カテゴリが 6 種（UIELEMENTENABLED = `{49D2F9CF-…}`・INPUTMODECOMPARTMENT を含む）で SECUREMODE `{49D2F9CE-…}` が無い。言語のオプションに表示される Nagi の名前が `Display Description`（`@…\nagi_windows_tsf.dll,-101`）から出る（0.1.24 時点では「オプション」「削除」が出ない — **再起動後の確認待ち**）。診断の `tsf.log` 判定が、更新直後・起動不可ホスト・再試行の連続で NG にならない。
- [ ] **WebView2 設定画面（H-130・H-131）の見た目と操作**: ライト/ダーク/ハイコントラスト、DPI 100/150/200% と別 DPI のモニター間の移動、ウィンドウの最小サイズ、ホイール・タッチパッドのスクロール、キーボードだけの操作（サイドバーは ↑↓・Home・End、Tab、Space）、ナレーター（項目名・「{項目名}を既定に戻す」・進捗の読み上げ）、「動作確認」ページの実行中の進捗・中止・レポートのコピー/保存（コピーできないときは選択済みのダイアログ）。
- [x] **設定画面の見た目・永続化・アニメーション（H-132・H-118・H-140）**: ダーク / ライトの切替、ウィンドウの位置・大きさの保存と復元、ページ切替・ダイアログ・トーストのアニメーションは問題なし（ユーザー報告。0.1.27・0.1.28、2026-11-22。細目は未記録）。**未確認**: 動きを減らす設定（`prefers-reduced-motion`）・ハイコントラスト（`forced-colors`）での止まり方、別 DPI。
- [ ] **キー割当の編集 UI（H-137）**: 設定 → 「キー割当」。場面（共通・未入力・入力中・変換中）のタブ。共通・未入力は変更できない理由が出る。**キーを押して取り込む**（`Ctrl+j`・`Shift+Left`・F キー・変換 / 無変換・IME 処理中でも取り込める）。**別のキーに変更 / 別のキーも追加 / 無効にする**（0.1.29。既定・プリセットのキー）、ユーザーの割り当ての削除と「既定に戻す」、場面ごと・全体のリセット、プリセットの切替、書き出し・取り込み（不正な行があると何も反映されず行番号が出る）。保存した行がハイライトされる。`config.toml` のコメント・他のセクションが保たれ、保存が即座に変換に効く（再起動不要）。0.1.26〜0.1.28 は「開く・問題ない」までの報告（0.1.26 は反応が見えない不具合）。**0.1.29 の 3 択と `replace_keymap`（途中の状態ができない）は確認待ち**。
- [ ] **ユーザー辞書・学習と抑制語（H-138）**: 設定 → 「ユーザー辞書」で登録・編集・削除・検索・並べ替え・書き出し（0.1.26 で書き出しは問題なし）・取り込み（重複を「残す / 上書き」。5MB 超は断られる）。`%APPDATA%\Nagi\user_dict\registered.tsv`（取り込みは `imported.tsv`）に Mozc TSV で保存され、変換にすぐ効く。「学習と抑制語」で学習の件数・最終保存と、抑制語の一覧・追加・削除。読み・表記がログ（`tsf.log`・`settings-ui.log`）に出ない。個別の確認は未記録。
- [ ] **サイドバーの並びと表記（H-139・H-141）**: 入力と変換 / 辞書と学習 / 表示 / システムの 4 グループ（保存済みの最後のページが再編後も同じ画面を開く）。項目名・説明に「（Windows のみ）」が出ない（サイドバーは「タスクバーのアイコン」）。0.1.29 の確認待ち。
- [ ] **診断の想定内ホスト（H-142）**: `tsf.log` に制限されたホスト（`SystemSettings.exe` など 9 件）の起動不可があっても、診断の `tsf.log` が NG・要確認にならず「想定内」と出る（0.1.27・0.1.28 は NG 0・OK 23）。`msedgewebview2.exe` などの起動不可は注意として残る。
- [ ] サイトの刷新（`web/`）: https://nagi.shiki-01.com/ のファーストビュー・ヘッダーとフッターのロゴが共通・各ページ・スマートフォン幅での崩れ・ダークモード。Windows 欄が配布中の表示になっている。

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

> `ありかとう` は「在り難う」「あり難う」が辞書にあり読みとして成立するため、**LLM なし（辞書のみ）では出ないことがある**（H-116。診断 `input.fuzzy` でも外れて NG にしない）。次の 1 項目目の期待は LLM あり（`--features llama` とモデルがある構成）の場合に限る。

- [ ] 濁点・小書きの付け忘れ（`ありかとう`・`おねかいします`。**LLM あり**の場合）で、Space 変換の候補に出所「もしかして」（macOS は候補ウィンドウのラベル、Windows は候補ウィンドウの表示）が出る。選ぶと補正後の読みで確定する。
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

## 10. 英字の綴りの訂正候補 H-146（macOS / Windows。既定 OFF）— 未検証

前提: `cargo xtask fetch-english` 相当の英語の語彙が入っていること（無ければ訂正候補は出ない）。設定画面「英字の綴りの訂正候補」を ON にする。

- [ ] **10.1** ひらがなモードで Shift+R → `ecieve` と打って（英字の入力中）Space → 候補の先頭は `Recieve`、続けて `Receive`（もしかして）。入力は自動では変わらない。
- [ ] **10.2** 設定 OFF（既定）では 10.1 で `Receive` が出ない。
- [ ] **10.3** 英数モードの直接入力では何も変わらない（OS・アプリのスペルチェックが働く）。
- [ ] **10.4** 「英単語を学習する」の項目が設定画面に出る（モバイルの学習に効く。デスクトップでは学習しない）。
