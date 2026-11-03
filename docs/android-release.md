# Android リリース手順（APK・更新マニフェスト・データ資産）

Android 版 Nagi の配布は APK の直接配布（`docs/mobile-design.md` §6）。ビルド・署名・マニフェスト生成は
GitHub Actions `android-apk`（`.github/workflows/android-apk.yml`）が行い、マニフェストの公開（`nagi.shiki-01.com`
への配置）だけは手作業になる。

## 1. Secrets

| 名前 | 内容 | 未登録のとき |
| --- | --- | --- |
| `ANDROID_KEYSTORE_BASE64` | keystore（`.jks`）を base64 した文字列 | debug 鍵で署名（毎回鍵が変わるため上書き更新できない） |
| `ANDROID_KEYSTORE_PASSWORD` | keystore のパスワード | 同上 |
| `ANDROID_KEY_ALIAS` | 鍵のエイリアス | 同上 |
| `ANDROID_KEY_PASSWORD` | 鍵のパスワード | 同上 |
| `NAGI_UPDATE_SIGNING_KEY` | 更新配布物の ed25519 秘密鍵（seed、16 進 64 文字） | `latest.json` の生成をスキップ（警告のみ） |

### keystore の作り方

```sh
keytool -genkeypair -v -keystore nagi.jks -alias nagi -keyalg RSA -keysize 4096 -validity 36500
base64 -w0 nagi.jks   # macOS は base64 -i nagi.jks。出力を ANDROID_KEYSTORE_BASE64 に登録
```

keystore とパスワードは安全な場所に必ずバックアップする。

> **注意: 署名鍵（keystore）を変えると、既にインストールされた Nagi を上書き更新できなくなる**
> （Android は同じ鍵で署名された APK しか上書きを許さない）。利用者はアンインストール（学習データ消失）してから
> 入れ直すことになる。debug 鍵のビルドも同様に毎回別の鍵になる。配布用は必ず固定の release 鍵を使うこと。

### 更新用 ed25519 鍵

アプリは APK・データ資産を ed25519 署名で検証してから適用する（署名対象は配布物のバイト列全体）。

```sh
cd core && cargo xtask gen-update-key   # 秘密鍵と公開鍵を標準出力に出す（保存されない）
```

- 秘密鍵: Actions Secrets `NAGI_UPDATE_SIGNING_KEY` に登録（リポジトリ・ログ・チャットに残さない）
- 公開鍵: `config/update-public-key.txt` に書く（macOS・Windows と共通）。ビルド時に workflow が
  `frontend-android/app/src/main/assets/update-public-key.txt` へコピーし、アプリが assets から読む。
  無い・壊れている場合、アプリは「更新の検証鍵がありません」として更新を拒否する
- 鍵を差し替えると、旧鍵で署名された配布物は新しいアプリでは検証に通らない（旧アプリは旧鍵のまま）。

## 2. リリース手順

1. `git tag android-vX.Y.Z && git push origin android-vX.Y.Z`（表示バージョンはタグの `X.Y.Z`、versionCode は run_number）。
   タグ起動では llama（LLM 推論）が常に有効。手動実行（`workflow_dispatch`）は `llama` 入力（既定 true）に従う
2. workflow が `dist/` に次を作り、artifact とタグ起動時の GitHub Release（prerelease）に添付する
   - `nagi-X.Y.Z.apk` / `nagi-X.Y.Z.apk.sha256`
   - `latest.json`（`NAGI_UPDATE_SIGNING_KEY` があるときだけ。APK 全体を ed25519 署名した更新マニフェスト）
3. **ユーザー作業**: `latest.json` を `https://nagi.shiki-01.com/updates/android/latest.json` にアップロードする
   （アプリの既定の確認先。コアの `default_check_url` の規約で os=android）
4. 実機で、旧版を入れた状態から「アップデート」画面で確認 → ダウンロード → インストールできることを確認する

`latest.json` の `download_url` は GitHub Release の asset URL
（`https://github.com/<repo>/releases/download/android-vX.Y.Z/nagi-X.Y.Z.apk`）。手動実行（タグなし）で作った
`latest.json` の URL は、そのタグを後で切るまで存在しないので公開しないこと。

## 3. マニフェスト

基本の形はコアの `update_check::Manifest`（`version` のみ必須。APK の取得には残りも必要）。

```json
{
  "version": "0.2.0",
  "notes_url": "https://github.com/<repo>/releases/tag/android-v0.2.0",
  "download_url": "https://github.com/<repo>/releases/download/android-v0.2.0/nagi-0.2.0.apk",
  "sha256": "<APK の SHA-256、小文字 16 進 64 文字>",
  "signature": "<APK のバイト列全体への ed25519 署名、16 進 128 文字>"
}
```

### データ資産（辞書・モデル）の更新: `data_assets`（Android 拡張・任意）

本体 APK を出さずに辞書などだけを差し替えるための配列。コアは未知フィールドを無視するので互換。

```json
{
  "version": "0.2.0",
  "data_assets": [
    {
      "name": "dict",
      "version": "2026.11.1",
      "url": "https://nagi.shiki-01.com/updates/android/data/nagi-dict-2026.11.1.bin",
      "sha256": "<ファイルの SHA-256>",
      "signature": "<ファイルのバイト列全体への ed25519 署名>",
      "min_app": "0.1.0",
      "max_app": null,
      "dest": "dict/nagi-dict.bin"
    }
  ]
}
```

- `min_app` / `max_app`: 対応する本体アプリの版（含む。`x.y.z` の数値比較）。範囲外のアプリには提示しない。null は制限なし
- `dest`: アプリのデータ領域（`files/data/`）からの相対パス。絶対パス・`..` を含むものは無視される
- `version` は `x.y.z` 形式で、アプリが記録している版より新しいときだけ更新対象になる（同梱品は 0.0.0 扱い）
- 適用は `.partial` に取得 → SHA-256 照合 → ed25519 検証 → rename。失敗時は旧データのまま。成功後はエンジンを再起動する
- 署名の作り方: 配布するファイルに対し、APK と同じく `cargo xtask sign-update --file <ファイル> --version <v> --download-url <url> --out <tmp.json>`
  を実行し、出力された `sha256` / `signature` を `data_assets` の要素に転記する（`data_assets` の自動生成は未実装）

## 4. アプリ側の挙動（要点）

- 確認先は固定（`https://nagi.shiki-01.com/updates/android/latest.json`）。自動確認は既定 ON、24 時間に 1 回まで。ネットワーク失敗は黙って無視する
- 検証（SHA-256・署名）に失敗、または取得に失敗したときは何もインストールせず、旧版のまま動く
- 初回は「提供元不明のアプリ」のインストール許可（`REQUEST_INSTALL_PACKAGES`）を設定画面で求められる
