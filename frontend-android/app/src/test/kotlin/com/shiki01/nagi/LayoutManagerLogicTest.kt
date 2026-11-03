package com.shiki01.nagi

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutManagerLogicTest {
    private fun info(
        id: String,
        mode: LayoutMode?,
        source: LayoutSource = LayoutSource.PRESET,
        error: String? = null,
        name: String = "名前-$id",
        overrides: Boolean = false,
    ) = LayoutInfo(id, name, mode, source, 10, error, overrides)

    // ---- プレビューの寸法 ----

    @Test
    fun 行の高さは高さ比で按分し行間を挟む() {
        val spans = PreviewGeometry.rowSpans(listOf(1.0, 2.0, 0.5), 30f, 3f)
        assertEquals(PreviewSpan(0f, 30f), spans[0])
        assertEquals(PreviewSpan(33f, 60f), spans[1])
        assertEquals(PreviewSpan(96f, 15f), spans[2])
        assertEquals(111f, PreviewGeometry.totalHeight(listOf(1.0, 2.0, 0.5), 30f, 3f), 0.001f)
    }

    @Test
    fun 行が無いときの高さは0() {
        assertEquals(0f, PreviewGeometry.totalHeight(emptyList(), 30f, 3f), 0f)
        assertTrue(PreviewGeometry.rowSpans(emptyList(), 30f, 3f).isEmpty())
    }

    @Test
    fun キーの幅は幅比で按分し間隔を引く() {
        // 幅 106、間隔 3、キー 3 つ（比 1:2:1）→ 使える幅 100 を 25 / 50 / 25
        val spans = PreviewGeometry.keySpans(listOf(1.0, 2.0, 1.0), 106f, 3f)
        assertEquals(3, spans.size)
        assertEquals(0f, spans[0].start, 0.001f)
        assertEquals(25f, spans[0].size, 0.001f)
        assertEquals(28f, spans[1].start, 0.001f)
        assertEquals(50f, spans[1].size, 0.001f)
        assertEquals(81f, spans[2].start, 0.001f)
        assertEquals(25f, spans[2].size, 0.001f)
        // 最後のキーの右端が行の幅に一致する
        assertEquals(106f, spans[2].start + spans[2].size, 0.001f)
    }

    @Test
    fun キーが1つなら行の幅いっぱい() {
        val spans = PreviewGeometry.keySpans(listOf(0.7), 200f, 4f)
        assertEquals(PreviewSpan(0f, 200f), spans.single())
    }

    @Test
    fun 行の幅が足りなくても負にならない() {
        val spans = PreviewGeometry.keySpans(listOf(1.0, 1.0, 1.0), 2f, 3f)
        assertTrue(spans.all { it.size == 0f })
    }

    @Test
    fun 同梱のプリセットの幅の割り当ては行の幅に収まる() {
        for (f in LayoutFixtures.presetFiles) {
            val layout = (LayoutFixtures.parse(f.readText(Charsets.UTF_8)) as LayoutParseResult.Ok).layout
            for (row in layout.rows) {
                val spans = PreviewGeometry.keySpans(row.keys.map { it.width }, 300f, 3f)
                assertEquals(row.keys.size, spans.size)
                assertEquals(300f, spans.last().start + spans.last().size, 0.01f)
            }
        }
    }

    // ---- 割り当ての選択肢 ----

    @Test
    fun 有効なレイアウトはユーザーのものが同梱を上書きする() {
        val presets = listOf(info("flick-kana", LayoutMode.KANA), info("numbers", LayoutMode.NUMBER))
        val users = listOf(info("flick-kana", LayoutMode.KANA, LayoutSource.USER, name = "自作"))
        val eff = LayoutChoices.effective(presets, users)
        assertEquals(listOf("flick-kana", "numbers"), eff.keys.toList())
        assertEquals(LayoutSource.USER, eff.getValue("flick-kana").source)
    }

    @Test
    fun 不正なユーザーのレイアウトは同梱へ戻り単独なら出さない() {
        val presets = listOf(info("flick-kana", LayoutMode.KANA))
        val users = listOf(
            info("flick-kana", null, LayoutSource.USER, error = "壊れている"),
            info("broken", null, LayoutSource.USER, error = "壊れている"),
        )
        val eff = LayoutChoices.effective(presets, users)
        assertEquals(listOf("flick-kana"), eff.keys.toList())
        assertEquals(LayoutSource.PRESET, eff.getValue("flick-kana").source)
    }

    @Test
    fun かなの選択肢はかなとカスタムだけ() {
        val presets = listOf(
            info("flick-kana", LayoutMode.KANA),
            info("qwerty-alpha", LayoutMode.ALPHA),
            info("numbers", LayoutMode.NUMBER),
        )
        val users = listOf(info("mine", LayoutMode.CUSTOM, LayoutSource.USER, name = "自作"))
        val eff = LayoutChoices.effective(presets, users)
        val kana = LayoutChoices.slotOptions(AssignSlot.KANA, eff, "flick-kana")
        assertEquals(listOf("flick-kana", "mine"), kana.map { it.first })
        assertEquals("自作（自分）", kana.last().second)
        val alpha = LayoutChoices.slotOptions(AssignSlot.ALPHA, eff, "qwerty-alpha")
        assertEquals(listOf("qwerty-alpha", "mine"), alpha.map { it.first })
    }

    @Test
    fun 現在の割り当てはモードが合わなくても選択肢に残る() {
        val eff = LayoutChoices.effective(listOf(info("flick-kana", LayoutMode.KANA), info("numbers", LayoutMode.NUMBER)), emptyList())
        val kana = LayoutChoices.slotOptions(AssignSlot.KANA, eff, "numbers")
        assertEquals(listOf("flick-kana", "numbers"), kana.map { it.first })
        // 存在しない・無効な割り当ては足さない
        assertEquals(listOf("flick-kana"), LayoutChoices.slotOptions(AssignSlot.KANA, eff, "ghost").map { it.first })
    }

    @Test
    fun 入力欄の選択肢は自動かな英字切り替えないに続いて各レイアウト() {
        val eff = LayoutChoices.effective(
            listOf(info("flick-kana", LayoutMode.KANA), info("numbers", LayoutMode.NUMBER)),
            listOf(info("kana", LayoutMode.KANA, LayoutSource.USER)),
        )
        val opts = LayoutChoices.fieldOptions(FieldKind.EMAIL, eff)
        assertEquals(listOf("", "kana", "alpha", "none", "flick-kana", "numbers"), opts.map { it.first })
        assertEquals("自動（英字）", opts[0].second)
        assertEquals("自動（切り替えない）", LayoutChoices.fieldOptions(FieldKind.NORMAL, eff)[0].second)
        assertEquals("自動（数字）", LayoutChoices.fieldOptions(FieldKind.NUMBER, eff)[0].second)
    }

    @Test
    fun 入力欄の値の保存と復元() {
        assertEquals(LayoutChoices.AUTO, LayoutChoices.fieldSelected(null))
        assertEquals("alpha", LayoutChoices.fieldSelected("alpha"))
        assertNull(LayoutChoices.fieldValueToStore(LayoutChoices.AUTO))
        assertEquals("none", LayoutChoices.fieldValueToStore("none"))
    }

    @Test
    fun 切替先の表示名() {
        val names = mapOf("numbers" to "数字")
        assertEquals("切り替えない", LayoutChoices.targetLabel(null, names))
        assertEquals("かな", LayoutChoices.targetLabel("kana", names))
        assertEquals("英字", LayoutChoices.targetLabel("alpha", names))
        assertEquals("数字", LayoutChoices.targetLabel("numbers", names))
        assertEquals("unknown", LayoutChoices.targetLabel("unknown", names))
        assertEquals(
            "いまの切替先: 英字",
            LayoutChoices.fieldSubtitle(FieldKind.URL, emptyMap(), names),
        )
        assertEquals(
            "いまの切替先: 切り替えない",
            LayoutChoices.fieldSubtitle(FieldKind.URL, mapOf(FieldKind.URL to "none"), names),
        )
    }

    @Test
    fun モードごとの割り当て先() {
        assertEquals(listOf(AssignSlot.KANA), LayoutTexts.slotsFor(LayoutMode.KANA))
        assertEquals(listOf(AssignSlot.ALPHA), LayoutTexts.slotsFor(LayoutMode.ALPHA))
        assertEquals(listOf(AssignSlot.KANA, AssignSlot.ALPHA), LayoutTexts.slotsFor(LayoutMode.CUSTOM))
        for (m in listOf(LayoutMode.NUMBER, LayoutMode.SYMBOL, LayoutMode.EMOJI, null)) {
            assertTrue(LayoutTexts.slotsFor(m).isEmpty())
        }
    }

    @Test
    fun 一覧の補足行() {
        assertEquals("flick-kana ・ かな ・ 10 キー", LayoutTexts.summary(info("flick-kana", LayoutMode.KANA)))
        assertEquals("x ・ 使えません", LayoutTexts.summary(info("x", null, error = "壊れている")))
    }

    // ---- 取り込みの確認文 ----

    @Test
    fun 確認文に名前とキー数と衝突が入る() {
        val plain = LayoutImportText.confirmMessage("テスト", "t1", LayoutMode.KANA, 12, false, false, false)
        assertTrue(plain.contains("名前: テスト"))
        assertTrue(plain.contains("id: t1"))
        assertTrue(plain.contains("モード: かな"))
        assertTrue(plain.contains("キー数: 12"))
        assertFalse(plain.contains("置き換え"))
        assertFalse(plain.contains("同梱"))
        assertFalse(plain.contains("リンク"))

        val replace = LayoutImportText.confirmMessage("テスト", "t1", LayoutMode.KANA, 12, true, false, false)
        assertTrue(replace.contains("置き換えます"))

        val override = LayoutImportText.confirmMessage("テスト", "flick-kana", LayoutMode.KANA, 12, false, true, false)
        assertTrue(override.contains("同梱のレイアウト（flick-kana）と同じ id"))

        val link = LayoutImportText.confirmMessage("テスト", "t1", LayoutMode.KANA, 12, false, false, true)
        assertTrue(link.contains("リンク"))
    }

    @Test
    fun 取り込み結果の文() {
        assertEquals("「A」を取り込みました", LayoutImportText.success("A", false))
        assertEquals("「A」を置き換えました", LayoutImportText.success("A", true))
        assertNotNull(LayoutImportText.confirmTitle(true))
        assertTrue(LayoutImportText.confirmTitle(true) != LayoutImportText.confirmTitle(false))
    }

    // ---- nagi://layout の受け入れ ----

    @Test
    fun VIEW以外やデータなしは対象外() {
        assertEquals(IntakeResult.None, LayoutIntake.classify(false, "nagi://layout?data=abc"))
        assertEquals(IntakeResult.None, LayoutIntake.classify(true, null))
        assertEquals(IntakeResult.None, LayoutIntake.classify(true, "  "))
    }

    @Test
    fun スキームとホストが合えば受け入れる() {
        val r = LayoutIntake.classify(true, "nagi://layout?data=abc")
        assertTrue(r is IntakeResult.Accept)
        assertEquals("nagi://layout?data=abc", (r as IntakeResult.Accept).url)
        assertTrue(LayoutIntake.classify(true, "NAGI://Layout/?data=abc") is IntakeResult.Accept)
        assertTrue(LayoutIntake.classify(true, " nagi://layout?data=abc ") is IntakeResult.Accept)
    }

    @Test
    fun 別のスキームやホストは拒否する() {
        assertTrue(LayoutIntake.classify(true, "https://layout?data=abc") is IntakeResult.Reject)
        assertTrue(LayoutIntake.classify(true, "nagi://other?data=abc") is IntakeResult.Reject)
        assertTrue(LayoutIntake.classify(true, "nagi://layout.evil.example/?data=abc") is IntakeResult.Reject)
        assertTrue(LayoutIntake.classify(true, "nagi:layout?data=abc") is IntakeResult.Reject)
    }

    @Test
    fun 大きすぎるリンクは拒否する() {
        val big = "nagi://layout?data=" + "A".repeat(LayoutLink.MAX_DATA_CHARS + 5000)
        assertTrue(LayoutIntake.classify(true, big) is IntakeResult.Reject)
    }

    @Test
    fun 貼り付けがリンクかの判定() {
        assertTrue(LayoutIntake.looksLikeLink("  nagi://layout?data=x"))
        assertTrue(LayoutIntake.looksLikeLink("NAGI://layout"))
        assertFalse(LayoutIntake.looksLikeLink("{\"schema\": 2}"))
        assertTrue(LayoutIntake.classifyUrl("nagi://layout?data=x") is IntakeResult.Accept)
    }

    @Test
    fun 生成したリンクを受け入れて解釈できる() {
        val json = LayoutFixtures.minimal()
        val link = LayoutLink.encode(json)
        assertFalse(LayoutIntake.linkTooLong(link))
        assertTrue(LayoutIntake.classify(true, link) is IntakeResult.Accept)
        assertTrue(LayoutLink.parse(link) is DeepLinkResult.Ok)
    }

    @Test
    fun リンクの長さ上限() {
        val ok = "nagi://layout?data=" + "A".repeat(LayoutLink.MAX_DATA_CHARS)
        assertFalse(LayoutIntake.linkTooLong(ok))
        assertTrue(LayoutIntake.linkTooLong(ok + "A"))
    }

    // ---- 複製 ----

    @Test
    fun 複製のidは空いているものを選ぶ() {
        assertEquals("flick-kana-copy", LayoutCopy.freeId("flick-kana", setOf("flick-kana")))
        assertEquals("flick-kana-copy2", LayoutCopy.freeId("flick-kana", setOf("flick-kana", "flick-kana-copy")))
        assertEquals("a-copy3", LayoutCopy.freeId("a", setOf("a-copy", "a-copy2")))
    }

    @Test
    fun 複製のidは64文字に収まり規則に合う() {
        val base = "a".repeat(64)
        val id = LayoutCopy.freeId(base, setOf(base))
        assertEquals(64, id.length)
        assertTrue(LayoutIds.isValid(id))
        assertTrue(id.endsWith("-copy"))
        val id2 = LayoutCopy.freeId(base, setOf(base, id))
        assertEquals(64, id2.length)
        assertTrue(id2.endsWith("-copy2"))
        assertTrue(LayoutIds.isValid(id2))
    }

    @Test
    fun 複製の名前は64文字に収まる() {
        assertEquals("12キー（かな） のコピー", LayoutCopy.copyName("12キー（かな）"))
        val long = "あ".repeat(64)
        val copy = LayoutCopy.copyName(long)
        assertEquals(64, copy.codePointCount(0, copy.length))
        // サロゲートペアの途中で切らない
        val emoji = "😀".repeat(70)
        val c2 = LayoutCopy.copyName(emoji)
        assertEquals(64, c2.codePointCount(0, c2.length))
        assertTrue(c2.all { it.isHighSurrogate() || it.isLowSurrogate() })
    }

    @Test
    fun 書き換えたJSONは同じ内容でidと名前だけ変わる() {
        for (f in LayoutFixtures.presetFiles) {
            val raw = f.readText(Charsets.UTF_8)
            val original = (LayoutFixtures.parse(raw) as LayoutParseResult.Ok).layout
            val rewritten = LayoutCopy.rewrite(raw, "copy-1", "コピー")
            assertNotNull(rewritten)
            val copy = (LayoutFixtures.parse(rewritten!!) as LayoutParseResult.Ok).layout
            assertEquals("copy-1", copy.id)
            assertEquals("コピー", copy.name)
            assertEquals(original.mode, copy.mode)
            assertEquals(original.keyCount, copy.keyCount)
            assertEquals(original.rows, copy.rows)
        }
    }

    @Test
    fun 壊れたJSONは書き換えられない() {
        assertNull(LayoutCopy.rewrite("{", "a", "b"))
        assertNull(LayoutCopy.minify("not json"))
    }

    @Test
    fun 空白を詰めても同じレイアウトになる() {
        val raw = LayoutFixtures.presetFiles.first().readText(Charsets.UTF_8)
        val min = LayoutCopy.minify(raw)
        assertNotNull(min)
        assertTrue(min!!.length <= raw.length)
        assertEquals(
            (LayoutFixtures.parse(raw) as LayoutParseResult.Ok).layout,
            (LayoutFixtures.parse(min) as LayoutParseResult.Ok).layout,
        )
        assertEquals(2, JSONObject(min).getInt("schema"))
    }
}
