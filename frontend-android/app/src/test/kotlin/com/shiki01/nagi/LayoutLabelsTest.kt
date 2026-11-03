package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutLabelsTest {
    private fun key(action: KeyAction, label: String? = null, icon: String? = null) =
        LayoutKey("k", 1.0, label, icon, KeyRole.NORMAL, action, emptyList())

    // ---- 改行・空白のラベル ----

    @Test
    fun 改行キーのラベル() {
        // 入力中は「確定」
        assertEquals("確定", LayoutLabels.enterLabel(true, "検索", "改行"))
        // 入力欄の指定があればそれ
        assertEquals("検索", LayoutLabels.enterLabel(false, "検索", "改行"))
        // 無ければレイアウトの label、それも無ければ「改行」
        assertEquals("改行", LayoutLabels.enterLabel(false, null, "改行"))
        assertEquals("⏎", LayoutLabels.enterLabel(false, null, "⏎"))
        assertEquals("改行", LayoutLabels.enterLabel(false, null, null))
    }

    @Test
    fun 空白キーのラベル() {
        assertEquals("変換", LayoutLabels.spaceLabel(true, true, "空白"))
        // 英字（かなでない）では入力中でも変換にしない
        assertEquals("空白", LayoutLabels.spaceLabel(true, false, "空白"))
        assertEquals("空白", LayoutLabels.spaceLabel(false, true, null))
        assertEquals("space", LayoutLabels.spaceLabel(false, true, "space"))
    }

    // ---- 表示文字 ----

    @Test
    fun 静的な表示文字の優先順位() {
        assertEquals("L", LayoutLabels.staticLabel(key(KeyAction.Text("a", null), label = "L")))
        assertEquals("a", LayoutLabels.staticLabel(key(KeyAction.Text("a", null))))
        assertEquals("⇧", LayoutLabels.staticLabel(key(KeyAction.Act("shift"), icon = "shift")))
        assertEquals("⇧", LayoutLabels.staticLabel(key(KeyAction.Act("shift"), icon = "sf:shift.fill")))
        // label があれば icon より優先（Android は文字で描くため）
        assertEquals("S", LayoutLabels.staticLabel(key(KeyAction.Act("shift"), label = "S", icon = "shift")))
        assertEquals("あ", LayoutLabels.staticLabel(key(KeyAction.Toggle(listOf("あ", "い")))))
        assertEquals("空白", LayoutLabels.staticLabel(key(KeyAction.Act("space"))))
        assertEquals("numbers", LayoutLabels.staticLabel(key(KeyAction.LayoutSwitch("numbers"))))
        assertEquals("", LayoutLabels.staticLabel(key(KeyAction.Spacer)))
        assertNull(LayoutLabels.iconGlyph("sf:no.such.symbol"))
        assertNull(LayoutLabels.iconGlyph(null))
    }

    @Test
    fun Shift中の英字() {
        val a = KeyAction.Text("q", "Q")
        assertEquals("Q", LayoutLabels.shifted(a, true))
        assertEquals("q", LayoutLabels.shifted(a, false))
        // shift 指定が無ければ大文字化
        assertEquals("W", LayoutLabels.shifted(KeyAction.Text("w", null), true))
        // 記号などは shift 指定が優先
        assertEquals("!", LayoutLabels.shifted(KeyAction.Text("1", "!"), true))
        assertEquals("Q", LayoutLabels.textKeyFace(key(a), a, true))
        assertEquals("q", LayoutLabels.textKeyFace(key(a), a, false))
    }

    // ---- トグル入力 ----

    @Test
    fun トグル入力は同じキーの連打で循環する() {
        // 初回
        assertEquals(0, ToggleLogic.nextIndex(null, 5, false, 0))
        // 続き
        assertEquals(1, ToggleLogic.nextIndex(0, 5, true, 300))
        assertEquals(2, ToggleLogic.nextIndex(1, 5, true, 300))
        // 末尾の次は先頭
        assertEquals(0, ToggleLogic.nextIndex(4, 5, true, 300))
    }

    @Test
    fun トグル入力は時間切れ_別キー_1要素では続かない() {
        assertEquals(0, ToggleLogic.nextIndex(2, 5, true, ToggleLogic.TIMEOUT_MS))
        assertEquals(0, ToggleLogic.nextIndex(2, 5, true, ToggleLogic.TIMEOUT_MS + 1))
        assertEquals(0, ToggleLogic.nextIndex(2, 5, false, 100))
        assertEquals(0, ToggleLogic.nextIndex(0, 1, true, 100))
        assertTrue(ToggleLogic.continues(5, true, ToggleLogic.TIMEOUT_MS - 1))
        assertFalse(ToggleLogic.continues(5, true, ToggleLogic.TIMEOUT_MS))
        assertFalse(ToggleLogic.continues(1, true, 10))
        assertFalse(ToggleLogic.continues(5, false, 10))
        assertFalse(ToggleLogic.continues(5, true, -1))
    }

    // ---- 単語削除 ----

    @Test
    fun 単語削除の文字数() {
        assertEquals(5, TextOps.wordDeleteLength("hello"))
        assertEquals(5, TextOps.wordDeleteLength("say hello"))
        // 末尾の空白は飛ばして直前の単語まで（空白も一緒に消える）
        assertEquals(8, TextOps.wordDeleteLength("say hello   "))
        // 文字の種類が変わるところまで（英数字 / かな / 漢字 / 記号）
        assertEquals(3, TextOps.wordDeleteLength("abcあいう"))
        assertEquals(3, TextOps.wordDeleteLength("日本語"))
        assertEquals(1, TextOps.wordDeleteLength("foo,"))
        assertEquals(0, TextOps.wordDeleteLength(""))
        // 空白だけなら全部
        assertEquals(3, TextOps.wordDeleteLength("   "))
    }
}
