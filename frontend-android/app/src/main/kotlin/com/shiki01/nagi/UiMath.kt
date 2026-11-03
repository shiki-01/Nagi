package com.shiki01.nagi

// 本体アプリの見た目に使う、Android に依存しない純粋な計算（単体テスト対象）

/** ARGB の 2 色を [ratio]（0.0 = from, 1.0 = to）で混ぜる */
internal fun blendColors(from: Int, to: Int, ratio: Float): Int {
    val r = ratio.coerceIn(0f, 1f)
    fun channel(shift: Int): Int {
        val a = (from ushr shift) and 0xFF
        val b = (to ushr shift) and 0xFF
        return Math.round(a + (b - a) * r).coerceIn(0, 255)
    }
    return (channel(24) shl 24) or (channel(16) shl 16) or (channel(8) shl 8) or channel(0)
}

/** 色の不透明度だけを [alpha]（0〜255）に差し替える */
internal fun withAlpha(color: Int, alpha: Int): Int = (color and 0x00FFFFFF) or (alpha.coerceIn(0, 255) shl 24)

/** 進捗を 0〜[max] の整数にする。全体の大きさが分からない（0 以下）ときは null（不確定の進捗） */
internal fun progressScaled(done: Long, total: Long, max: Int): Int? {
    if (total <= 0L || max <= 0) return null
    return (done.coerceAtLeast(0L) * max / total).toInt().coerceIn(0, max)
}
