package com.shiki01.nagi

import org.json.JSONObject

/** 12 キーの 1 キー分の表（コアの `get_key_table` の要素）。`toggle` は 中央→左→上→右→下 の順 */
internal data class KanaKeyDef(
    val key: String,
    val center: String,
    val left: String,
    val up: String,
    val right: String,
    val down: String,
    val toggle: List<String>,
)

internal object KeyTables {
    private fun k(key: String, c: String, l: String, u: String, r: String, d: String) =
        KanaKeyDef(key, c, l, u, r, d, listOf(c, l, u, r, d))

    /** コアから表が取れないときの複製表（`core/engine/src/touch.rs` の `key_chars` と同じ内容） */
    val FALLBACK: List<KanaKeyDef> = listOf(
        k("a", "あ", "い", "う", "え", "お"),
        k("ka", "か", "き", "く", "け", "こ"),
        k("sa", "さ", "し", "す", "せ", "そ"),
        k("ta", "た", "ち", "つ", "て", "と"),
        k("na", "な", "に", "ぬ", "ね", "の"),
        k("ha", "は", "ひ", "ふ", "へ", "ほ"),
        k("ma", "ま", "み", "む", "め", "も"),
        k("ya", "や", "（", "ゆ", "）", "よ"),
        k("ra", "ら", "り", "る", "れ", "ろ"),
        k("wa", "わ", "を", "ん", "ー", "〜"),
        k("punct", "、", "。", "？", "！", "…"),
    )

    /** `get_key_table` の結果を読む。`keys` が無い・空なら null */
    fun parse(o: JSONObject?): List<KanaKeyDef>? {
        val arr = o?.optJSONArray("keys") ?: return null
        val out = ArrayList<KanaKeyDef>()
        for (i in 0 until arr.length()) {
            val e = arr.optJSONObject(i) ?: continue
            val key = e.optString("key", "")
            if (key.isEmpty()) continue
            val center = e.optString("center", "")
            val toggle = ArrayList<String>()
            val t = e.optJSONArray("toggle")
            if (t != null) {
                for (j in 0 until t.length()) toggle.add(t.optString(j, ""))
            }
            out.add(
                KanaKeyDef(
                    key = key,
                    center = center,
                    left = e.optString("left", ""),
                    up = e.optString("up", ""),
                    right = e.optString("right", ""),
                    down = e.optString("down", ""),
                    toggle = if (toggle.isEmpty()) listOf(center) else toggle,
                ),
            )
        }
        return if (out.isEmpty()) null else out
    }

    /** ジェスチャー名（`tap` / `toggle` / `flick_*`）に対応する文字（コアを使えないときの直接入力用） */
    fun charFor(def: KanaKeyDef, gesture: String): String = when (gesture) {
        "flick_left" -> def.left
        "flick_up" -> def.up
        "flick_right" -> def.right
        "flick_down" -> def.down
        else -> def.center
    }
}
