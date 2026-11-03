package com.shiki01.nagi

// 数字・記号の入力中の扱い（IN-16・IN-17）の判定。android.* に依存しない純粋関数（単体テスト対象）。
// iOS の `Keyboard/TouchLogic.swift`（NumberCompose / EnglishCompose / LayoutSwitchKeep）と同じ規則。

internal object NumberCompose {
    fun isNumberOrSymbol(mode: LayoutMode): Boolean = mode == LayoutMode.NUMBER || mode == LayoutMode.SYMBOL

    /**
     * 数字・記号の文字を、コアの入力中の文字列（`insert_reading`）に入れるか。
     * 設定 `input.compose_numbers` がオン、パスワード欄でなく、日本語入力の流れ（かな入力から数字・記号の画面に来た、
     * または既に入力中の文字列がある）のときだけ。それ以外は従来どおりアプリへ直接入れる
     */
    fun shouldCompose(setting: Boolean, isSecure: Boolean, kanaContext: Boolean, hasPreedit: Boolean): Boolean =
        setting && !isSecure && (kanaContext || hasPreedit)
}

internal object EnglishCompose {
    /** 英字のキーも入力中の文字列に続けるか（設定 `input.keep_preedit_on_english`。入力中の文字列があるときだけ） */
    fun shouldCompose(keepSetting: Boolean, isSecure: Boolean, hasPreedit: Boolean): Boolean =
        keepSetting && !isSecure && hasPreedit
}

internal object LayoutSwitchKeep {
    private fun japanese(m: LayoutMode) = m == LayoutMode.KANA || NumberCompose.isNumberOrSymbol(m)

    /**
     * レイアウトを切り替えるとき、入力中の文字列を確定せずに保つか。保たないときは先に確定してから切り替える。
     * かな・数字・記号の間（かな同士は対象外）は [keepOnSymbols]、英字とそれらの間は [keepOnEnglish]。
     * 数字・記号が関わる切替は、切替先で入力を続けられるよう [composeNumbers] がオンのときだけ保つ。
     * 絵文字・カスタムへの切替は常に確定する
     */
    fun shouldKeep(
        from: LayoutMode,
        to: LayoutMode,
        hasPreedit: Boolean,
        isSecure: Boolean,
        keepOnSymbols: Boolean,
        keepOnEnglish: Boolean,
        composeNumbers: Boolean,
    ): Boolean {
        if (!hasPreedit || isSecure) return false
        val numeric = NumberCompose.isNumberOrSymbol(from) || NumberCompose.isNumberOrSymbol(to)
        if (numeric && !composeNumbers) return false
        if (japanese(from) && japanese(to)) return keepOnSymbols && !(from == LayoutMode.KANA && to == LayoutMode.KANA)
        if ((from == LayoutMode.ALPHA && japanese(to)) || (to == LayoutMode.ALPHA && japanese(from))) return keepOnEnglish
        return false
    }
}
