package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class IconifyLogicTest {
    private fun shape(body: String) = IconifyLogic.shapeFromBody(body, 0.0, 0.0, 24.0, 24.0)

    // ---- 名前の検証 ----

    @Test
    fun iconify形式だけを読む() {
        assertEquals(IconRef("mdi", "home"), IconifyLogic.parse("iconify:mdi:home"))
        assertEquals(IconRef("material-symbols", "arrow-back-2"), IconifyLogic.parse("iconify:material-symbols:arrow-back-2"))
        assertNull(IconifyLogic.parse("sf:globe"))
        assertNull(IconifyLogic.parse("globe"))
        assertNull(IconifyLogic.parse(null))
        assertNull(IconifyLogic.parse("iconify:mdi"))
        assertNull(IconifyLogic.parse("iconify:mdi:home:extra"))
        assertNull(IconifyLogic.parse("iconify:MDI:home"))
        assertNull(IconifyLogic.parse("iconify:mdi:ho/me"))
        assertNull(IconifyLogic.parse("iconify:mdi:-home"))
        assertNull(IconifyLogic.parse("iconify::home"))
    }

    @Test
    fun 保存ファイル名とURL() {
        val r = IconRef("mdi", "home")
        assertEquals("mdi:home", r.id)
        assertEquals("mdi__home.json", r.fileName)
        assertEquals("https://api.iconify.design/mdi.json?icons=home,account", IconifyLogic.apiUrl("mdi", listOf("home", "account")))
    }

    @Test
    fun レイアウトのアイコンを重複なく集める() {
        val json = LayoutFixtures.minimal(
            key = """{"id": "a", "kind": "text", "text": "a", "icon": "iconify:mdi:home",
                "longPress": [{"label": "x", "kind": "text", "text": "x", "icon": "iconify:tabler:cut"}]},
               {"id": "b", "kind": "text", "text": "b", "icon": "iconify:mdi:home"},
               {"id": "c", "kind": "action", "action": "shift", "icon": "shift"}""",
        )
        val result = LayoutFixtures.parse(json)
        assertTrue((result as? LayoutParseResult.Err)?.reason, result is LayoutParseResult.Ok)
        val layout = (result as LayoutParseResult.Ok).layout
        assertEquals(listOf(IconRef("mdi", "home"), IconRef("tabler", "cut")), IconifyLogic.collect(layout))
    }

    @Test
    fun 通知文() {
        assertEquals("", IconifyLogic.failureNote(emptyList()))
        assertTrue(IconifyLogic.failureNote(listOf("mdi:x", "mdi:y")).contains("取得できなかったアイコン: mdi:x、mdi:y"))
    }

    // ---- SVG の body ----

    @Test
    fun pathを取り出す() {
        val s = shape("""<path fill="currentColor" d="M10 20h4v-8z"/>""")!!
        assertEquals(1, s.paths.size)
        val p = s.paths[0]
        assertEquals("M10 20h4v-8z", p.d)
        assertTrue(p.fill)
        assertFalse(p.stroke)
    }

    @Test
    fun gから線の属性を継承する() {
        val s = shape(
            """<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2">
               <path d="M5 12h14"/><path d="M12 5v14" stroke-width="3"/></g>""",
        )!!
        assertEquals(2, s.paths.size)
        assertFalse(s.paths[0].fill)
        assertTrue(s.paths[0].stroke)
        assertEquals(2.0, s.paths[0].strokeWidth, 0.0)
        assertEquals("round", s.paths[0].cap)
        assertEquals("round", s.paths[0].join)
        assertEquals(3.0, s.paths[1].strokeWidth, 0.0)
    }

    @Test
    fun gを閉じたあとは継承しない() {
        val s = shape("""<g fill="none" stroke="currentColor"><path d="M0 0h1"/></g><path d="M2 2h1"/>""")!!
        assertEquals(2, s.paths.size)
        assertTrue(s.paths[1].fill)
        assertFalse(s.paths[1].stroke)
    }

    @Test
    fun 描かないものは捨てる() {
        // fill も stroke も無い・defs の中身・mask の中身
        assertNull(shape("""<path fill="none" d="M0 0h1"/>"""))
        assertNull(shape("""<defs><path id="a" d="M0 0h1"/></defs>"""))
        val s = shape("""<mask id="m"><path d="M0 0h9"/></mask><path d="M1 1h2"/>""")!!
        assertEquals(listOf("M1 1h2"), s.paths.map { it.d })
    }

    @Test
    fun 図形をpathに変換する() {
        val c = shape("""<circle cx="12" cy="12" r="5"/>""")!!.paths[0].d
        assertEquals("M7 12a5 5 0 1 0 10 0a5 5 0 1 0 -10 0Z", c)
        val line = shape("""<line x1="1" y1="2" x2="3.5" y2="4" stroke="currentColor"/>""")!!.paths[0].d
        assertEquals("M1 2L3.5 4", line)
        val poly = shape("""<polygon points="0,0 4,0 4,4"/>""")!!.paths[0].d
        assertEquals("M0 0L4 0L4 4Z", poly)
        val pl = shape("""<polyline points="0 0 4 0" fill="none" stroke="currentColor"/>""")!!.paths[0].d
        assertEquals("M0 0L4 0", pl)
        val r = shape("""<rect x="2" y="3" width="10" height="6"/>""")!!.paths[0].d
        assertEquals("M2 3h10v6h-10Z", r)
        val rr = shape("""<rect x="0" y="0" width="10" height="6" rx="1"/>""")!!.paths[0].d
        assertTrue(rr.startsWith("M1 0h8a1 1 0 0 1 1 1"))
        val e = shape("""<ellipse cx="5" cy="5" rx="4" ry="2"/>""")!!.paths[0].d
        assertTrue(e.startsWith("M1 5a4 2 0 1 0 8 0"))
    }

    @Test
    fun 不正な図形は読み飛ばす() {
        assertNull(shape("""<circle cx="1" cy="1"/>"""))
        assertNull(shape("""<polygon points="1,2,3"/>"""))
        assertNull(shape("""<rect width="0" height="3"/>"""))
        assertNull(shape("""<path/>"""))
        assertNull(shape(""))
        assertNull(shape("not svg"))
    }

    @Test
    fun evenoddとviewBoxの検査() {
        assertTrue(shape("""<path fill-rule="evenodd" d="M0 0h1"/>""")!!.paths[0].evenOdd)
        assertNull(IconifyLogic.shapeFromBody("""<path d="M0 0h1"/>""", 0.0, 0.0, 0.0, 24.0))
        assertNull(IconifyLogic.shapeFromBody("""<path d="M0 0h1"/>""", 0.0, 0.0, 24.0, Double.NaN))
    }

    // ---- API の応答 ----

    @Test
    fun 応答からアイコンを取り出す() {
        val text = """{"prefix":"mdi","width":24,"height":24,
            "icons":{"home":{"body":"<path fill=\"currentColor\" d=\"M10 20z\"/>"},
                     "wide":{"body":"<path d=\"M0 0h1\"/>","width":32}},
            "aliases":{"house":{"parent":"home"}},
            "not_found":["nope"]}"""
        val r = IconifyLogic.parseResponse(text, listOf("home", "wide", "house", "nope"))
        assertEquals(setOf("home", "wide", "house"), r.shapes.keys)
        assertEquals(listOf("nope"), r.missing)
        assertEquals(24.0, r.shapes["home"]!!.width, 0.0)
        assertEquals(32.0, r.shapes["wide"]!!.width, 0.0)
        assertEquals(24.0, r.shapes["wide"]!!.height, 0.0)
    }

    @Test
    fun 既定の大きさは16() {
        val r = IconifyLogic.parseResponse("""{"icons":{"a":{"body":"<path d=\"M0 0h1\"/>"}}}""", listOf("a"))
        assertEquals(16.0, r.shapes["a"]!!.width, 0.0)
        assertEquals(16.0, r.shapes["a"]!!.height, 0.0)
    }

    @Test
    fun 壊れた応答は全部取得できなかった扱い() {
        val r = IconifyLogic.parseResponse("<html>", listOf("a", "b"))
        assertTrue(r.shapes.isEmpty())
        assertEquals(listOf("a", "b"), r.missing)
        val empty = IconifyLogic.parseResponse("{}", listOf("a"))
        assertEquals(listOf("a"), empty.missing)
    }

    // ---- 保存形式 ----

    @Test
    fun エンコードとデコードは往復する() {
        val src = IconShape(
            0.0, 1.0, 24.0, 20.0,
            listOf(
                IconPath("M0 0h1", fill = true, stroke = false, strokeWidth = 1.0, cap = "butt", join = "miter", evenOdd = true),
                IconPath("M2 2h1", fill = false, stroke = true, strokeWidth = 2.5, cap = "round", join = "bevel", evenOdd = false),
            ),
        )
        assertEquals(src, IconifyLogic.decode(IconifyLogic.encode(src)))
    }

    @Test
    fun 壊れた保存データはnull() {
        assertNull(IconifyLogic.decode("oops"))
        assertNull(IconifyLogic.decode("""{"v":2,"w":24,"h":24,"p":[{"d":"M0 0"}]}"""))
        assertNull(IconifyLogic.decode("""{"v":1,"w":0,"h":24,"p":[{"d":"M0 0"}]}"""))
        assertNull(IconifyLogic.decode("""{"v":1,"w":24,"h":24,"p":[]}"""))
        assertNull(IconifyLogic.decode("""{"v":1,"w":24,"h":24}"""))
        assertNotNull(IconifyLogic.decode("""{"v":1,"w":24,"h":24,"p":[{"d":"M0 0h1"}]}"""))
    }
}
