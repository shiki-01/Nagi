package com.shiki01.nagi

import org.json.JSONArray
import org.json.JSONObject

/** 入力中の文字列（preedit）。位置は UTF-16 コード単位（`String` の添字と一致する） */
internal data class Preedit(
    val text: String,
    val cursor: Int,
    /** 変換中の注目文節の範囲。なければ -1 */
    val focusStart: Int,
    val focusEnd: Int,
)

/** 候補バーに出す候補 */
internal data class Candidates(
    val items: List<String>,
    val selected: Int,
    val visible: Boolean,
)

/**
 * コアの描画差分（`ProcessKeyResult`）。`preedit` / `candidates` が null のときは「変化なし」。
 * 非同期通知（`update_preedit` / `update_candidates`）も同じ型にして扱う。
 */
internal data class EngineResult(
    val consumed: Boolean,
    val commit: String? = null,
    val deleteBefore: Int = 0,
    val preedit: Preedit? = null,
    val candidates: Candidates? = null,
    val revision: Int = 0,
)

internal object Protocol {
    fun parsePreedit(o: JSONObject?): Preedit? {
        if (o == null) return null
        val text = o.optString("text", "")
        var fs = -1
        var fe = -1
        if (o.has("focus") && !o.isNull("focus")) {
            val idx = o.optInt("focus", -1)
            val segs: JSONArray? = o.optJSONArray("segments")
            val seg = if (segs != null && idx >= 0 && idx < segs.length()) segs.optJSONObject(idx) else null
            val range = seg?.optJSONObject("range")
            if (range != null) {
                fs = range.optInt("start", -1)
                fe = range.optInt("end", -1)
            }
        }
        return Preedit(text, o.optInt("cursor", text.length), fs, fe)
    }

    fun parseCandidates(o: JSONObject?): Candidates? {
        if (o == null) return null
        val arr = o.optJSONArray("items") ?: JSONArray()
        val items = ArrayList<String>(arr.length())
        for (i in 0 until arr.length()) {
            items.add(arr.optJSONObject(i)?.optString("surface", "") ?: "")
        }
        return Candidates(items, o.optInt("selected", 0), o.optBoolean("visible", true))
    }

    /** `ProcessKeyResult` を読む。`result` が null（エラー応答）なら null */
    fun parseResult(o: JSONObject?): EngineResult? {
        if (o == null) return null
        return EngineResult(
            consumed = o.optBoolean("consumed", false),
            commit = if (o.has("commit") && !o.isNull("commit")) o.optString("commit") else null,
            deleteBefore = o.optInt("delete_before", 0),
            preedit = parsePreedit(o.optJSONObject("preedit")),
            candidates = parseCandidates(o.optJSONObject("candidates")),
            revision = o.optInt("revision", 0),
        )
    }

    /** 通知（`update_preedit` / `update_candidates`）を描画差分にする。それ以外は null */
    fun parseNotification(method: String, params: JSONObject?): EngineResult? {
        if (params == null) return null
        return when (method) {
            "update_preedit" -> EngineResult(
                consumed = true,
                preedit = parsePreedit(params.optJSONObject("preedit")),
                revision = params.optInt("revision", 0),
            )
            "update_candidates" -> EngineResult(
                consumed = true,
                candidates = parseCandidates(params.optJSONObject("candidates")),
                revision = params.optInt("revision", 0),
            )
            else -> null
        }
    }
}
