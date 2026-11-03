package com.shiki01.nagi

// ポップアップを描画領域に収める計算（android.* に依存しない純粋関数。単体テスト対象）。
// iOS の `PopupClamp`（TouchLogic.swift）と同じ規則。

/** 矩形（左・上・右・下。画面座標で下が +） */
internal data class BoxF(val left: Float, val top: Float, val right: Float, val bottom: Float) {
    val width: Float get() = right - left
    val height: Float get() = bottom - top
}

/** 平行移動量 */
internal data class ShiftF(val dx: Float, val dy: Float) {
    val isZero: Boolean get() = dx == 0f && dy == 0f
}

internal object PopupClamp {
    /**
     * [rect] を [bounds] の中に収めるのに必要な移動量。
     * 下端のはみ出しは上へ、上端は下へ、左右は内側へ動かす。[rect] が [bounds] より大きい方向は上・左の端を揃える。
     * [bounds] が空（まだ測れていない）や、値が有限でないときは動かさない
     */
    fun shift(rect: BoxF, bounds: BoxF): ShiftF {
        if (bounds.width <= 0f || bounds.height <= 0f) return ShiftF(0f, 0f)
        val all = floatArrayOf(rect.left, rect.top, rect.right, rect.bottom)
        if (all.any { it.isNaN() || it.isInfinite() }) return ShiftF(0f, 0f)
        return ShiftF(
            axis(rect.left, rect.right, bounds.left, bounds.right),
            axis(rect.top, rect.bottom, bounds.top, bounds.bottom),
        )
    }

    private fun axis(low: Float, high: Float, boundsLow: Float, boundsHigh: Float): Float = when {
        high - low > boundsHigh - boundsLow || low < boundsLow -> boundsLow - low
        high > boundsHigh -> boundsHigh - high
        else -> 0f
    }

    /**
     * フリックの案内 5 セル全体の外接矩形。中央のセルは [key]（押したキー）と同じ位置で、
     * 左右上下のセルは [spacing] を挟んで隣に並ぶ。文字のある方向だけを数える
     */
    fun flickCluster(key: BoxF, spacing: Float, left: Boolean, up: Boolean, right: Boolean, down: Boolean): BoxF {
        val w = key.width
        val h = key.height
        return BoxF(
            left = if (left) key.left - w - spacing else key.left,
            top = if (up) key.top - h - spacing else key.top,
            right = if (right) key.right + w + spacing else key.right,
            bottom = if (down) key.bottom + h + spacing else key.bottom,
        )
    }

    /** フリックの案内全体を [bounds] に収める共通の移動量（全セルに同じ量を足す） */
    fun flickShift(
        key: BoxF,
        spacing: Float,
        left: Boolean,
        up: Boolean,
        right: Boolean,
        down: Boolean,
        bounds: BoxF,
    ): ShiftF = shift(flickCluster(key, spacing, left, up, right, down), bounds)
}
