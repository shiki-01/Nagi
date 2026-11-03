package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutClipboardTest {
    @Test
    fun nullと空白は空() {
        assertEquals(LayoutClipboardKind.EMPTY, LayoutClipboard.classify(null))
        assertEquals(LayoutClipboardKind.EMPTY, LayoutClipboard.classify("  \n"))
    }

    @Test
    fun レイアウトのjsonはJSON() {
        val json = """{"id":"my-kana","name":"x","mode":"kana","rows":[]}"""
        assertEquals(LayoutClipboardKind.JSON, LayoutClipboard.classify(json))
        assertEquals(LayoutClipboardKind.JSON, LayoutClipboard.classify("\n  $json  "))
    }

    @Test
    fun 別のjsonや壊れたjsonはOTHER() {
        assertEquals(LayoutClipboardKind.OTHER, LayoutClipboard.classify("""{"a":1}"""))
        assertEquals(LayoutClipboardKind.OTHER, LayoutClipboard.classify("""{"id":"a","rows":"""))
        assertEquals(LayoutClipboardKind.OTHER, LayoutClipboard.classify("こんにちは"))
    }

    @Test
    fun nagiのレイアウトリンクはLINK() {
        assertEquals(LayoutClipboardKind.LINK, LayoutClipboard.classify("nagi://layout?data=abc"))
        assertEquals(LayoutClipboardKind.LINK, LayoutClipboard.classify("  NAGI://layout?data=abc\n"))
    }

    @Test
    fun layout以外のnagiリンクはOTHER() {
        assertEquals(LayoutClipboardKind.OTHER, LayoutClipboard.classify("nagi://other?data=abc"))
    }

    @Test
    fun 案内文にサイトのURLを含む() {
        assertTrue(LayoutClipboard.NOT_FOUND.contains("nagi.shiki-01.com/custom-keyboard"))
    }

    @Test
    fun 取り込み後の選択肢はモード別() {
        fun slots(m: LayoutMode?) = PostImportChoices.build(m).map { it.slot }
        assertEquals(listOf(AssignSlot.KANA, null), slots(LayoutMode.KANA))
        assertEquals(listOf(AssignSlot.ALPHA, null), slots(LayoutMode.ALPHA))
        assertEquals(listOf(AssignSlot.KANA, AssignSlot.ALPHA, null), slots(LayoutMode.CUSTOM))
        assertTrue(slots(LayoutMode.NUMBER).isEmpty())
        assertTrue(slots(LayoutMode.SYMBOL).isEmpty())
        assertTrue(slots(LayoutMode.EMOJI).isEmpty())
        assertTrue(slots(null).isEmpty())
    }

    @Test
    fun 選択肢のラベル() {
        assertEquals(
            listOf("かな入力で使う", "英字入力で使う", "あとで"),
            PostImportChoices.build(LayoutMode.CUSTOM).map { it.label },
        )
    }

    @Test
    fun 割り当ての補足行() {
        assertEquals("かな: A / 英字: B", LayoutAssignSummary.text("A", "B"))
        assertEquals("zz", LayoutAssignSummary.nameOrId("zz", null))
    }
}
