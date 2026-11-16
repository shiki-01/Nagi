# Windows UI E2E

Windows の TSF フロントを、実際の OS 入力経路（`SendInput` → TSF → Nagi の IME → テキスト欄）で検証する仕組み。
シナリオは macOS と共通（`tests/e2e/README.md`）。Mac / Xcode は不要で、GitHub Actions の `windows-latest` だけで完結する
（`.github/workflows/windows-e2e.yml`、手動実行）。

## 構成

- `frontend-windows-e2e/`（bin `nagi-e2e-win`）: テストホスト（標準 EDIT を持つ自プロセス内のウィンドウ）を前面化し、
  `SendInput`（スキャンコード・US 配列）でキーを送る。`regsvr32` で IME を登録し、
  `ITfInputProcessorProfileMgr::ActivateProfile`（`TF_IPPMF_FORPROCESS`）でこのプロセスだけ Nagi を有効にする。
  `platforms` に `windows` を含むスイートを実行し、`results.json`・`screenshots/`・PASS/FAIL・終了コード（失敗 1、環境不備 2）を出す。
- 観測: `marked` は IME 側のプローブ、`committed` は EDIT の `WM_GETTEXT` から未確定部分を除いたもの。
  `candidates_contain` は Windows では取得できず、注記だけ残して検証しない。
- スクリーンショットは GDI で画面全体を取り、依存なしの PNG エンコーダー（無圧縮）で保存する。

## 観測プローブ（テスト専用）

`frontend-windows-tsf/src/e2e_probe.rs`。環境変数 `NAGI_E2E_PROBE=<path>` が設定されたプロセスでだけ、composition の更新・確定のたびに
`{"marked":..., "committed_delta":...}` を JSON Lines で追記する。`marked` が `null` なら未確定は変わらない。
未設定なら何もしない（ファイルも開かない）。入力内容をログに残さない方針（CLAUDE.md）に対し、これは環境変数で明示的に有効にした
テスト用プロセスにだけ働く例外で、診断ログ `tsf.log` には入力内容を書かない。ドライバーは自プロセス（IME を読み込むプロセス）の環境にだけ設定する。

## 決定的にする

LLM なし（imserver を `llama` feature なしでビルド）。`NAGI_CONFIG_DIR` は `tests/e2e/fixtures/user-dir` の一時コピー。
`APPDATA` を一時ディレクトリにし、`cargo xtask e2e-fixture-data` の最小辞書を `%APPDATA%\Nagi\data` として置く
（TSF は data-dir をそこから決める。ユーザー辞書の重ね合わせにはシステム辞書が要るため）。

## 確認済み（2026-11、CI 17/17 成功）

`romaji-basic.json`（13 件）と `convert-userdict.json`（4 件）が `windows-latest` で成功した。

- ホストランナーで `SendInput` / 前面化 / 画面キャプチャが通る（対話デスクトップが使える）
- 英語環境でも、`regsvr32 /s` による登録と `ActivateProfile`（プロセス内有効化）が通る
- IME の起動待ち（`a` → `あ` が出るまで再試行、既定 30 秒）で足りる
- キー受理、ローマ字→かな、保留ローマ字の表示、BS / Esc / Enter、促音・拗音・`n` の扱い
- Space 変換、2 回目の Space で次候補、Esc で読みに戻る、Enter で確定、ユーザー辞書の候補、imserver の自動起動

観測は TSF プローブの自己申告（`marked`）と EDIT の `WM_GETTEXT`（`committed`）による。

## E2E の範囲外（実機が必要）

次は E2E で通っていない。実機での確認項目は `docs/real-hardware-checklist.md` §8、手順は `docs/windows-verification.md`。

- ←/→、Shift / Ctrl 付きのキー
- 候補ウィンドウ（表示・位置・ページ・数字キー・出所ラベル）、表示属性（下線）
- モード切替キー（半角/全角・変換・無変換・かな・IME ON/OFF）、言語バー・トレイ
- 再変換、ライブ変換・LLM 差し替え（E2E では設定で OFF）、学習・曖昧入力
- imserver の強制終了と `restore_session`、スリープ復帰
- JIS 配列・AltGr、他ホスト（メモ帳・ブラウザ・Office・UWP）
- 環境/電源/サーマル、パスワード欄
- インストーラー・更新・アンインストール、VC++ なし・ARM64・Authenticode
- 標準 EDIT が composition を欄内にインライン表示するか（`committed` は「末尾が marked のときだけ除く」ので E2E の成否には影響しない）
