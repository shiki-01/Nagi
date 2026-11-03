package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class UpdateNoticeTest {
    @Test
    fun noticeAppearsOnlyWhenVersionChanged() {
        assertEquals("0.1.1 から 0.1.2 に更新しました", UpdateNotice.text("0.1.1", "0.1.2"))
    }

    @Test
    fun firstLaunchHasNoNotice() {
        assertNull(UpdateNotice.text("", "0.1.2"))
    }

    @Test
    fun sameVersionHasNoNotice() {
        assertNull(UpdateNotice.text("0.1.2", "0.1.2"))
        assertNull(UpdateNotice.text(" 0.1.2 ", "0.1.2"))
    }

    @Test
    fun unknownCurrentVersionHasNoNotice() {
        assertNull(UpdateNotice.text("0.1.1", ""))
    }
}
