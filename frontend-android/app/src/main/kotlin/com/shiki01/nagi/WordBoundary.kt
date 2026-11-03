package com.shiki01.nagi

import java.text.BreakIterator
import java.util.Locale

/**
 * ⌫ の左スワイプで消す「単語」の長さを決める（android.* に依存しない純粋な処理。単体テスト対象）。
 * 規則は iOS の `WordBoundary`（Keyboard/TouchLogic.swift）と同じ:
 * 末尾が改行なら改行 1 つだけ。そうでなければ末尾の空白をまとめて消し、その手前の同じ字種
 * （漢字・ひらがな・カタカナ・英数字）の連なりを 1 単語として消す。記号は 1 文字だけ。
 * iOS は Character（拡張書記素クラスタ）で数えるが、Android の `deleteSurroundingText` は UTF-16 コード単位なので、
 * 数えるのは書記素単位、返すのは UTF-16 コード単位の長さにする。
 */
internal object WordBoundary {
    /** 1 回で消す最大の文字数（書記素の数。暴走防止） */
    const val MAX_COUNT = 50

    private enum class Kind { SPACE, NEWLINE, KANJI, HIRAGANA, KATAKANA, WORD, OTHER }

    private fun kindOf(g: String): Kind {
        if (g == "\n" || g == "\r\n" || g == "\r") return Kind.NEWLINE
        if (g.isEmpty()) return Kind.OTHER
        val cp = g.codePointAt(0)
        if (Character.isWhitespace(cp) || Character.isSpaceChar(cp)) {
            return if (cp == 0x0A || cp == 0x0D || cp == 0x2028 || cp == 0x2029) Kind.NEWLINE else Kind.SPACE
        }
        when (cp) {
            in 0x3041..0x309F -> return Kind.HIRAGANA
            in 0x30A0..0x30FF, in 0x31F0..0x31FF, in 0xFF66..0xFF9F -> return Kind.KATAKANA
            in 0x3400..0x4DBF, in 0x4E00..0x9FFF, in 0xF900..0xFAFF, 0x3005 -> return Kind.KANJI
            in 0x20000..0x2FA1F -> return Kind.KANJI
        }
        if (Character.isLetter(cp) || Character.isDigit(cp)) return Kind.WORD
        return Kind.OTHER
    }

    /** [text] を書記素に分ける（末尾から数えるので、先頭から順に並べて返す） */
    private fun graphemes(text: String): List<String> {
        val out = ArrayList<String>()
        val bi = BreakIterator.getCharacterInstance(Locale.ROOT)
        bi.setText(text)
        var start = bi.first()
        var end = bi.next()
        while (end != BreakIterator.DONE) {
            out.add(text.substring(start, end))
            start = end
            end = bi.next()
        }
        return out
    }

    /**
     * カーソル手前のテキスト [before] から、削除する UTF-16 コード単位の数を返す（0 は何もしない）。
     * 長い文脈を渡されても、末尾から [MAX_COUNT] 書記素ぶんしか見ない
     */
    fun deleteLength(before: CharSequence): Int {
        if (before.isEmpty()) return 0
        // 長い文字列全体を分割しないよう、末尾の一部だけを見る（書記素の途中で切らないよう余裕を持たせる）
        val tailLen = minOf(before.length, (MAX_COUNT + 2) * 8)
        val gs = graphemes(before.subSequence(before.length - tailLen, before.length).toString())
        if (gs.isEmpty()) return 0
        var index = gs.size - 1
        if (kindOf(gs[index]) == Kind.NEWLINE) return gs[index].length
        var count = 0
        var units = 0
        while (index >= 0 && count < MAX_COUNT && kindOf(gs[index]) == Kind.SPACE) {
            units += gs[index].length
            count++
            index--
        }
        if (index < 0 || count >= MAX_COUNT) return units
        val wordKind = kindOf(gs[index])
        if (wordKind == Kind.NEWLINE) return units
        if (wordKind == Kind.OTHER) return units + gs[index].length
        while (index >= 0 && count < MAX_COUNT && kindOf(gs[index]) == wordKind) {
            units += gs[index].length
            count++
            index--
        }
        return units
    }

    /** 入力中の文字列（コアの backspace を何回送るか）。[preeditBeforeCursor] のカーソル手前を書記素で数える */
    fun deleteCount(preeditBeforeCursor: CharSequence): Int {
        val n = deleteLength(preeditBeforeCursor)
        if (n == 0) return 0
        val s = preeditBeforeCursor.subSequence(preeditBeforeCursor.length - n, preeditBeforeCursor.length).toString()
        return graphemes(s).size
    }
}
