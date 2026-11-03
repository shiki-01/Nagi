package com.shiki01.nagi

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class DiagFormatTest {
    @Test
    fun lineIsSingleLine() {
        assertEquals("12:00:00.000 core 起動成功", DiagFormat.line("12:00:00.000", "core", "起動成功"))
        val l = DiagFormat.line("t", "a b", "x\ny\r\nz\tw")
        assertFalse(l.contains('\n'))
        assertFalse(l.contains('\r'))
        assertFalse(l.contains('\t'))
        assertEquals("t a_b x y  z w", l)
    }

    @Test
    fun longMessageIsCut() {
        val l = DiagFormat.line("t", "k", "a".repeat(1000))
        assertEquals("t k ".length + DiagFormat.MAX_MESSAGE_CHARS, l.length)
    }

    @Test
    fun smallLogIsUntouched() {
        val b = "line1\nline2\n".toByteArray()
        assertSame(b, DiagFormat.trimTail(b, 100, 50))
    }

    @Test
    fun overLimitKeepsLatterHalfFromLineStart() {
        val lines = (1..50).map { "line%02d".format(it) } // 各 6 文字 + 改行 = 7 バイト
        val bytes = (lines.joinToString("\n") + "\n").toByteArray()
        assertEquals(350, bytes.size)
        val out = DiagFormat.trimTail(bytes, 300, 100)
        val text = String(out)
        assertTrue(out.size <= 100)
        assertTrue(text.startsWith("line"))
        assertTrue(text.endsWith("line50\n"))
        // 先頭は行の頭にそろう
        assertTrue(text.lines().first().length == 6)
    }

    @Test
    fun noNewlineSkipsUtf8ContinuationBytes() {
        val bytes = "あ".repeat(200).toByteArray() // 3 バイト × 200
        val out = DiagFormat.trimTail(bytes, 300, 100)
        val text = String(out)
        assertFalse(text.contains('�'))
        assertTrue(out.size <= 100)
        assertArrayEquals(text.toByteArray(), out)
    }
}
