package com.shiki01.nagi

import android.text.InputType

// 英字入力の予測候補・スペル訂正（H-146）。判断はキーボード側（入力欄の種類・設定）で、候補の中身はコア
// （`english_suggest`）が決める。android.* の定数のほかに依存しない純粋関数（単体テストの対象）。
// iOS の `Keyboard/TouchLogic.swift`（EnglishAssistPolicy / EnglishWord / EnglishAutocorrectUndo / EnglishSelection）と同じ規則。

internal object EnglishAssistPolicy {
    /** 自動修正の契機になる文字（語の区切り）。スペースと句読点 */
    private val TRIGGERS = setOf(" ", ".", ",", "!", "?", ";", ":")

    /**
     * 候補バーに英語の候補を出すか。英字のレイアウトで、入力中の文字列（かなの composing）が無く、
     * パスワード欄でないとき
     */
    fun shouldSuggest(isAlpha: Boolean, setting: Boolean, isSecure: Boolean, hasPreedit: Boolean): Boolean =
        setting && isAlpha && !isSecure && !hasPreedit

    /**
     * 自動修正してよいか。候補を出す条件に加え、設定がオンで、入力欄が文字の欄（`TYPE_CLASS_TEXT`）で、
     * 候補を断っておらず（`TYPE_TEXT_FLAG_NO_SUGGESTIONS`）、URL・メールアドレス・パスワードの欄でないとき
     */
    fun shouldAutocorrect(suggest: Boolean, setting: Boolean, inputType: Int): Boolean {
        if (!suggest || !setting) return false
        if ((inputType and InputType.TYPE_MASK_CLASS) != InputType.TYPE_CLASS_TEXT) return false
        if ((inputType and InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS) != 0) return false
        return when (inputType and InputType.TYPE_MASK_VARIATION) {
            InputType.TYPE_TEXT_VARIATION_URI,
            InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD,
            -> false
            else -> true
        }
    }

    /** 自動修正の契機になる文字（語の区切り）か */
    fun isTrigger(text: String): Boolean = text in TRIGGERS
}

/** カーソル直前の英単語（英字・アポストロフィ・ハイフンの並び）の切り出し */
internal object EnglishWord {
    /** 語の最大の長さ（これより長い並びは語として扱わない） */
    const val MAX_LENGTH = 32

    private fun isAsciiLetter(c: Char): Boolean = c in 'a'..'z' || c in 'A'..'Z'

    private fun isWordChar(c: Char): Boolean = isAsciiLetter(c) || c == '\'' || c == '’' || c == '-'

    /**
     * [before]（カーソル直前のテキスト）の末尾の語。末尾が英字でなければ空（`end-`・`don'` は空）。
     * 先頭のアポストロフィとハイフンは語に含めない（`'hello` → `hello`）。数字が直前に付いている並び
     * （`abc123def` の `def`）は語として扱わない。日本語の直後の英字（`今日はmeeting`）は語として扱う
     */
    fun current(before: String): String {
        var start = before.length
        while (start > 0) {
            val c = before[start - 1]
            if (!isWordChar(c)) {
                if (c in '0'..'9') return ""
                break
            }
            start--
            if (before.length - start > MAX_LENGTH) return ""
        }
        if (start == before.length || !isAsciiLetter(before[before.length - 1])) return ""
        var word = before.substring(start)
        while (word.isNotEmpty() && !isAsciiLetter(word[0])) word = word.substring(1)
        return word.replace('’', '\'')
    }
}

/** カーソル直前のテキストの追跡（自分で入れた・消した文字で更新する。長くなりすぎないよう末尾だけ残す） */
internal object EnglishBuffer {
    /** 追う長さ（UTF-16 コード単位） */
    const val MAX_LENGTH = 64

    fun append(buffer: String, text: String): String = (buffer + text).takeLast(MAX_LENGTH)

    /** 末尾の [count] コード単位を消す */
    fun drop(buffer: String, count: Int): String = buffer.dropLast(count.coerceAtLeast(0))

    /** 末尾の 1 文字（コードポイント）を消す（⌫ 1 回ぶん） */
    fun dropOne(buffer: String): String {
        if (buffer.isEmpty()) return buffer
        val n = if (buffer.length >= 2 && Character.isLowSurrogate(buffer.last()) &&
            Character.isHighSurrogate(buffer[buffer.length - 2])
        ) {
            2
        } else {
            1
        }
        return buffer.dropLast(n)
    }
}

/** 直前に適用した自動修正（⌫ ですぐ取り消せるように覚えておく） */
internal data class EnglishAutocorrectUndo(
    /** 打った語 */
    val original: String,
    /** 直した語 */
    val replacement: String,
    /** 直した語の後ろに入れた区切り（スペース・句読点） */
    val trailing: String,
) {
    /** カーソル直前が「直した語 + 区切り」のままか（ほかの操作が挟まっていないか） */
    fun matches(before: String?): Boolean = before != null && before.endsWith(replacement + trailing)

    /** 取り消しで消す文字数（直した語と区切り）と、代わりに入れる文字列（打った語。区切りは入れない） */
    val revert: Pair<Int, String> get() = Pair((replacement + trailing).length, original)
}

/** 候補を選んだときの入力欄への操作: 打っている途中の語を消して、候補とスペースを入れる */
internal object EnglishSelection {
    /**
     * - [word]: カーソル直前の語（[EnglishWord.current]）
     * - [suggestion]: 選んだ候補
     * - [nextIsSpace]: カーソル直後がすでにスペースか（重ねて入れない）
     *
     * 消す文字数（UTF-16 コード単位）と、入れる文字列を返す
     */
    fun edit(word: String, suggestion: String, nextIsSpace: Boolean): Pair<Int, String> =
        Pair(word.length, suggestion + if (nextIsSpace) "" else " ")
}

/** 英字入力の候補バーの組み立て */
internal object EnglishBar {
    /**
     * コアの結果を候補バーの表示にする。打ったままの語しか無ければ null（候補バーの代わりにツールバーを出す）。
     * 直す対象 [autocorrect] があるときは、打ったままの語を引用符で囲んで「このまま」と分かるようにし、
     * 直し先（スペースで入る語）を選択中の色で示す
     */
    fun build(result: EnglishSuggestResult, autocorrect: String?): CandidateBar? {
        val s = result.suggestions
        if (s.size <= 1) return null
        val items = s.map { if (it.kind == EnglishSuggestionKind.TYPED && autocorrect != null) "“${it.text}”" else it.text }
        val selected = if (autocorrect != null) s.indexOfFirst { it.text == autocorrect } else -1
        return CandidateBar(
            items = items,
            selected = selected,
            kind = CandidateKind.ENGLISH,
            pinned = null,
            hints = items.map { null },
            readings = items.map { null },
        )
    }
}
