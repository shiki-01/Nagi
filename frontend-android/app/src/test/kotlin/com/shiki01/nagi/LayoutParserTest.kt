package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutParserTest {
    private fun ok(json: String, known: Set<String>? = null): LayoutModel {
        val r = LayoutFixtures.parse(json, known)
        if (r is LayoutParseResult.Err) throw AssertionError("拒否された: ${r.reason}")
        return (r as LayoutParseResult.Ok).layout
    }

    private fun assertRejected(json: String, contains: String, known: Set<String>? = null) {
        val reason = LayoutFixtures.reason(json, known)
        assertNotNull("拒否されるはず: $json", reason)
        assertTrue("理由「$reason」に「$contains」が含まれるはず", reason!!.contains(contains))
    }

    // ---- 同梱のプリセット ----

    @Test
    fun プリセットが見つかる() {
        assertTrue(LayoutFixtures.presetFiles.size >= 8)
    }

    @Test
    fun 全プリセットが検証を通りファイル名とidが一致する() {
        val known = LayoutFixtures.presetIds
        for (f in LayoutFixtures.presetFiles) {
            val layout = ok(f.readText(Charsets.UTF_8), known)
            assertEquals(f.name.removeSuffix(".json"), layout.id)
        }
    }

    @Test
    fun flickKanaは12キー名をコアのキーIDに対応づける() {
        val file = LayoutFixtures.presetFiles.first { it.name == "flick-kana.json" }
        val layout = ok(file.readText(Charsets.UTF_8), LayoutFixtures.presetIds)
        assertEquals(LayoutMode.KANA, layout.mode)
        val ids = layout.rows.flatMap { it.keys }.mapNotNull { (it.action as? KeyAction.Flick)?.kanaKey }
        assertEquals(listOf("a", "ka", "sa", "ta", "na", "ha", "ma", "ya", "ra", "wa", "punct"), ids)
    }

    @Test
    fun toggleAlphaのトグルは長押しを持つ() {
        val file = LayoutFixtures.presetFiles.first { it.name == "toggle-alpha.json" }
        val layout = ok(file.readText(Charsets.UTF_8), LayoutFixtures.presetIds)
        val toggles = layout.rows.flatMap { it.keys }.filter { it.action is KeyAction.Toggle }
        assertEquals(10, toggles.size)
        assertTrue(toggles.all { it.longPress.isNotEmpty() })
    }

    // ---- テスト用レイアウト（iOS と同じ期待） ----

    @Test
    fun 受理されるテスト用レイアウト() {
        val known = LayoutFixtures.presetIds
        for (name in listOf("valid-toggle-kana.json", "valid-toggle-alpha.json", "valid-shortcut.json")) {
            ok(LayoutFixtures.testLayout(name), known)
        }
    }

    @Test
    fun 拒否されるテスト用レイアウトは期待の理由を含む() {
        val known = LayoutFixtures.presetIds
        val expected = mapOf(
            "invalid-rows-9.json" to "rows は 1〜8 行",
            "invalid-height-sum.json" to "行の高さの比の合計",
            "invalid-id.json" to "id は英小文字",
            "invalid-no-name.json" to "name",
            "invalid-label-17.json" to "label は 16 文字以内",
            "invalid-unknown-kana.json" to "未知のキー名",
            "invalid-missing-target.json" to "存在するレイアウトでも組み込み名でもありません",
            "invalid-steps-both.json" to "どちらか一方だけ",
        )
        for ((file, text) in expected) assertRejected(LayoutFixtures.testLayout(file), text, known)
    }

    // ---- 不正な JSON ----

    @Test
    fun 空とJSONでないものは拒否() {
        assertRejected("", "空")
        assertRejected("   ", "空")
        assertRejected("これはJSONではない", "JSON として読み取れません")
        assertRejected("[1, 2]", "JSON として読み取れません")
    }

    @Test
    fun BOM付きでも読める() {
        ok("﻿" + LayoutFixtures.minimal())
    }

    @Test
    fun schemaは2だけ() {
        assertRejected(LayoutFixtures.minimal().replace("\"schema\": 2", "\"schema\": 1"), "未対応のスキーマ")
        assertRejected(LayoutFixtures.minimal().replace("\"schema\": 2,", ""), "schema")
        assertRejected(LayoutFixtures.minimal().replace("\"schema\": 2", "\"schema\": \"2\""), "schema")
    }

    @Test
    fun 必須項目の欠落() {
        assertRejected(LayoutFixtures.minimal().replace("\"mode\": \"kana\",", ""), "mode")
        assertRejected("""{"schema": 2, "id": "t1", "name": "x", "mode": "kana"}""", "rows")
        assertRejected(LayoutFixtures.minimal().replace("\"mode\": \"kana\"", "\"mode\": \"foo\""), "mode は")
    }

    @Test
    fun 不明なkindは拒否() {
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "slider"}"""), "kind")
        assertRejected(LayoutFixtures.minimal("""{"id": "a"}"""), "kind")
    }

    @Test
    fun 型違いは理由つきで拒否() {
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": 5}"""), "文字列")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "width": "1"}"""), "数")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "toggle", "cycle": "abc"}"""), "配列")
    }

    @Test
    fun 未知のプロパティとlook系は無視する() {
        val layout = ok(
            LayoutFixtures.minimal(
                """{"id": "a", "kind": "text", "text": "a", "unknown": 1, "look": {"fontSize": 99}, "visible": {"field": "x", "in": []}}""",
                extra = """, "look": {"cornerRadius": 999}, "toolbar": {"keys": []}, "assets": {}, "foo": true""",
            ),
        )
        assertEquals(1, layout.keyCount)
    }

    // ---- 上限値 ----

    @Test
    fun レイアウトidの規則() {
        assertRejected(LayoutFixtures.minimal(id = "Upper"), "id は英小文字")
        assertRejected(LayoutFixtures.minimal(id = "-start"), "id は英小文字")
        assertRejected(LayoutFixtures.minimal(id = "a/b"), "id は英小文字")
        assertRejected(LayoutFixtures.minimal(id = "a".repeat(65)), "id は英小文字")
        ok(LayoutFixtures.minimal(id = "a".repeat(64)))
        ok(LayoutFixtures.minimal(id = "0a_b-c"))
    }

    @Test
    fun nameは1から64文字() {
        assertRejected(LayoutFixtures.minimal().replace("\"テスト\"", "\"\""), "name は")
        assertRejected(LayoutFixtures.minimal().replace("\"テスト\"", "\"" + "あ".repeat(65) + "\""), "name は")
        ok(LayoutFixtures.minimal().replace("\"テスト\"", "\"" + "あ".repeat(64) + "\""))
    }

    @Test
    fun 行は1から8行() {
        val row = """{"keys": [{"id": "KEY", "kind": "text", "text": "a"}]}"""
        fun rows(n: Int, h: String = "") = (1..n).joinToString(",") { row.replace("KEY", "k$it").replace("{\"keys\"", "{$h\"keys\"") }
        fun layout(n: Int, h: String = "") =
            """{"schema": 2, "id": "t1", "name": "x", "mode": "kana", "rows": [${rows(n, h)}]}"""
        assertRejected("""{"schema": 2, "id": "t1", "name": "x", "mode": "kana", "rows": []}""", "rows は 1〜8 行")
        assertRejected(layout(9, "\"height\": 0.1,"), "rows は 1〜8 行")
        ok(layout(8, "\"height\": 0.5,"))
    }

    @Test
    fun 行の高さの比() {
        fun layout(h: String) =
            """{"schema": 2, "id": "t1", "name": "x", "mode": "kana", "rows": [{"height": $h, "keys": [{"id": "a", "kind": "text", "text": "a"}]}]}"""
        assertRejected(layout("0"), "高さが範囲外")
        assertRejected(layout("4.1"), "高さが範囲外")
        ok(layout("4"))
        // 合計 4.5 まで
        val two = """{"schema": 2, "id": "t1", "name": "x", "mode": "kana", "rows": [
            {"height": 2.5, "keys": [{"id": "a", "kind": "text", "text": "a"}]},
            {"height": 2.1, "keys": [{"id": "b", "kind": "text", "text": "a"}]}]}"""
        assertRejected(two, "行の高さの比の合計")
        ok(two.replace("2.1", "2"))
    }

    @Test
    fun 行内のキー数は24まで() {
        fun keys(n: Int) = (1..n).joinToString(",") { """{"id": "k$it", "kind": "text", "text": "a", "width": 0.5}""" }
        fun layout(n: Int) = """{"schema": 2, "id": "t1", "name": "x", "mode": "kana", "rows": [{"keys": [${keys(n)}]}]}"""
        assertRejected(layout(25), "24 個以内")
        ok(layout(24))
    }

    @Test
    fun 行の幅の合計は1から20() {
        fun layout(w: String) =
            """{"schema": 2, "id": "t1", "name": "x", "mode": "kana", "rows": [{"keys": [{"id": "a", "kind": "text", "text": "a", "width": $w}]}]}"""
        assertRejected(layout("0.5"), "幅の合計が範囲外")
        ok(layout("20"))
        assertRejected(layout("21"), "幅は")
        assertRejected(layout("0"), "幅は")
        val over = (1..3).joinToString(",") { """{"id": "k$it", "kind": "text", "text": "a", "width": 7}""" }
        assertRejected(LayoutFixtures.minimal(over), "幅の合計が範囲外")
    }

    @Test
    fun キーidの規則と重複() {
        assertRejected(LayoutFixtures.minimal("""{"id": "bad id", "kind": "text", "text": "a"}"""), "id は")
        assertRejected(LayoutFixtures.minimal("""{"id": "${"a".repeat(33)}", "kind": "text", "text": "a"}"""), "id は")
        ok(LayoutFixtures.minimal("""{"id": "${"a".repeat(32)}", "kind": "text", "text": "a"}"""))
        assertRejected(
            LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a"},{"id": "a", "kind": "text", "text": "b"}"""),
            "キー id の重複",
        )
        // spacer も id が必要
        assertRejected(LayoutFixtures.minimal("""{"kind": "spacer"}"""), "id")
    }

    @Test
    fun 文字数はコードポイントで数える() {
        val emoji16 = "😀".repeat(16)
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "label": "$emoji16"}"""))
        assertRejected(
            LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "label": "${"😀".repeat(17)}"}"""),
            "label は 16 文字以内",
        )
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "${"a".repeat(17)}"}"""), "text は")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": ""}"""), "text は")
    }

    @Test
    fun iconの書式() {
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "icon": "sf:shift.fill"}"""))
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "icon": "iconify:mdi:home"}"""))
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "icon": "shift"}"""))
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "icon": "iconify:mdi"}"""), "icon")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "icon": "sf:"}"""), "icon")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "icon": "${"a".repeat(65)}"}"""), "icon")
    }

    @Test
    fun roleは3種だけ() {
        val l = ok(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "role": "function"}"""))
        assertEquals(KeyRole.FUNCTION, l.rows[0].keys[0].role)
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "role": "blue"}"""), "role")
    }

    // ---- kind ごと ----

    @Test
    fun flickはcenterかkanaが必要() {
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "flick"}"""), "center")
        val l = ok(LayoutFixtures.minimal("""{"id": "a", "kind": "flick", "center": {"text": "@"}, "up": {"text": "/"}}"""))
        val f = l.rows[0].keys[0].action as KeyAction.Flick
        assertNull(f.kanaKey)
        assertEquals("@", f.center)
        assertEquals("/", f.up)
        assertNull(f.left)
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "flick", "center": {"text": ""}}"""), "center の text")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "flick", "kana": "あいうえお"}"""), "kana は")
    }

    @Test
    fun toggleのcycle() {
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "toggle", "cycle": []}"""), "cycle")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "toggle"}"""), "cycle")
        val seventeen = (1..17).joinToString(",") { "\"x$it\"" }
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "toggle", "cycle": [$seventeen]}"""), "cycle")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "toggle", "cycle": ["a", ""]}"""), "cycle の要素")
        val l = ok(LayoutFixtures.minimal("""{"id": "a", "kind": "toggle", "cycle": ["a", "b", "c"]}"""))
        assertEquals(listOf("a", "b", "c"), (l.rows[0].keys[0].action as KeyAction.Toggle).cycle)
    }

    @Test
    fun actionの名前の規則() {
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "action", "action": "1abc"}"""), "action")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "action"}"""), "action")
        // 未知の名前でも検証は通る（実行時に無視される）
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "action", "action": "somethingNew"}"""))
    }

    @Test
    fun shortcutの手順() {
        val l = ok(LayoutFixtures.minimal("""{"id": "a", "kind": "shortcut", "steps": [{"text": "（）"}, {"action": "moveLeft"}]}"""))
        val steps = (l.rows[0].keys[0].action as KeyAction.Shortcut).steps
        assertEquals(listOf(ShortcutStep(null, "（）"), ShortcutStep("moveLeft", null)), steps)
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "shortcut", "steps": []}"""), "steps")
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "shortcut", "steps": [{}]}"""), "どちらか一方だけ")
        val many = (1..33).joinToString(",") { """{"text": "a"}""" }
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "shortcut", "steps": [$many]}"""), "steps")
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "shortcut", "steps": [{"text": "${"a".repeat(256)}"}]}"""))
        assertRejected(
            LayoutFixtures.minimal("""{"id": "a", "kind": "shortcut", "steps": [{"text": "${"a".repeat(257)}"}]}"""),
            "text は",
        )
    }

    @Test
    fun layoutの切替先() {
        val key = """{"id": "a", "kind": "layout", "target": "my-layout"}"""
        // 実行時（存在確認なし）は通る
        ok(LayoutFixtures.minimal(key))
        // 取り込み時は存在するレイアウト・組み込み名・自身の id のみ
        assertRejected(LayoutFixtures.minimal(key), "存在するレイアウトでも組み込み名でもありません", emptySet())
        ok(LayoutFixtures.minimal(key), setOf("my-layout"))
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "layout", "target": "numbers"}"""), emptySet())
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "layout", "target": "alpha"}"""), emptySet())
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "layout", "target": "t1"}"""), emptySet())
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "layout", "target": "Bad Id"}"""), "target")
    }

    @Test
    fun 長押しの項目() {
        val l = ok(
            LayoutFixtures.minimal(
                """{"id": "a", "kind": "toggle", "cycle": ["a"], "longPress": [
                    {"label": "1", "kind": "text", "text": "1"},
                    {"label": "x", "kind": "action", "action": "moveLeft"},
                    {"label": "y", "kind": "shortcut", "steps": [{"text": "a"}]}]}""",
            ),
        )
        assertEquals(3, l.rows[0].keys[0].longPress.size)
        fun withPopup(item: String) = LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "longPress": [$item]}""")
        assertRejected(withPopup("""{"label": "x", "kind": "toggle", "cycle": ["a"]}"""), "kind は text / action / shortcut")
        assertRejected(withPopup("""{"label": "x", "kind": "layout", "target": "numbers"}"""), "kind は text / action / shortcut")
        assertRejected(withPopup("""{"kind": "text", "text": "a"}"""), "label")
        assertRejected(withPopup("""{"label": "", "kind": "text", "text": "a"}"""), "label")
        assertRejected(
            withPopup("""{"label": "x", "kind": "text", "text": "a", "longPress": [{"label": "y", "kind": "text", "text": "b"}]}"""),
            "longPress は置けません",
        )
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "longPress": []}"""), "longPress は 1〜12")
        val thirteen = (1..13).joinToString(",") { """{"label": "x", "kind": "text", "text": "a"}""" }
        assertRejected(LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "a", "longPress": [$thirteen]}"""), "longPress は 1〜12")
        // flick の longPress は検証では許す（描画で無視する）
        ok(LayoutFixtures.minimal("""{"id": "a", "kind": "flick", "kana": "あ", "longPress": [{"label": "1", "kind": "text", "text": "1"}]}"""))
    }

    @Test
    fun JSON全体の大きさの上限() {
        val big = LayoutFixtures.minimal(extra = ", \"memo\": \"" + "a".repeat(LayoutLimits.JSON_BYTES_MAX) + "\"")
        assertRejected(big, "大きすぎます")
    }

    @Test
    fun 実行時の読み込みは不正ならnull() {
        assertNull(LayoutParser.parseOrNull("{}"))
        assertNull(LayoutParser.parseOrNull("not json"))
        assertNotNull(LayoutParser.parseOrNull(LayoutFixtures.minimal()))
    }
}
