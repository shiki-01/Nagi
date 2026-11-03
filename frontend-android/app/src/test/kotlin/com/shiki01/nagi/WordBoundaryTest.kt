package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Test

/** iOS の `TouchLogicTests`（WordBoundary）と同じ入力・期待値。数えるのは書記素、返すのは UTF-16 コード単位 */
class WordBoundaryTest {
    private fun len(s: String) = WordBoundary.deleteLength(s)

    @Test
    fun 空文字は何もしない() {
        assertEquals(0, len(""))
    }

    @Test
    fun 漢字の連なりとかなは別の単語() {
        assertEquals(2, len("今日は天気"))
        assertEquals(1, len("今日は"))
        assertEquals(3, len("これはテスト"))
        assertEquals(3, len("あした、そうだ"))
    }

    @Test
    fun 英単語と末尾の空白() {
        assertEquals(5, len("hello world"))
        assertEquals(7, len("hello world  "))
        assertEquals(6, len("abc123"))
    }

    @Test
    fun 記号は1文字だけ() {
        assertEquals(1, len("こんにちは。"))
        assertEquals(1, len("a!!"))
    }

    @Test
    fun 改行は1つだけ() {
        assertEquals(1, len("abc\n"))
    }

    @Test
    fun 空白だけ_記号の前の空白() {
        assertEquals(3, len("   "))
        assertEquals(2, len("あ。 "))
        // 空白の手前が改行なら、空白だけで止まる
        assertEquals(2, len("a\n  "))
    }

    @Test
    fun 最大50文字で止まる() {
        assertEquals(WordBoundary.MAX_COUNT, len("a".repeat(200)))
    }

    @Test
    fun カタカナの長音() {
        assertEquals(4, len("あのコーヒー"))
    }

    @Test
    fun サロゲートペアの漢字はコード単位で数える() {
        // U+20BB7（𠮷）は UTF-16 で 2 コード単位。「家𠮷野」は漢字の連なりで 3 文字 = 4 コード単位
        assertEquals(4, len("は家𠮷野"))
    }

    @Test
    fun 絵文字は1つだけ消す() {
        // 😀（U+1F600）は 2 コード単位。記号扱いで 1 文字ぶん
        assertEquals(2, len("ab😀"))
    }

    @Test
    fun 入力中の文字列では書記素の数を返す() {
        assertEquals(2, WordBoundary.deleteCount("今日は天気"))
        assertEquals(0, WordBoundary.deleteCount(""))
        assertEquals(3, WordBoundary.deleteCount("あABC"))
    }
}
