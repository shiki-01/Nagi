package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Test

class DataUsageTest {
    @Test
    fun formatBytesUnits() {
        assertEquals("0 B", formatBytes(0))
        assertEquals("0 B", formatBytes(-5))
        assertEquals("1023 B", formatBytes(1023))
        assertEquals("1.0 KB", formatBytes(1024))
        assertEquals("1.5 KB", formatBytes(1536))
        assertEquals("1.0 MB", formatBytes(1024L * 1024))
        assertEquals("70.4 MB", formatBytes(73_871_968L))
        assertEquals("2.0 GB", formatBytes(2L * 1024 * 1024 * 1024))
    }

    @Test
    fun fileTreeSizeSumsFiles() {
        val dir = java.nio.file.Files.createTempDirectory("nagi-usage").toFile()
        try {
            java.io.File(dir, "a").writeBytes(ByteArray(10))
            val sub = java.io.File(dir, "sub")
            sub.mkdirs()
            java.io.File(sub, "b").writeBytes(ByteArray(25))
            assertEquals(35L, fileTreeSize(dir))
            assertEquals(0L, fileTreeSize(java.io.File(dir, "none")))
        } finally {
            dir.deleteRecursively()
        }
    }
}
