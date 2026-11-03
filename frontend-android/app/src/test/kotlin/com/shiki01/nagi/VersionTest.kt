package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VersionTest {
    @Test
    fun parseHandlesMissingAndInvalidParts() {
        assertEquals(Triple(1L, 2L, 3L), Version.parse("1.2.3"))
        assertEquals(Triple(1L, 2L, 0L), Version.parse("1.2"))
        assertEquals(Triple(0L, 0L, 0L), Version.parse(""))
        assertEquals(Triple(0L, 1L, 0L), Version.parse("0.1.0-debug"))
        assertEquals(Triple(2026L, 11L, 1L), Version.parse(" 2026.11.1 "))
    }

    @Test
    fun compareIsNumericNotLexical() {
        assertTrue(Version.isNewer("0.10.0", "0.9.9"))
        assertTrue(Version.isNewer("1.0.0", "0.99.99"))
        assertFalse(Version.isNewer("0.1.0", "0.1.0"))
        assertFalse(Version.isNewer("0.1.0", "0.1.1"))
        assertEquals(0, Version.compare("1.2", "1.2.0"))
    }
}
