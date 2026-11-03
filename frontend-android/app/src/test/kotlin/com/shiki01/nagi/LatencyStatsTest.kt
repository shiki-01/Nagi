package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LatencyStatsTest {
    @Test
    fun percentileUsesNearestRank() {
        val v = (1..100).map { it.toDouble() }.reversed()
        assertEquals(50.0, LatencyStats.percentile(v, 50)!!, 0.0)
        assertEquals(95.0, LatencyStats.percentile(v, 95)!!, 0.0)
        assertEquals(100.0, LatencyStats.percentile(v, 100)!!, 0.0)
    }

    @Test
    fun percentileOfSmallLists() {
        assertNull(LatencyStats.percentile(emptyList(), 50))
        assertEquals(7.0, LatencyStats.percentile(listOf(7.0), 95)!!, 0.0)
        // 4 件の p50 は 2 番目、p95 は 4 番目
        val v = listOf(40.0, 10.0, 30.0, 20.0)
        assertEquals(20.0, LatencyStats.percentile(v, 50)!!, 0.0)
        assertEquals(40.0, LatencyStats.percentile(v, 95)!!, 0.0)
    }

    @Test
    fun summarizeReportsCountAndMax() {
        assertNull(LatencyStats.summarize(emptyList()))
        val s = LatencyStats.summarize(listOf(1.0, 3.0, 2.0))!!
        assertEquals(3, s.count)
        assertEquals(2.0, s.p50, 0.0)
        assertEquals(3.0, s.p95, 0.0)
        assertEquals(3.0, s.max, 0.0)
    }

    @Test
    fun formatMsHasOneDecimal() {
        assertEquals("12.3 ms", LatencyStats.formatMs(12.34))
        assertEquals("0.0 ms", LatencyStats.formatMs(0.0))
    }

    @Test
    fun logKeepsOnlyTheLatestEntries() {
        LatencyLog.clear()
        for (i in 1..(LatencyLog.CAPACITY + 20)) LatencyLog.record(i.toDouble())
        val snap = LatencyLog.snapshot()
        assertEquals(LatencyLog.CAPACITY, snap.size)
        assertEquals(21.0, snap.first(), 0.0)
        assertEquals((LatencyLog.CAPACITY + 20).toDouble(), snap.last(), 0.0)
        LatencyLog.clear()
        assertNull(LatencyLog.summary())
    }
}
