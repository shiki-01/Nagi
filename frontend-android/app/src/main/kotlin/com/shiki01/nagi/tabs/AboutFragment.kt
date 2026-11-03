package com.shiki01.nagi.tabs

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import androidx.fragment.app.Fragment
import com.shiki01.nagi.*
import com.shiki01.nagi.core.NagiCore

/**
 * About タブ（iOS の `NagiAboutView` と同じ順）: バージョン / プライバシー / ライセンス / サイト・フィードバック。
 * 開発者モードのスイッチは設定タブにある。
 */
class AboutFragment : Fragment() {
    override fun onCreateView(inflater: LayoutInflater, container: ViewGroup?, savedInstanceState: Bundle?): View {
        val ctx = requireContext()
        val parts = ctx.pageParts("About")
        val root = parts.content

        root.addSection(
            "バージョン",
            ctx.card(
                ctx.infoRow("Nagi", UpdateChecker.currentVersion(ctx)),
                ctx.infoRow("コア", coreVersion()),
            ),
        )

        root.addSection(
            "プライバシー",
            ctx.card(
                ctx.cardBody(
                    ctx.body("・変換・学習などの処理はすべて端末の中で行います。"),
                    ctx.body("・入力した内容を外部に送信しません。"),
                    ctx.body(
                        "・インターネット通信（INTERNET 権限）は、アップデートの確認と、" +
                            "辞書・変換モデル・英語/カタカナ語データの取得にだけ使います。" +
                            "取得先に送るのはファイルの要求だけで、入力内容は含みません。",
                    ),
                    ctx.body(
                        "・ログに入力内容を残しません。診断ログは既定で無効で、有効にしても端末内にだけ記録します。" +
                            "開発者モードで明示的にオンにしたときだけ、入力内容を含められます。",
                    ),
                    ctx.body("・パスワード欄などでは学習を止め、診断ログにも入力内容を残しません。"),
                ),
            ),
        )

        root.addSection(
            "ライセンス",
            ctx.card(
                ctx.cardBody(
                    ctx.body("Nagi のコード: MIT OR Apache-2.0"),
                    ctx.body(
                        "このアプリに同梱するのはシステム辞書だけです。変換モデルは含まれず、" +
                            "利用者が取得したものを使います。それぞれのライセンスが適用されます。",
                        secondary = true,
                    ),
                ),
                ctx.navRow("辞書・モデルの出典", "Mozc OSS 辞書・zenz-v3.1") { ctx.showSourcesDialog() },
                ctx.navRow("ライセンス全文（NOTICE）", "リポジトリの NOTICE と同じ内容") {
                    ctx.showAssetTextDialog("ライセンス全文（NOTICE）", AboutLinks.NOTICE_ASSET, AboutLinks.REPO_URL)
                },
                ctx.navRow("オープンソースライセンス", "使っている部品のライセンス（THIRD_PARTY_LICENSES）") {
                    ctx.showAssetTextDialog("オープンソースライセンス", AboutLinks.THIRD_PARTY_ASSET, AboutLinks.THIRD_PARTY_URL)
                },
            ),
        )

        root.addSection(
            "アイコンの出典（レイアウト）",
            ctx.card(*ctx.iconLicenseRows().toTypedArray()),
            "レイアウトで使う Iconify のアイコンの、アイコンセットごとの作者とライセンスです。タップでライセンスのページを開きます。",
        )

        root.addSection(
            "サイト・フィードバック",
            ctx.card(
                ctx.navRow("サイト", "nagi.shiki-01.com") { ctx.openUrl(AboutLinks.SITE_URL) },
                ctx.navRow("不具合・要望", "GitHub の Issues（github.com/shiki-01/nagi）") { ctx.openUrl(AboutLinks.ISSUES_URL) },
            ),
            "診断ログを添えるときは、共有する前に内容を確認してください。",
        )
        return parts.root
    }

    private fun coreVersion(): String {
        if (NagiCore.loadError != null) return "読み込めません"
        return try {
            NagiCore.nativeVersion()
        } catch (e: Throwable) {
            "不明"
        }
    }
}
