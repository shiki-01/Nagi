package com.shiki01.nagi

import android.app.Activity
import android.app.AlertDialog
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Typeface
import android.net.Uri
import android.os.Bundle
import android.widget.ScrollView
import com.shiki01.nagi.core.NagiCore
import java.io.IOException

/** このアプリについて: バージョン・プライバシー・出典とライセンス・NOTICE・サイト・開発者モード */
class AboutActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("このアプリについて")
        val root = parts.content

        root.addSection(
            "バージョン",
            card(
                infoRow("Nagi", UpdateChecker.currentVersion(this)),
                infoRow("コア", coreVersion()),
            ),
        )

        root.addSection(
            "プライバシー",
            card(
                cardBody(
                    body("・変換・学習などの処理はすべて端末の中で行います。"),
                    body("・入力した内容を外部に送信しません。"),
                    body(
                        "・インターネット通信（INTERNET 権限）は、アップデートの確認と、" +
                            "辞書・変換モデル・英語/カタカナ語データの取得にだけ使います。" +
                            "取得先に送るのはファイルの要求だけで、入力内容は含みません。",
                    ),
                    body(
                        "・ログに入力内容を残しません。診断ログは既定で無効で、有効にしても端末内にだけ記録します。" +
                            "開発者モードで明示的にオンにしたときだけ、入力内容を含められます。",
                    ),
                    body("・パスワード欄などでは学習を止め、診断ログにも入力内容を残しません。"),
                ),
            ),
        )

        root.addSection(
            "ライセンス・出典",
            card(
                cardBody(
                    body("Nagi のコード: MIT OR Apache-2.0"),
                    body(
                        "このアプリに同梱するのはシステム辞書だけです。変換モデルは含まれず、" +
                            "利用者が取得したものを使います。それぞれのライセンスが適用されます。",
                        secondary = true,
                    ),
                ),
                cardBody(
                    textOf("システム辞書", UiSize.BODY_SP, cText, bold = true),
                    body("Mozc OSS 辞書（dictionary_oss）", secondary = true),
                    body("https://github.com/google/mozc", secondary = true),
                    body(
                        "IPAdic（Nara Institute of Science and Technology 他）と沖縄辞書を含みます。" +
                            "IPAdic は、著作権表示と免責条項をすべての複製に含めることを条件に、" +
                            "利用・複製・配布が許可されています。",
                        secondary = true,
                    ),
                ),
                cardBody(
                    textOf("変換モデル", UiSize.BODY_SP, cText, bold = true),
                    body("zenz-v3.1（small / xsmall、GGUF）", secondary = true),
                    body("作者: Miwa Keita", secondary = true),
                    body("https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf", secondary = true),
                    body(
                        "ライセンス: CC BY-SA 4.0。モデルを再配布する場合は、表示（クレジット）と" +
                            "同一条件での共有を満たす必要があります。",
                        secondary = true,
                    ),
                ),
                navRow("ライセンス全文（NOTICE）", "リポジトリの NOTICE と同じ内容") { showNotice() },
            ),
        )

        root.addSection(
            "サイト・フィードバック",
            card(
                navRow("サイト", "nagi.shiki-01.com") { openUrl(SITE_URL) },
                navRow("不具合・要望", "GitHub の Issues（github.com/shiki-01/nagi）") { openUrl(ISSUES_URL) },
            ),
            "診断ログを添えるときは、共有する前に内容を確認してください。",
        )

        root.addSection(
            "開発者",
            card(
                switchRow(
                    "開発者モード",
                    "オンにすると、ホームに診断ログ・応答時間・メモリを見る「開発者」の行が出ます",
                    AppPrefs.developerMode(this),
                ) { AppPrefs.setDeveloperMode(this, it) },
            ),
        )

        setContentView(parts.root)
    }

    private fun coreVersion(): String {
        if (NagiCore.loadError != null) return "読み込めません"
        return try {
            NagiCore.nativeVersion()
        } catch (e: Throwable) {
            "不明"
        }
    }

    private fun openUrl(url: String) {
        try {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        } catch (e: ActivityNotFoundException) {
            toast("ブラウザが見つかりません")
        }
    }

    /** assets の NOTICE.txt をスクロール表示する。無ければリポジトリの URL を案内する */
    private fun showNotice() {
        val text = try {
            assets.open(NOTICE_ASSET).bufferedReader(Charsets.UTF_8).use { it.readText() }
        } catch (e: IOException) {
            null
        }
        if (text.isNullOrEmpty()) {
            AlertDialog.Builder(this)
                .setTitle("ライセンス全文（NOTICE）")
                .setMessage(
                    "NOTICE をこのアプリに同梱できていません。全文は $REPO_URL の NOTICE ファイルで確認できます。",
                )
                .setPositiveButton("リポジトリを開く") { _, _ -> openUrl(REPO_URL) }
                .setNegativeButton("閉じる", null)
                .show()
            return
        }
        val tv = textOf(text, 12f, cText)
        tv.typeface = Typeface.MONOSPACE
        tv.setTextIsSelectable(true)
        val pad = dp(UiSize.ROW_PAD_DP)
        tv.setPadding(pad, pad, pad, pad)
        val scroll = ScrollView(this)
        scroll.addView(tv)
        AlertDialog.Builder(this)
            .setTitle("NOTICE")
            .setView(scroll)
            .setPositiveButton("閉じる", null)
            .show()
    }

    companion object {
        private const val SITE_URL = "https://nagi.shiki-01.com"
        private const val REPO_URL = "https://github.com/shiki-01/nagi"
        private const val ISSUES_URL = "https://github.com/shiki-01/nagi/issues"
        private const val NOTICE_ASSET = "NOTICE.txt"
    }
}
