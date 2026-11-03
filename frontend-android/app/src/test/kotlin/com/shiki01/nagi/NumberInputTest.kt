package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NumberInputTest {
    @Test
    fun 数字記号は設定オンでかなの流れか入力中のときだけコアへ() {
        assertTrue(NumberCompose.shouldCompose(true, isSecure = false, kanaContext = true, hasPreedit = false))
        assertTrue(NumberCompose.shouldCompose(true, false, kanaContext = false, hasPreedit = true))
        // 英字から来て入力中でもない（電話番号欄など）は直接入力
        assertFalse(NumberCompose.shouldCompose(true, false, kanaContext = false, hasPreedit = false))
        // 設定オフ・パスワード欄は常に直接入力
        assertFalse(NumberCompose.shouldCompose(false, false, true, true))
        assertFalse(NumberCompose.shouldCompose(true, true, true, true))
    }

    @Test
    fun 英字のキーは設定オンで入力中のときだけ続ける() {
        assertTrue(EnglishCompose.shouldCompose(true, false, true))
        assertFalse(EnglishCompose.shouldCompose(false, false, true))
        assertFalse(EnglishCompose.shouldCompose(true, true, true))
        assertFalse(EnglishCompose.shouldCompose(true, false, false))
    }

    private fun keep(
        from: LayoutMode,
        to: LayoutMode,
        preedit: Boolean = true,
        secure: Boolean = false,
        symbols: Boolean = true,
        english: Boolean = false,
        compose: Boolean = true,
    ) = LayoutSwitchKeep.shouldKeep(from, to, preedit, secure, symbols, english, compose)

    @Test
    fun かなと数字記号の切替は既定で保つ() {
        assertTrue(keep(LayoutMode.KANA, LayoutMode.NUMBER))
        assertTrue(keep(LayoutMode.NUMBER, LayoutMode.KANA))
        assertTrue(keep(LayoutMode.NUMBER, LayoutMode.SYMBOL))
        assertFalse(keep(LayoutMode.KANA, LayoutMode.NUMBER, symbols = false))
    }

    @Test
    fun かな同士と入力中でないときとパスワード欄は保たない() {
        assertFalse(keep(LayoutMode.KANA, LayoutMode.KANA))
        assertFalse(keep(LayoutMode.KANA, LayoutMode.NUMBER, preedit = false))
        assertFalse(keep(LayoutMode.KANA, LayoutMode.NUMBER, secure = true))
    }

    @Test
    fun 数字記号が関わる切替はcompose_numbersがオフなら確定() {
        assertFalse(keep(LayoutMode.KANA, LayoutMode.NUMBER, compose = false))
        assertFalse(keep(LayoutMode.NUMBER, LayoutMode.KANA, compose = false))
    }

    @Test
    fun 英字との切替は既定で確定しオンなら保つ() {
        assertFalse(keep(LayoutMode.KANA, LayoutMode.ALPHA))
        assertTrue(keep(LayoutMode.KANA, LayoutMode.ALPHA, english = true))
        assertTrue(keep(LayoutMode.ALPHA, LayoutMode.KANA, english = true))
        assertTrue(keep(LayoutMode.ALPHA, LayoutMode.NUMBER, english = true))
        assertFalse(keep(LayoutMode.ALPHA, LayoutMode.NUMBER, english = true, compose = false))
    }

    @Test
    fun 絵文字とカスタムへの切替は常に確定() {
        assertFalse(keep(LayoutMode.KANA, LayoutMode.EMOJI, english = true))
        assertFalse(keep(LayoutMode.CUSTOM, LayoutMode.KANA))
        assertEquals(false, keep(LayoutMode.NUMBER, LayoutMode.EMOJI))
    }

    @Test
    fun 既定値はiOSに合わせる() {
        assertTrue(KeyboardDefaults.COMPOSE_NUMBERS)
        assertTrue(KeyboardDefaults.KEEP_PREEDIT_ON_SYMBOLS)
        assertFalse(KeyboardDefaults.KEEP_PREEDIT_ON_ENGLISH)
    }
}
