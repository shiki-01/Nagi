package com.shiki01.nagi

// 候補の分類と、候補バーに出す形への整理（android.* に依存しない純粋関数。iOS の `CandidateKind.swift` と同じ規則）。

/** コアから届いた候補の種類 */
internal enum class CandidateKind {
    /** 候補なし（空、または非表示） */
    HIDDEN,

    /** 入力中（変換前）・変換中の候補 */
    CONVERSION,

    /** 確定直後の次語候補（入力中の読みが無い状態で届く。選ぶとそのまま挿入される） */
    NEXT_WORD,
}

/**
 * 候補バーの 1 回ぶんの表示内容。[pinned] は右端に固定する補正候補の添字（[items] の添字。無ければ null）。
 * [hints] は候補ごとの「もしかして」表示（補正候補でなければ null。[items] と同じ長さ）
 */
internal data class CandidateBar(
    val items: List<String>,
    val selected: Int,
    val kind: CandidateKind,
    val pinned: Int?,
    val hints: List<String?>,
    val readings: List<String?>,
) {
    companion object {
        val EMPTY = CandidateBar(emptyList(), 0, CandidateKind.HIDDEN, null, emptyList(), emptyList())
    }
}

internal object CandidateKinds {
    /**
     * 候補の振り分け。次語候補は、入力中の読みが無い状態で届いた候補のうち、表示中・出所がすべて学習のみ・
     * covers なしのもの。入力中の読みがあるときは出所にかかわらず通常の候補（古い次語候補の通知が遅れて届いても
     * 次語として出さない）
     */
    fun classify(c: Candidates?, hasPreedit: Boolean): CandidateKind {
        if (c == null || !c.visible || c.entries.isEmpty()) return CandidateKind.HIDDEN
        if (hasPreedit) return CandidateKind.CONVERSION
        val allNextWord = c.entries.all { it.covers == null && it.sources == listOf(CandSource.LEARNED) }
        return if (allNextWord) CandidateKind.NEXT_WORD else CandidateKind.CONVERSION
    }

    /** 補正候補（出所に FUZZY を含む）に付ける「もしかして」の表示。補正候補でなければ null */
    fun fuzzyHint(item: CandidateItem): String? {
        if (CandSource.FUZZY !in item.sources) return null
        val r = item.reading
        return if (r.isNullOrEmpty()) "もしかして" else "もしかして: $r"
    }

    /** 右端に固定する補正候補の添字: 補正候補のうちコアの並びで最初のもの。次語候補のときは固定しない */
    fun pinnedFuzzyIndex(hints: List<String?>, nextWord: Boolean): Int? {
        if (nextWord) return null
        val i = hints.indexOfFirst { it != null }
        return if (i >= 0) i else null
    }

    /**
     * 候補バーの表示内容を作る。パスワード欄（[secure]）では次語候補を出さない。
     * 次語候補のときは選択の強調もしない（まだ何も選んでいない）
     */
    fun buildBar(c: Candidates?, hasPreedit: Boolean, secure: Boolean): CandidateBar {
        if (c == null) return CandidateBar.EMPTY
        var kind = classify(c, hasPreedit)
        if (secure && kind == CandidateKind.NEXT_WORD) kind = CandidateKind.HIDDEN
        if (kind == CandidateKind.HIDDEN) return CandidateBar.EMPTY
        val hints = c.entries.map { fuzzyHint(it) }
        val nextWord = kind == CandidateKind.NEXT_WORD
        return CandidateBar(
            items = c.items,
            selected = if (nextWord) -1 else c.selected,
            kind = kind,
            pinned = pinnedFuzzyIndex(hints, nextWord),
            hints = hints,
            readings = c.entries.map { it.reading },
        )
    }

    /** 固定表示の補正候補の読み上げ用の説明 */
    fun pinnedDescription(surface: String, hint: String?): String =
        if (hint == null) surface else "$hint、$surface"
}
