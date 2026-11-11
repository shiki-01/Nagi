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
