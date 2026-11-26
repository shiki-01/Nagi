package com.shiki01.nagi

import android.text.InputType
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

// 英字入力の予測候補・スペル訂正（H-146）。iOS の TouchLogicTests と同じ例
class EnglishAssistTest {
    @Test
    fun カーソル直前の英単語を切り出す() {
        assertEquals("recieve", EnglishWord.current("I recieve"))
        assertEquals("", EnglishWord.current("hello "))
        assertEquals("don't", EnglishWord.current("don't"))
        assertEquals("don't", EnglishWord.current("don’t"))
        assertEquals("quote", EnglishWord.current("'quote"))
        assertEquals("state-of-the-art", EnglishWord.current("state-of-the-art"))
        assertEquals("", EnglishWord.current("end-"))
        assertEquals("", EnglishWord.current("abc123def"))
        assertEquals("meeting", EnglishWord.current("今日はmeeting"))
        assertEquals("", EnglishWord.current(""))
        assertEquals("", EnglishWord.current("a".repeat(40)))
        assertEquals("a".repeat(32), EnglishWord.current("a".repeat(32)))
        assertEquals("next", EnglishWord.current("line\nnext"))
    }

    @Test
    fun 候補を出す条件() {
        assertTrue(EnglishAssistPolicy.shouldSuggest(isAlpha = true, setting = true, isSecure = false, hasPreedit = false))
        assertFalse(EnglishAssistPolicy.shouldSuggest(isAlpha = false, setting = true, isSecure = false, hasPreedit = false))
        assertFalse(EnglishAssistPolicy.shouldSuggest(isAlpha = true, setting = false, isSecure = false, hasPreedit = false))
        assertFalse(EnglishAssistPolicy.shouldSuggest(isAlpha = true, setting = true, isSecure = true, hasPreedit = false))
        assertFalse(EnglishAssistPolicy.shouldSuggest(isAlpha = true, setting = true, isSecure = false, hasPreedit = true))
    }

    @Test
    fun 自動修正する条件() {
        val text = InputType.TYPE_CLASS_TEXT
        assertTrue(EnglishAssistPolicy.shouldAutocorrect(true, true, text))
        assertTrue(EnglishAssistPolicy.shouldAutocorrect(true, true, text or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES))
        assertTrue(EnglishAssistPolicy.shouldAutocorrect(true, true, text or InputType.TYPE_TEXT_VARIATION_SHORT_MESSAGE))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(false, true, text))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, false, text))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, text or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, text or InputType.TYPE_TEXT_VARIATION_URI))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, text or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, text or InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, text or InputType.TYPE_TEXT_VARIATION_PASSWORD))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, InputType.TYPE_CLASS_NUMBER))
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, InputType.TYPE_CLASS_PHONE))
        // 種類の指定が無い欄（TYPE_NULL）でも直さない
        assertFalse(EnglishAssistPolicy.shouldAutocorrect(true, true, InputType.TYPE_NULL))
    }

    @Test
    fun 自動修正の契機() {
        for (t in listOf(" ", ".", ",", "!", "?", ";", ":")) assertTrue(t, EnglishAssistPolicy.isTrigger(t))
        assertFalse(EnglishAssistPolicy.isTrigger("a"))
        assertFalse(EnglishAssistPolicy.isTrigger("'"))
        assertFalse(EnglishAssistPolicy.isTrigger("-"))
        assertFalse(EnglishAssistPolicy.isTrigger("\n"))
        assertFalse(EnglishAssistPolicy.isTrigger(". "))
    }

    @Test
    fun 自動修正の取り消し() {
        val undo = EnglishAutocorrectUndo("recieve", "receive", " ")
        assertTrue(undo.matches("I receive "))
        assertFalse(undo.matches("I receive x"))
        assertFalse(undo.matches(null))
        assertEquals(8, undo.revert.first)
        assertEquals("recieve", undo.revert.second)
    }

    @Test
    fun 候補を選んだときの操作() {
        val e = EnglishSelection.edit("recie", "receive", nextIsSpace = false)
        assertEquals(5, e.first)
        assertEquals("receive ", e.second)
        assertEquals("the", EnglishSelection.edit("", "the", nextIsSpace = true).second)
    }

    @Test
    fun カーソル直前のテキストの追跡() {
        assertEquals("abc", EnglishBuffer.append("ab", "c"))
        assertEquals(EnglishBuffer.MAX_LENGTH, EnglishBuffer.append("x".repeat(60), "y".repeat(10)).length)
        assertEquals("a", EnglishBuffer.drop("abc", 2))
        assertEquals("", EnglishBuffer.drop("abc", 5))
        assertEquals("ab", EnglishBuffer.dropOne("abc"))
        // サロゲートペアは 1 文字として消す
        assertEquals("a", EnglishBuffer.dropOne("a😀"))
        assertEquals("", EnglishBuffer.dropOne(""))
    }

    @Test
    fun 候補バーの表示() {
        val r = EnglishSuggestResult(
            listOf(
                EnglishSuggestion("recieve", EnglishSuggestionKind.TYPED),
                EnglishSuggestion("receive", EnglishSuggestionKind.CORRECTION),
                EnglishSuggestion("recieved", EnglishSuggestionKind.COMPLETION),
            ),
            "receive",
        )
        val bar = EnglishBar.build(r, "receive")
        assertNotNull(bar)
        bar!!
        assertEquals(listOf("“recieve”", "receive", "recieved"), bar.items)
        assertEquals(1, bar.selected)
        assertEquals(CandidateKind.ENGLISH, bar.kind)
        assertNull(bar.pinned)
        // 自動修正しないとき（設定オフ・URL の欄など）は引用符も選択の強調も付けない
        val plain = EnglishBar.build(r, null)!!
        assertEquals(listOf("recieve", "receive", "recieved"), plain.items)
        assertEquals(-1, plain.selected)
        // 打ったままの語しか無ければ出さない
        assertNull(EnglishBar.build(EnglishSuggestResult(listOf(EnglishSuggestion("hello", EnglishSuggestionKind.TYPED)), null), null))
        assertNull(EnglishBar.build(EnglishSuggestResult(emptyList(), null), null))
    }
}
