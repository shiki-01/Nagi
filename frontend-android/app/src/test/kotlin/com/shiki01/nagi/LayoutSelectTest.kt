package com.shiki01.nagi

import android.text.InputType
import android.view.inputmethod.EditorInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutSelectTest {
    private val text = InputType.TYPE_CLASS_TEXT

    @Test
    fun 入力欄の種類を判定する() {
        assertEquals(FieldKind.NORMAL, FieldKind.fromInputType(0))
        assertEquals(FieldKind.NORMAL, FieldKind.fromInputType(text))
        assertEquals(FieldKind.NORMAL, FieldKind.fromInputType(text or InputType.TYPE_TEXT_FLAG_MULTI_LINE))
        assertEquals(FieldKind.NORMAL, FieldKind.fromInputType(text or InputType.TYPE_TEXT_VARIATION_SHORT_MESSAGE))
        assertEquals(FieldKind.EMAIL, FieldKind.fromInputType(text or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS))
        assertEquals(FieldKind.EMAIL, FieldKind.fromInputType(text or InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS))
        assertEquals(FieldKind.URL, FieldKind.fromInputType(text or InputType.TYPE_TEXT_VARIATION_URI))
        assertEquals(FieldKind.PASSWORD, FieldKind.fromInputType(text or InputType.TYPE_TEXT_VARIATION_PASSWORD))
        assertEquals(FieldKind.PASSWORD, FieldKind.fromInputType(text or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD))
        assertEquals(FieldKind.PASSWORD, FieldKind.fromInputType(text or InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD))
        assertEquals(FieldKind.NUMBER, FieldKind.fromInputType(InputType.TYPE_CLASS_NUMBER))
        assertEquals(FieldKind.PHONE, FieldKind.fromInputType(InputType.TYPE_CLASS_PHONE))
        assertEquals(FieldKind.DATETIME, FieldKind.fromInputType(InputType.TYPE_CLASS_DATETIME))
    }

    @Test
    fun 数字のパスワードは数字として扱う() {
        val pin = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
        assertEquals(FieldKind.NUMBER, FieldKind.fromInputType(pin))
    }

    @Test
    fun 種類ごとの既定の切替先() {
        assertNull(LayoutSelect.defaultTarget(FieldKind.NORMAL))
        for (k in listOf(FieldKind.EMAIL, FieldKind.URL, FieldKind.PASSWORD)) assertEquals("alpha", LayoutSelect.defaultTarget(k))
        for (k in listOf(FieldKind.NUMBER, FieldKind.PHONE, FieldKind.DATETIME)) assertEquals("numbers", LayoutSelect.defaultTarget(k))
    }

    @Test
    fun 利用者の割り当てが既定より優先される() {
        val a = mapOf(FieldKind.EMAIL to "flick-alpha", FieldKind.NUMBER to "none", FieldKind.NORMAL to "kana")
        assertEquals("flick-alpha", LayoutSelect.resolveFieldTarget(FieldKind.EMAIL, a))
        assertNull(LayoutSelect.resolveFieldTarget(FieldKind.NUMBER, a))
        assertEquals("kana", LayoutSelect.resolveFieldTarget(FieldKind.NORMAL, a))
        // 割り当てが無い種類は既定
        assertEquals("alpha", LayoutSelect.resolveFieldTarget(FieldKind.URL, a))
        assertEquals("numbers", LayoutSelect.resolveFieldTarget(FieldKind.PHONE, a))
    }

    @Test
    fun 切替先が指す入力モード() {
        assertEquals(true, LayoutSelect.kanaModeOfTarget("kana"))
        assertEquals(true, LayoutSelect.kanaModeOfTarget("qwerty-kana"))
        assertEquals(false, LayoutSelect.kanaModeOfTarget("alpha"))
        assertEquals(false, LayoutSelect.kanaModeOfTarget("flick-alpha"))
        assertNull(LayoutSelect.kanaModeOfTarget("numbers"))
        assertNull(LayoutSelect.kanaModeOfTarget("my-custom"))
        assertEquals(true, LayoutSelect.kanaModeOf(LayoutMode.KANA))
        assertEquals(false, LayoutSelect.kanaModeOf(LayoutMode.ALPHA))
        assertNull(LayoutSelect.kanaModeOf(LayoutMode.NUMBER))
    }

    @Test
    fun 候補バーの切替先() {
        assertEquals("qwerty-kana", LayoutSelect.toggleTarget(LayoutMode.KANA, "flick-kana"))
        assertEquals("flick-kana", LayoutSelect.toggleTarget(LayoutMode.KANA, "qwerty-kana"))
        assertEquals("qwerty-kana", LayoutSelect.toggleTarget(LayoutMode.KANA, "my-custom"))
        assertEquals("flick-alpha", LayoutSelect.toggleTarget(LayoutMode.ALPHA, "qwerty-alpha"))
        assertEquals("qwerty-alpha", LayoutSelect.toggleTarget(LayoutMode.ALPHA, "flick-alpha"))
        assertNull(LayoutSelect.toggleTarget(LayoutMode.NUMBER, "numbers"))
    }

    @Test
    fun 改行キーのラベルをimeOptionsから決める() {
        assertEquals("検索", LayoutSelect.enterLabel(EditorInfo.IME_ACTION_SEARCH))
        assertEquals("送信", LayoutSelect.enterLabel(EditorInfo.IME_ACTION_SEND))
        assertEquals("次へ", LayoutSelect.enterLabel(EditorInfo.IME_ACTION_NEXT))
        assertEquals("完了", LayoutSelect.enterLabel(EditorInfo.IME_ACTION_DONE))
        assertEquals("移動", LayoutSelect.enterLabel(EditorInfo.IME_ACTION_GO))
        assertEquals("前へ", LayoutSelect.enterLabel(EditorInfo.IME_ACTION_PREVIOUS))
        assertNull(LayoutSelect.enterLabel(EditorInfo.IME_ACTION_NONE))
        assertNull(LayoutSelect.enterLabel(EditorInfo.IME_ACTION_UNSPECIFIED))
        assertNull(LayoutSelect.enterLabel(0))
        // 他のフラグが混ざっていてもアクションだけを見る
        assertEquals("検索", LayoutSelect.enterLabel(EditorInfo.IME_ACTION_SEARCH or EditorInfo.IME_FLAG_NO_EXTRACT_UI))
        // 改行を入れる欄（IME_FLAG_NO_ENTER_ACTION）ではレイアウトのまま
        assertNull(LayoutSelect.enterLabel(EditorInfo.IME_ACTION_SEARCH or EditorInfo.IME_FLAG_NO_ENTER_ACTION))
    }

    @Test
    fun 種類のキー文字列の往復() {
        for (k in FieldKind.values()) assertEquals(k, FieldKind.fromKey(k.key))
        assertNull(FieldKind.fromKey("unknown"))
        assertFalse(FieldKind.values().map { it.key }.toSet().size != FieldKind.values().size)
        assertTrue(FieldKind.values().all { it.label.isNotEmpty() })
    }
}
