package com.shiki01.nagi

/**
 * 組み込みのフォールバック。JSON のレイアウト（assets のプリセット・ユーザーのレイアウト）を 1 つも読めないとき、
 * [KeyboardView] は従来のコード組み立て（12 キー / QWERTY / 数字・記号。`KeyboardView` の `legacy*` 関数）に落ちる。
 * どれで組み立てるかの判定をここに置く（純粋関数。単体テスト対象）。
 */
internal object BuiltinLayouts {
    enum class Kind { FLICK, QWERTY, NUMBERS }

    /** JSON から読めずに数字・記号の画面として扱う切替先の名前 */
    private val NUMBER_PAGES = setOf(LayoutIds.NUMBERS, LayoutIds.SYMBOLS, LayoutIds.EMOJI)

    /**
     * 組み込みで組み立てる種類。[explicitId] は明示的に開いたレイアウトの ID（null は かな / 英字 の割り当て）、
     * [kanaMode] はかな入力か、[flickPreferred] は従来の設定「12 キー」か
     */
    fun kindFor(explicitId: String?, kanaMode: Boolean, flickPreferred: Boolean): Kind = when {
        explicitId != null && explicitId in NUMBER_PAGES -> Kind.NUMBERS
        kanaMode && flickPreferred -> Kind.FLICK
        else -> Kind.QWERTY
    }
}
