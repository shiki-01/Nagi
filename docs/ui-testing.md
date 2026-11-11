# UI 自動テスト（E2E）

実際の OS の入力経路（キー送出 → IME → テキスト欄）を通して、IME の動作を自動で検証する仕組みです。
手元に Xcode を入れず、実行は GitHub Actions のホストランナーで完結させることを前提にしています。

## 全体像

| 層 | 対象 | 手段 | 実行場所 | 状態 |
| --- | --- | --- | --- | --- |
| 1 | コア（状態機械・変換） | `cargo test`（YAML ゴールデン）、`cargo xtask e2e-headless` | どこでも（Linux 可） | 検証済み |
| 2 | macOS IMKit | SwiftPM のみ（`NagiTestHost` + `nagi-e2e`）。CGEvent で送出し、テキスト欄の状態ファイルを検証 | `macos-e2e.yml`（macos-latest） | **未検証** |
| 2 | Windows TSF | Rust（`nagi-e2e-win`）。SendInput で送出し、TSF プローブと EDIT を検証 | `windows-e2e.yml`（windows-latest） | **未検証** |
| 2 | iOS キーボード拡張 | XCUITest（`NagiSpikeUITests`）。シミュレータ上で操作 | `ios-uitest.yml`（macos-15） | **未検証** |
| 3 | 見た目の確認 | 各ドライバーが保存するスクリーンショットを、Claude が成果物から目視確認 | 上記の成果物 | 運用のみ |

pass/fail は、層 1・2 の決定的な assert で判定します。スクリーンショットの AI 判定は補助であり、合否には使いません。

## 共通シナリオ

`tests/e2e/scenarios/*.json` に、プラットフォーム共通の形式でシナリオを置きます。仕様は `tests/e2e/README.md` を参照してください。
変換の決定性のため、LLM 無効・語彙なしの最小システム辞書・`tests/e2e/fixtures/user-dir/` のユーザー辞書だけを使います。
シナリオの期待値は、まず層 1（`cargo xtask e2e-headless`）で正しいことを確認してから、OS 側のドライバーで再生します。

```bash
cd core
cargo build -p nagi-server --bin imserver
cargo xtask e2e-headless            # macOS / Windows 向けシナリオ
cargo xtask e2e-headless --ios      # ios-kana も、コアの touch API で期待値を確認
```

## 各プラットフォーム

### macOS（`frontend-macos/`、`tests/e2e/macos/`）

- `NagiTestHost`: `NSTextInputClient` を実装したテスト専用ホスト。確定・未確定の文字列を JSON の状態ファイルへ書き出す
- `nagi-e2e`: Nagi.app の配置・入力ソースの選択・キー送出・検証・スクリーンショット保存を行うドライバー
- 実行: `tests/e2e/macos/run.sh`、または `macos-e2e.yml`（手動実行）
- 必要な権限: アクセシビリティ、画面収録。CI ではランナーの TCC に付与する手順をワークフローに入れている（効くかは未検証）
- 失敗時の成果物: `results.json`、`screenshots/`、`host.log`、`unified.log`、`diagnose.txt`

### Windows（`frontend-windows-e2e/`、`frontend-windows-tsf/src/e2e_probe.rs`）

- 詳細は `docs/windows-e2e.md`
- composition の観測は、環境変数 `NAGI_E2E_PROBE` を設定したときだけ有効になるテスト専用プローブで行う（入力内容を常時ログに残さない方針に反しない opt-in）

### iOS（`frontend-ios-spike/UITests/`）

- XCUITest がシミュレータ上でキー（`key.<ラベル>`）、候補（`candidate.<n>`）、入力欄（`e2e.input`）を操作・検証する
- 起動引数 `-nagi-e2e` で、入力テスト画面から始める
- 実行: `ios-uitest.yml`（手動実行）。Xcode は CI 側にあり、手元には不要

## CI での実行結果（2026-11-11 時点）

| 対象 | 結果 | 備考 |
| --- | --- | --- |
| iOS | **13 件中 10 件 PASS**（シミュレータ上で実際にキーボードを操作） | 失敗 3 件はシナリオ開始時のキーボード表示の不安定さ（堅牢化中） |
| Windows | ビルド・登録・IME 有効化・TSF の起動までは成功した実行がある。キー入力が IME に届かない問題と、実行ごとに Nagi が列挙されないばらつきが残る | 調整中 |
| macOS | ビルド・権限・IME の登録・入力ソースの有効化までは成功。`TISSelectInputSource` が全試行で `-50` になり、Nagi を選択できない | GitHub ホストランナーでは不可能と判断 |

### macOS: 自前の Mac で実行する

ホストランナーでは、第三者製 IME を入力ソースとして選択できません（US レイアウトの選択は成功するが、Nagi の mode だけ `-50`）。
自前の Mac（Xcode 不要）で一度だけ次を行えば、`nagi-e2e` が実行できます。

1. `cargo xtask bundle-macos` で作った `Nagi.app` を `~/Library/Input Methods` に置く
2. システム設定 > キーボード > 入力ソース で Nagi（日本語）を手動で追加する（初回のみ）
3. 実行元（Terminal）にアクセシビリティと画面収録を許可する
4. `tests/e2e/macos/run.sh` を実行する

常時回したい場合は、この Mac を self-hosted ランナーとして登録し、`macos-e2e.yml` の `runs-on` を `self-hosted` に変更します。

## 未検証の事項と、最初に疑う箇所

これらはいずれもこの開発環境（Linux）では実行できず、CI での初回実行で確認する必要があります。

- macOS: Swift のコンパイルエラー。CGEvent 送出に必要な権限。`launchctl setenv` で渡した環境変数が IME に届くか
- Windows: ホストランナーで対話デスクトップが使えるか。`ActivateProfile` が通るか
- iOS: シミュレータでのキーボード拡張の有効化。シミュレータ向け xcframework のビルド。拡張内の要素が XCUITest から見えるか
- iOS のユーザー辞書は App Group 内にあり、決定的な辞書を注入する手段が未実装。変換系のシナリオは iOS では未対応

## 実機確認として残すもの

自動化できない、または自動化の費用対効果が低い項目です。

- 日本語入力の体感（遅延・フリックの手応え・触覚）
- 各種アプリ（ブラウザ・Office・チャット）との互換性
- Windows の SmartScreen・インストーラー・更新の実機確認
- iOS 実機でのメモリ上限・フルアクセス・端末依存の挙動
- macOS の公証・署名後のインストール

実機チェックリスト（`docs/real-hardware-checklist.md`）のうち、キー入力と composition の基本動作（§2〜§5、§8.3〜§8.4）は E2E シナリオへの移行対象です。移行後にチェックリストから外します。

## 再開メモ（2026-11-11 時点で中断）

### 到達点

- **Windows**: CI（windows-latest）で E2E **17/17 成功**。実際の SendInput → TSF → IME → 入力欄の経路で検証できている
  - E2E が製品の不具合を 1 件検出し修正した: `frontend-windows-tsf/src/render.rs` の `key_id` が lParam 下位 16bit（リピート回数）を含み、OnTestKeyDown（`0x1E0001`）と OnKeyDown（`0x1E0000`）で不一致 → 結果の引き継ぎが外れて 1 キーが 2 回評価されていた。リピート回数を除く修正と回帰テストを入れた（別コミット。問題があればそのコミットだけ戻せる）
- **iOS**: シミュレータ上の XCUITest で ios-kana **13 件中 12 件 PASS**。残りは `cursor_move_insert`（期待 "あくい" / 実測 "あいく"）
  - 切り分け用に、失敗メッセージへ各ステップ後のテキスト履歴（`| 経過: step1 ... -> "あ" cand=N; ...`）を入れた版を push 済み。**この版の CI 結果は未確認**
- **macOS**: GitHub ホストランナーでは不可能と判断（`TISSelectInputSource` が全試行で `-50`。US の選択は成功するが Nagi の mode だけ失敗）。自前の Mac での実行手順は上記「macOS: 自前の Mac で実行する」

### 再開手順

1. 最新の `ios-uitest` の実行結果を確認する（コミット「ios-uitest: 失敗メッセージに各ステップ後のテキスト履歴を含める」）
   - `FAIL ios-kana/cursor_move_insert` の `| 経過:` 以降を読み、原因を判定する
     - `step3 tap ←` で text が変わる → `←` の経路の問題
     - `step4` で "あくい" になった後に "あいく" へ戻る → 後から戻す処理がある
     - `cand=0` のまま → プリエディット無し（`hasPreedit` 偽）の経路で、ホスト側のキャレットだけが動いている疑い
   - 実装は `frontend-ios-spike/Keyboard/KeyboardModel.swift` の `arrow()`（887 行付近）。入力中はコアの `perform(.moveCursor)`、プリエディット無しのときだけ `adjustTextPosition`
   - ヘッドレス（`cargo xtask e2e-headless --ios`）では "あくい" で通っている。iOS 拡張の実設定（ライブ変換が既定 ON、曖昧入力 ON）とフィクスチャ（`live_conversion = false`）の差が候補
   - テスト側の期待値の誤りなら `tests/e2e/scenarios/ios-kana.json` を直す。実装の不具合なら根拠を付けてユーザーに相談する
2. 13 件が通ったら後始末をする
   - `.github/workflows/ios-uitest.yml` と `windows-e2e.yml` の検証用 `push` トリガー（`claude/ui-automation-testing-4mgml9` 限定）を削除する（`main` に入れる前に必須）
   - `macos-e2e.yml` は push トリガー削除済み
   - 3 つの新規ワークフローは `main` に無いと `workflow_dispatch` できない。`main` へ入れた後に手動実行へ切り替わる
   - `docs/real-hardware-checklist.md` のうち、E2E に移せた項目（§2〜§5、§8.3〜§8.4 のキー入力・composition の基本動作）を整理する
   - `ci.yml` の `windows-tsf` ジョブに `-p nagi-windows-e2e` を足す（単体指定で feature 不足を検出できる。ローカルでは `cargo check --target x86_64-pc-windows-gnu -p nagi-windows-e2e` で確認済み）
3. 未着手・未解決
   - iOS のユーザー辞書を決定的にする手段（App Group への注入）が無く、変換系シナリオは iOS で未対応
   - Windows の `EnumProfiles` に Nagi が出るまで 45 秒待つ警告が出ていた（`GetProfile` が成功すれば待たずに進める修正は入れた）
   - macOS を常時回すなら、自前の Mac を self-hosted ランナーにして `macos-e2e.yml` の `runs-on` を変更する

### CI の調べ方（このセッションで分かったこと）

- ジョブログの末尾だけでは診断が見えないことが多い。診断は標準出力（ジョブログ）に出す方針にする。アーティファクトは、実行環境のネットワークポリシーで取得できない
- ログの要点抽出は Haiku に委任すると、コンテキストを節約できる
- macOS ランナーは課金が重い。検証用トリガーのパスを広げると意図せず起動するので注意する
- `cargo check -p <crate>` は単体指定で行う（他クレートとの feature 統合で、CI でのみ失敗する不具合を見逃す）
