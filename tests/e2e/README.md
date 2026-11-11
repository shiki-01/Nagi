# UI E2E シナリオ（全プラットフォーム共通）

実際の OS の入力経路（キー送出 → IME → テキスト欄）を通して動作を検証するための、プラットフォーム共通のシナリオ定義です。
設計と実行方法は `docs/ui-testing.md` を参照してください。

## 前提

- Xcode を手元に入れない。ビルド・実行は GitHub Actions のホストランナー（macOS / Windows）、またはローカルでは SwiftPM・Cargo のみで完結させる
- 変換は決定的にする: LLM 無効（`--no-llm`）、システム辞書なし、ユーザー辞書は `fixtures/user-dir/` のものだけを使う
- 各ドライバーは次の環境を用意してからシナリオを実行する
  - エンジンの設定ディレクトリ: `NAGI_CONFIG_DIR` を `fixtures/user-dir/` のコピー（一時ディレクトリ）に向ける
  - 各シナリオの開始時にテキスト欄を空にし、IME を日本語（ひらがな）モードにする

## ファイル

- `scenarios/*.json`: シナリオ。Swift（Foundation）・Rust（serde_json）・XCUITest のいずれでも読める JSON のみを使う（コメント不可）
- `fixtures/user-dir/`: 決定的な変換のための設定とユーザー辞書（Mozc TSV）

## スキーマ

```json
{
  "suite": "romaji-basic",
  "platforms": ["macos", "windows"],
  "scenarios": [
    {
      "name": "konnichiha_commit",
      "steps": [
        { "type": "konnichiha" },
        { "expect": { "marked": "こんにちは" } },
        { "key": "Return" },
        { "expect": { "marked": "", "committed": "こんにちは" } }
      ]
    }
  ]
}
```

- `platforms`: そのスイートを実行するプラットフォーム（`macos` / `windows` / `ios`）。省略時は全て
- `steps` の要素は次のいずれか 1 つ
  - `{ "type": "<ASCII 文字列>" }`: 物理キーとして 1 文字ずつ送る（macOS / Windows）
  - `{ "key": "<名前>" }`: 特殊キー。`Return` `Space` `Backspace` `Escape` `Left` `Right` `Up` `Down` `Tab` `Delete`。修飾は `Shift+Left` のように `+` で連結
  - `{ "tap": ["あ", "か"] }`: ソフトキーボードのキーを順にタップ（iOS）。キー名は表示ラベル
  - `{ "flick": { "key": "か", "dir": "up" } }`: フリック入力（iOS）。`dir` は `up` `down` `left` `right`
  - `{ "expect": { ... } }`: 検証（直前の操作を反映するまで最大 `timeout_ms`（既定 2000）待ってから比較）
  - `{ "screenshot": "<名前>" }`: スクリーンショットを成果物へ保存（検証には使わない。AI による目視確認用）
- `expect` のキー（指定したものだけを比較する）
  - `marked`: 未確定文字列（マークドテキスト / コンポジション）
  - `committed`: 確定済みの文字列（テキスト欄の内容から未確定部分を除いたもの）
  - `text`: テキスト欄の表示全体（`committed` + `marked`）。未確定部分を分離して取れない環境（iOS の XCUITest 等）ではこちらを使う
  - `candidates_contain`: 候補一覧に含まれるべき文字列の配列（取得できる環境のみ）
  - `timeout_ms`: 待ち時間の上限

## 結果の出力

各ドライバーは次を出力する。

- 標準出力: シナリオごとの `PASS` / `FAIL`（失敗時は期待値と実測値）
- `results.json`: `[{ "suite", "name", "ok", "message" }]`
- `screenshots/`: `screenshot` ステップの画像と、失敗時の自動スクリーンショット
- 終了コード: 1 つでも失敗があれば非 0
