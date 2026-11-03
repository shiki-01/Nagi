package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyboardThemeTest {
    @Test
    fun アクセントカラーの名前と既定() {
        assertEquals(AccentColor.PINK, AccentColor.fromJson("pink"))
        assertEquals(AccentColor.GRAY, AccentColor.fromJson("gray"))
        assertEquals(AccentColor.BLUE, AccentColor.fromJson("unknown"))
        assertEquals(AccentColor.BLUE, AccentColor.fromJson(null))
        assertEquals(KeyboardDefaults.ACCENT, AccentColor.BLUE.json)
        assertEquals(setOf("blue", "green", "orange", "pink", "gray"), AccentColor.values().map { it.json }.toSet())
    }

    @Test
    fun 青は従来の色のまま() {
        assertEquals(0xFF3B82F6.toInt(), AccentColor.BLUE.argb(false))
        assertEquals(0xFF3B82F6.toInt(), AccentColor.BLUE.argb(true))
    }

    @Test
    fun アクセントはどれも不透明で_ライトとダークで色がある() {
        for (c in AccentColor.values()) {
            assertEquals(c.json, 0xFF, (c.argb(false) ushr 24) and 0xFF)
            assertEquals(c.json, 0xFF, (c.argb(true) ushr 24) and 0xFF)
        }
    }

    @Test
    fun 不透明度だけ変える() {
        assertEquals(0x403B82F6, AccentColor.withAlpha(0xFF3B82F6.toInt(), 0x40))
        assertEquals(0x00112233, AccentColor.withAlpha(0xFF112233.toInt(), 0))
        assertEquals(0xFF112233.toInt(), AccentColor.withAlpha(0x00112233, 300))
        assertEquals(0x00112233, AccentColor.withAlpha(0x00112233, -5))
    }

    @Test
    fun 触覚の強さ() {
        assertEquals(HapticLevel.HEAVY, HapticLevel.fromJson("heavy"))
        assertEquals(HapticLevel.OFF, HapticLevel.fromJson("off"))
        assertNull(HapticLevel.fromJson("strong"))
        assertNull(HapticLevel.fromJson(null))
        assertEquals(KeyboardDefaults.HAPTIC_LEVEL, HapticLevel.LIGHT.json)
        assertEquals(setOf("off", "light", "medium", "heavy"), HapticLevel.values().map { it.json }.toSet())
    }

    @Test
    fun 既定値は範囲の中() {
        val d = KeyboardDefaults
        assertTrue(d.FLICK_THRESHOLD_DP in d.FLICK_THRESHOLD_MIN..d.FLICK_THRESHOLD_MAX)
        assertTrue(d.FLICK_SENSITIVITY in d.FLICK_SENSITIVITY_MIN..d.FLICK_SENSITIVITY_MAX)
        assertTrue(d.FLICK_LONG_PRESS_MS in d.FLICK_LONG_PRESS_MIN..d.FLICK_LONG_PRESS_MAX)
        assertTrue(d.SPACE_SWIPE_STEP_DP in d.SPACE_SWIPE_STEP_MIN..d.SPACE_SWIPE_STEP_MAX)
        assertTrue(d.SPACE_SWIPE_ACCEL_STRENGTH in d.SPACE_SWIPE_ACCEL_STRENGTH_MIN..d.SPACE_SWIPE_ACCEL_STRENGTH_MAX)
        assertTrue(d.KEY_REPEAT_DELAY_MS in d.KEY_REPEAT_DELAY_MIN..d.KEY_REPEAT_DELAY_MAX)
        assertTrue(d.KEY_REPEAT_MS in d.KEY_REPEAT_MIN..d.KEY_REPEAT_MAX)
        assertTrue(d.KEY_SPACING_DP in d.KEY_SPACING_MIN..d.KEY_SPACING_MAX)
        assertTrue(d.KEY_CORNER_DP in d.KEY_CORNER_MIN..d.KEY_CORNER_MAX)
        assertTrue(d.HEIGHT_SCALE in d.HEIGHT_SCALE_MIN..d.HEIGHT_SCALE_MAX)
        assertTrue(d.HEIGHT_SCALE_LANDSCAPE in d.HEIGHT_SCALE_LANDSCAPE_MIN..d.HEIGHT_SCALE_LANDSCAPE_MAX)
        assertTrue(d.ONE_HANDED_WIDTH in OneHandedLogic.WIDTH_MIN..OneHandedLogic.WIDTH_MAX)
        assertEquals(OneHandedLogic.WIDTH_DEFAULT, d.ONE_HANDED_WIDTH, 0f)
        assertEquals(OneHanded.OFF, OneHanded.fromJson(d.ONE_HANDED))
        assertEquals(HeightPreset.STANDARD, HeightPreset.matching(d.HEIGHT_SCALE))
    }

    @Test
    fun 既定に戻す対象に重複がなく_触覚は旧キーも消す() {
        assertEquals(ResetKeys.ALL.size, ResetKeys.ALL.toSet().size)
        assertTrue(ResetKeys.ALL.containsAll(ResetKeys.HAPTIC))
        assertTrue(PrefKeys.HAPTIC in ResetKeys.HAPTIC && PrefKeys.HAPTIC_LEVEL in ResetKeys.HAPTIC)
        // この画面の項目がすべて入っている
        val groups = listOf(
            ResetKeys.TOGGLE_INPUT, ResetKeys.FLICK_THRESHOLD, ResetKeys.FLICK_SENSITIVITY, ResetKeys.FLICK_POPUP,
            ResetKeys.SPACE_SWIPE, ResetKeys.SPACE_SWIPE_STEP, ResetKeys.SPACE_SWIPE_ACCEL,
            ResetKeys.SPACE_SWIPE_ACCEL_STRENGTH, ResetKeys.BACKSPACE_SWIPE, ResetKeys.KEY_REPEAT_DELAY,
            ResetKeys.KEY_REPEAT, ResetKeys.THEME, ResetKeys.ACCENT, ResetKeys.KEY_SPACING, ResetKeys.KEY_CORNER,
            ResetKeys.HAPTIC, ResetKeys.KEY_SOUND, ResetKeys.ONE_HANDED, ResetKeys.ONE_HANDED_WIDTH,
            ResetKeys.HEIGHT_SCALE, ResetKeys.HEIGHT_SCALE_LANDSCAPE, ResetKeys.AUTO_CHECK_UPDATES,
            ResetKeys.FLICK_HINTS, ResetKeys.FLICK_LONG_PRESS, ResetKeys.COMPOSE_NUMBERS, ResetKeys.KEEP_PREEDIT_ON_SYMBOLS,
            ResetKeys.KEEP_PREEDIT_ON_ENGLISH, ResetKeys.LAYOUT_BY_FIELD,
        )
        for (g in groups) assertTrue(g.toString(), ResetKeys.ALL.containsAll(g))
    }
}
