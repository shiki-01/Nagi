package com.shiki01.nagi

import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutLinkTest {
    private val json = LayoutFixtures.minimal()

    private fun ok(uri: String): DeepLinkResult.Ok {
        val r = LayoutLink.parse(uri)
        if (r is DeepLinkResult.Err) throw AssertionError("拒否された: ${r.reason}")
        return r as DeepLinkResult.Ok
    }

    private fun reason(uri: String): String {
        val r = LayoutLink.parse(uri)
        if (r is DeepLinkResult.Ok) throw AssertionError("受理された: $uri")
        return (r as DeepLinkResult.Err).reason
    }

    private fun b64url(text: String, padding: Boolean): String {
        val enc = Base64.getUrlEncoder()
        return (if (padding) enc else enc.withoutPadding()).encodeToString(text.toByteArray(Charsets.UTF_8))
    }

    @Test
    fun パディングなしのbase64urlを読める() {
        val r = ok("nagi://layout?data=" + b64url(json, false))
        assertEquals(json, r.json)
        assertEquals("t1", r.layout.id)
    }

    @Test
    fun パディングありと百分率エンコードも読める() {
        assertEquals(json, ok("nagi://layout?data=" + b64url(json, true)).json)
        assertEquals(json, ok("nagi://layout?data=" + b64url(json, true).replace("=", "%3D")).json)
    }

    @Test
    fun encodeとparseは往復する() {
        val preset = LayoutFixtures.presetFiles.first { it.name == "flick-kana.json" }.readText(Charsets.UTF_8)
        val r = ok(LayoutLink.encode(preset))
        assertEquals(preset, r.json)
        assertEquals("flick-kana", r.layout.id)
    }

    @Test
    fun 日本語を含むJSONも往復する() {
        val text = LayoutFixtures.minimal("""{"id": "a", "kind": "text", "text": "あ", "label": "亜😀"}""")
        assertEquals(text, ok(LayoutLink.encode(text)).json)
    }

    @Test
    fun スキームとホストを確かめる() {
        assertEquals("対応していない URL です", reason("https://example.com/?data=abc"))
        assertEquals("対応していない URL です", reason("nagi://other?data=" + b64url(json, false)))
        assertEquals("対応していない URL です", reason("not a url"))
        // 大文字小文字は区別しない
        ok("NAGI://Layout?data=" + b64url(json, false))
        ok("nagi://layout/?data=" + b64url(json, false))
    }

    @Test
    fun dataが無い_空() {
        assertTrue(reason("nagi://layout").contains("data がありません"))
        assertTrue(reason("nagi://layout?x=1").contains("data がありません"))
        assertTrue(reason("nagi://layout?data=").contains("data がありません"))
    }

    @Test
    fun 他のクエリがあってもdataを見つける() {
        ok("nagi://layout?utm=1&data=" + b64url(json, false) + "&x=2")
    }

    @Test
    fun base64として読めないものは拒否() {
        assertTrue(reason("nagi://layout?data=!!!!").contains("base64url"))
        assertTrue(reason("nagi://layout?data=a").contains("base64url"))
    }

    @Test
    fun UTF8でないデータは拒否() {
        val bad = Base64.getUrlEncoder().withoutPadding().encodeToString(byteArrayOf(0xFF.toByte(), 0xFE.toByte(), 0x41))
        assertTrue(reason("nagi://layout?data=$bad").contains("UTF-8"))
    }

    @Test
    fun JSONが不正なら検証の理由を返す() {
        val notJson = b64url("hello", false)
        assertTrue(reason("nagi://layout?data=$notJson").contains("JSON として読み取れません"))
        val badLayout = b64url(json.replace("\"schema\": 2", "\"schema\": 9"), false)
        assertTrue(reason("nagi://layout?data=$badLayout").contains("未対応のスキーマ"))
    }

    @Test
    fun dataの長さの上限() {
        val over = "A".repeat(LayoutLink.MAX_DATA_CHARS + 1)
        assertTrue(reason("nagi://layout?data=$over").contains("大きすぎます"))
        // URL 全体の上限（data 以外が大きい場合）も弾く
        val longUrl = "nagi://layout?x=" + "a".repeat(LayoutLink.MAX_DATA_CHARS + 5000) + "&data=AAAA"
        assertTrue(reason(longUrl).contains("大きすぎます"))
    }

    @Test
    fun assetsを含むレイアウトはURLでは受け付けない() {
        val withAssets = LayoutFixtures.minimal(extra = """, "assets": {"bg": "data:image/png;base64,AAAA"}""")
        assertTrue(reason(LayoutLink.encode(withAssets)).contains("assets"))
        // 空の assets は問題なし
        ok(LayoutLink.encode(LayoutFixtures.minimal(extra = """, "assets": {}""")))
    }
}
