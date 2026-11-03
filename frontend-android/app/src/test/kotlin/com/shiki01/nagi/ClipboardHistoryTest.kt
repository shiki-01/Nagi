package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

/** iOS の `ClipboardHistoryTests` と同じ観点（種類の判定・刈り込み・記録・表示用の整形）+ 保存形式 */
class ClipboardHistoryTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val hour = 3_600_000L

    private fun config(
        enabled: Boolean = true,
        retentionMs: Long? = 24 * hour,
        maxItems: Int = 50,
        kinds: Set<ClipboardKind> = ClipboardKind.values().toSet(),
        previewChars: Int = 40,
    ) = ClipboardConfig(enabled, retentionMs, maxItems, kinds, previewChars)

    private var seq = 0
    private fun newId(): String = "id${seq++}"

    private fun entry(id: String, text: String, at: Long, pinned: Boolean = false) =
        ClipboardEntry(id, text, ClipboardKind.classify(text), at, pinned)

    private fun record(items: List<ClipboardEntry>, text: String, now: Long, c: ClipboardConfig = config()) =
        ClipboardHistoryLogic.recorded(items, text, now, c) { newId() }

    // ---- 種類 ----

    @Test
    fun 種類の分類() {
        assertEquals(ClipboardKind.URL, ClipboardKind.classify("https://example.com/a?b=1"))
        assertEquals(ClipboardKind.URL, ClipboardKind.classify("  HTTP://Example.com \n"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.classify("https://"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.classify("https://example.com は URL"))
        assertEquals(ClipboardKind.EMAIL, ClipboardKind.classify("taro.yamada+x@example.co.jp"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.classify("taro@example"))
        assertEquals(ClipboardKind.NUMBER, ClipboardKind.classify("090-1234-5678"))
        assertEquals(ClipboardKind.NUMBER, ClipboardKind.classify("¥1,234.50"))
        assertEquals(ClipboardKind.NUMBER, ClipboardKind.classify("１２３４"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.classify("---"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.classify("abc123"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.classify("こんにちは"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.classify("   "))
    }

    @Test
    fun 知らない種類はtextとして読む() {
        assertEquals(ClipboardKind.TEXT, ClipboardKind.fromJson("future-kind"))
        assertEquals(ClipboardKind.TEXT, ClipboardKind.fromJson(null))
        assertEquals(ClipboardKind.EMAIL, ClipboardKind.fromJson("email"))
    }

    // ---- 設定値 ----

    @Test
    fun 保存期間の変換() {
        assertEquals(hour, ClipboardHistoryLogic.retentionMs("1h"))
        assertEquals(24 * hour, ClipboardHistoryLogic.retentionMs("24h"))
        assertEquals(7 * 24 * hour, ClipboardHistoryLogic.retentionMs("7d"))
        assertEquals(30 * 24 * hour, ClipboardHistoryLogic.retentionMs("30d"))
        assertNull(ClipboardHistoryLogic.retentionMs("forever"))
        assertEquals(24 * hour, ClipboardHistoryLogic.retentionMs("???"))
        assertEquals(24 * hour, ClipboardHistoryLogic.retentionMs(null))
    }

    @Test
    fun 最大数と表示文字数は範囲に丸める() {
        assertEquals(50, ClipboardHistoryLogic.maxItems(null))
        assertEquals(50, ClipboardHistoryLogic.maxItems("x"))
        assertEquals(10, ClipboardHistoryLogic.maxItems("1"))
        assertEquals(200, ClipboardHistoryLogic.maxItems("9999"))
        assertEquals(100, ClipboardHistoryLogic.maxItems("100"))
        assertEquals(40, ClipboardHistoryLogic.previewChars(null))
        assertEquals(120, ClipboardHistoryLogic.previewChars("120"))
        assertEquals(1, ClipboardHistoryLogic.previewChars("0"))
    }

    @Test
    fun 保存する種類の保存形式() {
        assertEquals(ClipboardKind.values().toSet(), ClipboardHistoryLogic.parseKinds(null))
        assertEquals(setOf(ClipboardKind.URL, ClipboardKind.TEXT), ClipboardHistoryLogic.parseKinds("url, foo,text"))
        assertEquals(emptySet<ClipboardKind>(), ClipboardHistoryLogic.parseKinds(""))
        assertEquals("text,url", ClipboardHistoryLogic.encodeKinds(setOf(ClipboardKind.URL, ClipboardKind.TEXT)))
        assertEquals("すべて", ClipboardHistoryLogic.kindsSummary(ClipboardKind.values().toSet()))
        assertEquals("なし", ClipboardHistoryLogic.kindsSummary(emptySet()))
        assertEquals("文字列、URL", ClipboardHistoryLogic.kindsSummary(setOf(ClipboardKind.URL, ClipboardKind.TEXT)))
    }

    // ---- 取り込みの判定 ----

    @Test
    fun オフ_空白のみ_長すぎる_許可されない種類は記録しない() {
        assertNull(ClipboardHistoryLogic.acceptedKind("abc", config(enabled = false)))
        assertNull(ClipboardHistoryLogic.acceptedKind("  \n", config()))
        assertNull(ClipboardHistoryLogic.acceptedKind("a".repeat(ClipboardHistoryLogic.MAX_TEXT_LENGTH + 1), config()))
        assertEquals(ClipboardKind.TEXT, ClipboardHistoryLogic.acceptedKind("a".repeat(ClipboardHistoryLogic.MAX_TEXT_LENGTH), config()))
        val noUrl = config(kinds = setOf(ClipboardKind.TEXT))
        assertNull(ClipboardHistoryLogic.acceptedKind("https://example.com", noUrl))
        assertEquals(ClipboardKind.TEXT, ClipboardHistoryLogic.acceptedKind("hello", noUrl))
        assertEquals(emptyList<ClipboardEntry>(), record(emptyList(), "https://example.com", 0L, noUrl))
    }

    @Test
    fun 新しいものが先頭に入り種類が付く() {
        val a = record(emptyList(), "hello", 1000L)
        val b = record(a, "https://example.com", 2000L)
        assertEquals(listOf("https://example.com", "hello"), b.map { it.text })
        assertEquals(ClipboardKind.URL, b[0].kind)
        assertEquals(2000L, b[0].copiedAt)
    }

    @Test
    fun 同じ内容は重複させず先頭へ移し日時を更新しidとピン留めを保つ() {
        val first = record(emptyList(), "hello", 1000L)
        val pinned = first.map { it.copy(pinned = true) }
        val second = record(pinned, "world", 2000L)
        val third = record(second, "hello", 3000L)
        assertEquals(listOf("hello", "world"), third.map { it.text })
        assertEquals(first[0].id, third[0].id)
        assertTrue(third[0].pinned)
        assertEquals(3000L, third[0].copiedAt)
        // 直前と同じ内容の連続も 1 件のまま
        val again = record(third, "hello", 4000L)
        assertEquals(2, again.size)
    }

    // ---- 刈り込み ----

    @Test
    fun 期限切れは消えるがピン留めは残る() {
        val now = 100 * hour
        val items = listOf(
            entry("a", "new", now - hour),
            entry("b", "old", now - 25 * hour),
            entry("c", "old-pinned", now - 99 * hour, pinned = true),
        )
        assertEquals(listOf("a", "c"), ClipboardHistoryLogic.prune(items, now, config()).map { it.id })
        // 無期限なら消えない
        assertEquals(3, ClipboardHistoryLogic.prune(items, now, config(retentionMs = null)).size)
        // ちょうど期限は残る
        val edge = listOf(entry("e", "edge", now - 24 * hour))
        assertEquals(1, ClipboardHistoryLogic.prune(edge, now, config()).size)
    }

    @Test
    fun 件数の上限を超えた分は古い順に消えピン留めは数に含めない() {
        val now = 10 * hour
        val items = listOf(
            entry("p1", "pin1", 1L, pinned = true),
            entry("n1", "n1", now - 1),
            entry("n2", "n2", now - 2),
            entry("n3", "n3", now - 3),
            entry("n4", "n4", now - 4),
        )
        val pruned = ClipboardHistoryLogic.prune(items, now, config(maxItems = 2, retentionMs = null))
        assertEquals(listOf("p1", "n1", "n2"), pruned.map { it.id })
    }

    @Test
    fun 記録すると上限で古いものが消える() {
        var items = emptyList<ClipboardEntry>()
        val c = config(maxItems = 3)
        for (i in 1..5) items = record(items, "text$i", i * 1000L, c)
        assertEquals(listOf("text5", "text4", "text3"), items.map { it.text })
    }

    // ---- ピン留め・並び ----

    @Test
    fun 表示順はピン留めが先頭で続けて新しい順() {
        val items = listOf(
            entry("a", "a", 1000L),
            entry("b", "b", 3000L),
            entry("c", "c", 2000L, pinned = true),
            entry("d", "d", 500L, pinned = true),
        )
        assertEquals(listOf("c", "d", "b", "a"), ClipboardHistoryLogic.ordered(items).map { it.id })
    }

    @Test
    fun ピン留めの切り替えと上限() {
        val items = listOf(entry("a", "a", 1L), entry("b", "b", 2L, pinned = true))
        val toggled = ClipboardHistoryLogic.togglePin(items, "a")
        assertTrue(toggled.first { it.id == "a" }.pinned)
        assertFalse(ClipboardHistoryLogic.togglePin(toggled, "b").first { it.id == "b" }.pinned)
        assertEquals(items, ClipboardHistoryLogic.togglePin(items, "none"))
        // 上限に達していたら新しくピン留めしない（外すことはできる）
        val full = (0 until ClipboardHistoryLogic.MAX_PINNED).map { entry("p$it", "p$it", it.toLong(), pinned = true) } +
            entry("x", "x", 99L)
        assertEquals(full, ClipboardHistoryLogic.togglePin(full, "x"))
        assertFalse(ClipboardHistoryLogic.togglePin(full, "p0").first { it.id == "p0" }.pinned)
    }

    // ---- 表示 ----

    @Test
    fun 文字数での切り詰め() {
        assertEquals("abc", ClipboardHistoryLogic.preview("abc", 3))
        assertEquals("abc…", ClipboardHistoryLogic.preview("abcd", 3))
        assertEquals("あい…", ClipboardHistoryLogic.preview("あいう", 2))
        assertEquals("a…", ClipboardHistoryLogic.preview("ab", 0))
        // サロゲートペアを途中で切らない
        assertEquals("😀😀…", ClipboardHistoryLogic.preview("😀😀😀", 2))
        assertEquals("😀😀", ClipboardHistoryLogic.preview("😀😀", 2))
    }

    @Test
    fun 相対時刻() {
        val now = 100 * 24 * hour
        assertEquals("たった今", ClipboardHistoryLogic.relativeTime(now - 59_000L, now))
        assertEquals("たった今", ClipboardHistoryLogic.relativeTime(now + 5_000L, now))
        assertEquals("3 分前", ClipboardHistoryLogic.relativeTime(now - 3 * 60_000L, now))
        assertEquals("2 時間前", ClipboardHistoryLogic.relativeTime(now - 2 * hour, now))
        assertEquals("3 日前", ClipboardHistoryLogic.relativeTime(now - 72 * hour, now))
    }

    @Test
    fun 履歴ページの状態() {
        assertEquals(ClipboardHistoryLogic.PageState.DISABLED, ClipboardHistoryLogic.pageState(false, true, 5))
        assertEquals(ClipboardHistoryLogic.PageState.SECURE, ClipboardHistoryLogic.pageState(true, true, 5))
        assertEquals(ClipboardHistoryLogic.PageState.EMPTY, ClipboardHistoryLogic.pageState(true, false, 0))
        assertEquals(ClipboardHistoryLogic.PageState.ITEMS, ClipboardHistoryLogic.pageState(true, false, 1))
        assertNull(ClipboardHistoryLogic.PageState.ITEMS.message)
    }

    // ---- 保存形式 ----

    @Test
    fun エンコードしてデコードすると同じになる() {
        val items = listOf(
            ClipboardEntry("id-1", "改行\nを含む \"引用\" と絵文字😀", ClipboardKind.TEXT, 1_700_000_000_123L, true),
            ClipboardEntry("id-2", "https://example.com", ClipboardKind.URL, 5L),
            ClipboardEntry("id-3", "a@b.co", ClipboardKind.EMAIL, 6L),
            ClipboardEntry("id-4", "0312345678", ClipboardKind.NUMBER, 7L),
        )
        assertEquals(items, ClipboardHistoryCodec.decode(ClipboardHistoryCodec.encode(items)))
    }

    @Test
    fun スキーマ版が入る() {
        val json = org.json.JSONObject(ClipboardHistoryCodec.encode(emptyList()))
        assertEquals(ClipboardHistoryCodec.SCHEMA, json.getInt("schema"))
        assertEquals(0, json.getJSONArray("items").length())
    }

    @Test
    fun 壊れた内容や読めない項目は空か飛ばす() {
        assertEquals(emptyList<ClipboardEntry>(), ClipboardHistoryCodec.decode(null))
        assertEquals(emptyList<ClipboardEntry>(), ClipboardHistoryCodec.decode(""))
        assertEquals(emptyList<ClipboardEntry>(), ClipboardHistoryCodec.decode("not json"))
        assertEquals(emptyList<ClipboardEntry>(), ClipboardHistoryCodec.decode("{\"schema\":1}"))
        val json = """{"schema":9,"items":[{"text":"no id"},{"id":"x","text":"ok","kind":"zzz","copiedAt":3},"junk",{"id":"y"}]}"""
        val out = ClipboardHistoryCodec.decode(json)
        assertEquals(1, out.size)
        assertEquals(ClipboardEntry("x", "ok", ClipboardKind.TEXT, 3L, false), out[0])
    }

    // ---- ファイル保存 ----

    private fun store(): ClipboardHistoryStore = ClipboardHistoryStore(File(tmp.root, "sub/${ClipboardHistoryStore.FILE_NAME}"))

    @Test
    fun ストアは記録_ピン留め_削除_全削除ができ再読み込みしても残る() {
        val s = store()
        val c = config()
        assertTrue(s.record("hello", 1000L, c))
        assertTrue(s.record("https://example.com", 2000L, c))
        assertFalse(s.record("   ", 3000L, c))
        assertEquals(listOf("https://example.com", "hello"), s.items(3000L).map { it.text })

        val hello = s.items(3000L).first { it.text == "hello" }
        assertTrue(s.togglePin(hello.id))
        // 別のインスタンスで読んでも同じ（ファイルに保存されている）
        assertEquals(listOf("hello", "https://example.com"), store().items(3000L).map { it.text })

        s.delete(hello.id)
        assertEquals(listOf("https://example.com"), s.items(3000L).map { it.text })
        s.clear()
        assertEquals(emptyList<ClipboardEntry>(), s.items(3000L))
    }

    @Test
    fun ストアのitemsは設定を渡すと期限切れを除くがファイルは変えない() {
        val s = store()
        val c = config(retentionMs = hour)
        s.record("old", 0L, c)
        assertEquals(emptyList<ClipboardEntry>(), s.items(10 * hour, c))
        assertEquals(1, s.items(10 * hour).size)
        s.prune(10 * hour, c)
        assertEquals(0, s.items(10 * hour).size)
    }

    @Test
    fun ストアは壊れたファイルを空として扱い一時ファイルを残さない() {
        val f = File(tmp.root, "broken.json")
        f.writeText("{{{")
        val s = ClipboardHistoryStore(f)
        assertEquals(emptyList<ClipboardEntry>(), s.items(0L))
        assertTrue(s.record("recover", 1L, config()))
        assertEquals(listOf("recover"), s.items(1L).map { it.text })
        assertFalse(File(f.path + ".tmp").exists())
    }
}
