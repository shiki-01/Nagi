package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MainTabsTest {
    @Test
    fun tabsAreInIosOrder() {
        assertEquals(
            listOf("ホーム", "入力テスト", "設定", "レイアウト", "About"),
            MainTab.ordered.map { it.label },
        )
        assertEquals(MainTab.HOME, MainTab.DEFAULT)
    }

    @Test
    fun idsAreUniqueAndRoundTrip() {
        val ids = MainTab.ordered.map { it.id }
        assertEquals(ids.size, ids.toSet().size)
        for (t in MainTab.ordered) assertEquals(t, MainTab.fromId(t.id))
        assertNull(MainTab.fromId(null))
        assertNull(MainTab.fromId("unknown"))
    }

    @Test
    fun everyTabHasAGlyph() {
        for (t in MainTab.ordered) assertTrue(t.glyph.isNotEmpty())
    }

    @Test
    fun exactlyOneTabIsSelected() {
        for (selected in MainTab.ordered) {
            val states = MainTab.withSelection(selected)
            assertEquals(MainTab.ordered, states.map { it.first })
            assertEquals(listOf(selected), states.filter { it.second }.map { it.first })
        }
    }

    @Test
    fun layoutLinkOpensLayoutsTab() {
        val r = MainRouting.resolve(true, "nagi://layout?data=abc", null)
        assertEquals(MainTab.LAYOUTS, r.tab)
        assertTrue(r.layoutIntent)
    }

    @Test
    fun rejectedLinkAlsoOpensLayoutsTabToShowTheReason() {
        val r = MainRouting.resolve(true, "https://example.com/", null)
        assertEquals(MainTab.LAYOUTS, r.tab)
        assertTrue(r.layoutIntent)
    }

    @Test
    fun nonViewDataIsIgnoredAndExtraTabIsUsed() {
        val r = MainRouting.resolve(false, "nagi://layout?data=abc", MainTab.INPUT_TEST.id)
        assertEquals(MainTab.INPUT_TEST, r.tab)
        assertFalse(r.layoutIntent)
    }

    @Test
    fun plainLaunchKeepsTheCurrentTab() {
        val r = MainRouting.resolve(false, null, null)
        assertNull(r.tab)
        assertFalse(r.layoutIntent)
        assertNull(MainRouting.resolve(false, null, "bogus").tab)
    }

    @Test
    fun viewWithoutDataIsNotALayoutIntent() {
        val r = MainRouting.resolve(true, "  ", MainTab.HOME.id)
        assertEquals(MainTab.HOME, r.tab)
        assertFalse(r.layoutIntent)
    }
}
