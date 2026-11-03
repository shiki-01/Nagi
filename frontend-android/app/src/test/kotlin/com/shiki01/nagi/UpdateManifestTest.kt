package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateManifestTest {
    @Test
    fun parsesCoreCompatibleManifest() {
        val m = ManifestParser.parse(
            """{"version":"0.1.9","notes_url":"https://example.com/n","download_url":"https://example.com/a.apk",
               "sha256":"ab","signature":"cd"}""",
        )
        assertNotNull(m)
        assertEquals("0.1.9", m!!.version)
        assertEquals("https://example.com/n", m.notesUrl)
        assertEquals("https://example.com/a.apk", m.downloadUrl)
        assertEquals("ab", m.sha256)
        assertEquals("cd", m.signature)
        assertTrue(m.dataAssets.isEmpty())
    }

    @Test
    fun versionOnlyIsEnough_butMissingOrBrokenIsNull() {
        val m = ManifestParser.parse("""{"version":"1.0.0"}""")
        assertNotNull(m)
        assertNull(m!!.downloadUrl)
        assertNull(ManifestParser.parse("""{"download_url":"x"}"""))
        assertNull(ManifestParser.parse("""{"version":""}"""))
        assertNull(ManifestParser.parse("not json"))
    }

    @Test
    fun parsesDataAssetsAndSkipsBrokenOnes() {
        val m = ManifestParser.parse(
            """{"version":"0.2.0","data_assets":[
              {"name":"dict","version":"2026.11.1","url":"https://e.com/d","sha256":"a","signature":"b",
               "min_app":"0.1.0","max_app":null,"dest":"dict/nagi-dict.bin"},
              {"name":"evil","version":"1.0.0","url":"https://e.com/e","sha256":"a","signature":"b","dest":"../x"},
              {"name":"nover","url":"https://e.com/e","sha256":"a","signature":"b","dest":"x"},
              "garbage"
            ]}""",
        )!!
        assertEquals(1, m.dataAssets.size)
        val a = m.dataAssets[0]
        assertEquals("dict", a.name)
        assertEquals("2026.11.1", a.version)
        assertEquals("0.1.0", a.minApp)
        assertNull(a.maxApp)
        assertEquals("dict/nagi-dict.bin", a.dest)
    }

    private fun asset(min: String?, max: String?) =
        DataAsset("d", "1.0.0", "u", "s", "g", min, max, "d/x")

    @Test
    fun compatibilityRange() {
        assertTrue(asset(null, null).isCompatible("0.0.1"))
        assertTrue(asset("0.1.0", null).isCompatible("0.1.0"))
        assertFalse(asset("0.2.0", null).isCompatible("0.1.9"))
        assertTrue(asset(null, "0.3.0").isCompatible("0.3.0"))
        assertFalse(asset(null, "0.3.0").isCompatible("0.3.1"))
        assertFalse(asset("0.1.0", "0.9.0").isCompatible("0.10.0"))
    }

    @Test
    fun safeDestRejectsTraversal() {
        assertTrue(ManifestParser.isSafeDest("dict/nagi-dict.bin"))
        assertTrue(ManifestParser.isSafeDest("models/zenz.gguf"))
        assertFalse(ManifestParser.isSafeDest(""))
        assertFalse(ManifestParser.isSafeDest("/etc/passwd"))
        assertFalse(ManifestParser.isSafeDest("../x"))
        assertFalse(ManifestParser.isSafeDest("a/../../x"))
        assertFalse(ManifestParser.isSafeDest("a//b"))
        assertFalse(ManifestParser.isSafeDest("a\\b"))
        assertFalse(ManifestParser.isSafeDest("./a"))
    }
}
