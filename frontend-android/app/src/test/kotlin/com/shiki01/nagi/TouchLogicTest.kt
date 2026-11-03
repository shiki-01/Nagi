package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TouchLogicTest {
    // ---- スペースのスワイプ（iOS の SpaceSwipeTracker と同じ期待値） ----

    @Test
    fun 少し動いただけではタップのまま() {
        val t = SpaceSwipeTracker(14f)
        assertEquals(0, t.update(5f))
        assertEquals(0, t.update(-13.9f))
        assertFalse(t.isActive)
    }

    @Test
    fun しきい値でカーソル移動に入り_距離に比例して動かす() {
        val t = SpaceSwipeTracker(14f)
        assertEquals(1, t.update(14f))
        assertTrue(t.isActive)
        assertEquals(0, t.update(20f))
        assertEquals(2, t.update(43f)) // 合計 3 文字
        assertEquals(4, t.update(100f)) // 合計 7 文字
    }

    @Test
    fun 左スワイプは負() {
        val t = SpaceSwipeTracker(10f)
        assertEquals(-2, t.update(-25f))
        assertEquals(-1, t.update(-39f))
    }

    @Test
    fun 指を戻すと戻る_入ったあとは元の位置でも離して空白を入れない() {
        val t = SpaceSwipeTracker(10f)
        assertEquals(3, t.update(30f))
        assertEquals(-3, t.update(5f))
        assertTrue(t.isActive)
    }

    @Test
    fun 距離の下限と非数() {
        val t = SpaceSwipeTracker(0f)
        // 0 以下の距離は 1px に丸める（0 で割らない）
        assertEquals(2, t.update(2f))
        val t2 = SpaceSwipeTracker(14f)
        assertEquals(0, t2.update(Float.NaN))
        assertEquals(0, t2.update(Float.POSITIVE_INFINITY))
        assertFalse(t2.isActive)
        assertEquals(1, SpaceSwipeTracker(Float.NaN).update(1f))
    }

    // ---- ⌫ の連打の加速（iOS の RepeatKeyView と同じ） ----

    @Test
    fun 加速の下限() {
        assertEquals(25, RepeatSchedule.minInterval(60)) // 60 * 32 / 100 = 19 → 25 に引き上げ
        assertEquals(128, RepeatSchedule.minInterval(400)) // 400 * 32 / 100
        assertEquals(25, RepeatSchedule.minInterval(30))
    }

    @Test
    fun 四回ごとに一割五分縮める() {
        assertEquals(60, RepeatSchedule.nextInterval(60, 60, 1))
        assertEquals(60, RepeatSchedule.nextInterval(60, 60, 3))
        assertEquals(51, RepeatSchedule.nextInterval(60, 60, 4))
        assertEquals(51, RepeatSchedule.nextInterval(51, 60, 5))
        assertEquals(43, RepeatSchedule.nextInterval(51, 60, 8))
    }

    @Test
    fun 下限より速くならない() {
        assertEquals(25, RepeatSchedule.nextInterval(25, 60, 4))
        assertEquals(25, RepeatSchedule.nextInterval(28, 60, 8)) // 28 * 85 / 100 = 23 → 25
        var interval = 60
        for (count in 1..RepeatSchedule.MAX_REPEATS) interval = RepeatSchedule.nextInterval(interval, 60, count)
        assertEquals(25, interval)
    }

    // ---- Shift / Caps Lock ----

    @Test
    fun 一回押すと次の1文字だけ() {
        val r = ShiftLogic.tap(ShiftState.OFF, null, 1000L)
        assertEquals(ShiftState.ONCE, r.state)
        assertEquals(1000L, r.lastTapAt)
    }

    @Test
    fun 半秒以内の二回押しでCapsLock() {
        val first = ShiftLogic.tap(ShiftState.OFF, null, 1000L)
        val second = ShiftLogic.tap(first.state, first.lastTapAt, 1400L)
        assertEquals(ShiftState.CAPS, second.state)
        assertNull(second.lastTapAt)
    }

    @Test
    fun 半秒を過ぎると解除() {
        val first = ShiftLogic.tap(ShiftState.OFF, null, 1000L)
        val second = ShiftLogic.tap(first.state, first.lastTapAt, 1500L)
        assertEquals(ShiftState.OFF, second.state)
    }

    @Test
    fun CapsLock中の一回で解除() {
        assertEquals(ShiftState.OFF, ShiftLogic.tap(ShiftState.CAPS, null, 5000L).state)
    }

    @Test
    fun 二回押しの境界() {
        assertTrue(ShiftLogic.isDoubleTap(1000L, 1499L))
        assertFalse(ShiftLogic.isDoubleTap(1000L, 1500L))
        assertFalse(ShiftLogic.isDoubleTap(null, 1000L))
        assertFalse(ShiftLogic.isDoubleTap(1000L, 900L)) // 時計が戻った
    }

    @Test
    fun capslockキーは固定と解除を行き来する() {
        assertEquals(ShiftState.CAPS, ShiftLogic.toggleCaps(ShiftState.OFF))
        assertEquals(ShiftState.CAPS, ShiftLogic.toggleCaps(ShiftState.ONCE))
        assertEquals(ShiftState.OFF, ShiftLogic.toggleCaps(ShiftState.CAPS))
    }

    // ---- 片手モード ----

    @Test
    fun 片手モードの幅() {
        assertEquals(1f, OneHandedLogic.bodyFraction(OneHanded.OFF, 0.8f), 0f)
        assertEquals(0.8f, OneHandedLogic.bodyFraction(OneHanded.LEFT, 0.8f), 0f)
        assertEquals(0.9f, OneHandedLogic.bodyFraction(OneHanded.RIGHT, 0.95f), 0f)
        assertEquals(0.6f, OneHandedLogic.bodyFraction(OneHanded.LEFT, 0.3f), 0f)
        assertEquals(0.8f, OneHandedLogic.bodyFraction(OneHanded.LEFT, Float.NaN), 0f)
    }

    @Test
    fun 片手モードの幅はピクセルに四捨五入する() {
        assertEquals(1000, OneHandedLogic.bodyWidthPx(1000, OneHanded.OFF, 0.8f))
        assertEquals(800, OneHandedLogic.bodyWidthPx(1000, OneHanded.LEFT, 0.8f))
        assertEquals(972, OneHandedLogic.bodyWidthPx(1080, OneHanded.RIGHT, 0.9f))
        assertEquals(0, OneHandedLogic.bodyWidthPx(0, OneHanded.LEFT, 0.8f))
    }

    @Test
    fun 片手モードの反対側と読み込み() {
        assertEquals(OneHanded.RIGHT, OneHanded.LEFT.flipped())
        assertEquals(OneHanded.LEFT, OneHanded.RIGHT.flipped())
        assertEquals(OneHanded.OFF, OneHanded.OFF.flipped())
        assertEquals(OneHanded.LEFT, OneHanded.fromJson("left"))
        assertEquals(OneHanded.OFF, OneHanded.fromJson("bogus"))
        assertEquals(OneHanded.OFF, OneHanded.fromJson(null))
    }

    // ---- 高さのプリセット・キーの余白 ----

    @Test
    fun 高さのプリセット() {
        assertEquals(HeightPreset.COMPACT, HeightPreset.matching(0.9f))
        assertEquals(HeightPreset.STANDARD, HeightPreset.matching(1.0f))
        assertEquals(HeightPreset.TALL, HeightPreset.matching(1.1f))
        // スライダーの計算誤差は許す
        assertEquals(HeightPreset.STANDARD, HeightPreset.matching(1.0000001f))
        assertNull(HeightPreset.matching(1.05f))
        assertNotNull(HeightPreset.fromJson("tall"))
        assertNull(HeightPreset.fromJson("huge"))
        assertEquals(0.9f, HeightPreset.COMPACT.scale, 0f)
        assertEquals(1.1f, HeightPreset.TALL.scale, 0f)
    }

    @Test
    fun キーの余白は既定で従来の値() {
        // 既定の間隔 4dp で、左右 2dp・上下 3dp（従来の固定値）
        assertEquals(2f, KeyMetrics.horizontalMarginDp(KeyboardDefaults.KEY_SPACING_DP), 0f)
        assertEquals(3f, KeyMetrics.verticalMarginDp(KeyboardDefaults.KEY_SPACING_DP), 0f)
        assertEquals(0f, KeyMetrics.horizontalMarginDp(0f), 0f)
        assertEquals(6f, KeyMetrics.verticalMarginDp(8f), 0f)
    }

    // ---- ⌫ の左スワイプ・フリック感度 ----

    @Test
    fun 左スワイプの判定() {
        assertTrue(SwipeLogic.isLeftSwipe(-50f, 10f, 40f))
        assertFalse(SwipeLogic.isLeftSwipe(-40f, 0f, 40f)) // ちょうどは含めない
        assertFalse(SwipeLogic.isLeftSwipe(50f, 0f, 40f)) // 右
        assertFalse(SwipeLogic.isLeftSwipe(-50f, 60f, 40f)) // 縦の動きの方が大きい
        assertFalse(SwipeLogic.isLeftSwipe(Float.NaN, 0f, 40f))
    }

    @Test
    fun フリック感度は判定距離を割る() {
        assertEquals(14f, FlickLogic.effectiveThresholdDp(14f, 1.0f), 1e-4f)
        assertEquals(7f, FlickLogic.effectiveThresholdDp(14f, 2.0f), 1e-4f)
        assertEquals(28f, FlickLogic.effectiveThresholdDp(14f, 0.5f), 1e-4f)
        // 範囲外は丸める
        assertEquals(7f, FlickLogic.effectiveThresholdDp(14f, 4.0f), 1e-4f)
        assertEquals(28f, FlickLogic.effectiveThresholdDp(14f, 0.1f), 1e-4f)
        // 不正な倍率は 1.0 とみなす
        assertEquals(14f, FlickLogic.effectiveThresholdDp(14f, 0f), 1e-4f)
        assertEquals(14f, FlickLogic.effectiveThresholdDp(14f, -1f), 1e-4f)
        assertEquals(14f, FlickLogic.effectiveThresholdDp(14f, Float.NaN), 1e-4f)
    }

    @Test
    fun 感度を上げると短い移動でフリックになる() {
        val w = 100f
        val dx = 10f * 3f // 10dp 相当（density 3 とする）
        val normal = FlickLogic.judge(dx, 0f, FlickLogic.effectiveThresholdDp(14f, 1.0f) * 3f, w, w)
        val sensitive = FlickLogic.judge(dx, 0f, FlickLogic.effectiveThresholdDp(14f, 2.0f) * 3f, w, w)
        assertEquals(FlickDirection.TAP, normal.direction)
        assertEquals(FlickDirection.RIGHT, sensitive.direction)
    }
}
