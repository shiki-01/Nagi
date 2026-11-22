# 機能ギャップ・キー設定の整理

作成: 2026-11-21（コードは変更せず、現状の読み取りのみで作成）。

**更新: 2026-11-22（0.1.19〜0.1.29。0.1.26 で第 2 波を追加）**: **第 2 波のうち、キーコンフィグ編集 UI（§4.6）と、辞書・学習の管理 UI（ユーザー辞書の一覧・登録・編集・削除・書き出し・取り込み、学習の統計、抑制語）を Windows の設定画面（Web UI）に実装した**（0.1.26。キー割当は 0.1.29 までに操作を作り直した。決定は `docs/decisions.md` H-137・H-138）。印は「**実装済み（H-137）**」「**実装済み（H-138）**」。残り（設定の一括エクスポート/インポート、遡及再評価の表示、拡張入力モードの表示と同期、アクセシビリティ、macOS の SwiftUI 設定画面のキー割当/辞書管理 UI、第 4 波）は未実装。**既定のキーとプリセットの割当は 0.1.19（H-120）から変わっていない**（`core/engine/src/keymap.rs`・`config/default.toml` は 0.1.19 以降の差分なしを確認）ので、§4.1・§4.4・§4.5 の表は現状どおり。

以下は 0.1.24 時点の記述。 第 1 波（§5 の A・B・E のコア側）を実装した。実装済みの項目には「**実装済み（H-120）**」の印を付けた（決定は `docs/decisions.md` H-120）。印のない項目・UI は未実装のまま。**第 2 波**（0.1.24 時点では未実装 → **キーコンフィグ編集 UI と辞書・学習の管理 UI は 0.1.26 で実装済み**）: キーコンフィグ編集 UI（§4.6）、辞書・学習の管理 UI、曖昧入力の UI、設定の一括エクスポート/インポート。なお本文の「現状」の記述のうち、実装済みの印がある箇所は 0.1.18 時点（2026-11-21 の読み取り）のものを残してあるので、印を優先して読むこと。

- 「実装済み」「未実装」は、`docs/progress.md`・`docs/decisions.md`・`docs/ime-design-doc.md`・`config/default.toml`・`core/engine/src/keymap.rs`・各フロントのソースを読んで判断した。根拠は文書名＋節番号またはファイルパスで付けた
- 一般に知られている日本語入力の機能・キー割り当てに基づく記述は「一般的な機能」として扱う。確証がない点は「要確認」と明記した
- 本書は他製品の名称を挙げない。比較は「機能 × Nagi の現状 × 推奨度」で行い、キー体系はその特徴（ファンクションキー重視など）で呼ぶ
- 工数感: S = 数時間〜1 日、M = 数日、L = 1 週間以上

---

## 1. 要約: 推奨実装トップ 10

| 順 | 項目 | 推奨 | 工数 | 理由 | Nagi の設計との相性 |
| --- | --- | --- | --- | --- | --- |
| 1 | **実装済み（H-120）** F8（半角カナ）・F9（全角英数）・F10（半角英数）の既定割当、F6〜F10 の循環（無変換 = `CycleForm`、F9・F10 は連打で小文字 → 大文字 → 先頭大文字） | 高 | S | 現状 **F8〜F10 は未割当で、入力中・変換中に押すとキーが消費されて何も起きない**（`keymap.rs` の `resolve_composing` / `resolve_converting` に F6・F7 しか無い）。乗り換え時に最初に困る点 | 良い。Rewriter に全半角・半角カナ変換（`ToHankaku`、`ZENKAKU_ALNUM_ID`、`HANKAKU_ALNUM_ID`）が既にあり、Action とキー割当の追加だけ |
| 2 | **実装済み（H-120）** 全角スペースの入力（`input.space_width` = half / full / follow_mode。Shift+Space は反対の幅） | 高 | S | 現状、未入力の Space は素通し（半角のみ）。コア・設定に全角スペースの記述は無い（`core/engine`・`core/config` を検索して該当なし）。日本語文書では頻出 | 良い。Direct 文脈で `InsertText("　")` を返すだけ。設定 `input.space_width` を足す |
| 3 | **実装済み（H-120）** キーマップのプリセット（`standard` / `function_keys` / `ctrl_letters` / `henkan_keys`）と、無変換・カタカナキーの割当 | 高 | M | `keymap.preset` は設定に存在するが**値は読まれておらず**（`core/config/src/lib.rs` の `KeymapSection`、エンジンは `KeyMap::default_preset()` 固定）、設計書 §4.9 の「プリセットを同梱」は未実装。無変換キーは Windows で `Muhenkan` に正規化されるが割当が無い | 良い。`KeyMap::with_overrides` の上に「プリセット = 上書き表」を載せるだけ |
| 4 | キーコンフィグ編集 UI（設定画面）。**実装済み（H-137。Windows の設定画面。コア RPC は `set_keymap` / `reset_keymap` / `export_keymap` / `import_keymap` / `replace_keymap`。macOS の SwiftUI 画面は未実装）** | 高 | M〜L | 設定 GUI は記述子駆動で、`keymap.*` は対象外（G-41、H-97）。Windows・macOS とも TOML の手編集のみ | 良い。コアの `get_settings` 記述子に「キー割当表」の型を足し、各フロントは汎用描画（§4.6 の設計案） |
| 5 | **実装済み（H-120。コア）** 入力モードの拡充（カタカナ・半角カナ・全角英数・半角英数を「モード」として持つ）。**フロントは `extended_input_modes` を宣言しておらず、モード表示・OS 同期は未対応** | 中 | M | `InputMode` は `Hiragana` / `Alphanumeric` の 2 つだけ（`core/protocol/src/view.rs`）。ひらがな以外を連続で打つには毎回 F7 等が必要 | 注意。プロトコル（`InputMode`）の拡張と、各フロントのモード表示・OS 側コンパートメント同期（Windows の `compartment_values`）の変更を伴う。後方互換のため加算的に追加する |
| 6 | **実装済み（H-120）** 候補のページ送り（PageUp / PageDown = `PrevPage` / `NextPage`） | 中 | S | 現状、変換中・入力中の PageUp / PageDown は `NoOp`（消費のみ）。1 ページ `num_candidates`（既定 9）を超える候補は ↓ の連打で進む | 良い。Action（`NextPage` / `PrevPage`）を 2 つ足す。候補ウィンドウ側のページ表示は既存（Windows `candidates.rs`、macOS `CandidatePaging.swift`） |
| 7 | 単語登録（選択範囲 → 登録）と、ユーザー辞書・学習の管理 UI | 中 | M〜L | Windows の設定 GUI は「ユーザー辞書の管理 UI（一覧・追加・削除・インポート）が未実装、フォルダを開くのみ」（H-97）。抑制語の UI も未実装（H-89、`docs/real-hardware-checklist.md` §9.3）。選択範囲からの登録は未確認（要確認） | 良い。コアに取り込み（`import_user_dict`）・抑制語（`suppress_candidate`）の RPC が既にあり、UI が無いだけ |
| 8 | スタイル変換（範囲 × LLM アクション。差別化点 2） | 高（長期） | L | 設計書 Phase 4。`ServerCapabilities.style_conversion` は常に false、`persona` 設定は注入されていない。指示追従モデルの別途用意が必要（設計書 §4.12） | ブロッカー: 追従モデルの選定・配布・メモリ（アイドルメモリ目標との両立）。状態 `StyleAction` は設計済みで IR の `Styled` もある |
| 9 | スクリーンリーダー対応（候補の読み上げ・状態通知） | 中 | M | 設計書 §4.14「環境に馴染む」に記載。実機確認の記録が無い（要確認） | Windows は UILess 対応済み（H-108）で土台あり。macOS・モバイルは要確認 |
| 10 | 設定のエクスポート/インポート（設定・キー割当・辞書・学習の一括） | 中 | M | 設定ファイルはテキストなので手動コピーは可能だが、一括の導線は無い（iOS の設定バックアップは `docs/ios-backlog.md` §3 の案のみ）。端末間移行の代替（クラウド同期は非目標） | 良い。ユーザーデータは `config.toml`・`learning.tsv`・`user_dict/` の固定構成 |

### 実装しない（見送り）と判断してよいもの

| 項目 | 理由 |
| --- | --- |
| クラウド同期・クラウド変換 | 非目標（設計書 §2.4、CLAUDE.md）。ローカル完結がプライバシー方針。端末間移行は項目 10 のエクスポート/インポートで代替 |
| 手書き・音声入力 | 非目標 |
| かな入力（配列そのもの）の物理キーボード対応 | 非目標（設計書 §2.4）。ただし日本語配列の利用者には一定の需要があるため、ローマ字テーブル差し替え（配列のカスタム）は後続（§3）で検討余地あり。優先度は低 |
| 一定時間での自動確定・「ー」「っ」の連続入力補助 | 見送り済み（H-74）。入力を失わない原則・正しい入力を壊すため |
| 郵便番号 → 住所の変換 | 住所データの再配布条件・辞書サイズ・鮮度の問題。優先度は低。必要なら外部データ取得の枠（`fetch-*` タスク）に載せる |
| 診断・フィードバックの自動送信 | 方針上「ログに入力内容を残さない」「ネットワークは更新確認とデータ取得のみ」。送信機能は作らず、診断はローカル出力（`nagi-settings --diag` 等、H-116）に留める |

---

## 2. 未実装・見送り項目一覧（プラットフォーム別）

状態の凡例: 未 = 未着手、見送り = 決定により行わない、実機未確認 = 実装済みだが確認が残る（未実装ではない。誤認防止のため併記）。

### 2.1 コア

| 項目 | 状態 | 根拠 | ブロッカー・備考 |
| --- | --- | --- | --- |
| スタイル変換・`StyleAction` | 未 | 設計書 §4.12・Phase 4、`progress.md`「Phase 3 以降」、`methods.rs` の `style_conversion` | 指示追従モデルが別途必要 |
| 設定 `profile.persona` の注入 | 未 | `default.toml`（「v2 以降」）、G-12 の「`profile` 注入は未実装」 | スタイル変換と一体 |
| キーマップのプリセット | **実装済み（H-120）** | `core/engine/src/keymap.rs`（`Preset`）、`config/default.toml` | §4 参照 |
| `Direct` 文脈のキー上書き | 未（対象外と決めている） | `keymap.rs`（`with_overrides` は composing / converting 用）、`default.toml` | 英数モード・未入力時のキーは変えられない |
| `Insert` / `InsertText` / `SetMode` のキー上書き | 見送り（設計上の判断） | `keymap.rs` の `parse_action` | モード切替キーを自由に変えられない（§4.2） |
| 設計書にあるが `Action` に無いもの: `ToggleSecret`、`StyleAction(id)`（`CommitPartial` は `CommitFocused` の別名、`ToggleLive`・`ConvertSelection` は **H-120 で実装済み**） | 一部未 | 設計書 §4.9、`core/engine/src/action.rs` | 手動シークレットモードが無い |
| 入力モード（カタカナ・半角カナ・全角英数・半角英数） | **実装済み（H-120。コア）。フロントの宣言・表示は未** | `core/protocol/src/view.rs` | §1 の 5 |
| 全角スペース入力 | **実装済み（H-120）** | `input.space_width` | §1 の 2 |
| ファンクションキー F8〜F10、無変換キー、カタカナキーの割当 | **実装済み（H-120）** | `keymap.rs` | §1 の 1・3 |
| 候補のページ送りキー | **実装済み（H-120）** | `keymap.rs`（PageUp / PageDown = `PrevPage` / `NextPage`） | §1 の 6 |
| 郵便番号・単位変換 | 未 | `core/rewriter`（電卓・日付・記号・絵文字・漢数字・全半角のみ） | 見送り候補（§1） |
| 英語の自動判定の段階 2・3（先頭を英字／入力中に切替） | 未（予約） | `default.toml` の `[input.auto_english]`、H-51 | 段階 1 のみ実装 |
| 英語の学習による補正、アプリごとの上書き | 未 | H-51 の「未実装・未確認」 | |
| 確定済みテキストの再変換候補の選択結果の学習（Windows） | 未 | `progress.md` Windows 節「未実装」 | |
| 次語予測の LLM 版・英字の予測 | 未（要判断） | 設計書 §4.11「未決」、`default.toml` の `predict.next_word`（既定 false） | |
| 前方一致予測の「secure input / 停止アプリを見ていない」点 | **解消（H-120）** | `core/server/src/pipeline.rs` の `predict`、`core/server/tests/predict_secure.rs` | 設計書 §4.11 の【実装の現状】の記述の更新が残る（要確認） |
| 実行中推論の中断（abort）、モデルの解放（minimal / メモリ逼迫時） | 未（解放は一部実装済み。要確認） | G-12・G-51、`progress.md` | |
| 投機的デコーディング・GPU オフロード | 未 | 設計書 Phase 6 | |
| 曖昧入力: 抑制語の登録 UI、「もしかして」強度の画面、確定後の見直し（`retro_review`）の表示と置換 | **抑制語の管理 UI は Windows で実装済み（H-138）**。「もしかして」強度の画面・`retro_review` の表示と置換は未（フロント側） | H-89、`real-hardware-checklist.md` §9.3・§9.6 | コアは実装済み |
| 辞書の半自動拡張・自前 fine-tune | 未（バックログ） | 設計書 Phase 7 以降 | |
| Linux 対応 | 未（将来） | 設計書 §5.4・Phase 7 | |

### 2.2 macOS

| 項目 | 状態 | 根拠 | 備考 |
| --- | --- | --- | --- |
| 実機での全体確認 | 実機未確認が残る | `progress.md` の macOS フロント、`pending-checks.md` の macOS 節 | 公証付きリリースは Secrets 登録済みで run 済みの記録あり（H-37・訂正記述） |
| 設定画面でのキー割当編集 | 未 | 記述子駆動（G-41）で `keymap.*` は対象外 | |
| 更新の手動確認の導線 | 未 | H-100「macOS には同等の導線は未実装」 | |
| ホストのテキスト削除を伴う確定取消（`delete_before` 対応） | 実装有無は要確認 | G-28（フロント側の対応が別途必要と記載） | |
| 英数モード切替時の入力ソース同期の相互作用 | 要実機確認 | H-49 の「未確認・未対応」 | |

### 2.3 Windows

| 項目 | 状態 | 根拠 | ブロッカー |
| --- | --- | --- | --- |
| ARM64 版の実機確認 | 実機未確認 | H-107、`real-hardware-checklist.md` | 実機なし。x64 エミュレーションのアプリでは使えない（ARM64X は未着手） |
| 32bit ホスト対応 | 未 | `progress.md` Windows 節 | |
| 複数ホストでのエンジン共有（常駐ブローカー） | 見送り | H-39・H-43 §4、`progress.md` | 実測が先 |
| 署名（Authenticode） | 未申請 | H-110・H-113 | 署名団体への申請が必要 |
| 設定画面（第 2 波）: キーコンフィグ編集 UI、辞書・学習の管理 UI、曖昧入力の UI、設定の一括エクスポート/インポート | **キーコンフィグ編集 UI（H-137）と辞書・学習・抑制語の管理 UI（H-138）は実装済み（0.1.26）**。曖昧入力の UI（強さ・見直しの表示）と一括エクスポート/インポートは未 | H-97・H-104・H-130・H-137・H-138 | 設定画面は 0.1.24 で WebView2 + Web UI に変更（本体のダークモードはシステム追従）。数値入力のスピナーなどは未確認 |
| `ITfFnConfigure`（言語のオプションの「オプション」「削除」） | **未解決**（0.1.20・0.1.24 とも出ない。再起動後の確認待ち） | H-124・H-126・H-129 | |
| 設定画面・アイコン・診断・候補位置・変換 UI のテンプレート | 0.1.24 で診断（NG 0・OK 23）と WebView2 の設定画面は確認済み。個別の項目は実機未確認 | H-114〜H-131、`real-hardware-checklist.md` §8.9 | |
| 候補ウィンドウのマウス操作 | 見送り（v1 非目標） | 設計書 §2.4 | |
| 辞書・モデルのアプリ内ダウンロード導線 | 未 | `progress.md` Windows 節 | |

### 2.4 iOS

| 項目 | 状態 | 根拠 | 備考 |
| --- | --- | --- | --- |
| 速打ち時のクリック音・触覚が途切れる件 | 未対応（改善要望） | `ios-device-checklist.md` §1、H-44 | |
| 取り消し（undo）・やり直し・全選択・ホストアプリを開く | 見送り | H-47 付近の判断（`decisions.md` の「取り消し（undo）/ やり直し」の記述） | 拡張から使える API の制約 |
| 確定済みテキストの再変換 | 未 | `ios-settings.md`（`exp.reconvert`）、選択範囲の取得に制約 | |
| 注目文節だけの確定 | コア側は `CommitFocused` あり。iOS の UI は見送りの記述あり（要確認） | H-30 付近の「見送り」 | |
| 自動確定・「ー」「っ」補助 | 見送り | H-74 | |
| 句読点の自動置換・括弧の自動閉じ・クリップボードのタブ以外のアイデア | 未 | `ios-backlog.md` §3 | |
| `retro_review` の表示 | 未 | `ios-device-checklist.md` 5.8.9 | |
| `privacy.secure_field_off` の設定項目 | 固定 ON（設定項目は未） | `ios-settings.md` | |
| 設定画面: キーボード並べ替え、任意アクセント色、ログ上限 | 未 | `ios-settings.md` | |
| レイアウト編集のサイト GUI の一部 | 要確認 | `ios-keyboard-layout.md`（フェーズ 5 は未実装の記述） | |
| 実機確認が残る項目（49 件） | 実機未確認 | `pending-checks.md` iOS 節 | |

### 2.5 Android

| 項目 | 状態 | 根拠 | 備考 |
| --- | --- | --- | --- |
| 実機での全体確認 | 実機未確認 | H-76・H-86 | |
| グライド入力 | 未（G3） | `glide-input.md` §4 | |
| スタイル変換 | 未 | コア未実装 | |
| `data_assets` 自動生成 | 未 | `android-release.md` | |
| 候補・曖昧入力などのうち iOS にだけ有る項目 | 要確認 | `mobile-roadmap.md` | |

---

## 3. 機能の比較表（機能 × Nagi の現状 × 推奨度）

「一般的な機能」として列挙した。Nagi の欄の根拠は §2 と各文書。

### 3.1 変換・候補

| 機能 | Nagi の現状 | 推奨度 | メモ |
| --- | --- | --- | --- |
| 予測変換・サジェスト | 実装済み（Tab で前方一致、入力中の候補バー、次語予測は既定オフ。G-23・H-14・H-19） | 低（改善のみ） | LLM による次語・英字の予測は未決 |
| ユーザー辞書の同期・クラウド | 非目標（設計書 §2.4） | 見送り | エクスポート/インポートで代替（§1 の 10） |
| 顔文字・絵文字・記号入力 | 実装済み（H-21・H-55・H-60） | — | 肌色・履歴は未実装（iOS のアイデア。要確認） |
| 郵便番号変換 | 未 | 低 | §1 の見送り欄 |
| 日付・時刻・和暦 | 実装済み（H-72） | — | |
| 単位変換 | 未 | 低 | 電卓と同じ Rewriter に足せる。需要は限定的 |
| 数式・計算 | 実装済み（H-55） | — | |
| 漢数字・位取り表記・全角/半角英数 | 実装済み（`core/rewriter`、候補末尾にも並ぶ） | — | |
| カタカナ英語・原語の候補 | 実装済み（H-52） | — | |
| 英日の自動判定・綴りの「もしかして」 | 段階 1 実装済み（H-51・H-70） | 中 | 段階 2・3 は精度評価後 |
| ファンクションキー F6〜F10 | **実装済み（H-120）**（F6〜F10・無変換の循環） | — | §1 の 1 |
| 複数文節・文節区切り調整 | 実装済み（Shift+←/→、Ctrl+Shift+←/→） | — | |
| 再変換（確定済みテキスト） | 実装済み（macOS・Windows。iOS は未） | 中 | iOS は API 制約 |
| 候補の出所表示 | 実装済み | — | |
| 候補のページ送り | **実装済み（H-120）**（PageUp / PageDown） | — | §1 の 6 |
| 候補の学習削除 | 実装済み（Ctrl+Delete） | — | 管理 UI は未 |
| 範囲のスタイル変換 | 未 | 高（長期） | §1 の 8 |
| 日本語への自動誤り補正（曖昧入力） | 実装済み（既定オフ、H-81・H-88・H-89） | 中 | 既定をどうするかは評価次第 |

### 3.2 入力・モード・キー

| 機能 | Nagi の現状 | 推奨度 | メモ |
| --- | --- | --- | --- |
| オン/オフ切替（全角/半角・英数/かなキー・IME オン/オフキー・Ctrl+Shift+J / ;） | 実装済み（§4.1）。Ctrl+Space は OS 側の機能 | — | CapsLock での切替は未確認（要確認） |
| 無変換・変換キーでの切替 | **実装済み（H-120）**（未入力の無変換 = 英数 ⇄ かな、入力中・変換中の無変換 = 表記の循環。`henkan_keys` プリセットで無変換 = 英数、変換 = 変換） | — | §4 のプリセット |
| 直接入力（英数）モード | 実装済み | — | |
| 半角カナ・全角英数の入力モード | **実装済み（H-120。コア）**。フロントの表示・同期は未 | 中 | §1 の 5 |
| 全角スペース | **実装済み（H-120）**（`input.space_width`・Shift+Space） | — | §1 の 2 |
| テンキー入力 | テンキー Enter は正規化済み。テンキー数字は通常の数字と同じ扱い（要確認） | 低 | |
| 句読点・括弧・記号のスタイル | 句読点は設定可（`input.punctuation`）。括弧・記号の全半角は `half_width_alnum` のみ | 低〜中 | 記号の全半角スタイルは要確認 |
| ローマ字テーブルのカスタム | 標準のみ（v1。設計書 付録 A-3）。かな入力は非目標 | 低 | 別配列は後続 |
| ライブ変換のオン/オフ | 設定 + `ToggleLive`（**H-120**。キーは既定では未割当で、`[keymap.*]` で割り当てる） | — | |
| パスワード欄での挙動 | 学習・文脈取得・ログを停止（macOS・Windows の secure input 対応は要確認。iOS は実装済み） | 高（確認） | `pending-checks.md` |
| Undo（確定取消） | 実装済み（Ctrl+Backspace、確定直後のみ） | — | |
| クリップボード履歴 | iOS のみ実装（H-69） | 低（デスクトップ） | OS 標準機能と重なる |
| 多言語混在 | 英語の自動判定の範囲のみ | 低 | |

### 3.3 管理・周辺

| 機能 | Nagi の現状 | 推奨度 |
| --- | --- | --- |
| 入力履歴・学習の管理 UI（一覧・個別削除） | 学習の全削除はコマンド・設定画面、候補単位の削除はキー。**学習の統計と抑制語の管理は Windows の設定画面に実装済み（H-138。件数と最終保存だけで、学習した語の一覧は出さない）**。語の一覧・個別削除の UI は未 | 中 |
| 辞書ツール（一覧・編集・インポート/エクスポート） | **Windows の設定画面に実装済み（H-138。一覧・検索・登録・編集・削除・Mozc TSV の書き出し・取り込み）**。コア・macOS の設定にもインポートあり。macOS の SwiftUI 画面の管理 UI は未 | 中 |
| 単語登録（選択範囲から） | 要確認（Windows は UI 無し） | 中 |
| ショートカットのカスタマイズ | **Windows の設定画面で編集できる（H-137）**。macOS は TOML の手編集（composing / converting） | 高 |
| 設定のエクスポート/インポート | 手動コピーのみ | 中 |
| アクセシビリティ（スクリーンリーダー） | Windows は UILess 対応。他は要確認 | 中 |
| 診断・フィードバック送信 | 診断はローカル出力のみ。送信は方針上行わない | 見送り（送信） |
| アップデート | 実装済み（署名検証付き。Windows は自動適用、macOS は起動時確認） | — |

---

## 4. キーコンフィグ

### 4.1 現在のデフォルトキー設定（実装から読み取り）

**この節の表は 0.1.19（H-120）で更新済み**（0.1.18 までの「未割当」は、行ごとに「実装済み」の記述に直した）。出典: `core/engine/src/keymap.rs`（`resolve`・`resolve_composing`・`resolve_converting`・`form_key`）。文脈は 未入力（Direct）/ 入力中（Composing）/ 変換中（Converting）。範囲選択（複数文節の選択）は変換中の内部状態で、キー解決は変換中と同じ表を使う（`StateKind::RangeSelecting`）。候補選択はこの変換中の表の一部。キー名は `Ctrl+Shift+Right` 形式。

**全文脈共通（英数モードでも有効）**

| キー | 動作（Action） | 備考 |
| --- | --- | --- |
| 英数キー（`Eisu`） | `SetMode(Alphanumeric)` | |
| かなキー（`Kana`）/ ひらがなキー | `SetMode(Hiragana)`。ただし**ひらがなモードの未入力時は `Reconvert`** | 英数モードからは 1 回目でモード切替、2 回目で再変換 |
| 半角/全角キー（`Zenkaku`） | `ToggleMode` | |
| Ctrl+Shift+J | `SetMode(Hiragana)` | かなキーの無い配列向け |
| Ctrl+Shift+;（`:` も可） | `SetMode(Alphanumeric)` | |
| カタカナキー（`Katakana`） | `SetMode(Katakana)` | **H-120 で追加**。全文脈・全モードで有効 |

英数モードでは上記以外のキーはすべて素通し（`None`）。

**未入力（Direct・ひらがなモード）**

| キー | 動作 |
| --- | --- |
| 印字文字（英数・記号・かな配列由来の文字） | `Insert`（入力開始。先頭が `/ $ @ ! : #` のときは素通しの語入力） |
| 修飾なしで複数文字が生成されるキー | `InsertText` |
| Ctrl+Backspace | `UndoCommit`（確定直後のみ有効） |
| 変換キー（`Henkan`） | `Reconvert` |
| Ctrl+Shift+R | `Reconvert` |
| Space | `input.space_width` に従う（既定 `half` = 半角のまま素通し。`full` = 全角スペースを確定、`follow_mode` = モードに従う）。**H-120** |
| Shift+Space | `space_width` と反対の幅のスペースを確定（既定では全角スペース）。**H-120** |
| 無変換キー（`Muhenkan`） | `ToggleMode`（英数 ⇄ かな。英数モードからも）。**H-120** |
| Enter・矢印・Tab・Ctrl+文字・Cmd/Alt+文字・Esc など | 割当なし = 素通し |

**入力中（Composing）**

| キー | 動作 |
| --- | --- |
| 印字文字 | `Insert` |
| Space（Shift+Space も） | `StartConvert` |
| Enter | `Commit` |
| Esc | `Cancel`（ライブ変換の表示中は、まず変換だけ破棄してひらがなに戻す） |
| Backspace / Ctrl+H | `Backspace` |
| Delete | `DeleteForward` |
| ← / → | `MoveCursor` |
| Home / Ctrl+A | `MoveCursorHome` |
| End / Ctrl+E | `MoveCursorEnd` |
| Ctrl+K / F7 | `ToKatakana` |
| F6 | `ToHiragana` |
| F8 / F9 / F10 | `ToHankakuKana` / `ToZenkakuAlnum` / `ToHankakuAlnum`（F9・F10 は連打で小文字 → 大文字 → 先頭大文字）。**H-120** |
| 無変換 | `CycleForm`（ひらがな → カタカナ → 半角カナ → 全角英数 → 半角英数）。**H-120** |
| Tab | `Predict`（サジェスト表示中は `SuggestNext`） |
| ↓ | `NoOp`（サジェスト表示中は `SuggestNext`） |
| ↑ | `NoOp`（サジェスト表示中は `SuggestPrev`） |
| PageUp / PageDown | `PrevPage` / `NextPage`（サジェストを 1 ページ単位で動かす）。**H-120** |
| Ctrl+（上記以外の英字）、Cmd+文字、Alt+文字 | `CommitAndPassThrough`（確定してアプリへ渡す） |
| **F1〜F5・F11・F12・Ctrl/Alt 付きの F6〜F10 など上記以外** | 割当なし。入力中はキーを**消費して何もしない**（Direct と違い素通しにならない）。F8〜F10・無変換は H-120 で割当済み |

サジェスト選択中は Enter でその候補を確定、Esc で選択解除、Space で変換（`engine.rs` の `handle_key`）。

**変換中（Converting。候補選択・範囲選択を含む）**

| キー | 動作 |
| --- | --- |
| Space / ↓ / Ctrl+N / Tab | `NextCand` |
| Shift+Space / ↑ / Ctrl+P | `PrevCand` |
| ← / → | `MoveFocus`（注目文節の移動） |
| Shift+← / → | `ResizeSegment`（文節区切りの伸縮） |
| Ctrl+Shift+← / → | `ExtendSelection`（複数文節の選択） |
| Enter | `Commit` |
| Esc / Backspace / Ctrl+H | `Cancel`（入力中へ戻る） |
| 数字 1〜9 | `SelectCand(n)` |
| 数字 0・その他の印字文字 | `Insert`（現在の変換を確定して新しい入力を開始） |
| Ctrl+K / F7 | `ToKatakana` |
| F6 | `ToHiragana` |
| Ctrl+Delete | `DeleteLearning`（注目文節の学習を削除） |
| Ctrl+（上記以外の英字）、Cmd+文字、Alt+文字 | `CommitAndPassThrough` |
| PageUp / PageDown | `PrevPage` / `NextPage`（候補を 1 ページ単位で動かし、ページの先頭を選ぶ。端で折り返す）。**H-120** |
| F8 / F9 / F10・無変換 | 入力中と同じ（注目文節、範囲選択中は選択範囲に作用）。**H-120** |
| Home / End | `NoOp` |
| F1〜F5・F11・F12 ほか | 割当なし（消費のみ） |

既定で割当が無いが、キーマップで割り当てられる Action: `ToHankaku`（半角化）、`CommitFocused`（先頭から注目文節までを確定。別名 `CommitPartial`）、`ToggleMode`、`Predict`、`SuggestNext` / `SuggestPrev`、**`ToggleLive`（ライブ変換のオン・オフ）・`ConvertSelection`（選択した文節だけを再変換）**（H-120）など。

**プラットフォーム固有の差**

| 項目 | macOS | Windows |
| --- | --- | --- |
| 英数キー / かなキー | 物理キー（keycode 0x66 / 0x68）を `Eisu` / `Kana` に正規化 | `VK_OEM_ATTN`（英数）→ `Eisu`、`VK_KANA` と `VK_OEM_BACKTAB` → `Kana`、`VK_OEM_COPY` → ひらがなキー、`VK_OEM_FINISH` → カタカナキー（コアで `SetMode(Katakana)`。**H-120**） |
| 半角/全角キー | メニュー・Ctrl+Shift+J / ; で代替 | `VK_KANJI` / `VK_OEM_AUTO` / `VK_OEM_ENLW` → `Zenkaku` |
| IME オン/オフ専用キー | — | `VK_IME_ON` → ひらがなモード、`VK_IME_OFF` → 英数モード |
| 変換・無変換キー | 一般的な配列には無い | `VK_CONVERT` → `Henkan`（未入力時は再変換）、`VK_NONCONVERT` → `Muhenkan`（未入力は `ToggleMode`、入力中・変換中は `CycleForm`。**H-120**） |
| モード表示の同期 | 入力モードを OS の入力ソースへ同期 | 開閉・変換モードのコンパートメントへ同期（H-39〜H-43） |
| 修飾キー | `Cmd` が `Meta` | `Win` が `Meta`。AltGr は Ctrl+Alt で届くため文字として扱う |

iOS・Android は物理キー前提のこの表を使わず、タッチ用のアクション（`perform_action` / `insert_reading`）で操作する（H-11）。

### 4.2 キーマップ TOML の書式と変更方法（現状の仕様）

出典: `config/default.toml` の `[keymap]`、`core/config/src/lib.rs`、`keymap.rs`。

```toml
[keymap]
preset = "standard"         # standard | function_keys | ctrl_letters | henkan_keys（H-120。旧名 "default" も標準。未知の名前は行番号付きの警告を出して標準）

[keymap.composing]          # 入力中のキー上書き
"Ctrl+j" = "ToKatakana"

[keymap.converting]         # 変換中のキー上書き
"Tab" = "NextCand"
```

- 書式: `"修飾+キー名" = "Action 名"`。修飾は `Ctrl` / `Shift` / `Alt`（`Option`）/ `Meta`（`Cmd` / `Super`）、順不同
- キー名: 1 文字、または `Space` `Enter` `Escape` `BackSpace` `Delete` `Tab` `Left` `Right` `Up` `Down` `Home` `End` `PageUp` `PageDown` `F1`〜`F12` `Eisu` `Kana` `Zenkaku` `Muhenkan` `Henkan`
- Action 名: 引数なし（`StartConvert` `Predict` `NextCand` `PrevCand` `Commit` `CommitFocused`（別名 `CommitPartial`）`Cancel` `Backspace` `DeleteForward` `MoveCursorHome` `MoveCursorEnd` `DeleteLearning` `ToKatakana` `ToHiragana` `ToHankaku` `ToHankakuKana` `ToZenkakuAlnum` `ToHankakuAlnum` `CycleForm` `NextPage` `PrevPage` `ToggleLive` `ConvertSelection` `ToggleMode` `CommitAndPassThrough` `SuggestNext` `SuggestPrev` `NoOp`）、方向つき（`MoveCursor:Left` `MoveFocus:Right` `ResizeSegment:Left` `ExtendSelection:Right` など）、`SelectCand:1`〜`SelectCand:9`
- 対象外: `Insert` / `InsertText` / `SetMode`（モード切替と文字挿入は変更不可。**プリセットだけが `SetMode` と `Direct` 文脈の割当を持てる**）。`Direct`（未入力）文脈の利用者の上書き。英数モード（常に素通し。モード切替キーのみ有効）
- 不正な行はその行だけ無視し、理由を通知する（`with_overrides` の警告）。ホットリロードは設定全体の仕組みに従う（失敗時は旧設定維持）
- 変更手順: 設定ファイル（macOS `~/Library/Application Support/com.shiki.nagi/config.toml`、Windows `%APPDATA%\Nagi\config.toml`）を編集して保存する
- 注意: 上書きは「そのキーの割当」だけを差し替え、キーが生む文字入力の扱い（`printable` 判定）より**先に**評価される（`resolve` で `overrides` を既定表より先に引く）。優先順位は `keymap.rs` 冒頭: プリセットの共通割当 → 内蔵の共通割当 → （英数系は素通し）→ 利用者の上書き → プリセットの文脈別 → 既定表。利用者の上書きはプリセットに常に優先する
- 検証・一覧（H-120）: JSON-RPC の `validate_keymap`（TOML 断片の誤り・二重割当・重要キーの上書き・文字入力との衝突・OS 予約キーを行番号付きで返す）と `get_keymap`（現在有効な割当・プリセット・割り当てられる Action の一覧）。UI は未実装

### 4.3 設定画面でのキーコンフィグ編集の現状

| プラットフォーム | 状況 |
| --- | --- |
| Windows | **編集 UI は実装済み（0.1.26〜0.1.29。H-137）**: 設定 →「キー割当」。場面のタブ、キーを押して取り込む、別のキーに変更 / 別のキーも追加 / 無効にする、プリセットの切替、リセット（行・場面・全体）、書き出し・取り込み。保存は `config.toml` の `[keymap.*]` だけ（原子的・再起動不要）。共通・未入力の文脈と `Insert` / `SetMode` は上書き不可（理由が画面に出る）。以下は 0.1.24 時点の記述: `nagi-settings.exe`（0.1.24 から WebView2）は記述子から項目を描くが、キー割当の表は無かった。**`keymap.preset`（4 種）と `input.space_width` は設定項目として記述子に載っており（`core/config/src/settings.rs`）、設定画面から選べる**（H-120。実機での確認は未記録）。コアの `get_keymap` / `validate_keymap` は実装済み（H-120） |
| macOS | 未実装（SwiftUI の `SettingsView.swift` は記述子駆動で、キー割当は対象外。コアの RPC は共通なので作れる。H-132・H-137） |
| iOS | 物理キー割当は対象外。キーボードレイアウト（キー配置）の編集は別機能（サイト GUI + 取り込み。`ios-keyboard-layout.md`） |
| Android | 同上（レイアウト取り込み） |

### 4.4 一般的な割り当てとの対応表

「キー体系の特徴」で 3 つに分けて示す。確証が無い割当は「要確認」。

| 動作 | ファンクションキー重視の体系 | Ctrl+文字キー重視の体系 | 変換/無変換キー重視の体系 | Nagi の既定 |
| --- | --- | --- | --- | --- |
| ひらがなに変換 | F6 | Ctrl+U | （一般的な割当は要確認） | F6（Ctrl+U は `ctrl_letters` プリセットで割当。標準では確定して素通し） |
| カタカナに変換 | F7 | Ctrl+I | 同上 | F7 / Ctrl+K（Ctrl+I は `ctrl_letters` プリセット） |
| 半角カナに変換 | F8 | Ctrl+O | 同上 | **F8（H-120）**（Ctrl+O は `ctrl_letters` プリセット） |
| 全角英数に変換 | F9 | Ctrl+P | 同上 | **F9（H-120）**（Ctrl+P は `ctrl_letters` プリセット） |
| 半角英数に変換 | F10 | Ctrl+T（要確認） | 同上 | **F10（H-120）**（Ctrl+T は `ctrl_letters` プリセット） |
| 変換（候補を開く） | Space / 変換キー | Space | 変換キー | Space（変換キーは未入力時のみ再変換） |
| 前候補 | Shift+Space / ↑ | Ctrl+P（要確認） | ↑ | Shift+Space / ↑ / Ctrl+P（`ctrl_letters` では Ctrl+P が全角英数になり、前候補は ↑・Shift+Space） |
| 次候補 | Space / ↓ / Tab | Ctrl+N（要確認） | ↓ | Space / ↓ / Ctrl+N / Tab |
| 文節の移動 | ← / → | Ctrl+F / Ctrl+B（要確認） | ← / → | ← / → |
| 文節区切りの伸縮 | Shift+← / → | 同左（要確認） | 同左 | Shift+← / → |
| 取り消し | Esc | Ctrl+G（要確認） | Esc | Esc（Ctrl+G は `ctrl_letters` プリセット） |
| 1 文字削除 | Backspace | Ctrl+H | Backspace | Backspace / Ctrl+H |
| 確定 | Enter | Ctrl+M（要確認） | Enter | Enter（Ctrl+M は未割当） |
| 英数／日本語の切替 | 半角/全角キー | Ctrl+Space（OS 依存） | 無変換 = 英数、変換 = かな（要確認） | 半角/全角、英数、かな、IME オン/オフ。**無変換は未入力で英数 ⇄ かな、入力中・変換中で表記の循環（H-120）。`henkan_keys` では無変換 = 英数** |
| 予測候補を開く | Tab | Tab | Tab | Tab（入力中） |
| 再変換 | 変換キー（選択中） | 同左 | 変換キー | 変換キー / かなキー（ひらがなモード時）/ Ctrl+Shift+R |
| 確定の取消 | Ctrl+Backspace（要確認） | 同左 | 同左 | Ctrl+Backspace |
| 全角スペース | Shift+Space（設定による、要確認） | 同左 | 同左 | **Shift+Space（既定 `space_width = half` の場合。H-120）** |

### 4.5 プリセット案と、未対応のキー

> **0.1.19（H-120）で実装済み**: 下の 4 プリセット（`standard` / `function_keys` / `ctrl_letters` / `henkan_keys`）、F8〜F10、全角英数・半角英数への変換 Action（`ToZenkakuAlnum` / `ToHankakuAlnum`）と循環（`CycleForm`）、無変換・カタカナキーの割当、全角スペース、ページ送り、ライブ変換の切替（`ToggleLive`）。以下は設計時の案で、実装の内容は §4.1・§4.2 と `config/default.toml` の `[keymap]` を参照。未実装のまま残るのは「キーでのシークレットモード切替」「Ctrl+G 等の取消の別名（`ctrl_letters` の Ctrl+G を除く）」。

製品名は使わず、キー体系の特徴で呼ぶ。プリセットは `[keymap.composing]` / `[keymap.converting]` の上書き表として同梱する想定（`preset` の値で選ぶ）。

| プリセット | 特徴 | 追加する割当（案） | 現状の Action で足りるか |
| --- | --- | --- | --- |
| 標準（`default`） | 現行どおり | なし | — |
| ファンクションキー重視 | F6〜F10 で表記を切り替える | F8 = 半角カナ、F9 = 全角英数、F10 = 半角英数（連打で全角/半角/大文字小文字の循環）。変換中は F6〜F10 が注目文節に作用 | **不足**。全角英数・半角英数への変換 Action（`ToZenkakuAlnum` / `ToHankakuAlnum` 相当）と循環の仕様が要る。`ToHankaku` は半角カナ・半角英数記号の一方向のみ |
| Ctrl+文字キー重視 | Ctrl+U / I / O / P でひらがな・カタカナ・半角カナ・全角英数 | Ctrl+U = ひらがな、Ctrl+I = カタカナ、Ctrl+O = 半角カナ、Ctrl+P = 全角英数 | 同上。入力中の Ctrl+P / Ctrl+N（候補選択）との衝突に注意（変換中の Ctrl+N / P は現状 次/前候補） |
| 変換/無変換キー重視 | 無変換 = 英数（または循環）、変換 = 変換・再変換 | `Muhenkan` を `SetMode(Alphanumeric)` またはカタカナ/英数の循環に | モード系は `SetMode` の上書きが禁止（`parse_action`）。上書き可能にするか、プリセットが専用の組み込み割当を持つ必要あり |

**Nagi で未対応のキー（まとめ。0.1.18 時点の記述。H-120 で F8〜F10・Ctrl+U / I / O / P（`ctrl_letters` プリセット）・無変換キー・カタカナキー・Shift+Space の全角スペース・PageUp / PageDown のページ送り・ライブ変換のキー切替〈`ToggleLive` を割り当てる形〉は対応済み。残り: キーでのシークレットモード切替）**: ~~F8・F9・F10、Ctrl+U / I / O / P、無変換キー、カタカナキー、Shift+Space、PageUp / PageDown、ライブ変換のキー切替~~、Ctrl+G 等の取消の別名（`ctrl_letters` の Ctrl+G のみ）、キーでのシークレットモード切替。

### 4.6 キーコンフィグ UI の設計案

> **実装済み（H-137、0.1.26〜0.1.29）**: 下の項目 1・2・5・6（一覧・キー取得・競合表示・インポート/エクスポート・リセット）は Windows の設定画面で実装した。キー取得は `Ctrl, Shift, Alt, Meta` の順・英字は小文字・IME 処理中は `e.code` で補う。競合の警告は拒否ではなく注意として保存後に出す。macOS の SwiftUI 画面は未実装。
>
> **実装状況（H-120。0.1.24 時点の記述）**: 項目 3 の検証 RPC（`validate_keymap`）、項目 4 のプリセット切替の土台（`get_keymap` の `preset` 引数）、項目 7 の現在値の列挙（`get_keymap`）、項目 8 の `preset` の実装・`SetMode` のプリセットでの割当は**コアで実装済み**。UI（一覧・キー取得・競合表示・インポート/エクスポート・リセット。項目 1・2・5・6）は、この時点では未実装だった（**0.1.26 で Windows に実装。上の注**）。

コアの記述子（`get_settings`）に「キー割当表」型を足し、各フロントは汎用部品で描く（ロジックをフロントに持ち込まない原則）。

1. **一覧**: 文脈（入力中 / 変換中）のタブ。行 = 動作（Action の日本語名）、列 = 割当キー（複数可）、既定との差分を強調表示
2. **キー取得**: 行を選び「キーを押す」でキー入力を取得し、キー仕様文字列（`Ctrl+Shift+Right` 形式）に正規化する。正規化はコアの `parse_key` と同じ規約を使い、フロント側の独自解釈を持たない
3. **競合検出**: コアに検証 RPC（案: `validate_keymap`）を置き、(a) 同一文脈での同じキーの二重割当、(b) 既定の重要キー（Enter / Esc / Space / 矢印 / Backspace）の上書き、(c) 文字入力と衝突するキー（修飾なし印字文字。`Insert` を奪う割当）、(d) OS 予約のキー（Windows の Win キー系、macOS の Cmd+Space など）を警告する。警告は拒否ではなく確認にする
4. **プリセット切替**: 「標準 / ファンクションキー重視 / Ctrl+文字キー重視 / 変換・無変換キー重視」を選ぶと上書き表を差し替え。切替時は現在の個別変更を保持するか破棄するかを確認
5. **インポート/エクスポート**: `[keymap.*]` 部分だけの TOML を書き出し・読み込み。読み込み時は不正行を行番号つきで一覧表示（既存の「不正な行はその行のみ無視して通知」と同じ方針）
6. **リセット**: 項目ごと（その動作を既定に戻す）、文脈ごと、全体の 3 段階。確認ダイアログを出す
7. **現在値の確認**: 候補ウィンドウ・設定画面で、現在有効な割当の早見表を表示（実装は `resolve` の表を列挙する RPC。案）
8. **前提となるコア変更**: `Direct` 文脈の上書き対応の要否判断、`SetMode` 系の上書き可否の判断（モード切替キーを変えたい需要が大きい）、プリセットの読み込み（`preset` の実装）

---

## 5. 推奨ロードマップ案

| 段階 | 内容 | 目安 | 前提・依存 |
| --- | --- | --- | --- |
| A（すぐ）**【実装済み・H-120】** | F8〜F10 の Action と既定割当（半角カナ・全角英数・半角英数、循環）／全角スペース入力と設定／無変換キーの既定割当／候補ページ送り／未割当キーの扱い（入力中に消費だけして何も起きない状態）の見直し（F8〜F10・無変換・ページ送りは割当済み。残りの未割当キーは消費のみのまま） | S × 4 | コアのみ。ゴールデンテスト追加（`140`〜`180`） |
| B **【実装済み・H-120】** | キーマップのプリセット実装（`preset`）と同梱プリセット（3 種 + 標準）／`Direct` 文脈・`SetMode` はプリセットだけが持つ（利用者の上書きは不可のまま）／キー割当の検証 RPC（`validate_keymap`）・一覧 RPC（`get_keymap`） | M | A の Action が揃っていること |
| C **【第 2 波。キーコンフィグ UI は Windows で実装済み・H-137。macOS の SwiftUI 画面と設定の一括エクスポート/インポートは未】** | キーコンフィグ UI（設定画面。Windows・macOS）／設定のエクスポート/インポート | M〜L | B（実装済み）。記述子に新しい型を足す（加算的変更） |
| D **【第 2 波。Windows の辞書・学習・抑制語の管理 UI は実装済み・H-138。選択範囲から単語登録・見直し表示・macOS は未】** | 辞書・学習の管理 UI（一覧・個別削除・選択範囲から単語登録・抑制語）、曖昧入力の抑制語と見直し表示 | L | コアの RPC は実装済み |
| E **【コアは実装済み・H-120。フロントの表示・同期は未】** | 入力モードの拡充（カタカナ・半角カナ・全角英数・半角英数）、ライブ変換のキー切替（`ToggleLive`） | M | プロトコル拡張は `extended_input_modes`（加算的）。各フロントが宣言してモード表示・OS 同期を実装するのは未 |
| F | アクセシビリティ（読み上げ）の確認と補完／~~secure input・停止アプリでの前方一致予測の学習停止~~（H-120 で実装済み） | M | 実機での確認が必要 |
| G（長期） | スタイル変換（指示追従モデルの選定・配布・メモリ予算）、ペルソナ注入、英語の自動判定の段階 2・3 | L | 精度・メモリの評価が先 |
| H（別系統） | 実機確認の消化（ARM64、macOS、iOS の 49 項目、Windows の §8.9）、署名の申請、Android のグライド入力、Linux | — | 実機・外部手続き待ち |

---

## 付録: 要確認事項（読み取りで確証が持てなかった点）

- ~~`keymap.preset` に未知の値を書いた場合の警告の有無~~ → H-120 で行番号付きの警告を出して標準を使う
- 選択範囲からの単語登録が macOS で動くか（macOS の設定画面にある「ユーザ辞書を編集…」の挙動）
- macOS・Windows の secure input / 停止アプリ時の挙動（学習・文脈取得・ログの停止）の実装範囲
- テンキー数字・記号の全半角スタイルの扱い
- ~~設計書 §4.11 の「前方一致予測が secure input と停止アプリを見ていない」が現在も当てはまるか~~ → H-120 で解消（設計書 §4.11 の記述の更新は未確認）
- CapsLock による英数切替の扱い
- Android の機能が iOS と同等かどうかの細部
