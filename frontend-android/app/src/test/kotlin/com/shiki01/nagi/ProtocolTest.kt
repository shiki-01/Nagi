package com.shiki01.nagi

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtocolTest {
    // core の実測（m2-notes B-2）と同じ形: 今日 | はあ | め、注目 1、選択 0..4
    private val convertingPreedit = """
        {"cursor":5,"focus":1,
         "segments":[
           {"kind":"japanese","range":{"start":0,"end":2},"reading":"きょう","surface":"今日"},
           {"kind":"japanese","range":{"start":2,"end":4},"reading":"はあ","surface":"はあ"},
           {"kind":"japanese","range":{"start":4,"end":5},"reading":"め","surface":"め"}],
         "selection":{"start":0,"end":4},
         "text":"今日はあめ"}
    """.trimIndent()

    @Test
    fun preeditの文節と注目と選択を読む() {
        val p = Protocol.parsePreedit(JSONObject(convertingPreedit))
        assertNotNull(p)
        p!!
        assertEquals("今日はあめ", p.text)
        assertEquals(5, p.cursor)
        assertEquals(3, p.segments.size)
        assertEquals("今日", p.segments[0].surface)
        assertEquals("きょう", p.segments[0].reading)
        assertEquals(2, p.segments[1].start)
        assertEquals(4, p.segments[1].end)
        assertEquals(1, p.focus)
        // 注目文節の範囲は segments[focus].range
        assertEquals(2, p.focusStart)
        assertEquals(4, p.focusEnd)
        assertEquals(0, p.selectionStart)
        assertEquals(4, p.selectionEnd)
    }

    @Test
    fun 入力中のpreeditは注目も選択も無い() {
        val o = JSONObject(
            """{"cursor":3,"segments":[{"kind":"japanese","range":{"start":0,"end":3},"reading":"かきく","surface":"かきく"}],"text":"かきく"}""",
        )
        val p = Protocol.parsePreedit(o)!!
        assertEquals(-1, p.focus)
        assertEquals(-1, p.focusStart)
        assertEquals(-1, p.selectionStart)
        assertEquals(1, p.segments.size)
    }

    @Test
    fun segmentsが無くても読める() {
        val p = Protocol.parsePreedit(JSONObject("""{"text":"あ"}"""))!!
        assertEquals("あ", p.text)
        assertEquals(1, p.cursor)
        assertTrue(p.segments.isEmpty())
    }

    @Test
    fun nullはnull() {
        assertNull(Protocol.parsePreedit(null))
        assertNull(Protocol.parseCandidates(null))
        assertNull(Protocol.parseResult(null))
    }

    @Test
    fun 候補を読む() {
        val o = JSONObject(
            """{"visible":true,"selected":1,"items":[{"surface":"今日"},{"surface":"京"},{"surface":"きょう"}]}""",
        )
        val c = Protocol.parseCandidates(o)!!
        assertEquals(listOf("今日", "京", "きょう"), c.items)
        assertEquals(1, c.selected)
        assertTrue(c.visible)
    }

    @Test
    fun 結果を読む() {
        val o = JSONObject(
            """{"commit":"じゃ","consumed":true,"delete_before":2,"revision":4,"mode":"alphanumeric",
                "reconvert_selection":true,"preedit":{"cursor":0,"segments":[],"text":""}}""",
        )
        val r = Protocol.parseResult(o)!!
        assertTrue(r.consumed)
        assertEquals("じゃ", r.commit)
        assertEquals(2, r.deleteBefore)
        assertEquals(4, r.revision)
        assertEquals("alphanumeric", r.mode)
        assertTrue(r.reconvertSelection)
        assertEquals("", r.preedit!!.text)
        assertNull(r.candidates)
    }

    @Test
    fun 消費されなかった結果() {
        val r = Protocol.parseResult(JSONObject("""{"consumed":false,"revision":4}"""))!!
        assertFalse(r.consumed)
        assertNull(r.commit)
        assertNull(r.preedit)
        assertNull(r.mode)
        assertFalse(r.reconvertSelection)
    }

    @Test
    fun 通知を読む() {
        val n = Protocol.parseNotification(
            "update_candidates",
            JSONObject("""{"revision":7,"candidates":{"visible":true,"selected":0,"items":[{"surface":"雨"}]}}"""),
        )!!
        assertEquals(7, n.revision)
        assertEquals(listOf("雨"), n.candidates!!.items)
        assertNull(Protocol.parseNotification("other", JSONObject()))
        assertNull(Protocol.parseNotification("update_preedit", null))
    }

    @Test
    fun 続けた結果は後のpreeditと候補を優先して重ねる() {
        val a = EngineResult(
            consumed = true,
            preedit = Preedit("a", 1, -1, -1, focus = 0),
            candidates = Candidates(listOf("x"), 0, true),
            revision = 3,
        )
        val b = EngineResult(consumed = true, preedit = Preedit("b", 1, -1, -1, focus = 1), revision = 4)
        val m = a.merge(b)
        assertEquals("b", m.preedit!!.text)
        assertEquals(listOf("x"), m.candidates!!.items)
        assertEquals(4, m.revision)
        assertTrue(m.consumed)
    }
}
