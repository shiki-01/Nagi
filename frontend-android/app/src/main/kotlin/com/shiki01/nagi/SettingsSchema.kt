package com.shiki01.nagi

import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToLong

internal enum class SettingType { BOOL, INT, FLOAT, CHOICE, TEXT }

internal data class SettingChoice(val value: String, val label: String)

/**
 * `get_settings` の項目 1 件。値は型に応じて Boolean / Int / Double / String、未指定（プロファイル任せ）は null。
 */
internal data class SettingDef(
    val key: String,
    val group: String,
    val label: String,
    val description: String,
    val type: SettingType,
    val min: Double = 0.0,
    val max: Double = 0.0,
    val step: Double = 1.0,
    val choices: List<SettingChoice> = emptyList(),
    val value: Any? = null,
    val default: Any? = null,
    val explicit: Boolean = false,
    val requiresRestart: Boolean = false,
    val detail: Boolean = false,
    val platforms: String = "both",
)

/** 設定スキーマの解釈と `update_settings` の組み立て。android.* に依存しない（org.json のみ） */
internal object SettingsSchema {
    /** "ios" 専用でも Android で意味が通る項目 */
    private val IOS_KEYS_FOR_ANDROID = setOf("input.dakuten_cycle")

    /** 専用画面で扱うため汎用フォームには出さない項目 */
    private val HIDDEN_KEYS = setOf("conversion.model")

    /** `get_settings` の結果（または `update_settings` 結果の `snapshot`）から項目を読む。読めない項目は飛ばす */
    fun parseItems(snapshot: JSONObject): List<SettingDef> {
        val arr = snapshot.optJSONArray("items") ?: return emptyList()
        val out = ArrayList<SettingDef>(arr.length())
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            parseItem(o)?.let { out.add(it) }
        }
        return out
    }

    private fun parseItem(o: JSONObject): SettingDef? {
        val key = o.optString("key", "")
        if (key.isEmpty()) return null
        val kind = o.optJSONObject("kind") ?: return null
        val params = kind.optJSONObject("params")
        val type = when (kind.optString("type")) {
            "bool" -> SettingType.BOOL
            "int" -> SettingType.INT
            "float" -> SettingType.FLOAT
            "choice" -> SettingType.CHOICE
            "text" -> SettingType.TEXT
            else -> return null
        }
        val choices = ArrayList<SettingChoice>()
        val opts: JSONArray? = params?.optJSONArray("options")
        if (opts != null) {
            for (i in 0 until opts.length()) {
                val c = opts.optJSONObject(i) ?: continue
                choices.add(SettingChoice(c.optString("value"), c.optString("label")))
            }
        }
        return SettingDef(
            key = key,
            group = o.optString("group", ""),
            label = o.optString("label", key),
            description = o.optString("description", ""),
            type = type,
            min = params?.optDouble("min", 0.0) ?: 0.0,
            max = params?.optDouble("max", 0.0) ?: 0.0,
            step = (params?.optDouble("step", 1.0) ?: 1.0).let { if (it > 0.0) it else 1.0 },
            choices = choices,
            value = parseValue(o.optJSONObject("value")),
            default = parseValue(o.optJSONObject("default")),
            explicit = o.optBoolean("explicit", false),
            requiresRestart = o.optBoolean("requires_restart", false),
            detail = o.optString("level", "simple") == "detail",
            platforms = o.optString("platforms", "both"),
        )
    }

    /** `{"type":"int","value":150}` を Kotlin の値にする。null / 読めない値は null */
    fun parseValue(o: JSONObject?): Any? {
        if (o == null) return null
        return when (o.optString("type")) {
            "bool" -> if (o.has("value")) o.optBoolean("value") else null
            "int" -> if (o.has("value")) o.optInt("value") else null
            "float" -> if (o.has("value")) o.optDouble("value") else null
            "text" -> if (o.has("value") && !o.isNull("value")) o.optString("value") else null
            else -> null
        }
    }

    /** Android の設定画面に出す項目か（desktop 専用と、意味が通らない ios 専用は除く） */
    fun isVisibleOnAndroid(d: SettingDef): Boolean = when {
        d.key in HIDDEN_KEYS -> false
        d.platforms == "desktop" -> false
        d.platforms == "ios" -> d.key in IOS_KEYS_FOR_ANDROID
        else -> true
    }

    /** 表示する項目（[showDetail] が false なら simple のみ） */
    fun visible(items: List<SettingDef>, showDetail: Boolean): List<SettingDef> =
        items.filter { isVisibleOnAndroid(it) && (showDetail || !it.detail) }

    /** group ごとにまとめる（出現順を保つ） */
    fun groups(items: List<SettingDef>): List<Pair<String, List<SettingDef>>> {
        val map = LinkedHashMap<String, MutableList<SettingDef>>()
        for (d in items) map.getOrPut(d.group) { ArrayList() }.add(d)
        return map.map { (k, v) -> k to v }
    }

    /** 型付きの値の JSON（`{"type":"int","value":200}`）。型と合わない値は null */
    fun valueJson(type: SettingType, v: Any?): JSONObject? = when (type) {
        SettingType.BOOL -> (v as? Boolean)?.let { JSONObject().put("type", "bool").put("value", it) }
        SettingType.INT -> (v as? Number)?.let { JSONObject().put("type", "int").put("value", it.toInt()) }
        SettingType.FLOAT -> (v as? Number)?.let { JSONObject().put("type", "float").put("value", it.toDouble()) }
        SettingType.CHOICE, SettingType.TEXT ->
            (v as? String)?.let { JSONObject().put("type", "text").put("value", it) }
    }

    /** 変更 1 件。[value] が null なら `value` を省略（明示指定を消して既定へ戻す） */
    fun changeJson(key: String, type: SettingType, value: Any?): JSONObject {
        val o = JSONObject().put("key", key)
        valueJson(type, value)?.let { o.put("value", it) }
        return o
    }

    /** `update_settings` の params（`{"changes":[...]}`）。pairs は (key, 型, 値 or null) */
    fun updateParams(changes: List<Triple<String, SettingType, Any?>>): JSONObject {
        val arr = JSONArray()
        for ((k, t, v) in changes) arr.put(changeJson(k, t, v))
        return JSONObject().put("changes", arr)
    }

    /** 値を範囲内に丸める（整数・小数。範囲が 0..0 のように潰れていれば丸めない） */
    fun clamp(d: SettingDef, v: Double): Double =
        if (d.max > d.min) v.coerceIn(d.min, d.max) else v

    /** ステッパー: 現在値から [direction]（+1 / -1）だけ刻み分動かす。刻みの格子に揃えて範囲内に丸める */
    fun stepped(d: SettingDef, current: Double, direction: Int): Double {
        val n = ((current - d.min) / d.step).roundToLong() + direction
        val raw = d.min + n * d.step
        // 小数の誤差（0.1 刻みなど）を落とす
        val rounded = (raw * 1_000_000.0).roundToLong() / 1_000_000.0
        return clamp(d, rounded)
    }

    /** 現在値を Double にする（整数・小数項目）。未指定なら既定値、それも無ければ最小値 */
    fun numberOf(d: SettingDef): Double =
        ((d.value ?: d.default) as? Number)?.toDouble() ?: d.min

    /** 数値の表示（整数は小数点なし、小数は刻みの桁数まで） */
    fun formatNumber(d: SettingDef, v: Double): String =
        if (d.type == SettingType.INT) v.roundToLong().toString() else trimFloat(v)

    private fun trimFloat(v: Double): String {
        val s = String.format(java.util.Locale.ROOT, "%.4f", v).trimEnd('0').trimEnd('.')
        return if (s.isEmpty() || s == "-") "0" else s
    }

    /** 入力文字列を数値項目の値にする（範囲内に丸め、整数は四捨五入）。読めなければ null */
    fun parseNumberInput(d: SettingDef, text: String): Any? {
        val x = text.trim().toDoubleOrNull() ?: return null
        val c = clamp(d, x)
        return if (d.type == SettingType.INT) c.roundToLong().toInt() else c
    }
}
