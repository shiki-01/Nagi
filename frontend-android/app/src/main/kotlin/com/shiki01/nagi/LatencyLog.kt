package com.shiki01.nagi

import java.util.Locale

/** 応答時間の集計結果（ミリ秒） */
internal data class LatencySummary(val count: Int, val p50: Double, val p95: Double, val max: Double)

/** 応答時間の p50 / p95 などの計算（android.* に依存しない純粋関数） */
internal object LatencyStats {
    /** 最近接順位法のパーセンタイル（[p] は 1〜100）。空なら null */
    fun percentile(values: List<Double>, p: Int): Double? {
        if (values.isEmpty()) return null
        val sorted = values.sorted()
        val rank = Math.ceil(p.coerceIn(1, 100) * sorted.size / 100.0).toInt().coerceIn(1, sorted.size)
        return sorted[rank - 1]
    }

    fun summarize(values: List<Double>): LatencySummary? {
        val p50 = percentile(values, 50) ?: return null
        val p95 = percentile(values, 95) ?: return null
        return LatencySummary(values.size, p50, p95, values.fold(0.0) { a, b -> if (b > a) b else a })
    }

    fun formatMs(v: Double): String = String.format(Locale.ROOT, "%.1f ms", v)
}

/** コアの RPC 1 回あたりの所要時間（ミリ秒）の直近 [CAPACITY] 件。どのスレッドからでも使える */
internal object LatencyLog {
    const val CAPACITY = 100

    private val buf = ArrayDeque<Double>()

    @Synchronized
    fun record(ms: Double) {
        buf.addLast(ms)
        while (buf.size > CAPACITY) buf.removeFirst()
    }

    @Synchronized
    fun snapshot(): List<Double> = buf.toList()

    @Synchronized
    fun clear() {
        buf.clear()
    }

    fun summary(): LatencySummary? = LatencyStats.summarize(snapshot())
}
