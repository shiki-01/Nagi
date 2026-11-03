package com.shiki01.nagi

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SettingsSchemaTest {
    private val snapshot = JSONObject(
        """
        {"items":[
          {"key":"conversion.sync_budget_ms","group":"変換","label":"待ち時間","description":"d",
           "kind":{"type":"int","params":{"min":10,"max":5000,"step":10}},
           "value":{"type":"int","value":150},"default":{"type":"int","value":150},
           "explicit":false,"requires_restart":false,"level":"detail","platforms":"both"},
          {"key":"profile.mode","group":"動作モード","label":"プロファイル","description":"d",
           "kind":{"type":"choice","params":{"options":[{"value":"auto","label":"自動"},{"value":"minimal","label":"辞書のみ"}]}},
           "value":{"type":"text","value":"auto"},"default":{"type":"text","value":"auto"},
           "explicit":true,"requires_restart":false,"level":"simple","platforms":"both"},
          {"key":"conversion.model","group":"変換","label":"モデル","description":"d","kind":{"type":"text"},
           "value":null,"default":null,"explicit":false,"requires_restart":true,"level":"detail","platforms":"both"},
          {"key":"apps.x","group":"アプリ","label":"x","description":"d","kind":{"type":"bool"},
           "value":{"type":"bool","value":true},"default":{"type":"bool","value":true},
           "explicit":false,"requires_restart":false,"level":"simple","platforms":"desktop"},
          {"key":"input.dakuten_cycle","group":"入力","label":"濁点","description":"d",
           "kind":{"type":"choice","params":{"options":[{"value":"small_first","label":"小書き先"}]}},
           "value":{"type":"text","value":"small_first"},"default":{"type":"text","value":"small_first"},
           "explicit":false,"requires_restart":false,"level":"simple","platforms":"ios"},
          {"key":"input.fuzzy.use_touch_coords","group":"入力","label":"座標","description":"d","kind":{"type":"bool"},
           "value":{"type":"bool","value":false},"default":{"type":"bool","value":false},
           "explicit":false,"requires_restart":false,"level":"simple","platforms":"ios"},
          {"key":"conversion.beam_width","group":"変換","label":"幅","description":"d",
           "kind":{"type":"float","params":{"min":0.0,"max":1.0,"step":0.1}},
           "value":null,"default":null,"explicit":false,"requires_restart":false,"level":"detail","platforms":"both"}
        ]}
        """.trimIndent(),
    )

    @Test
    fun parsesItems() {
        val items = SettingsSchema.parseItems(snapshot)
        assertEquals(7, items.size)
        val sync = items[0]
        assertEquals(SettingType.INT, sync.type)
        assertEquals(10.0, sync.min, 0.0)
        assertEquals(5000.0, sync.max, 0.0)
        assertEquals(150, sync.value)
        assertTrue(sync.detail)
        val mode = items[1]
        assertEquals(SettingType.CHOICE, mode.type)
        assertEquals(listOf("auto", "minimal"), mode.choices.map { it.value })
        assertEquals("auto", mode.value)
        assertTrue(mode.explicit)
        assertNull(items[2].value)
        assertTrue(items[2].requiresRestart)
    }

    @Test
    fun visibilityFiltersPlatformsLevelAndHiddenKeys() {
        val items = SettingsSchema.parseItems(snapshot)
        val simple = SettingsSchema.visible(items, showDetail = false).map { it.key }
        assertEquals(listOf("profile.mode", "input.dakuten_cycle"), simple)
        val detail = SettingsSchema.visible(items, showDetail = true).map { it.key }
        assertEquals(
            listOf("conversion.sync_budget_ms", "profile.mode", "input.dakuten_cycle", "conversion.beam_width"),
            detail,
        )
    }

    @Test
    fun groupsFollowFixedOrder() {
        // 出現順は 変換 → 動作モード → 入力 だが、並びは固定順（入力 → 変換 → 動作モード）
        val items = SettingsSchema.visible(SettingsSchema.parseItems(snapshot), true)
        assertEquals(listOf("入力", "変換", "動作モード"), SettingsSchema.groups(items).map { it.first })
    }

    @Test
    fun groupOrderIsIosFixedOrderAndUnknownGroupsComeLastInAppearanceOrder() {
        assertEquals(
            listOf(
                "入力", "変換", "動作モード", "予測変換", "学習", "キーボード", "アップデート", "開発者",
                "表記の候補", "ユーザー辞書", "アプリごとの設定",
            ),
            SettingsSchema.GROUP_ORDER,
        )
        val names = listOf("アプリごとの設定", "未知B", "学習", "表記の候補", "入力", "未知A", "ユーザー辞書", "未知B")
        assertEquals(
            listOf("入力", "学習", "表記の候補", "ユーザー辞書", "アプリごとの設定", "未知B", "未知A"),
            SettingsSchema.orderedGroupNames(names),
        )
    }

    @Test
    fun groupsKeepItemOrderWithinGroup() {
        fun def(key: String, group: String) = SettingDef(key, group, key, "d", SettingType.BOOL)
        val items = listOf(def("a.2", "変換"), def("b.1", "入力"), def("a.1", "変換"), def("b.2", "入力"))
        val g = SettingsSchema.groups(items)
        assertEquals(listOf("入力", "変換"), g.map { it.first })
        assertEquals(listOf("b.1", "b.2"), g[0].second.map { it.key })
        assertEquals(listOf("a.2", "a.1"), g[1].second.map { it.key })
    }

    @Test
    fun coreUpdateAutoCheckIsHidden() {
        // 更新確認で実際に見られるのは KeyboardPrefs.autoCheckUpdates だけなので、コアの記述子の行は出さない
        val d = SettingDef("update.auto_check", "アップデート", "アップデートを自動で確認", "d", SettingType.BOOL)
        assertFalse(SettingsSchema.isVisibleOnAndroid(d))
        assertTrue(SettingsSchema.visible(listOf(d), showDetail = true).isEmpty())
        assertTrue(SettingsSchema.visible(listOf(d.copy(detail = true)), showDetail = true).isEmpty())
    }

    @Test
    fun buildsUpdateSettingsChanges() {
        val params = SettingsSchema.updateParams(
            listOf(
                Triple("conversion.sync_budget_ms", SettingType.INT, 200),
                Triple("conversion.use_llm", SettingType.BOOL, false),
                Triple("learning.strength", SettingType.FLOAT, 0.5),
                Triple("profile.mode", SettingType.CHOICE, "minimal"),
                Triple("conversion.model", SettingType.TEXT, "/data/x.gguf"),
                Triple("conversion.beam_width", SettingType.FLOAT, null),
            ),
        )
        val changes = params.getJSONArray("changes")
        assertEquals(6, changes.length())
        val first = changes.getJSONObject(0)
        assertEquals("conversion.sync_budget_ms", first.getString("key"))
        assertEquals("int", first.getJSONObject("value").getString("type"))
        assertEquals(200, first.getJSONObject("value").getInt("value"))
        assertEquals("bool", changes.getJSONObject(1).getJSONObject("value").getString("type"))
        assertFalse(changes.getJSONObject(1).getJSONObject("value").getBoolean("value"))
        assertEquals("float", changes.getJSONObject(2).getJSONObject("value").getString("type"))
        assertEquals(0.5, changes.getJSONObject(2).getJSONObject("value").getDouble("value"), 0.0)
        // choice と text はどちらも "text"
        assertEquals("text", changes.getJSONObject(3).getJSONObject("value").getString("type"))
        assertEquals("text", changes.getJSONObject(4).getJSONObject("value").getString("type"))
        // null は value を省略（既定へ戻す）
        assertFalse(changes.getJSONObject(5).has("value"))
    }

    @Test
    fun mismatchedValueTypeIsOmitted() {
        assertNull(SettingsSchema.valueJson(SettingType.BOOL, "yes"))
        assertNull(SettingsSchema.valueJson(SettingType.INT, "1"))
        assertNull(SettingsSchema.valueJson(SettingType.TEXT, 3))
    }

    @Test
    fun stepperRespectsRangeAndStep() {
        val d = SettingDef("k", "g", "l", "d", SettingType.INT, min = 10.0, max = 5000.0, step = 10.0)
        assertEquals(160.0, SettingsSchema.stepped(d, 150.0, +1), 0.0)
        assertEquals(10.0, SettingsSchema.stepped(d, 10.0, -1), 0.0)
        assertEquals(5000.0, SettingsSchema.stepped(d, 5000.0, +1), 0.0)
        // 格子に乗っていない値は近い格子へ寄せてから動かす
        assertEquals(160.0, SettingsSchema.stepped(d, 153.0, +1), 0.0)
        val f = SettingDef("k", "g", "l", "d", SettingType.FLOAT, min = 0.0, max = 1.0, step = 0.1)
        assertEquals(0.3, SettingsSchema.stepped(f, 0.2, +1), 1e-9)
        assertEquals(1.0, SettingsSchema.stepped(f, 0.9, +1), 1e-9)
    }

    @Test
    fun numberInputIsClampedAndTyped() {
        val d = SettingDef("k", "g", "l", "d", SettingType.INT, min = 10.0, max = 5000.0, step = 10.0)
        assertEquals(5000, SettingsSchema.parseNumberInput(d, "99999"))
        assertEquals(10, SettingsSchema.parseNumberInput(d, "1"))
        assertEquals(200, SettingsSchema.parseNumberInput(d, " 200 "))
        assertNull(SettingsSchema.parseNumberInput(d, "abc"))
        val f = SettingDef("k", "g", "l", "d", SettingType.FLOAT, min = 0.0, max = 1.0, step = 0.1)
        assertEquals(0.25, SettingsSchema.parseNumberInput(f, "0.25"))
        assertEquals("0.25", SettingsSchema.formatNumber(f, 0.25))
        assertEquals("1", SettingsSchema.formatNumber(f, 1.0))
        assertEquals("150", SettingsSchema.formatNumber(d, 150.0))
    }
}
