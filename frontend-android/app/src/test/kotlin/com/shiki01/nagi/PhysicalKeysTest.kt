package com.shiki01.nagi

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhysicalKeysTest {
    @Test
    fun 名前のあるキーはkeysymに変換する() {
        assertEquals(0xff08, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_DEL, 0))
        assertEquals(0xffff, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_FORWARD_DEL, 0))
        assertEquals(0xff0d, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_ENTER, 10))
        assertEquals(0xff0d, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_NUMPAD_ENTER, 10))
        assertEquals(0xff09, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_TAB, 9))
        assertEquals(0xff1b, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_ESCAPE, 0))
        assertEquals(0x0020, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_SPACE, 32))
        assertEquals(0xff51, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_DPAD_LEFT, 0))
        assertEquals(0xff52, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_DPAD_UP, 0))
        assertEquals(0xff53, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_DPAD_RIGHT, 0))
        assertEquals(0xff54, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_DPAD_DOWN, 0))
        assertEquals(0xff50, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_MOVE_HOME, 0))
        assertEquals(0xff57, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_MOVE_END, 0))
        assertEquals(0xff55, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_PAGE_UP, 0))
        assertEquals(0xff56, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_PAGE_DOWN, 0))
    }

    @Test
    fun 日本語キーをkeysymに変換する() {
        assertEquals(0xff23, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_HENKAN, 0))
        assertEquals(0xff22, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_MUHENKAN, 0))
        assertEquals(0xff27, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_KATAKANA_HIRAGANA, 0))
        assertEquals(0xff2a, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_ZENKAKU_HANKAKU, 0))
        assertEquals(0xff2f, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_EISU, 0))
        assertEquals(0xff25, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_KANA, 0))
    }

    @Test
    fun モード切替キーの判定() {
        assertTrue(PhysicalKeys.isModeKeysym(PhysicalKeys.SYM_EISU))
        assertTrue(PhysicalKeys.isModeKeysym(PhysicalKeys.SYM_ZENKAKU_HANKAKU))
        assertFalse(PhysicalKeys.isModeKeysym(PhysicalKeys.SYM_HENKAN))
        assertFalse(PhysicalKeys.isModeKeysym('a'.code))
    }

    @Test
    fun 印字文字はコードポイントになる() {
        assertEquals('a'.code, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_A, 'a'.code))
        assertEquals('A'.code, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_A, 'A'.code))
        assertEquals("A", PhysicalKeys.textFor(KeyEvent.KEYCODE_A, 'A'.code))
        assertEquals("ー", PhysicalKeys.textFor(KeyEvent.KEYCODE_MINUS, 'ー'.code))
    }

    @Test
    fun 文字も名前も無いキーはコアへ送らない() {
        assertEquals(0, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_BACK, 0))
        assertEquals(0, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_VOLUME_UP, 0))
        // 結合アクセント（負の値）と制御文字は文字として扱わない
        assertEquals(0, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_A, Int.MIN_VALUE or 0x60))
        assertEquals(0, PhysicalKeys.keysymFor(KeyEvent.KEYCODE_A, 1))
        assertEquals("", PhysicalKeys.textFor(KeyEvent.KEYCODE_ENTER, 10))
        assertEquals("", PhysicalKeys.textFor(KeyEvent.KEYCODE_DEL, 0))
    }

    @Test
    fun スペースの文字は半角空白() {
        assertEquals(" ", PhysicalKeys.textFor(KeyEvent.KEYCODE_SPACE, 32))
    }

    @Test
    fun 修飾キー単独は無視する() {
        assertTrue(PhysicalKeys.isModifierKey(KeyEvent.KEYCODE_SHIFT_LEFT))
        assertTrue(PhysicalKeys.isModifierKey(KeyEvent.KEYCODE_CTRL_RIGHT))
        assertTrue(PhysicalKeys.isModifierKey(KeyEvent.KEYCODE_ALT_LEFT))
        assertTrue(PhysicalKeys.isModifierKey(KeyEvent.KEYCODE_META_LEFT))
        assertTrue(PhysicalKeys.isModifierKey(KeyEvent.KEYCODE_CAPS_LOCK))
        assertFalse(PhysicalKeys.isModifierKey(KeyEvent.KEYCODE_A))
    }

    @Test
    fun metaStateを修飾に写す() {
        val m = PhysicalKeys.modifiersFrom(KeyEvent.META_SHIFT_ON or KeyEvent.META_CTRL_ON)
        assertTrue(m.shift)
        assertTrue(m.ctrl)
        assertFalse(m.alt)
        assertFalse(m.meta)
        assertFalse(m.capsLock)
        assertTrue(PhysicalKeys.modifiersFrom(KeyEvent.META_CAPS_LOCK_ON).capsLock)
        assertTrue(PhysicalKeys.modifiersFrom(KeyEvent.META_ALT_ON).alt)
        assertTrue(PhysicalKeys.modifiersFrom(KeyEvent.META_META_ON).meta)
    }

    @Test
    fun 文字取得ではCtrlとAltを外しShiftを残す() {
        val meta = KeyEvent.META_SHIFT_ON or KeyEvent.META_CTRL_ON or KeyEvent.META_ALT_ON
        val c = PhysicalKeys.metaForChar(meta)
        assertEquals(KeyEvent.META_SHIFT_ON, c and KeyEvent.META_SHIFT_ON)
        assertEquals(0, c and KeyEvent.META_CTRL_ON)
        assertEquals(0, c and KeyEvent.META_ALT_ON)
        assertTrue(PhysicalKeys.hasCommandModifier(meta))
        assertFalse(PhysicalKeys.hasCommandModifier(KeyEvent.META_SHIFT_ON))
    }

    @Test
    fun process_keyの引数のjson() {
        val ev = PhysicalKeyEvent(
            keysym = 'j'.code,
            keycode = KeyEvent.KEYCODE_J,
            text = "j",
            modifiers = KeyModifiers(shift = true),
        )
        val o = ev.toParams("android")
        assertEquals("android", o.getString("session_id"))
        assertEquals(106, o.getInt("keysym"))
        assertEquals(KeyEvent.KEYCODE_J, o.getInt("keycode"))
        assertEquals("j", o.getString("text"))
        assertEquals("down", o.getString("kind"))
        val m = o.getJSONObject("modifiers")
        assertTrue(m.getBoolean("shift"))
        assertFalse(m.getBoolean("ctrl"))
        assertFalse(m.getBoolean("alt"))
        assertFalse(m.getBoolean("meta"))
        assertFalse(m.getBoolean("caps_lock"))
    }
}
