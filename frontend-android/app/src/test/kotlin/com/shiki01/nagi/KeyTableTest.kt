package com.shiki01.nagi

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class KeyTableTest {
    private val sample = """
        {"keys":[
          {"center":"あ","down":"お","key":"a","left":"い","right":"え","toggle":["あ","い","う","え","お"],"up":"う"},
          {"center":"、","down":"…","key":"punct","left":"。","right":"！","toggle":["、","。","？","！","…"],"up":"？"}
        ],"variants":[["か","が"]]}
    """.trimIndent()

    @Test
    fun get_key_tableの結果を読む() {
        val t = KeyTables.parse(JSONObject(sample))
        assertNotNull(t)
        t!!
        assertEquals(2, t.size)
        assertEquals("a", t[0].key)
        assertEquals("あ", t[0].center)
        assertEquals("い", t[0].left)
        assertEquals("う", t[0].up)
        assertEquals("え", t[0].right)
        assertEquals("お", t[0].down)
        assertEquals(listOf("あ", "い", "う", "え", "お"), t[0].toggle)
        assertEquals("punct", t[1].key)
    }

    @Test
    fun 読めなければnull() {
        assertNull(KeyTables.parse(null))
        assertNull(KeyTables.parse(JSONObject("{}")))
        assertNull(KeyTables.parse(JSONObject("""{"keys":[]}""")))
    }

    @Test
    fun フォールバック表は11キーで並びがコアと同じ() {
        val f = KeyTables.FALLBACK
        assertEquals(listOf("a", "ka", "sa", "ta", "na", "ha", "ma", "ya", "ra", "wa", "punct"), f.map { it.key })
        for (d in f) {
            assertEquals(listOf(d.center, d.left, d.up, d.right, d.down), d.toggle)
        }
        val ya = f.first { it.key == "ya" }
        assertEquals("（", ya.left)
        assertEquals("）", ya.right)
    }

    @Test
    fun ジェスチャーから文字を引く() {
        val sa = KeyTables.FALLBACK.first { it.key == "sa" }
        assertEquals("さ", KeyTables.charFor(sa, "tap"))
        assertEquals("さ", KeyTables.charFor(sa, "toggle"))
        assertEquals("し", KeyTables.charFor(sa, "flick_left"))
        assertEquals("す", KeyTables.charFor(sa, "flick_up"))
        assertEquals("せ", KeyTables.charFor(sa, "flick_right"))
        assertEquals("そ", KeyTables.charFor(sa, "flick_down"))
    }

    @Test
    fun 候補グリッドの列数() {
        assertEquals(4, CandidateGrid.columns(listOf("今日", "京")))
        assertEquals(3, CandidateGrid.columns(listOf("今日は", "きょうは雨")))
        assertEquals(2, CandidateGrid.columns(listOf("きょうはいいてんきですね")))
        assertEquals(4, CandidateGrid.columns(emptyList()))
    }
}
