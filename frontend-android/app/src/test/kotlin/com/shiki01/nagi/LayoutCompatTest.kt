package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutCompatTest {
    @Test
    fun 未対応の指定が無ければ空() {
        assertEquals(emptyList<String>(), LayoutCompat.unsupported(LayoutFixtures.minimal()))
        // 空の assets と null は数えない
        assertEquals(emptyList<String>(), LayoutCompat.unsupported(LayoutFixtures.minimal(extra = """, "assets": {}, "look": null""")))
    }

    @Test
    fun 壊れたJSONは空() {
        assertEquals(emptyList<String>(), LayoutCompat.unsupported("not json"))
    }

    @Test
    fun 全体_行_キーの指定を拾い_決まった順で返す() {
        val json = """{"schema": 2, "id": "t1", "name": "テスト", "mode": "kana",
            "toolbar": {"keys": [{"id": "p", "kind": "action", "action": "paste"}]},
            "metrics": {"keySpacing": 2},
            "rows": [
              {"look": {"fontSize": 20}, "keys": [
                {"id": "a", "kind": "text", "text": "a", "states": {"pressed": {"fill": "#fff"}}, "visible": {"field": "returnKeyType", "in": ["search"]}}
              ]}
            ]}"""
        assertEquals(listOf("look", "states", "metrics", "toolbar", "visible"), LayoutCompat.unsupported(json))
    }

    @Test
    fun 全部入りのレイアウトでも取り込みの検証は通る() {
        val json = LayoutFixtures.minimal(
            key = """{"id": "a", "kind": "text", "text": "a", "look": {"fontSize": 20}}""",
            extra = """, "roles": {"function": {"fill": "#eee"}}, "rules": [], "background": {"fill": "#000"}, "assets": {"bg": "data:image/png;base64,AAAA"}""",
        )
        assertEquals(listOf("look", "roles", "rules", "background", "assets"), LayoutCompat.unsupported(json))
        assertTrue(LayoutFixtures.parse(json) is LayoutParseResult.Ok)
    }

    @Test
    fun 表示用の文() {
        assertNull(LayoutCompat.notice(emptyList()))
        assertEquals("この端末では次の指定は反映されません: look（見た目）", LayoutCompat.notice(listOf("look")))
        assertNotNull(LayoutCompat.notice(listOf("unknown")))
        assertEquals("unknown", LayoutCompat.label("unknown"))
    }
}
