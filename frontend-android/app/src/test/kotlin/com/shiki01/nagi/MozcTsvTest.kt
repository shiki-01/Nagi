package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MozcTsvTest {
    @Test
    fun parsesWordAndComment() {
        val r = MozcTsv.parse("かな\t仮名\t名詞\tメモ\n")
        assertEquals(1, r.entries.size)
        assertEquals(UserDictEntry("かな", "仮名", "名詞", "メモ"), r.entries[0])
        assertTrue(r.issues.isEmpty())
    }

    @Test
    fun katakanaReadingBecomesHiragana() {
        val r = MozcTsv.parse("ナギ\t凪\t人名\n")
        assertEquals("なぎ", r.entries[0].reading)
        // 長音はそのまま
        assertEquals("らーめん", MozcTsv.toHiragana("ラーメン"))
    }

    @Test
    fun skipsCommentsBlankLinesAndReportsBadLines() {
        val text = "# コメント\n! MS-IME\n\nよみ\t語\n\tだけ\t名詞\nあ\t亜\t名詞\r\n"
        val r = MozcTsv.parse(text)
        assertEquals(1, r.entries.size)
        assertEquals("あ", r.entries[0].reading)
        // 列が 3 未満の行(5 行目の前の 4 行目)と、読みが空の行
        assertEquals(listOf(4, 5), r.issues.map { it.line })
    }

    @Test
    fun snippetUnescapesOnParseAndEscapesOnSerialize() {
        val body = "拝啓\n\t敬具\\"
        val entry = UserDictEntry("はい", body, MozcTsv.SNIPPET_POS, "")
        val tsv = MozcTsv.serialize(listOf(entry))
        assertEquals("はい\t拝啓\\n\\t敬具\\\\\tスニペット\t\n", tsv)
        val back = MozcTsv.parse(tsv)
        assertEquals(listOf(entry), back.entries)
    }

    @Test
    fun nonSnippetBackslashIsNotUnescaped() {
        val r = MozcTsv.parse("えん\t\\n\t記号\n")
        assertEquals("\\n", r.entries[0].surface)
    }

    @Test
    fun unescapeKeepsUnknownEscapes() {
        assertEquals("a\\xb", MozcTsv.unescape("a\\xb"))
        assertEquals("末尾\\", MozcTsv.unescape("末尾\\"))
    }

    @Test
    fun serializeThenParseRoundTrips() {
        val list = listOf(
            UserDictEntry("あ", "亜", "名詞", ""),
            UserDictEntry("い", "胃", "名詞", "コメント"),
            UserDictEntry("ぴーす", "(^o^)", "顔文字", ""),
        )
        assertEquals(list, MozcTsv.parse(MozcTsv.serialize(list)).entries)
    }

    @Test
    fun decodeHandlesBomAndUtf16() {
        val utf8Bom = byteArrayOf(0xef.toByte(), 0xbb.toByte(), 0xbf.toByte()) + "あ\t亜\t名詞".toByteArray(Charsets.UTF_8)
        assertEquals("あ\t亜\t名詞", MozcTsv.decode(utf8Bom))
        val utf16le = byteArrayOf(0xff.toByte(), 0xfe.toByte()) + "あ".toByteArray(Charsets.UTF_16LE)
        assertEquals("あ", MozcTsv.decode(utf16le))
        val utf16be = byteArrayOf(0xfe.toByte(), 0xff.toByte()) + "あ".toByteArray(Charsets.UTF_16BE)
        assertEquals("あ", MozcTsv.decode(utf16be))
    }

    @Test
    fun validateRejectsBrokenEntries() {
        assertNotNull(MozcTsv.validate(UserDictEntry("", "亜", "名詞")))
        assertNotNull(MozcTsv.validate(UserDictEntry("あ", " ", "名詞")))
        assertNotNull(MozcTsv.validate(UserDictEntry("あ", "亜\n亜", "名詞")))
        assertNull(MozcTsv.validate(UserDictEntry("あ", "亜\n亜", MozcTsv.SNIPPET_POS)))
        val tooLong = "x".repeat(MozcTsv.MAX_SNIPPET_CHARS + 1)
        assertNotNull(MozcTsv.validate(UserDictEntry("あ", tooLong, MozcTsv.SNIPPET_POS)))
        assertNull(MozcTsv.validate(UserDictEntry("あ", "亜", "名詞")))
    }

    @Test
    fun normalizeFlattensFieldsButKeepsSnippetNewlines() {
        val w = MozcTsv.normalize(UserDictEntry(" ナギ ", " 凪\t ", " 人名 ", "a\tb"))
        assertEquals(UserDictEntry("なぎ", "凪", "人名", "a b"), w)
        val s = MozcTsv.normalize(UserDictEntry("あ", "1行目\r\n2行目", MozcTsv.SNIPPET_POS))
        assertEquals("1行目\n2行目", s.surface)
    }

    @Test
    fun searchMatchesReadingSurfaceAndComment() {
        val e = UserDictEntry("なぎ", "凪", "人名", "Calm")
        assertTrue(MozcTsv.matches(e, ""))
        assertTrue(MozcTsv.matches(e, "ナギ"))
        assertTrue(MozcTsv.matches(e, "凪"))
        assertTrue(MozcTsv.matches(e, "calm"))
        assertFalse(MozcTsv.matches(e, "あめ"))
    }
}
