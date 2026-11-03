package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelRecommendationTest {
    private val small = ModelSpec("zenz-v3.1-small-Q5_K_M.gguf", 73_871_968L) // 約 70.4 MiB
    private val xsmall = ModelSpec("zenz-v3.1-xsmall-Q5_K_M.gguf", 20_970_304L) // 約 20.0 MiB

    @Test
    fun fitsUsesOverheadAndMargin() {
        // small: 70.45 + 20 + 40 = 130.45MB 必要
        assertFalse(ModelRecommendation.fits(small, 130))
        assertTrue(ModelRecommendation.fits(small, 131))
        // xsmall: 20 + 20 + 40 = 80MB 必要
        assertFalse(ModelRecommendation.fits(xsmall, 79))
        assertTrue(ModelRecommendation.fits(xsmall, 80))
    }

    @Test
    fun recommendsLargestThatFits() {
        assertEquals(small, ModelRecommendation.recommend(1000, listOf(xsmall, small)))
        assertEquals(xsmall, ModelRecommendation.recommend(100, listOf(small, xsmall)))
        assertNull(ModelRecommendation.recommend(60, listOf(small, xsmall)))
        assertNull(ModelRecommendation.recommend(1000, emptyList()))
    }

    @Test
    fun sameSizeTieBreaksByName() {
        val a = ModelSpec("a.gguf", 1_000_000L)
        val b = ModelSpec("b.gguf", 1_000_000L)
        assertEquals(a, ModelRecommendation.recommend(1000, listOf(b, a)))
    }

    @Test
    fun budgetFromAvailMemory() {
        assertEquals(500L, ModelRecommendation.budgetMbFromAvail(1000L * 1024 * 1024))
        assertEquals(64L, ModelRecommendation.budgetMbFromAvail(10L * 1024 * 1024))
        assertEquals(4096L, ModelRecommendation.budgetMbFromAvail(100L * 1024 * 1024 * 1024))
    }

    @Test
    fun labelAndSummary() {
        assertEquals("xsmall", ModelRecommendation.label(xsmall.name))
        assertEquals("small", ModelRecommendation.label(small.name))
        assertEquals("custom.gguf", ModelRecommendation.label("custom.gguf"))
        assertTrue(ModelRecommendation.summary(1000, listOf(small, xsmall)).contains("small"))
        assertTrue(ModelRecommendation.summary(10, listOf(small)).startsWith("推奨なし"))
        assertEquals("", ModelRecommendation.summary(1000, emptyList()))
    }

    @Test
    fun dictionaryOnlyReasons() {
        assertNotNull(ModelRecommendation.dictionaryOnlyReason(false, true, 1000))
        assertNotNull(ModelRecommendation.dictionaryOnlyReason(true, false, 1000))
        assertNotNull(ModelRecommendation.dictionaryOnlyReason(true, true, 149))
        assertNull(ModelRecommendation.dictionaryOnlyReason(true, true, 150))
    }

    @Test
    fun catalogMatchesCoreConstants() {
        val ids = ModelRecommendation.CATALOG.map { it.id }
        assertEquals(listOf("small", "xsmall"), ids)
        for (m in ModelRecommendation.CATALOG) {
            assertEquals(64, m.sha256.length)
            assertTrue(m.url.startsWith("https://huggingface.co/"))
            assertTrue(m.fileName.endsWith(".gguf"))
        }
        assertEquals(ModelRecommendation.DEFAULT_MODEL_FILE, ModelRecommendation.CATALOG[0].fileName)
    }
}
