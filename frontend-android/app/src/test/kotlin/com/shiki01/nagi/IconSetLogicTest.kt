package com.shiki01.nagi

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class IconSetLogicTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val full = """
        {"prefix":"mdi","total":7000,"info":{"name":"Material Design Icons","total":7000,
         "author":{"name":"Pictogrammers","url":"https://github.com/Templarian/MaterialDesign"},
         "license":{"title":"Apache 2.0","spdx":"Apache-2.0","url":"https://github.com/Templarian/MaterialDesign/blob/master/LICENSE"},
         "palette":false}}
    """.trimIndent()

    private fun info(prefix: String, name: String = prefix) =
        IconSetInfo(prefix, name, "MIT", "MIT", null, null, null)

    @Test
    fun infoの全項目を読む() {
        val i = IconSetLogic.parseInfo("mdi", full)!!
        assertEquals("mdi", i.prefix)
        assertEquals("Material Design Icons", i.name)
        assertEquals("Apache 2.0", i.licenseTitle)
        assertEquals("Apache-2.0", i.licenseSpdx)
        assertEquals("https://github.com/Templarian/MaterialDesign/blob/master/LICENSE", i.licenseUrl)
        assertEquals("Pictogrammers", i.authorName)
        assertEquals("https://github.com/Templarian/MaterialDesign", i.authorUrl)
    }

    @Test
    fun infoがトップレベルにある形も読む() {
        val i = IconSetLogic.parseInfo("x", """{"name":"X Set","license":{"title":"MIT"}}""")!!
        assertEquals("X Set", i.name)
        assertEquals("MIT", i.licenseTitle)
    }

    @Test
    fun 欠けた項目は既定になる() {
        val i = IconSetLogic.parseInfo("foo", """{"info":{"license":{"spdx":"CC-BY-4.0"}}}""")!!
        assertEquals("foo", i.name)
        assertEquals("CC-BY-4.0", i.licenseTitle)
        assertNull(i.authorName)
        assertNull(i.licenseUrl)
        assertEquals("作者: （情報なし）\nライセンス: CC-BY-4.0", i.detail)
        assertEquals("foo", i.heading)
        assertNull(i.openUrl)
    }

    @Test
    fun ライセンス名が読めなければnull() {
        assertNull(IconSetLogic.parseInfo("foo", """{"info":{"name":"A"}}"""))
        assertNull(IconSetLogic.parseInfo("foo", """{"info":{"license":{"title":"  "}}}"""))
        assertNull(IconSetLogic.parseInfo("foo", "not json"))
        assertNull(IconSetLogic.parseInfo("Bad/Prefix", full))
    }

    @Test
    fun nullや非httpのURLは捨てる() {
        val i = IconSetLogic.parseInfo(
            "foo",
            """{"info":{"license":{"title":"MIT","url":"javascript:alert(1)"},"author":{"name":null,"url":"ftp://x"}}}""",
        )!!
        assertNull(i.licenseUrl)
        assertNull(i.authorName)
        assertNull(i.authorUrl)
    }

    @Test
    fun 開くURLはライセンス優先で作者にフォールバックする() {
        val i = IconSetLogic.parseInfo("mdi", full)!!
        assertEquals(i.licenseUrl, i.openUrl)
        assertEquals("https://a.example", i.copy(licenseUrl = null, authorUrl = "https://a.example").openUrl)
    }

    @Test
    fun 保存形式は往復でき_prefix順に並ぶ() {
        val list = listOf(info("zz"), IconSetLogic.parseInfo("mdi", full)!!, info("ab"))
        val back = IconSetLogic.decode(IconSetLogic.encode(list))
        assertEquals(listOf("ab", "mdi", "zz"), back.map { it.prefix })
        assertEquals(IconSetLogic.parseInfo("mdi", full), back[1])
    }

    @Test
    fun 同じprefixは上書きする() {
        val list = IconSetLogic.upsert(listOf(info("a", "Old")), info("a", "New"))
        assertEquals(1, list.size)
        assertEquals("New", list[0].name)
    }

    @Test
    fun 壊れた保存内容や版違いは空_不正な項目は捨てる() {
        assertTrue(IconSetLogic.decode("garbage").isEmpty())
        assertTrue(IconSetLogic.decode("""{"v":2,"sets":[]}""").isEmpty())
        val text = """{"v":1,"sets":[
            {"prefix":"ok","licenseTitle":"MIT"},
            {"prefix":"BAD","licenseTitle":"MIT"},
            {"prefix":"nolicense"},
            "str"]}"""
        val list = IconSetLogic.decode(text)
        assertEquals(1, list.size)
        assertEquals("ok", list[0].prefix)
        assertEquals("ok", list[0].name)
    }

    @Test
    fun アイコンのファイル名からprefixを取り出す() {
        assertEquals("mdi", IconSetLogic.prefixOfIconFile("mdi__home.json"))
        assertEquals("material-symbols", IconSetLogic.prefixOfIconFile("material-symbols__a-b.json"))
        assertNull(IconSetLogic.prefixOfIconFile("mdi__home.json.tmp"))
        assertNull(IconSetLogic.prefixOfIconFile("home.json"))
        assertNull(IconSetLogic.prefixOfIconFile("__x.json"))
    }

    @Test
    fun 全文は空なら案内_あれば作者とライセンスを含む() {
        assertEquals("取得済みのアイコンはありません", IconSetLogic.plainText(emptyList()))
        val t = IconSetLogic.plainText(listOf(IconSetLogic.parseInfo("mdi", full)!!))
        assertTrue(t.contains("Material Design Icons（mdi）"))
        assertTrue(t.contains("作者: Pictogrammers"))
        assertTrue(t.contains("ライセンス: Apache 2.0"))
    }

    @Test
    fun collectionのURL() {
        assertEquals("https://api.iconify.design/collection?prefix=mdi&info=true", IconSetLogic.collectionUrl("mdi"))
    }

    // ---- 保存先 ----

    @Test
    fun リポジトリは原子的に保存し_上書きし_読み出せる() {
        val file = File(tmp.root, "layouts/icon-sets.json")
        val repo = IconSetRepository(file)
        assertTrue(repo.load().isEmpty())
        assertFalse(repo.has("mdi"))
        assertTrue(repo.save(IconSetLogic.parseInfo("mdi", full)!!))
        assertTrue(repo.save(info("ab")))
        assertTrue(repo.save(info("mdi", "Renamed")))
        assertTrue(repo.has("mdi"))
        val list = IconSetRepository(file).load()
        assertEquals(listOf("ab", "mdi"), list.map { it.prefix })
        assertEquals("Renamed", list[1].name)
        assertFalse(File(file.path + ".tmp").exists())
        assertNotNull(file.readText())
    }

    @Test
    fun 壊れたファイルは空として扱い_保存で復旧する() {
        val file = File(tmp.root, "icon-sets.json")
        file.writeText("{broken")
        val repo = IconSetRepository(file)
        assertTrue(repo.load().isEmpty())
        assertTrue(repo.save(info("a")))
        assertEquals(1, repo.load().size)
    }
}
