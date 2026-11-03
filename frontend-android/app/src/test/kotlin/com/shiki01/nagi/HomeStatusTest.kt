package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HomeStatusTest {
    @Test
    fun setupLinesShowOkOrNotSet() {
        assertEquals(StatusLine("キーボードの追加", "OK", Tone.SUCCESS), HomeStatus.keyboardAdded(true))
        assertEquals(StatusLine("キーボードの追加", "未設定", Tone.WARNING), HomeStatus.keyboardAdded(false))
        assertEquals("OK", HomeStatus.keyboardSelected(true).value)
        assertEquals("未選択", HomeStatus.keyboardSelected(false).value)
        assertEquals("OK", HomeStatus.setupDictionary(true).value)
        assertEquals(Tone.WARNING, HomeStatus.setupDictionary(false).tone)
        assertEquals("OK", HomeStatus.setupModel(1).value)
        assertEquals("未設定", HomeStatus.setupModel(0).value)
    }

    @Test
    fun dictionaryState() {
        assertEquals(StatusLine("状態", "あり", Tone.SUCCESS), HomeStatus.dictionaryState(true))
        assertEquals(StatusLine("状態", "なし", Tone.ERROR), HomeStatus.dictionaryState(false))
    }

    @Test
    fun modelStateCountsModels() {
        assertEquals("あり（2 個）", HomeStatus.modelState(2).value)
        assertEquals(Tone.SUCCESS, HomeStatus.modelState(2).tone)
        assertEquals("なし", HomeStatus.modelState(0).value)
        assertEquals(Tone.ERROR, HomeStatus.modelState(0).tone)
        assertEquals("辞書のみで変換します", HomeStatus.modelState(0).subtitle)
        assertNull(HomeStatus.modelState(1).subtitle)
    }

    @Test
    fun modelKindFromFileName() {
        assertEquals("xsmall", HomeStatus.modelKind("zenz-v3.1-xsmall-Q5_K_M.gguf"))
        assertEquals("small", HomeStatus.modelKind("zenz-v3.1-small-Q5_K_M.gguf"))
        assertEquals("不明", HomeStatus.modelKind("custom.gguf"))
        assertEquals("種類: small / 70.4 MB", HomeStatus.modelDetail("zenz-v3.1-small-Q5_K_M.gguf", 73_871_968L))
    }

    @Test
    fun llmVerdictOrder() {
        // 辞書が無い → モデルが無い → 予算不足 → 見込みあり、の順
        assertEquals(Tone.ERROR, HomeStatus.llmVerdict(false, 0, 100).tone)
        assertEquals(Tone.ERROR, HomeStatus.llmVerdict(false, 2, 4096).tone)
        val noModel = HomeStatus.llmVerdict(true, 0, 4096)
        assertEquals(Tone.WARNING, noModel.tone)
        assertTrue(noModel.detail.contains("モデルがありません"))
        val low = HomeStatus.llmVerdict(true, 1, 149)
        assertEquals(Tone.WARNING, low.tone)
        assertTrue(low.detail.contains("149 MB"))
        val ok = HomeStatus.llmVerdict(true, 1, 150)
        assertEquals(Tone.SUCCESS, ok.tone)
        assertEquals("使える見込み", ok.value)
        assertTrue(ok.detail.contains("150 MB"))
    }

    @Test
    fun inputTestCountsCodePoints() {
        assertEquals("0 文字", InputTestLogic.countText(""))
        assertEquals("3 文字", InputTestLogic.countText("あいう"))
        // サロゲートペアは 1 文字
        assertEquals("2 文字", InputTestLogic.countText("a😀"))
    }
}
