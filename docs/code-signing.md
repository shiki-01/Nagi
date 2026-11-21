# Windows 版のコード署名（SignPath Foundation）

Windows 版の配布物（セットアップ用 .exe・zip 内の .exe / .dll）に Authenticode 署名を付けるための手順。
方式は SignPath Foundation の OSS 向け無料プログラム（2026-11-21 ユーザー決定。比較は `docs/decisions.md` の H-110）。

- 公開ポリシー: <https://nagi.shiki-01.com/code-signing>（`web/src/routes/code-signing/+page.svelte`）
- SignPath 側の設定の写し: `.signpath/artifact-configuration.xml`（正は SignPath の Web 画面。変更したら両方を揃える）
- 署名の組み込み先: `.github/workflows/windows-release.yml`（Secrets が無ければ署名を飛ばして無署名で配布する）

この文書の SignPath 側の画面名・入力項目は、2026-11 時点で公式ページを直接確認できず、第三者の申請記録と公式ドキュメントの断片から書いている。申請時に画面と違えば、画面を正としてこの文書を直す。

## 0. 前提の確認（申請前）

| 条件 | Nagi の状況 |
| --- | --- |
| OSI 承認ライセンス | `MIT OR Apache-2.0`。どちらも OSI 承認。条件の「dual-licensing 不可」は「OSS + 商用ライセンスの併売」を指すと読めるが、不安なら申請フォームの備考に「両方 OSI 承認のライセンスの選択制」と書く |
| 公開リポジトリ | `github.com/shiki-01/nagi`（公開済み） |
| 無料で配布 | GitHub Releases・nagi.shiki-01.com |
| リリース実績 | `windows-v0.1.x` の prerelease が複数ある。**審査前に 1 つ以上を正式リリース（prerelease 解除）にしておく**と通りやすい |
| 同梱物に独自（非 OSS）部品が無い | 辞書・モデルは同梱せず利用者が取得する。VC++ 再頒布パッケージも同梱しない（案内のみ） |
| 文書がある | `docs/windows-install.md`・サイトのサポートページ |
| コード署名ポリシーの掲載 | `/code-signing`（署名対象・役割・プライバシー条項・SignPath の帰属表示） |
| 全員の MFA | GitHub と SignPath の両方で二要素認証を有効にする |
| 知名度 | 第三者の記録では「利用実績・外部の言及が少ない」として断られた例がある。断られたら、利用者が増えてから再申請するか Certum に切り替える |

## 1. 申請

1. GitHub アカウント（`shiki-01`）の二要素認証が有効か確認する（Settings → Password and authentication）。
2. <https://signpath.org/> の「Apply」（OSS プロジェクトの申請）を開く。
3. 入力する内容（目安）:
   - Project name: `Nagi`
   - Repository URL: `https://github.com/shiki-01/nagi`
   - Homepage / download page: `https://nagi.shiki-01.com/`（ダウンロードの導線があるページ）
   - Code signing policy URL: `https://nagi.shiki-01.com/code-signing`
   - License: `MIT OR Apache-2.0`
   - Description（英語）の例:
     > Nagi is an open-source Japanese input method (IME) whose kana-kanji conversion runs a small local LLM on the user's device. The Windows version is a TSF text service (Rust) plus a local engine process. Binaries are built only by GitHub Actions from the public repository. Dictionaries and models are not bundled; they are downloaded on the user's machine.
   - 署名したいもの: Windows の .exe / .dll（Authenticode）、セットアップ用 .exe
   - ビルド環境: GitHub Actions（`windows-release.yml`）
4. 送信後、審査の連絡（メール）を待つ。質問が来たら、上の表とポリシーページで答える。

## 2. 承認後の設定（SignPath の Web 画面）

承認されると SignPath の組織（Organization）が用意される。以下を作る（Foundation 側が作る項目もあるので、届いた案内を優先する）。

1. **GitHub との連携**: Trusted Build System として GitHub（GitHub.com）を組織に追加し、案内に従って SignPath の GitHub App を `shiki-01/nagi` にインストールする。
2. **Project**: slug は `nagi` を推奨（ワークフローの `SIGNPATH_PROJECT_SLUG` に入れる）。リポジトリ URL を登録する。
3. **Artifact configuration**: `.signpath/artifact-configuration.xml` の内容を貼り付ける。slug は `windows-release` を推奨。
4. **Signing policy**: `release-signing` を作る（Foundation の証明書を使う方）。
   - 承認者（Approvers）: `shiki-01`、承認は手動（Foundation の条件）
   - ビルドの出所の検証（origin verification）を有効にし、許可するブランチを `main` に、ワークフローを `.github/workflows/windows-release.yml` に限定する
   - 試験用に `test-signing`（自己署名の試験証明書）があれば、先にそちらで流れを確認する
5. **API トークン**: 組織に CI 用のユーザー（Submitter の権限だけ）を作り、その API トークンを発行する。

## 3. GitHub 側の設定

リポジトリの Settings → Secrets and variables → Actions に登録する。

| 種類 | 名前 | 値 |
| --- | --- | --- |
| Secret | `SIGNPATH_API_TOKEN` | 2-5 の API トークン |
| Variable | `SIGNPATH_ORGANIZATION_ID` | 組織 ID（SignPath の画面に出る GUID） |
| Variable | `SIGNPATH_PROJECT_SLUG` | `nagi` |
| Variable | `SIGNPATH_SIGNING_POLICY_SLUG` | `release-signing`（試験中は `test-signing`） |
| Variable | `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG` | `windows-release` |

`SIGNPATH_API_TOKEN` が未登録なら、ワークフローは署名を飛ばして従来どおり無署名で配布する。

## 4. 署名付きリリースの流れ

1. `gh workflow run windows-release.yml --ref main -f publish=false` で、署名まで通るか試す（成果物は Actions の artifact）。
2. 実行中に SignPath から承認依頼が届く（メール・Web 画面）。内容（コミット・ワークフロー・ファイル一覧）を確認して承認する。承認するまでワークフローは待つ（既定の待ち時間を超えると失敗するので、手元にいるときに実行する）。
3. 署名済みの成果物で release ジョブが進み、GitHub Releases に載る。
4. 署名の確認: エクスプローラーでファイルのプロパティ → デジタル署名に「SignPath Foundation」が出ること、または PowerShell で `Get-AuthenticodeSignature .\Nagi-setup-<ver>.exe`。

## 5. 注意

- 自動更新は ed25519 署名（`config/update-public-key.txt`）で検証しており、Authenticode とは独立。署名の有無で自動更新の動作は変わらない。
- Authenticode で署名しても SmartScreen の警告はすぐには消えない（ダウンロード数による評判が溜まるまで出る）。
- 署名の対象は、このリポジトリのソースから CI でビルドしたものだけ。第三者のバイナリを zip に足すときは、署名対象から外す（`.signpath/artifact-configuration.xml` に載せない）。
