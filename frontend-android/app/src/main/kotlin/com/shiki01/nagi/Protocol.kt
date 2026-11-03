package com.shiki01.nagi

import org.json.JSONArray
import org.json.JSONObject

/** 変換中の 1 文節。位置は `Preedit.text` 上の UTF-16 コード単位 */
internal data class PreeditSegment(
    val surface: String,
    val reading: String,
    val start: Int,
    val end: Int,
    /** セグメント種別（`japanese` / `literal` / `fixed` / `styled`）。復元（restore_session）に使う */
    val kind: String = "japanese",
)

/** 入力中の文字列（preedit）。位置は UTF-16 コード単位（`String` の添字と一致する） */
internal data class Preedit(
    val text: String,
    val cursor: Int,
    /** 変換中の注目文節の範囲。なければ -1 */
    val focusStart: Int,
    val focusEnd: Int,
    /** 文節列（変換中は複数、入力中は 1 つ以下） */
    val segments: List<PreeditSegment> = emptyList(),
    /** 注目文節の番号。変換中でなければ -1 */
    val focus: Int = -1,
    /** 複数文節の選択範囲（`text` 上の位置）。なければ -1 */
    val selectionStart: Int = -1,
    val selectionEnd: Int = -1,
)

/** 候補の出所（コアの `CandidateSource`）。知らない値は [UNKNOWN] */
internal enum class CandSource(val json: String) {
    LEARNED("learned"),
    USER_DICT("user_dict"),
    LLM("llm"),
    SYSTEM_DICT("system_dict"),
    KANA("kana"),
    REWRITER("rewriter"),
    FUZZY("fuzzy"),
    UNKNOWN("?"),
    ;

    companion object {
        fun fromJson(s: String?): CandSource = values().firstOrNull { it.json == s } ?: UNKNOWN
    }
}

/**
 * 候補 1 件の詳細。[covers] は入力中の候補が置き換える読みの長さ（部分変換。null は読み全体）、
 * [reading] は補正候補（出所 FUZZY）の補正後の読み
 */
internal data class CandidateItem(
    val surface: String,
    val sources: List<CandSource> = emptyList(),
    val covers: Int? = null,
    val reading: String? = null,
)

/** 候補バーに出す候補。[items] は表記だけ、[entries] は出所などを含む詳細（添字は [items] と一致する） */
internal data class Candidates(
    val items: List<String>,
    val selected: Int,
    val visible: Boolean,
    val entries: List<CandidateItem> = items.map { CandidateItem(it) },
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
    /** 入力モードの変化（`hiragana` / `alphanumeric`）。なければ null */
    val mode: String? = null,
    /** 選択中のテキストの再変換を求められた（フロントが選択を取って `reconvert` を送る） */
    val reconvertSelection: Boolean = false,
) {
    /**
     * 続けて返った [next] を重ねた結果。preedit / candidates は変化したときだけ入るので、
     * 後のものがあれば後のものを、無ければ前のものを残す。
     */
    fun merge(next: EngineResult): EngineResult = EngineResult(
        consumed = consumed || next.consumed,
        commit = if (commit != null && next.commit != null) commit + next.commit else next.commit ?: commit,
        deleteBefore = deleteBefore + next.deleteBefore,
        preedit = next.preedit ?: preedit,
        candidates = next.candidates ?: candidates,
        revision = maxOf(revision, next.revision),
        mode = next.mode ?: mode,
        reconvertSelection = reconvertSelection || next.reconvertSelection,
    )
}

internal object Protocol {
    fun parsePreedit(o: JSONObject?): Preedit? {
        if (o == null) return null
        val text = o.optString("text", "")
        val segs: JSONArray? = o.optJSONArray("segments")
        val segments = ArrayList<PreeditSegment>()
        if (segs != null) {
            for (i in 0 until segs.length()) {
                val s = segs.optJSONObject(i) ?: continue
                val range = s.optJSONObject("range")
                segments.add(
                    PreeditSegment(
                        surface = s.optString("surface", ""),
                        reading = s.optString("reading", ""),
                        start = range?.optInt("start", -1) ?: -1,
                        end = range?.optInt("end", -1) ?: -1,
                        kind = s.optString("kind", "japanese"),
                    ),
                )
            }
        }
        var focus = -1
        var fs = -1
        var fe = -1
        if (o.has("focus") && !o.isNull("focus")) {
            focus = o.optInt("focus", -1)
            val seg = segments.getOrNull(focus)
            if (seg != null) {
                fs = seg.start
                fe = seg.end
            }
        }
        var ss = -1
        var se = -1
        val sel = o.optJSONObject("selection")
        if (sel != null) {
            ss = sel.optInt("start", -1)
            se = sel.optInt("end", -1)
        }
        return Preedit(text, o.optInt("cursor", text.length), fs, fe, segments, focus, ss, se)
    }

    fun parseCandidates(o: JSONObject?): Candidates? {
        if (o == null) return null
        val arr = o.optJSONArray("items") ?: JSONArray()
        val items = ArrayList<String>(arr.length())
        val entries = ArrayList<CandidateItem>(arr.length())
        for (i in 0 until arr.length()) {
            val c = arr.optJSONObject(i)
            val surface = c?.optString("surface", "") ?: ""
            items.add(surface)
            val srcArr = c?.optJSONArray("sources")
            val sources = ArrayList<CandSource>()
            if (srcArr != null) {
                for (j in 0 until srcArr.length()) sources.add(CandSource.fromJson(srcArr.optString(j, null)))
            }
            val covers = if (c != null && c.has("covers") && !c.isNull("covers")) c.optInt("covers", -1).takeIf { it >= 0 } else null
            val reading = if (c != null && c.has("reading") && !c.isNull("reading")) c.optString("reading") else null
            entries.add(CandidateItem(surface, sources, covers, reading))
        }
        return Candidates(items, o.optInt("selected", 0), o.optBoolean("visible", true), entries)
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
            mode = if (o.has("mode") && !o.isNull("mode")) o.optString("mode") else null,
            reconvertSelection = o.optBoolean("reconvert_selection", false),
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
