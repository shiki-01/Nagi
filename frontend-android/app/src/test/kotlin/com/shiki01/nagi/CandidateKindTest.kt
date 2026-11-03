package com.shiki01.nagi

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CandidateKindTest {
    private fun item(
        s: String,
        vararg src: CandSource,
        covers: Int? = null,
        reading: String? = null,
    ) = CandidateItem(s, src.toList(), covers, reading)

    private fun cands(vararg e: CandidateItem, visible: Boolean = true, selected: Int = 0) =
        Candidates(e.map { it.surface }, selected, visible, e.toList())

    @Test
    fun 入力中の読みが無く出所が学習のみなら次語候補() {
        val c = cands(item("です", CandSource.LEARNED), item("ます", CandSource.LEARNED))
        assertEquals(CandidateKind.NEXT_WORD, CandidateKinds.classify(c, hasPreedit = false))
    }

    @Test
    fun 入力中の読みがあれば出所にかかわらず通常の候補() {
        val c = cands(item("です", CandSource.LEARNED))
        assertEquals(CandidateKind.CONVERSION, CandidateKinds.classify(c, hasPreedit = true))
    }

    @Test
    fun 学習以外の出所やcoversがあれば通常の候補() {
        assertEquals(
            CandidateKind.CONVERSION,
            CandidateKinds.classify(cands(item("a", CandSource.LEARNED), item("b", CandSource.SYSTEM_DICT)), false),
        )
        assertEquals(
            CandidateKind.CONVERSION,
            CandidateKinds.classify(cands(item("a", CandSource.LEARNED, covers = 2)), false),
        )
        // 出所が複数（学習 + 辞書）の候補も次語ではない
        assertEquals(
            CandidateKind.CONVERSION,
            CandidateKinds.classify(cands(item("a", CandSource.LEARNED, CandSource.SYSTEM_DICT)), false),
        )
    }

    @Test
    fun 非表示や空は隠す() {
        assertEquals(CandidateKind.HIDDEN, CandidateKinds.classify(null, false))
        assertEquals(CandidateKind.HIDDEN, CandidateKinds.classify(cands(visible = false), false))
        assertEquals(
            CandidateKind.HIDDEN,
            CandidateKinds.classify(cands(item("a", CandSource.LEARNED), visible = false), false),
        )
    }

    @Test
    fun もしかしての表示は補正候補だけに付く() {
        assertNull(CandidateKinds.fuzzyHint(item("漢字", CandSource.SYSTEM_DICT)))
        assertEquals("もしかして", CandidateKinds.fuzzyHint(item("漢字", CandSource.FUZZY)))
        assertEquals("もしかして: かんじ", CandidateKinds.fuzzyHint(item("漢字", CandSource.FUZZY, reading = "かんじ")))
        assertEquals("もしかして", CandidateKinds.fuzzyHint(item("漢字", CandSource.FUZZY, reading = "")))
    }

    @Test
    fun 固定表示は最初の補正候補で次語候補のときは無し() {
        val hints = listOf(null, "もしかして: a", "もしかして: b")
        assertEquals(1, CandidateKinds.pinnedFuzzyIndex(hints, nextWord = false))
        assertNull(CandidateKinds.pinnedFuzzyIndex(hints, nextWord = true))
        assertNull(CandidateKinds.pinnedFuzzyIndex(listOf(null, null), nextWord = false))
        assertNull(CandidateKinds.pinnedFuzzyIndex(emptyList(), nextWord = false))
    }

    @Test
    fun 候補バーの固定表示の添字は元の候補の添字() {
        val c = cands(
            item("青豚", CandSource.LLM),
            item("青豚", CandSource.FUZZY, reading = "あおぶた"),
            item("あおぶた", CandSource.KANA),
        )
        val bar = CandidateKinds.buildBar(c, hasPreedit = true, secure = false)
        assertEquals(CandidateKind.CONVERSION, bar.kind)
        assertEquals(1, bar.pinned)
        assertEquals(listOf("青豚", "青豚", "あおぶた"), bar.items)
        assertEquals("あおぶた", bar.readings[1])
    }

    @Test
    fun パスワード欄では次語候補を出さない() {
        val c = cands(item("です", CandSource.LEARNED))
        assertEquals(CandidateBar.EMPTY, CandidateKinds.buildBar(c, hasPreedit = false, secure = true))
        // 入力中の候補は出す
        assertEquals(CandidateKind.CONVERSION, CandidateKinds.buildBar(c, hasPreedit = true, secure = true).kind)
    }

    @Test
    fun 次語候補は選択の強調をしない() {
        val c = cands(item("です", CandSource.LEARNED), selected = 0)
        val bar = CandidateKinds.buildBar(c, hasPreedit = false, secure = false)
        assertEquals(CandidateKind.NEXT_WORD, bar.kind)
        assertEquals(-1, bar.selected)
        assertNull(bar.pinned)
    }

    @Test
    fun 固定表示の読み上げ() {
        assertEquals("もしかして: かんじ、漢字", CandidateKinds.pinnedDescription("漢字", "もしかして: かんじ"))
        assertEquals("漢字", CandidateKinds.pinnedDescription("漢字", null))
    }

    @Test
    fun parseCandidatesが出所とcoversと読みを保つ() {
        val o = JSONObject(
            """{"items":[
              {"surface":"青豚","sources":["fuzzy","llm"],"reading":"あおぶた"},
              {"surface":"今日","sources":["system_dict"],"covers":2},
              {"surface":"x"}],
              "selected":1,"visible":true}""",
        )
        val c = Protocol.parseCandidates(o)!!
        assertEquals(listOf("青豚", "今日", "x"), c.items)
        assertEquals(listOf(CandSource.FUZZY, CandSource.LLM), c.entries[0].sources)
        assertEquals("あおぶた", c.entries[0].reading)
        assertNull(c.entries[0].covers)
        assertEquals(2, c.entries[1].covers)
        assertNull(c.entries[1].reading)
        assertTrue(c.entries[2].sources.isEmpty())
        assertEquals(1, c.selected)
    }

    @Test
    fun 知らない出所はUNKNOWN() {
        val c = Protocol.parseCandidates(JSONObject("""{"items":[{"surface":"a","sources":["future"]}],"visible":true}"""))!!
        assertEquals(listOf(CandSource.UNKNOWN), c.entries[0].sources)
        assertFalse(CandSource.UNKNOWN in listOf(CandSource.LEARNED))
    }

    @Test
    fun 既存の3引数のCandidatesはentriesを自動で作る() {
        val c = Candidates(listOf("a", "b"), 0, true)
        assertEquals(2, c.entries.size)
        assertEquals("b", c.entries[1].surface)
    }
}
