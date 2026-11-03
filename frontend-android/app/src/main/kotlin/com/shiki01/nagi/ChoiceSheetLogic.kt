package com.shiki01.nagi

// 選択シート（choiceDialog）と選択行（selectRow）の、画面に依存しない判定・組み立て。

/** シートに並べる 1 行分。[note] は 2 行目の補足（空なら null）、[selected] は現在の選択 */
internal data class SheetItem(val label: String, val note: String?, val selected: Boolean)

internal object ChoiceSheetLogic {
    /** シートの最大の幅（dp）。これより広い画面では中央に寄せる */
    const val MAX_WIDTH_DP = 560

    /** シート全体の高さの上限（画面の高さに対する割合） */
    const val MAX_HEIGHT_RATIO = 0.85f

    /**
     * 選択肢の行を組み立てる。[notes] は [labels] と同じ並びの補足文（null や短い場合は補足なし。空白だけの補足も無し）。
     * [selectedIndex] が範囲外（-1 など）なら、どの行も選択中にしない
     */
    fun items(labels: List<String>, notes: List<String?>?, selectedIndex: Int): List<SheetItem> =
        labels.mapIndexed { i, label ->
            val note = notes?.getOrNull(i)?.takeIf { it.isNotBlank() }
            SheetItem(label, note, i == selectedIndex)
        }

    /** [values] の中で [selected] と一致する最初の位置。無ければ -1 */
    fun indexOfValue(values: List<String>, selected: String?): Int = values.indexOfFirst { it == selected }

    /** 選択行に出す位置。一致が無ければ先頭を出す */
    fun displayIndex(values: List<String>, selected: String?): Int = indexOfValue(values, selected).coerceAtLeast(0)

    /** 選び直したか（同じ項目の再選択や範囲外は無視する） */
    fun changed(picked: Int, current: Int, size: Int): Boolean = picked != current && picked in 0 until size

    /** 選択肢のスクロール領域の高さの上限。[reservedPx] はドラッグバー・タイトル・下の余白の分 */
    fun maxListHeight(screenHeightPx: Int, reservedPx: Int): Int =
        Math.max(1, (screenHeightPx * MAX_HEIGHT_RATIO).toInt() - reservedPx)

    /** シートの幅。画面が [maxWidthPx] より広ければ [maxWidthPx]、そうでなければ画面幅 */
    fun sheetWidth(screenWidthPx: Int, maxWidthPx: Int): Int = Math.min(screenWidthPx, maxWidthPx)

    /** 現在の選択が見えるようにスクロールする位置（行の上端から少し手前）。負にはしない */
    fun scrollTarget(rowTopPx: Int, marginPx: Int): Int = Math.max(0, rowTopPx - marginPx)
}
