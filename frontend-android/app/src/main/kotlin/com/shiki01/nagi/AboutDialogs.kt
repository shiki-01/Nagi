package com.shiki01.nagi

import android.app.AlertDialog
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Typeface
import android.net.Uri
import android.widget.ScrollView
import java.io.IOException

// About タブとレイアウトタブで共有する、出典・ライセンスの表示と URL を開く処理。

internal object AboutLinks {
    const val SITE_URL = "https://nagi.shiki-01.com"
    const val REPO_URL = "https://github.com/shiki-01/nagi"
    const val ISSUES_URL = "https://github.com/shiki-01/nagi/issues"

    /** CI が `NOTICE` を配置する assets のファイル名（android-apk.yml / android-check.yml） */
    const val NOTICE_ASSET = "NOTICE.txt"

    /** 同梱の `THIRD_PARTY_LICENSES.md` を置く assets のファイル名（無ければリポジトリの URL を案内する） */
    const val THIRD_PARTY_ASSET = "THIRD_PARTY_LICENSES.txt"

    const val THIRD_PARTY_URL = "$REPO_URL/blob/main/THIRD_PARTY_LICENSES.md"

    /** 辞書・モデルの出典（iOS の `NagiThirdPartyNoticeView` と同じ内容） */
    val SOURCES_TEXT: String = listOf(
        "システム辞書",
        "Mozc OSS 辞書（dictionary_oss）",
        "https://github.com/google/mozc",
        "IPAdic（Nara Institute of Science and Technology 他）と沖縄辞書を含みます。" +
            "IPAdic は、著作権表示と免責条項をすべての複製に含めることを条件に、利用・複製・配布が許可されています。",
        "",
        "変換モデル",
        "zenz-v3.1（small / xsmall、GGUF）",
        "作者: Miwa Keita",
        "https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf",
        "ライセンス: CC BY-SA 4.0。モデルを再配布する場合は、表示（クレジット）と同一条件での共有を満たす必要があります。",
    ).joinToString("\n")

    const val ICON_LICENSES_TEXT =
        "Android 版は、レイアウトの iconify: のアイコンを取得せず、代替の文字で表示します。" +
            "そのため、アイコンセットの作者・ライセンスの表示が必要な取り込み済みアイコンはありません。"
}

internal fun Context.openUrl(url: String) {
    try {
        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    } catch (e: ActivityNotFoundException) {
        toast("ブラウザが見つかりません")
    }
}

/** 長い文章を、選択できる文字でスクロール表示する */
internal fun Context.showTextDialog(title: String, text: String, monospace: Boolean = false) {
    val tv = textOf(text, if (monospace) 12f else UiSize.NOTE_SP + 1f, cText)
    if (monospace) tv.typeface = Typeface.MONOSPACE
    tv.setTextIsSelectable(true)
    val pad = dp(UiSize.ROW_PAD_DP)
    tv.setPadding(pad, pad, pad, pad)
    val scroll = ScrollView(this)
    scroll.addView(tv)
    AlertDialog.Builder(this)
        .setTitle(title)
        .setView(scroll)
        .setPositiveButton("閉じる", null)
        .show()
}

/**
 * assets の [asset] をスクロール表示する。同梱できていないときは、全文のあるリポジトリの [fallbackUrl] を案内する
 */
internal fun Context.showAssetTextDialog(title: String, asset: String, fallbackUrl: String) {
    val text = try {
        assets.open(asset).bufferedReader(Charsets.UTF_8).use { it.readText() }
    } catch (e: IOException) {
        null
    }
    if (text.isNullOrEmpty()) {
        AlertDialog.Builder(this)
            .setTitle(title)
            .setMessage("ライセンス文書をこのアプリに同梱できていません。全文は $fallbackUrl で確認できます。")
            .setPositiveButton("リポジトリを開く") { _, _ -> openUrl(fallbackUrl) }
            .setNegativeButton("閉じる", null)
            .show()
        return
    }
    showTextDialog(title, text, monospace = true)
}

internal fun Context.showSourcesDialog() = showTextDialog("辞書・モデルの出典", AboutLinks.SOURCES_TEXT)

internal fun Context.showIconLicensesDialog() = showTextDialog("アイコンの出典", AboutLinks.ICON_LICENSES_TEXT)
