package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class EnvLogicTest {
    @Test
    fun onTrimMemoryの段階をmemory_pressureに写す() {
        // ComponentCallbacks2: RUNNING_MODERATE=5 / RUNNING_LOW=10 / RUNNING_CRITICAL=15 / UI_HIDDEN=20 / BACKGROUND=40 / MODERATE=60 / COMPLETE=80
        assertNull(EnvLogic.memoryPressureForTrim(5))
        assertEquals("warning", EnvLogic.memoryPressureForTrim(10))
        assertEquals("critical", EnvLogic.memoryPressureForTrim(15))
        assertEquals("warning", EnvLogic.memoryPressureForTrim(20))
        assertEquals("warning", EnvLogic.memoryPressureForTrim(40))
        assertEquals("critical", EnvLogic.memoryPressureForTrim(60))
        assertEquals("critical", EnvLogic.memoryPressureForTrim(80))
    }

    @Test
    fun 段階の間の値は直前の段階に倒す() {
        assertNull(EnvLogic.memoryPressureForTrim(0))
        assertEquals("warning", EnvLogic.memoryPressureForTrim(12))
        assertEquals("critical", EnvLogic.memoryPressureForTrim(17))
        assertEquals("warning", EnvLogic.memoryPressureForTrim(59))
    }

    @Test
    fun 保持中の逼迫は重い方を残す() {
        assertEquals("critical", EnvLogic.mergePressure("critical", "warning"))
        assertEquals("critical", EnvLogic.mergePressure("warning", "critical"))
        assertEquals("warning", EnvLogic.mergePressure(null, "warning"))
        assertEquals("warning", EnvLogic.mergePressure("normal", "warning"))
    }

    @Test
    fun 熱の状態を写す() {
        assertEquals("nominal", EnvLogic.thermalForStatus(0))
        assertEquals("nominal", EnvLogic.thermalForStatus(1))
        assertEquals("fair", EnvLogic.thermalForStatus(2))
        assertEquals("serious", EnvLogic.thermalForStatus(3))
        assertEquals("critical", EnvLogic.thermalForStatus(4))
        assertEquals("critical", EnvLogic.thermalForStatus(6))
    }

    @Test
    fun 充電中でなければバッテリー駆動() {
        assertTrue(EnvLogic.onBatteryForPlugged(0))
        assertFalse(EnvLogic.onBatteryForPlugged(1))
        assertFalse(EnvLogic.onBatteryForPlugged(2))
    }

    @Test
    fun メモリ予算は空きの半分で低メモリ時は4分の1() {
        val mb = 1024L * 1024L
        assertEquals(500L, EnvLogic.memoryBudgetMb(1000 * mb, lowMemory = false))
        assertEquals(250L, EnvLogic.memoryBudgetMb(1000 * mb, lowMemory = true))
        assertEquals(64L, EnvLogic.memoryBudgetMb(10 * mb, lowMemory = false))
        assertEquals(4096L, EnvLogic.memoryBudgetMb(100_000 * mb, lowMemory = false))
    }
}
