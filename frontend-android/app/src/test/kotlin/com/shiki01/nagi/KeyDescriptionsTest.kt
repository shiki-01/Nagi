package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyDescriptionsTest {
    private fun key(action: KeyAction, label: String? = null, longPress: List<PopupItem> = emptyList()) =
        LayoutKey("k", 1.0, label, null, KeyRole.NORMAL, action, longPress)

    private fun desc(
        k: LayoutKey,
        flick: KanaKeyDef? = null,
        composing: Boolean = false,
        kana: Boolean = true,
        enter: String? = null,
        shift: ShiftState = ShiftState.OFF,
    ) = KeyDescriptions.forKey(k, flick, composing, kana, enter, shift)

    @Test
    fun フリックキーは行の名前と各方向の文字() {
        val ka = KeyTables.FALLBACK.first { it.key == "ka" }
        assertEquals(
            "か行（タップで か、左フリックで き、上で く、右で け、下で こ）",
            KeyDescriptions.flick(ka),
        )
        val key = key(KeyAction.Flick("か", "ka", null, null, null, null, null))
        assertEquals(KeyDescriptions.flick(ka), desc(key, ka))
    }

    @Test
    fun 句読点キーは記号に読み上げ名を付ける() {
        val p = KeyTables.FALLBACK.first { it.key == "punct" }
        val text = KeyDescriptions.flick(p)
        assertTrue(text, text.startsWith("句読点（タップで 読点、"))
        assertTrue(text, text.contains("句点"))
    }

    @Test
    fun 操作キーの名前() {
        fun act(name: String) = desc(key(KeyAction.Act(name)))
        assertEquals("削除", act("backspace"))
        assertEquals("空白", act("space"))
        assertEquals("改行", act("enter"))
        assertEquals("変換", act("convert"))
        assertEquals("次の入力方法", act("globe"))
        assertEquals("濁点・小書き", act("dakuten"))
        assertEquals("濁点・小書き", act("smallKana"))
        assertEquals("カーソルを左へ", act("moveLeft"))
        assertEquals("かなと英字の切り替え", act("modeSwitch"))
        assertEquals("キーボードを閉じる", act("dismiss"))
    }

    @Test
    fun 知らない操作はラベルか名前() {
        assertEquals("独自", desc(key(KeyAction.Act("custom"), label = "独自")))
        assertEquals("custom", desc(key(KeyAction.Act("custom"))))
    }

    @Test
    fun 空白と改行は今の状態で変わる() {
        val space = key(KeyAction.Act("space"))
        val enter = key(KeyAction.Act("enter"))
        assertEquals("変換", desc(space, composing = true))
        assertEquals("空白", desc(space, composing = true, kana = false))
        assertEquals("確定", desc(enter, composing = true))
        assertEquals("検索", desc(enter, enter = "検索"))
        assertEquals("確定", desc(enter, composing = true, enter = "検索"))
    }

    @Test
    fun シフトの状態を伝える() {
        val shift = key(KeyAction.Act("shift"))
        assertEquals("シフト", desc(shift))
        assertEquals("シフト（次の1文字が大文字）", desc(shift, shift = ShiftState.ONCE))
        assertEquals("シフト（キャプスロック中）", desc(shift, shift = ShiftState.CAPS))
        assertEquals("キャプスロック（オン）", desc(key(KeyAction.Act("capslock")), shift = ShiftState.CAPS))
    }

    @Test
    fun 文字キーと記号() {
        assertEquals("q", desc(key(KeyAction.Text("q", null))))
        assertEquals("Q", desc(key(KeyAction.Text("q", null)), shift = ShiftState.ONCE))
        assertEquals("感嘆符", desc(key(KeyAction.Text("q", "!")), shift = ShiftState.CAPS))
        assertEquals("読点", desc(key(KeyAction.Text("、", null))))
        assertEquals("ハイフン", desc(key(KeyAction.Text("-", null))))
        assertEquals("あ", desc(key(KeyAction.Text("あ", null))))
    }

    @Test
    fun トグルキー() {
        assertEquals("a（続けて押すと a b c）", desc(key(KeyAction.Toggle(listOf("a", "b", "c")))))
        assertEquals("a", desc(key(KeyAction.Toggle(listOf("a")))))
    }

    @Test
    fun レイアウト切替() {
        assertEquals("数字に切り替え", desc(key(KeyAction.LayoutSwitch("numbers"))))
        assertEquals("絵文字に切り替え", desc(key(KeyAction.LayoutSwitch("emoji"))))
        assertEquals("独自配列", desc(key(KeyAction.LayoutSwitch("my-layout"), label = "独自配列")))
        assertEquals("my-layout", desc(key(KeyAction.LayoutSwitch("my-layout"))))
    }

    @Test
    fun 長押しの項目を添える() {
        val k = key(
            KeyAction.Text("a", null),
            longPress = listOf(
                PopupItem(null, "1", null, KeyAction.Text("1", null)),
                PopupItem(null, "@", null, KeyAction.Text("@", null)),
            ),
        )
        assertEquals("a。長押しで 1 アットマーク", desc(k))
    }

    @Test
    fun ショートカットと空白キー() {
        assertEquals("ショートカット", desc(key(KeyAction.Shortcut(emptyList()))))
        assertEquals("署名", desc(key(KeyAction.Shortcut(emptyList()), label = "署名")))
        assertEquals("", desc(key(KeyAction.Spacer)))
    }

    @Test
    fun 候補と文節() {
        assertEquals("今日、1/3", KeyDescriptions.candidate("今日", 0, 3, false))
        assertEquals("京、2/3、選択中", KeyDescriptions.candidate("京", 1, 3, true))
        assertEquals("天気、文節 2/2", KeyDescriptions.segment("天気", 1, 2, false))
        assertEquals("今日は、文節 1/2、注目中", KeyDescriptions.segment("今日は", 0, 2, true))
    }

    @Test
    fun 組み込みキーの名前() {
        assertEquals("削除", KeyDescriptions.legacyFace("⌫"))
        assertEquals("カーソルを右へ", KeyDescriptions.legacyFace("→"))
        assertEquals("数字と記号", KeyDescriptions.legacyFace("☆123"))
        assertEquals("a", KeyDescriptions.legacyFace("a"))
    }
}
