package com.shiki01.nagi

import android.content.Context
import java.io.File
import java.util.Locale

/** バイト数を「1.5 KB」「70.4 MB」の形にする（1024 進。android.* に依存しない純粋関数） */
internal fun formatBytes(bytes: Long): String {
    val b = bytes.coerceAtLeast(0L)
    if (b < 1024L) return "$b B"
    val units = arrayOf("KB", "MB", "GB", "TB")
    var v = b / 1024.0
    var i = 0
    while (v >= 1024.0 && i < units.size - 1) {
        v /= 1024.0
        i++
    }
    return String.format(Locale.ROOT, "%.1f %s", v, units[i])
}

/** ファイルまたはディレクトリ（再帰）の合計バイト数。無ければ 0 */
internal fun fileTreeSize(f: File): Long {
    if (!f.exists()) return 0L
    if (f.isFile) return f.length()
    val children = f.listFiles() ?: return 0L
    var sum = 0L
    for (c in children) sum += fileTreeSize(c)
    return sum
}

internal data class UsageItem(val label: String, val bytes: Long)

/** 端末内に置いたデータの使用量（DM-02）。`filesDir` を集計する */
internal object DataUsage {
    /** 内訳（辞書・モデル・英語/カタカナ語・学習・ユーザー辞書・診断ログ・その他）と合計 */
    fun collect(ctx: Context): Pair<List<UsageItem>, Long> {
        val data = DataInstaller.dataDir(ctx)
        val user = DataInstaller.userDir(ctx)
        val dict = fileTreeSize(File(data, "dict"))
        val models = fileTreeSize(File(data, "models"))
        val optional = (fileTreeSize(data) - dict - models).coerceAtLeast(0L)
        val learning = (user.listFiles() ?: emptyArray())
            .filter { it.isFile && it.name.startsWith("learning") }
            .sumOf { it.length() }
        val userDict = fileTreeSize(File(user, "user_dict"))
        val diag = DiagLog.sizeBytes(ctx)
        val total = fileTreeSize(ctx.filesDir)
        val listed = dict + models + optional + learning + userDict + diag
        return Pair(
            listOf(
                UsageItem("辞書", dict),
                UsageItem("変換モデル", models),
                UsageItem("英語・カタカナ語データ", optional),
                UsageItem("学習データ", learning),
                UsageItem("ユーザー辞書", userDict),
                UsageItem("診断ログ", diag),
                UsageItem("その他（設定・レイアウトなど）", (total - listed).coerceAtLeast(0L)),
            ),
            total,
        )
    }
}
