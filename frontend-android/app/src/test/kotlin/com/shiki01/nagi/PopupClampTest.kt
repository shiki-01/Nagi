package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PopupClampTest {
    private val bounds = BoxF(0f, 0f, 400f, 300f)

    @Test
    fun 収まっているときは移動量0() {
        val s = PopupClamp.shift(BoxF(100f, 100f, 200f, 200f), bounds)
        assertTrue(s.isZero)
    }

    @Test
    fun 下端のはみ出しは上へ() {
        val s = PopupClamp.shift(BoxF(100f, 250f, 200f, 330f), bounds)
        assertEquals(0f, s.dx, 0f)
        assertEquals(-30f, s.dy, 0f)
    }

    @Test
    fun 上端のはみ出しは下へ() {
        val s = PopupClamp.shift(BoxF(100f, -20f, 200f, 60f), bounds)
        assertEquals(0f, s.dx, 0f)
        assertEquals(20f, s.dy, 0f)
    }

    @Test
    fun 左右は内側へ() {
        assertEquals(15f, PopupClamp.shift(BoxF(-15f, 100f, 80f, 200f), bounds).dx, 0f)
        assertEquals(-10f, PopupClamp.shift(BoxF(300f, 100f, 410f, 200f), bounds).dx, 0f)
    }

    @Test
    fun 複数の端にはみ出すときは両方向を補正する() {
        val s = PopupClamp.shift(BoxF(-5f, 260f, 90f, 320f), bounds)
        assertEquals(5f, s.dx, 0f)
        assertEquals(-20f, s.dy, 0f)
    }

    @Test
    fun 領域より大きいときは上と左を揃える() {
        val s = PopupClamp.shift(BoxF(10f, 10f, 500f, 400f), bounds)
        assertEquals(-10f, s.dx, 0f)
        assertEquals(-10f, s.dy, 0f)
    }

    @Test
    fun 領域が空や不正な矩形は動かさない() {
        assertTrue(PopupClamp.shift(BoxF(0f, 500f, 10f, 600f), BoxF(0f, 0f, 0f, 0f)).isZero)
        assertTrue(PopupClamp.shift(BoxF(Float.NaN, 0f, 10f, 10f), bounds).isZero)
    }

    @Test
    fun 外接矩形は文字のある方向だけを数える() {
        val key = BoxF(100f, 200f, 160f, 250f)
        val all = PopupClamp.flickCluster(key, 2f, left = true, up = true, right = true, down = true)
        assertEquals(BoxF(38f, 148f, 222f, 302f), all)
        val none = PopupClamp.flickCluster(key, 2f, left = false, up = false, right = false, down = false)
        assertEquals(key, none)
    }

    @Test
    fun 最下段のキーは全体が上へずれて相対位置が保たれる() {
        // キーボードの高さ 300、最下段のキー（高さ 50）が 250..300。下のセルが 300..350 ではみ出す
        val key = BoxF(100f, 250f, 160f, 300f)
        val s = PopupClamp.flickShift(key, 0f, left = true, up = true, right = true, down = true, bounds = bounds)
        assertEquals(0f, s.dx, 0f)
        assertEquals(-50f, s.dy, 0f)
        // ずらした後の外接矩形が下端に接する
        val moved = PopupClamp.flickCluster(key, 0f, true, true, true, true)
        assertEquals(300f, moved.bottom + s.dy, 0f)
    }

    @Test
    fun 下の文字が無ければ最下段でもずらさない() {
        val key = BoxF(100f, 250f, 160f, 300f)
        val s = PopupClamp.flickShift(key, 0f, left = true, up = true, right = true, down = false, bounds = bounds)
        assertTrue(s.isZero)
    }

    @Test
    fun 左端のキーの左セルは右へずれる() {
        val key = BoxF(0f, 100f, 60f, 150f)
        val s = PopupClamp.flickShift(key, 0f, left = true, up = true, right = true, down = true, bounds = bounds)
        assertEquals(60f, s.dx, 0f)
        assertEquals(0f, s.dy, 0f)
    }
}
