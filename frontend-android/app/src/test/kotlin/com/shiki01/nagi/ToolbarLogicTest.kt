package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** iOS の `ToolbarActionTests` と同じ観点（既定・保存/読込・不正値・出す判定・貼り付けの可視）+ 並べ替え */
class ToolbarLogicTest {
    @Test
    fun 項目はiosの10種で名前と順が同じ() {
        assertEquals(
            listOf(
                "dismiss", "paste", "cursorLeft", "cursorRight", "newline", "space", "deleteWord", "emoji", "nextKeyboard",
                "history",
            ),
            ToolbarItem.values().map { it.json },
        )
        assertEquals(
            listOf(
                "キーボードを閉じる", "貼り付け", "カーソルを左へ", "カーソルを右へ", "改行", "半角スペース", "単語を削除", "絵文字",
                "次のキーボード", "履歴",
            ),
            ToolbarItem.values().map { it.title },
        )
    }

    @Test
    fun 既定の4項目と順序() {
        assertEquals(
            listOf(ToolbarItem.DISMISS, ToolbarItem.PASTE, ToolbarItem.CURSOR_LEFT, ToolbarItem.CURSOR_RIGHT),
            ToolbarLogic.DEFAULT,
        )
        assertEquals(ToolbarLogic.DEFAULT, ToolbarLogic.parse(null))
    }

    @Test
    fun 空や空白や有効な名前が無い保存値は既定() {
        assertEquals(ToolbarLogic.DEFAULT, ToolbarLogic.parse(""))
        assertEquals(ToolbarLogic.DEFAULT, ToolbarLogic.parse("  "))
        assertEquals(ToolbarLogic.DEFAULT, ToolbarLogic.parse("foo,bar"))
        assertEquals(ToolbarLogic.DEFAULT, ToolbarLogic.parse(","))
    }

    @Test
    fun 知らない名前と重複は捨てて並びは保つ() {
        assertEquals(
            listOf(ToolbarItem.HISTORY, ToolbarItem.SPACE, ToolbarItem.PASTE),
            ToolbarLogic.parse("history, unknown,space,history,paste,"),
        )
    }

    @Test
    fun 保存して読むと同じ並びになる() {
        val all = ToolbarItem.values().toList().reversed()
        assertEquals(all, ToolbarLogic.parse(ToolbarLogic.encode(all)))
        assertEquals("space,paste", ToolbarLogic.encode(listOf(ToolbarItem.SPACE, ToolbarItem.PASTE, ToolbarItem.SPACE)))
    }

    @Test
    fun 設定画面の並びは有効な項目のあとに無効な項目が宣言順() {
        val enabled = listOf(ToolbarItem.HISTORY, ToolbarItem.DISMISS)
        val order = ToolbarLogic.displayOrder(enabled)
        assertEquals(ToolbarItem.values().size, order.size)
        assertEquals(enabled, order.take(2))
        assertEquals(ToolbarItem.values().filter { it !in enabled }, order.drop(2))
    }

    @Test
    fun 有効無効の切り替え() {
        val base = listOf(ToolbarItem.DISMISS, ToolbarItem.PASTE)
        assertEquals(base + ToolbarItem.EMOJI, ToolbarLogic.toggle(base, ToolbarItem.EMOJI))
        assertEquals(listOf(ToolbarItem.PASTE), ToolbarLogic.toggle(base, ToolbarItem.DISMISS))
        // 最後の 1 つは外さない
        assertEquals(listOf(ToolbarItem.PASTE), ToolbarLogic.toggle(listOf(ToolbarItem.PASTE), ToolbarItem.PASTE))
    }

    @Test
    fun 並べ替えは範囲内だけ動く() {
        val base = listOf(ToolbarItem.DISMISS, ToolbarItem.PASTE, ToolbarItem.SPACE)
        assertEquals(
            listOf(ToolbarItem.PASTE, ToolbarItem.DISMISS, ToolbarItem.SPACE),
            ToolbarLogic.move(base, ToolbarItem.PASTE, -1),
        )
        assertEquals(
            listOf(ToolbarItem.DISMISS, ToolbarItem.SPACE, ToolbarItem.PASTE),
            ToolbarLogic.move(base, ToolbarItem.PASTE, 1),
        )
        assertEquals(base, ToolbarLogic.move(base, ToolbarItem.DISMISS, -1))
        assertEquals(base, ToolbarLogic.move(base, ToolbarItem.SPACE, 1))
        // 無効な項目は動かさない
        assertEquals(base, ToolbarLogic.move(base, ToolbarItem.EMOJI, -1))
    }

    @Test
    fun ツールバーを出す条件() {
        assertTrue(ToolbarLogic.shouldShow(hasPreedit = false, candidatesEmpty = true, hasMessage = false))
        assertFalse(ToolbarLogic.shouldShow(hasPreedit = true, candidatesEmpty = true, hasMessage = false))
        assertFalse(ToolbarLogic.shouldShow(hasPreedit = false, candidatesEmpty = false, hasMessage = false))
        assertFalse(ToolbarLogic.shouldShow(hasPreedit = false, candidatesEmpty = true, hasMessage = true))
    }

    @Test
    fun 貼り付けはボタン設定がオンでクリップボードに文字があるときだけ出る() {
        val items = ToolbarLogic.DEFAULT
        assertEquals(items, ToolbarLogic.visible(items, pasteButton = true, clipboardHasText = true))
        val without = items - ToolbarItem.PASTE
        assertEquals(without, ToolbarLogic.visible(items, pasteButton = true, clipboardHasText = false))
        assertEquals(without, ToolbarLogic.visible(items, pasteButton = false, clipboardHasText = true))
        // 貼り付けを含まない並びは影響を受けない
        assertEquals(without, ToolbarLogic.visible(without, pasteButton = false, clipboardHasText = false))
    }

    @Test
    fun 補足に出す現在の並び() {
        assertEquals(
            "キーボードを閉じる → 貼り付け → カーソルを左へ → カーソルを右へ",
            ToolbarLogic.summary(ToolbarLogic.DEFAULT),
        )
    }
}
