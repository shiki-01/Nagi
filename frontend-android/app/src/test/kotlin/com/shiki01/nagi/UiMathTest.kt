package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class UiMathTest {
    @Test
    fun blendEndpoints() {
        val a = 0xFF102030.toInt()
        val b = 0xFFF0E0D0.toInt()
        assertEquals(a, blendColors(a, b, 0f))
        assertEquals(b, blendColors(a, b, 1f))
    }

    @Test
    fun blendMiddleAndClamp() {
        assertEquals(0xFF808080.toInt(), blendColors(0xFF000000.toInt(), 0xFFFFFFFF.toInt(), 0.5f))
        assertEquals(0xFFFFFFFF.toInt(), blendColors(0xFF000000.toInt(), 0xFFFFFFFF.toInt(), 2f))
        assertEquals(0xFF000000.toInt(), blendColors(0xFF000000.toInt(), 0xFFFFFFFF.toInt(), -1f))
    }

    @Test
    fun alphaReplaced() {
        assertEquals(0x33112233, withAlpha(0xFF112233.toInt(), 0x33))
        assertEquals(0xFF112233.toInt(), withAlpha(0x00112233, 999))
    }

    @Test
    fun progress() {
        assertEquals(500, progressScaled(50, 100, 1000))
        assertEquals(1000, progressScaled(200, 100, 1000))
        assertEquals(0, progressScaled(-5, 100, 1000))
        assertNull(progressScaled(10, 0, 1000))
        assertNull(progressScaled(10, -1, 1000))
    }
}
