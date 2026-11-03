package com.shiki01.nagi

import android.view.KeyEvent
import org.json.JSONObject

/** 修飾キーの状態（コアの `Modifiers`） */
internal data class KeyModifiers(
    val shift: Boolean = false,
    val ctrl: Boolean = false,
    val alt: Boolean = false,
    val meta: Boolean = false,
    val capsLock: Boolean = false,
)

/** 物理キーボードの 1 キー（コアの `process_key` の引数） */
internal data class PhysicalKeyEvent(
    val keysym: Int,
    val keycode: Int,
    val text: String,
    val modifiers: KeyModifiers,
) {
    fun toParams(sessionId: String): JSONObject = JSONObject()
        .put("session_id", sessionId)
        .put("keysym", keysym)
        .put("keycode", keycode)
        .put("text", text)
        .put(
            "modifiers",
            JSONObject()
                .put("shift", modifiers.shift)
                .put("ctrl", modifiers.ctrl)
                .put("alt", modifiers.alt)
                .put("meta", modifiers.meta)
                .put("caps_lock", modifiers.capsLock),
        )
        .put("kind", "down")
}

/**
 * Android の `KeyEvent` から XKB keysym への変換（`KeyEvent` の定数だけを使う純粋関数）。
 * 定数はコンパイル時に埋め込まれるので、ユニットテストでも動く。
 */
internal object PhysicalKeys {
    const val SYM_SPACE = 0x0020
    const val SYM_BACKSPACE = 0xff08
    const val SYM_TAB = 0xff09
    const val SYM_RETURN = 0xff0d
    const val SYM_ESCAPE = 0xff1b
    const val SYM_DELETE = 0xffff
    const val SYM_HOME = 0xff50
    const val SYM_LEFT = 0xff51
    const val SYM_UP = 0xff52
    const val SYM_RIGHT = 0xff53
    const val SYM_DOWN = 0xff54
    const val SYM_PAGE_UP = 0xff55
    const val SYM_PAGE_DOWN = 0xff56
    const val SYM_END = 0xff57
    const val SYM_F6 = 0xffc3
    const val SYM_F7 = 0xffc4
    const val SYM_MUHENKAN = 0xff22
    const val SYM_HENKAN = 0xff23
    const val SYM_HIRAGANA = 0xff25
    const val SYM_HIRAGANA_KATAKANA = 0xff27
    const val SYM_ZENKAKU_HANKAKU = 0xff2a
    const val SYM_EISU = 0xff2f

    private val named: Map<Int, Int> = mapOf(
        KeyEvent.KEYCODE_SPACE to SYM_SPACE,
        KeyEvent.KEYCODE_DEL to SYM_BACKSPACE,
        KeyEvent.KEYCODE_FORWARD_DEL to SYM_DELETE,
        KeyEvent.KEYCODE_TAB to SYM_TAB,
        KeyEvent.KEYCODE_ENTER to SYM_RETURN,
        KeyEvent.KEYCODE_NUMPAD_ENTER to SYM_RETURN,
        KeyEvent.KEYCODE_ESCAPE to SYM_ESCAPE,
        KeyEvent.KEYCODE_DPAD_LEFT to SYM_LEFT,
        KeyEvent.KEYCODE_DPAD_UP to SYM_UP,
        KeyEvent.KEYCODE_DPAD_RIGHT to SYM_RIGHT,
        KeyEvent.KEYCODE_DPAD_DOWN to SYM_DOWN,
        KeyEvent.KEYCODE_MOVE_HOME to SYM_HOME,
        KeyEvent.KEYCODE_MOVE_END to SYM_END,
        KeyEvent.KEYCODE_PAGE_UP to SYM_PAGE_UP,
        KeyEvent.KEYCODE_PAGE_DOWN to SYM_PAGE_DOWN,
        KeyEvent.KEYCODE_F6 to SYM_F6,
        KeyEvent.KEYCODE_F7 to SYM_F7,
        KeyEvent.KEYCODE_HENKAN to SYM_HENKAN,
        KeyEvent.KEYCODE_MUHENKAN to SYM_MUHENKAN,
        KeyEvent.KEYCODE_KATAKANA_HIRAGANA to SYM_HIRAGANA_KATAKANA,
        KeyEvent.KEYCODE_ZENKAKU_HANKAKU to SYM_ZENKAKU_HANKAKU,
        KeyEvent.KEYCODE_EISU to SYM_EISU,
        KeyEvent.KEYCODE_KANA to SYM_HIRAGANA,
    )

    private val modifierKeys: Set<Int> = setOf(
        KeyEvent.KEYCODE_SHIFT_LEFT,
        KeyEvent.KEYCODE_SHIFT_RIGHT,
        KeyEvent.KEYCODE_CTRL_LEFT,
        KeyEvent.KEYCODE_CTRL_RIGHT,
        KeyEvent.KEYCODE_ALT_LEFT,
        KeyEvent.KEYCODE_ALT_RIGHT,
        KeyEvent.KEYCODE_META_LEFT,
        KeyEvent.KEYCODE_META_RIGHT,
        KeyEvent.KEYCODE_CAPS_LOCK,
        KeyEvent.KEYCODE_NUM_LOCK,
        KeyEvent.KEYCODE_SCROLL_LOCK,
        KeyEvent.KEYCODE_FUNCTION,
        KeyEvent.KEYCODE_SYM,
    )

    /** 修飾キー単独（コアへ送らず OS に任せる） */
    fun isModifierKey(keyCode: Int): Boolean = keyCode in modifierKeys

    /** 入力モードを切り替えるキー（英数 / 全角半角 / かな / カタカナひらがな） */
    fun isModeKeysym(keysym: Int): Boolean =
        keysym == SYM_EISU || keysym == SYM_ZENKAKU_HANKAKU ||
            keysym == SYM_HIRAGANA || keysym == SYM_HIRAGANA_KATAKANA

    /** 印字できる文字か（制御文字・DEL・結合アクセント（負の値）を除く） */
    fun isPrintable(unicode: Int): Boolean = unicode in 0x20..0x10ffff && unicode != 0x7f

    /**
     * キーコードと [unicode]（`getUnicodeChar` の値）から keysym を求める。
     * 名前のあるキーはその keysym、印字文字は文字のコードポイント、どちらでもなければ 0（コアへ送らない）
     */
    fun keysymFor(keyCode: Int, unicode: Int): Int {
        val sym = named[keyCode]
        if (sym != null) return sym
        return if (isPrintable(unicode)) unicode else 0
    }

    /** そのキーで生成された文字列。生成されなければ空文字列 */
    fun textFor(keyCode: Int, unicode: Int): String {
        if (keyCode == KeyEvent.KEYCODE_SPACE) return " "
        if (!isPrintable(unicode)) return ""
        return String(Character.toChars(unicode))
    }

    /** `KeyEvent.getMetaState()` を修飾キーの状態に写す */
    fun modifiersFrom(metaState: Int): KeyModifiers = KeyModifiers(
        shift = (metaState and KeyEvent.META_SHIFT_ON) != 0,
        ctrl = (metaState and KeyEvent.META_CTRL_ON) != 0,
        alt = (metaState and KeyEvent.META_ALT_ON) != 0,
        meta = (metaState and KeyEvent.META_META_ON) != 0,
        capsLock = (metaState and KeyEvent.META_CAPS_LOCK_ON) != 0,
    )

    /** Ctrl / Alt / Meta のいずれかが押されているか（文字入力ではなくショートカットとして扱う） */
    fun hasCommandModifier(metaState: Int): Boolean =
        (metaState and (KeyEvent.META_CTRL_MASK or KeyEvent.META_ALT_MASK or KeyEvent.META_META_MASK)) != 0

    /** 文字の取得（`getUnicodeChar`）に使う meta 状態。Ctrl / Alt / Meta を外して Shift と CapsLock だけ残す */
    fun metaForChar(metaState: Int): Int =
        metaState and (KeyEvent.META_CTRL_MASK or KeyEvent.META_ALT_MASK or KeyEvent.META_META_MASK).inv()
}
