package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ChoiceSheetLogicTest {
    @Test
    fun itemsMarkOnlySelected() {
        val items = ChoiceSheetLogic.items(listOf("a", "b", "c"), null, 1)
        assertEquals(listOf(false, true, false), items.map { it.selected })
        assertEquals(listOf("a", "b", "c"), items.map { it.label })
        assertTrue(items.all { it.note == null })
    }

    @Test
    fun itemsOutOfRangeSelectionSelectsNothing() {
        assertTrue(ChoiceSheetLogic.items(listOf("a", "b"), null, -1).none { it.selected })
        assertTrue(ChoiceSheetLogic.items(listOf("a", "b"), null, 5).none { it.selected })
    }

    @Test
    fun itemsNotesAreOptionalAndBlankIgnored() {
        val items = ChoiceSheetLogic.items(listOf("a", "b", "c"), listOf("説明", "  ", null), 0)
        assertEquals("説明", items[0].note)
        assertNull(items[1].note)
        assertNull(items[2].note)
    }

    @Test
    fun itemsShortNotesListIsSafe() {
        val items = ChoiceSheetLogic.items(listOf("a", "b"), listOf("x"), 0)
        assertEquals("x", items[0].note)
        assertNull(items[1].note)
    }

    @Test
    fun itemsEmpty() {
        assertTrue(ChoiceSheetLogic.items(emptyList(), null, 0).isEmpty())
    }

    @Test
    fun indexOfValueAndDisplayIndex() {
        val values = listOf("x", "y", "z")
        assertEquals(2, ChoiceSheetLogic.indexOfValue(values, "z"))
        assertEquals(-1, ChoiceSheetLogic.indexOfValue(values, "q"))
        assertEquals(-1, ChoiceSheetLogic.indexOfValue(values, null))
        assertEquals(2, ChoiceSheetLogic.displayIndex(values, "z"))
        assertEquals(0, ChoiceSheetLogic.displayIndex(values, "q"))
        assertEquals(0, ChoiceSheetLogic.displayIndex(emptyList(), "q"))
    }

    @Test
    fun changedIgnoresSameAndOutOfRange() {
        assertTrue(ChoiceSheetLogic.changed(1, 0, 3))
        assertFalse(ChoiceSheetLogic.changed(0, 0, 3))
        assertFalse(ChoiceSheetLogic.changed(3, 0, 3))
        assertFalse(ChoiceSheetLogic.changed(-1, 0, 3))
    }

    @Test
    fun heightAndWidthLimits() {
        assertEquals(2000 * 85 / 100 - 300, ChoiceSheetLogic.maxListHeight(2000, 300))
        assertEquals(1, ChoiceSheetLogic.maxListHeight(100, 300))
        assertEquals(1200, ChoiceSheetLogic.sheetWidth(1200, 1500))
        assertEquals(1500, ChoiceSheetLogic.sheetWidth(2400, 1500))
    }

    @Test
    fun scrollTargetNeverNegative() {
        assertEquals(0, ChoiceSheetLogic.scrollTarget(10, 60))
        assertEquals(140, ChoiceSheetLogic.scrollTarget(200, 60))
    }
}
