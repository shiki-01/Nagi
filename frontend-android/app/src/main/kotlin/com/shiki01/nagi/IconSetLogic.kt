package com.shiki01.nagi

import java.io.File
import java.io.IOException
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

// Iconify アイコンセットの出典・ライセンス情報（CC BY 系の表示義務のため。iOS の `IconSetLicense` /
// `KeyboardIconStore.licenses()` 相当）。android.* に依存しない（org.json と java.io のみ。単体テスト対象）。
// 保存先は filesDir/layouts/icon-sets.json（[IconSetRepository]）。

/** アイコンセット（prefix）1 つ分の出典・ライセンス */
internal data class IconSetInfo(
    val prefix: String,
    /** アイコンセット名（例 Material Design Icons）。無ければ prefix */
    val name: String,
    /** ライセンス名（例 Apache 2.0）。表示義務のため必須 */
    val licenseTitle: String,
    val licenseSpdx: String?,
    val licenseUrl: String?,
    val authorName: String?,
    val authorUrl: String?,
) {
    /** 一覧の見出し（`名前（prefix）`） */
    val heading: String get() = if (name == prefix) prefix else "$name（$prefix）"

    /** 一覧の説明（作者とライセンス名。作者が不明でも行は出す） */
    val detail: String get() = "作者: ${authorName ?: "（情報なし）"}\nライセンス: $licenseTitle"

    /** タップで開く URL（ライセンス、無ければ作者）。無ければ null */
    val openUrl: String? get() = licenseUrl ?: authorUrl
}

internal object IconSetLogic {
    private const val FORMAT_VERSION = 1
    private val PREFIX = Regex("[a-z0-9]+(-[a-z0-9]+)*")

    /** `https://api.iconify.design/collection?prefix=<prefix>&info=true`（iOS の `collectionURL` と同じ。[prefix] は検証済み） */
    fun collectionUrl(prefix: String): String =
        "https://api.iconify.design/collection?prefix=$prefix&info=true"

    /** 空白だけ・JSON の null・文字列以外は null。前後の空白は落とす */
    private fun str(o: JSONObject?, key: String): String? {
        if (o == null || o.isNull(key)) return null
        val v = o.opt(key) as? String ?: return null
        return v.trim().ifEmpty { null }
    }

    /** 表示・外部起動に使う URL は http(s) だけ */
    fun safeUrl(raw: String?): String? {
        val t = raw?.trim() ?: return null
        val lower = t.lowercase()
        return if (lower.startsWith("https://") || lower.startsWith("http://")) t else null
    }

    /**
     * Iconify の応答からライセンス情報を取り出す。`/collection?info=true` の `{"info":{...}}`、
     * アイコン API の `{"info":{...}}`、info がトップレベルにある形のどれも受ける。
     * ライセンス名（title、無ければ spdx）が読めなければ null（表示義務を満たせないので保存しない）
     */
    fun parseInfo(prefix: String, text: String): IconSetInfo? {
        if (!PREFIX.matches(prefix)) return null
        val root = try {
            JSONObject(text)
        } catch (e: JSONException) {
            return null
        }
        val info = root.optJSONObject("info") ?: root
        val license = info.optJSONObject("license") ?: return null
        val title = str(license, "title") ?: str(license, "spdx") ?: return null
        val author = info.optJSONObject("author")
        return IconSetInfo(
            prefix = prefix,
            name = str(info, "name") ?: str(root, "title") ?: prefix,
            licenseTitle = title,
            licenseSpdx = str(license, "spdx"),
            licenseUrl = safeUrl(str(license, "url")),
            authorName = str(author, "name"),
            authorUrl = safeUrl(str(author, "url")),
        )
    }

    /** prefix 順に並べ、同じ prefix は後のものを残す */
    fun normalize(list: List<IconSetInfo>): List<IconSetInfo> {
        val map = LinkedHashMap<String, IconSetInfo>()
        for (i in list) map[i.prefix] = i
        return map.values.sortedBy { it.prefix }
    }

    /** [list] に [info] を足す（同じ prefix は上書き）。prefix 順 */
    fun upsert(list: List<IconSetInfo>, info: IconSetInfo): List<IconSetInfo> = normalize(list + info)

    /** `{"v":1,"sets":[...]}` にする。prefix 順 */
    fun encode(list: List<IconSetInfo>): String {
        val arr = JSONArray()
        for (i in normalize(list)) {
            val o = JSONObject()
            o.put("prefix", i.prefix)
            o.put("name", i.name)
            o.put("licenseTitle", i.licenseTitle)
            o.put("licenseSpdx", i.licenseSpdx ?: JSONObject.NULL)
            o.put("licenseUrl", i.licenseUrl ?: JSONObject.NULL)
            o.put("authorName", i.authorName ?: JSONObject.NULL)
            o.put("authorUrl", i.authorUrl ?: JSONObject.NULL)
            arr.put(o)
        }
        val root = JSONObject()
        root.put("v", FORMAT_VERSION)
        root.put("sets", arr)
        return root.toString(2)
    }

    /**
     * [encode] の逆。壊れている・版が違うときは空。prefix が不正・ライセンス名が無い項目は捨てる。
     * 名前が欠けていれば prefix にする。prefix 順
     */
    fun decode(text: String): List<IconSetInfo> {
        return try {
            val root = JSONObject(text)
            if (root.optInt("v", 0) != FORMAT_VERSION) return emptyList()
            val arr = root.optJSONArray("sets") ?: return emptyList()
            val out = ArrayList<IconSetInfo>()
            for (n in 0 until arr.length()) {
                val o = arr.optJSONObject(n) ?: continue
                val prefix = str(o, "prefix") ?: continue
                if (!PREFIX.matches(prefix)) continue
                val title = str(o, "licenseTitle") ?: str(o, "licenseSpdx") ?: continue
                out.add(
                    IconSetInfo(
                        prefix = prefix,
                        name = str(o, "name") ?: prefix,
                        licenseTitle = title,
                        licenseSpdx = str(o, "licenseSpdx"),
                        licenseUrl = safeUrl(str(o, "licenseUrl")),
                        authorName = str(o, "authorName"),
                        authorUrl = safeUrl(str(o, "authorUrl")),
                    ),
                )
            }
            normalize(out)
        } catch (e: JSONException) {
            emptyList()
        }
    }

    /** 保存済みアイコンのファイル名（[IconRef.fileName]）から prefix を取り出す。不正なら null */
    fun prefixOfIconFile(fileName: String): String? {
        if (!fileName.endsWith(".json")) return null
        val i = fileName.indexOf("__")
        if (i <= 0) return null
        val p = fileName.substring(0, i)
        return if (PREFIX.matches(p)) p else null
    }

    /** ダイアログ用の全文（一覧が空なら案内文） */
    fun plainText(list: List<IconSetInfo>): String {
        if (list.isEmpty()) return EMPTY_TEXT
        return list.joinToString("\n\n") { i ->
            val urls = listOfNotNull(i.licenseUrl, i.authorUrl).distinct()
            (listOf(i.heading, i.detail) + urls).joinToString("\n")
        }
    }

    const val EMPTY_TEXT = "取得済みのアイコンはありません"
}

/**
 * アイコンセットの出典を [file] に保存する。書き込みは一時ファイル + 置き換えで原子的に行う。
 * 複数のインスタンスが同じファイルを触っても壊れないよう、読み書きはプロセス内で直列化する
 */
internal class IconSetRepository(private val file: File) {
    /** 保存済みの一覧（prefix 順）。無い・壊れているときは空 */
    fun load(): List<IconSetInfo> = synchronized(LOCK) { loadLocked() }

    fun has(prefix: String): Boolean = load().any { it.prefix == prefix }

    /** [info] を保存する（同じ prefix は上書き）。成功なら true */
    fun save(info: IconSetInfo): Boolean = synchronized(LOCK) {
        val next = IconSetLogic.upsert(loadLocked(), info)
        val dir = file.parentFile
        if (dir != null && !dir.isDirectory && !dir.mkdirs()) return@synchronized false
        val tmp = File(file.path + ".tmp")
        try {
            tmp.writeText(IconSetLogic.encode(next), Charsets.UTF_8)
            if (!tmp.renameTo(file)) {
                file.delete()
                if (!tmp.renameTo(file)) {
                    tmp.delete()
                    return@synchronized false
                }
            }
            true
        } catch (e: IOException) {
            tmp.delete()
            false
        }
    }

    private fun loadLocked(): List<IconSetInfo> = try {
        if (file.isFile) IconSetLogic.decode(file.readText(Charsets.UTF_8)) else emptyList()
    } catch (e: IOException) {
        emptyList()
    }

    private companion object {
        val LOCK = Any()
    }
}
