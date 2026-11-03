package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FlickLogicTest {
    private val th = 20f
    private val w = 100f
    private val h = 100f

    private fun judge(dx: Float, dy: Float) = FlickLogic.judge(dx, dy, th, w, h)

    @Test
    fun 閾値未満はタップ() {
        val r = judge(5f, -10f)
        assertFalse(r.cancelled)
        assertEquals(FlickDirection.TAP, r.direction)
        assertEquals("tap", FlickLogic.gestureFor(r, false))
    }

    @Test
    fun トグル入力ではタップをtoggleに読み替える() {
        val r = judge(0f, 0f)
        assertEquals("toggle", FlickLogic.gestureFor(r, true))
        // フリックは読み替えない
        assertEquals("flick_left", FlickLogic.gestureFor(judge(-40f, 0f), true))
    }

    @Test
    fun 四方向を判定する() {
        assertEquals(FlickDirection.LEFT, judge(-40f, 5f).direction)
        assertEquals(FlickDirection.RIGHT, judge(40f, -5f).direction)
        assertEquals(FlickDirection.UP, judge(5f, -40f).direction)
        assertEquals(FlickDirection.DOWN, judge(-5f, 40f).direction)
        assertEquals("flick_up", judge(0f, -40f).direction.gesture)
    }

    @Test
    fun 大きい軸の方向になる() {
        assertEquals(FlickDirection.RIGHT, judge(50f, -30f).direction)
        assertEquals(FlickDirection.UP, judge(30f, -50f).direction)
    }

    @Test
    fun 片方の軸だけ閾値を超えればフリック() {
        assertEquals(FlickDirection.UP, judge(10f, -25f).direction)
    }

    @Test
    fun キーサイズの2倍を超えたら取り消し() {
        assertTrue(judge(201f, 0f).cancelled)
        assertTrue(judge(0f, -201f).cancelled)
        assertFalse(judge(200f, 0f).cancelled)
    }

    @Test
    fun 角度は右0_上90_左180_下270() {
        assertEquals(0.0, judge(40f, 0f).angleDeg, 1e-6)
        assertEquals(90.0, judge(0f, -40f).angleDeg, 1e-6)
        assertEquals(180.0, judge(-40f, 0f).angleDeg, 1e-6)
        assertEquals(270.0, judge(0f, 40f).angleDeg, 1e-6)
        assertEquals(45.0, judge(40f, -40f).angleDeg, 1e-6)
    }

    @Test
    fun 距離はキー幅比() {
        assertEquals(0.5, judge(30f, 40f).distance, 1e-6)
    }

    @Test
    fun 方向の文字を引く() {
        val ka = KeyTables.FALLBACK.first { it.key == "ka" }
        assertEquals("か", FlickLogic.charAt(ka, FlickDirection.TAP))
        assertEquals("き", FlickLogic.charAt(ka, FlickDirection.LEFT))
        assertEquals("く", FlickLogic.charAt(ka, FlickDirection.UP))
        assertEquals("け", FlickLogic.charAt(ka, FlickDirection.RIGHT))
        assertEquals("こ", FlickLogic.charAt(ka, FlickDirection.DOWN))
    }
}
