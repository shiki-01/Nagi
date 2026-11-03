package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutConflictTest {
    private val presets = setOf("flick-kana", "numbers")

    @Test
    fun ファイルと貼り付けは同梱と同じidを許可する() {
        assertNull(LayoutConflict.rejection("numbers", presets, true))
        assertNull(LayoutConflict.rejection("mine", presets, true))
    }

    @Test
    fun URL経由は同梱と同じidを拒否する() {
        val reason = LayoutConflict.rejection("numbers", presets, false)
        assertNotNull(reason)
        assertTrue(reason!!.contains("numbers"))
        assertNull(LayoutConflict.rejection("mine", presets, false))
    }

    @Test
    fun 同じidのユーザーのレイアウトは置き換え() {
        assertTrue(LayoutConflict.replaces("mine", setOf("mine", "other")))
        assertFalse(LayoutConflict.replaces("new", setOf("mine")))
        assertTrue(LayoutConflict.overridesPreset("flick-kana", presets))
        assertFalse(LayoutConflict.overridesPreset("mine", presets))
    }

    @Test
    fun 組み込みの種類の選択() {
        // 数字・記号・絵文字を開いているときは数字の画面
        assertEquals(BuiltinLayouts.Kind.NUMBERS, BuiltinLayouts.kindFor("numbers", true, true))
        assertEquals(BuiltinLayouts.Kind.NUMBERS, BuiltinLayouts.kindFor("symbols", false, false))
        // かなで 12 キー設定なら 12 キー、それ以外は QWERTY
        assertEquals(BuiltinLayouts.Kind.FLICK, BuiltinLayouts.kindFor(null, true, true))
        assertEquals(BuiltinLayouts.Kind.QWERTY, BuiltinLayouts.kindFor(null, true, false))
        assertEquals(BuiltinLayouts.Kind.QWERTY, BuiltinLayouts.kindFor(null, false, true))
        // 自作のレイアウトを開いていて読めない場合は、かな / 英字の判定に従う
        assertEquals(BuiltinLayouts.Kind.FLICK, BuiltinLayouts.kindFor("my-layout", true, true))
    }

    @Test
    fun レイアウトidの判定() {
        assertTrue(LayoutIds.isValid("flick-kana"))
        assertFalse(LayoutIds.isValid("../etc"))
        assertFalse(LayoutIds.isValid("A"))
        assertFalse(LayoutIds.isValid(""))
        assertEquals("flick-kana", LayoutIds.defaultFor(true))
        assertEquals("qwerty-alpha", LayoutIds.defaultFor(false))
    }
}
