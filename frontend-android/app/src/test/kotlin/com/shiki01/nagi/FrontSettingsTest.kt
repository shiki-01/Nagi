package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FrontSettingsTest {
    private fun core(key: String, group: String, detail: Boolean = false, platforms: String = "both") =
        SettingDef(key, group, key, "d", SettingType.BOOL, detail = detail, platforms = platforms)

    /** 全グループにわたるコアの項目（わざと固定順と違う並びで渡す） */
    private val coreItems = listOf(
        core("apps.code_mode", "アプリごとの設定", detail = true),
        core("userdict.enabled", "ユーザー辞書", detail = true),
        core("rewriter.emoji", "表記の候補", detail = true),
        core("update.auto_check", "アップデート"),
        core("predict.enabled", "予測変換"),
        core("learning.enabled", "学習"),
        core("profile.mode", "動作モード"),
        core("profile.auto_switch", "動作モード", detail = true),
        core("conversion.live_conversion", "変換"),
        core("conversion.live_conversion_delay_ms", "変換", detail = true),
        core("input.punctuation", "入力"),
        core("input.shift_letter", "入力", detail = true),
        core("input.fuzzy.use_touch_coords", "入力", detail = true, platforms = "ios"),
        core("conversion.model", "変換", detail = true),
        core("apps.desktop_only", "アプリごとの設定", platforms = "desktop"),
    )

    private fun keys(s: SettingsSection) = s.entries.map { it.key }

    private fun titles(sections: List<SettingsSection>) = sections.map { it.title }

    private fun plan(mode: SettingsUiMode, dev: Boolean = false) = SettingsPlan.build(coreItems, mode, dev)

    private fun section(sections: List<SettingsSection>, title: String) = sections.first { it.title == title }

    // ---- 表示モード ----

    @Test
    fun 表示モードの既定はシンプルで_読めない値もシンプル() {
        assertEquals(SettingsUiMode.SIMPLE, SettingsUiMode.fromJson(null))
        assertEquals(SettingsUiMode.SIMPLE, SettingsUiMode.fromJson("unknown"))
        assertEquals(SettingsUiMode.DETAIL, SettingsUiMode.fromJson("detail"))
        assertEquals(listOf("シンプル", "詳細"), SettingsUiMode.values().map { it.label })
        assertFalse(SettingsUiMode.SIMPLE.showDetail)
        assertTrue(SettingsUiMode.DETAIL.showDetail)
    }

    // ---- グループの並び ----

    @Test
    fun 詳細ではグループが固定順に並ぶ() {
        assertEquals(
            listOf(
                "入力", "変換", "動作モード", "予測変換", "学習", "キーボード", "クリップボード履歴", "アップデート", "開発者",
                "表記の候補", "ユーザー辞書", "アプリごとの設定",
            ),
            titles(plan(SettingsUiMode.DETAIL)),
        )
    }

    @Test
    fun シンプルでは詳細の項目だけのグループは出ない() {
        // 表記の候補・ユーザー辞書・アプリごとの設定 は、この入力ではシンプルの行が無い。開発者もオフなら出ない
        assertEquals(
            listOf("入力", "変換", "動作モード", "予測変換", "学習", "キーボード", "クリップボード履歴", "アップデート"),
            titles(plan(SettingsUiMode.SIMPLE)),
        )
    }

    @Test
    fun 未知のグループは最後に出現順() {
        val items = listOf(core("x.1", "未知B"), core("i.1", "入力"), core("x.2", "未知A"))
        val sections = SettingsPlan.build(items, SettingsUiMode.SIMPLE, false)
        assertEquals(listOf("入力", "学習", "キーボード", "クリップボード履歴", "アップデート", "未知B", "未知A"), titles(sections))
    }

    // ---- 出し分け ----

    @Test
    fun コアの項目はレベルで出し分け_desktop専用とios専用と隠す項目は出さない() {
        val simple = plan(SettingsUiMode.SIMPLE)
        assertEquals(
            listOf("input.punctuation"),
            keys(section(simple, "入力")).filter { it.startsWith("input.punctuation") || it.startsWith("input.shift") },
        )
        val detail = plan(SettingsUiMode.DETAIL)
        val all = detail.flatMap { s -> s.entries.filterIsInstance<SettingsEntry.Core>().map { it.def.key } }
        assertTrue("input.shift_letter" in all)
        assertFalse("conversion.model" in all)
        assertFalse("update.auto_check" in all)
        assertFalse("input.fuzzy.use_touch_coords" in all)
        assertFalse("apps.desktop_only" in all)
    }

    @Test
    fun フロント固有の項目はシンプルと詳細で出し分ける() {
        val simpleFront = plan(SettingsUiMode.SIMPLE).flatMap { s -> s.entries.filterIsInstance<SettingsEntry.Front>() }
            .map { it.def.key }.toSet()
        assertEquals(
            setOf(
                FrontKeys.TOGGLE_INPUT, FrontKeys.FLICK_SENSITIVITY, FrontKeys.FLICK_POPUP, FrontKeys.FLICK_HINTS,
                FrontKeys.HEIGHT, FrontKeys.ONE_HANDED, FrontKeys.HAPTIC, FrontKeys.SOUND, FrontKeys.THEME, FrontKeys.ACCENT,
                FrontKeys.LAYOUT_KANA, FrontKeys.LAYOUT_ALPHA, FrontKeys.LEARNING_CLEAR, FrontKeys.UPDATE_AUTO_CHECK,
                FrontKeys.UPDATE_OPEN, FrontKeys.CLIPBOARD_ENABLED, FrontKeys.CLIPBOARD_RETENTION, FrontKeys.CLIPBOARD_CLEAR,
            ),
            simpleFront,
        )
        val detailFront = plan(SettingsUiMode.DETAIL).flatMap { s -> s.entries.filterIsInstance<SettingsEntry.Front>() }
            .map { it.def.key }.toSet()
        // 詳細は、開発者ツール（開発者モードがオンのときだけ）以外のすべて
        assertEquals(FrontSettings.definitions.map { it.key }.toSet() - FrontKeys.DEVELOPER_OPEN, detailFront)
        assertTrue(detailFront.containsAll(simpleFront))
        for (k in listOf(
            FrontKeys.COMPOSE_NUMBERS, FrontKeys.KEEP_PREEDIT_ON_SYMBOLS, FrontKeys.KEEP_PREEDIT_ON_ENGLISH,
            FrontKeys.FLICK_LONG_PRESS, FrontKeys.LAYOUT_BY_FIELD, FrontKeys.HEIGHT_SCALE, FrontKeys.HEIGHT_SCALE_LANDSCAPE,
        )) {
            assertFalse(k, k in simpleFront)
            assertTrue(k, k in detailFront)
        }
    }

    @Test
    fun 入力グループはコアの項目のあとにフロント固有の項目がiosと同じ順で並ぶ() {
        val detail = plan(SettingsUiMode.DETAIL)
        assertEquals(
            listOf(
                "input.punctuation", "input.shift_letter",
                FrontKeys.TOGGLE_INPUT, FrontKeys.COMPOSE_NUMBERS, FrontKeys.KEEP_PREEDIT_ON_SYMBOLS,
                FrontKeys.KEEP_PREEDIT_ON_ENGLISH, FrontKeys.FLICK_SENSITIVITY, FrontKeys.FLICK_THRESHOLD, FrontKeys.FLICK_POPUP,
                FrontKeys.FLICK_HINTS, FrontKeys.FLICK_LONG_PRESS, FrontKeys.KEY_REPEAT_DELAY, FrontKeys.KEY_REPEAT,
                FrontKeys.SPACE_SWIPE, FrontKeys.SPACE_SWIPE_STEP, FrontKeys.SPACE_SWIPE_ACCEL,
                FrontKeys.SPACE_SWIPE_ACCEL_STRENGTH, FrontKeys.BACKSPACE_SWIPE,
            ),
            keys(section(detail, "入力")),
        )
    }

    @Test
    fun キーボードグループの並び() {
        assertEquals(
            listOf(
                FrontKeys.HEIGHT, FrontKeys.HEIGHT_SCALE, FrontKeys.HEIGHT_SCALE_LANDSCAPE, FrontKeys.ONE_HANDED,
                FrontKeys.ONE_HANDED_WIDTH, FrontKeys.KEY_SPACING, FrontKeys.KEY_CORNER, FrontKeys.LAYOUT_BY_FIELD,
                FrontKeys.HAPTIC, FrontKeys.SOUND, FrontKeys.THEME, FrontKeys.ACCENT, FrontKeys.CANDBAR_PASTE_BUTTON,
                FrontKeys.TOOLBAR_ITEMS, FrontKeys.LAYOUT_KANA, FrontKeys.LAYOUT_ALPHA,
            ),
            keys(section(plan(SettingsUiMode.DETAIL), "キーボード")),
        )
        assertEquals(
            listOf(
                FrontKeys.HEIGHT, FrontKeys.ONE_HANDED, FrontKeys.HAPTIC, FrontKeys.SOUND, FrontKeys.THEME,
                FrontKeys.ACCENT, FrontKeys.LAYOUT_KANA, FrontKeys.LAYOUT_ALPHA,
            ),
            keys(section(plan(SettingsUiMode.SIMPLE), "キーボード")),
        )
    }

    @Test
    fun クリップボード履歴グループの並びとシンプル詳細() {
        assertEquals(
            listOf(
                FrontKeys.CLIPBOARD_ENABLED, FrontKeys.CLIPBOARD_RETENTION, FrontKeys.CLIPBOARD_MAX_ITEMS,
                FrontKeys.CLIPBOARD_TYPES, FrontKeys.CLIPBOARD_PREVIEW_CHARS, FrontKeys.CLIPBOARD_CLEAR,
            ),
            keys(section(plan(SettingsUiMode.DETAIL), "クリップボード履歴")),
        )
        assertEquals(
            listOf(FrontKeys.CLIPBOARD_ENABLED, FrontKeys.CLIPBOARD_RETENTION, FrontKeys.CLIPBOARD_CLEAR),
            keys(section(plan(SettingsUiMode.SIMPLE), "クリップボード履歴")),
        )
        // 項目が無くても（コアが空でも）出る
        assertTrue("クリップボード履歴" in titles(SettingsPlan.build(emptyList(), SettingsUiMode.SIMPLE, false)))
    }

    @Test
    fun クリップボード履歴の既定値と選択肢はiosに揃っている() {
        fun d(key: String) = FrontSettings.definitions.first { it.key == key }
        assertFalse(d(FrontKeys.CLIPBOARD_ENABLED).defaultBool)
        assertEquals("24h", d(FrontKeys.CLIPBOARD_RETENTION).defaultChoice)
        assertEquals(listOf("1h", "24h", "7d", "30d", "forever"), d(FrontKeys.CLIPBOARD_RETENTION).choices.map { it.first })
        assertEquals("50", d(FrontKeys.CLIPBOARD_MAX_ITEMS).defaultChoice)
        assertEquals(listOf("10", "30", "50", "100", "200"), d(FrontKeys.CLIPBOARD_MAX_ITEMS).choices.map { it.first })
        assertEquals("40", d(FrontKeys.CLIPBOARD_PREVIEW_CHARS).defaultChoice)
        assertEquals(listOf("20", "40", "80", "120"), d(FrontKeys.CLIPBOARD_PREVIEW_CHARS).choices.map { it.first })
        assertEquals(listOf("text", "url", "number", "email"), d(FrontKeys.CLIPBOARD_TYPES).choices.map { it.first })
        assertTrue(d(FrontKeys.CANDBAR_PASTE_BUTTON).defaultBool)
        assertEquals("すべて", FrontSettings.defaultText(d(FrontKeys.CLIPBOARD_TYPES)))
        assertEquals(FrontSettings.CLIPBOARD_NOTE, section(plan(SettingsUiMode.DETAIL), "クリップボード履歴").footnote)
    }

    @Test
    fun 学習グループは項目が無くても削除ボタンのために出る() {
        for (mode in SettingsUiMode.values()) {
            val s = SettingsPlan.build(emptyList(), mode, false)
            assertEquals(listOf(FrontKeys.LEARNING_CLEAR), keys(section(s, "学習")))
        }
        // コアの学習の項目があるときは、削除ボタンは末尾
        val detail = plan(SettingsUiMode.DETAIL)
        assertEquals(listOf("learning.enabled", FrontKeys.LEARNING_CLEAR), keys(section(detail, "学習")))
    }

    @Test
    fun アップデートグループはフロントの自動確認とアップデート画面の入口() {
        for (mode in SettingsUiMode.values()) {
            assertEquals(
                listOf(FrontKeys.UPDATE_AUTO_CHECK, FrontKeys.UPDATE_OPEN),
                keys(section(plan(mode), "アップデート")),
            )
        }
        // コアの update.auto_check はどのモードでも出ない
        val coreRows = plan(SettingsUiMode.DETAIL).flatMap { it.entries }.filterIsInstance<SettingsEntry.Core>()
        assertTrue(coreRows.none { it.def.key == "update.auto_check" })
    }

    @Test
    fun 開発者グループ() {
        // シンプル + 開発者モードがオフ: 出ない
        assertFalse("開発者" in titles(plan(SettingsUiMode.SIMPLE, dev = false)))
        // シンプル + オン: 入口だけ
        assertEquals(listOf(FrontKeys.DEVELOPER_OPEN), keys(section(plan(SettingsUiMode.SIMPLE, dev = true), "開発者")))
        // 詳細 + オフ: スイッチだけ
        assertEquals(listOf(FrontKeys.DEVELOPER_MODE), keys(section(plan(SettingsUiMode.DETAIL, dev = false), "開発者")))
        // 詳細 + オン: スイッチ → 入口
        assertEquals(
            listOf(FrontKeys.DEVELOPER_MODE, FrontKeys.DEVELOPER_OPEN),
            keys(section(plan(SettingsUiMode.DETAIL, dev = true), "開発者")),
        )
    }

    @Test
    fun 反映のタイミングの注記は入力とキーボードに付く() {
        val detail = plan(SettingsUiMode.DETAIL)
        assertEquals(FrontSettings.RELOAD_NOTE, section(detail, "入力").footnote)
        assertEquals(FrontSettings.RELOAD_NOTE, section(detail, "キーボード").footnote)
        assertEquals(null, section(detail, "変換").footnote)
        assertEquals(null, section(detail, "学習").footnote)
    }

    // ---- 定義 ----

    @Test
    fun 定義のキーは重複しない() {
        val keys = FrontSettings.definitions.map { it.key }
        assertEquals(keys.size, keys.toSet().size)
    }

    @Test
    fun 既定値と範囲はiosに揃っている() {
        val d = KeyboardDefaults
        assertEquals(450, d.KEY_REPEAT_DELAY_MS)
        assertEquals(110, d.KEY_REPEAT_MS)
        assertEquals(6f, d.KEY_SPACING_DP, 0f)
        assertEquals(14f, d.SPACE_SWIPE_STEP_DP, 0f)
        assertTrue(d.KEY_SOUND)
        assertEquals(0.8f, d.HEIGHT_SCALE_MIN, 0f)
        assertEquals(1.3f, d.HEIGHT_SCALE_MAX, 0f)
        assertEquals(1.0f, d.HEIGHT_SCALE, 0f)
        // 定義にも同じ値が入っている
        fun slider(key: String) = FrontSettings.definitions.first { it.key == key }
        assertEquals(450f, slider(FrontKeys.KEY_REPEAT_DELAY).defaultFloat, 0f)
        assertEquals(110f, slider(FrontKeys.KEY_REPEAT).defaultFloat, 0f)
        assertEquals(6f, slider(FrontKeys.KEY_SPACING).defaultFloat, 0f)
        assertEquals(14f, slider(FrontKeys.SPACE_SWIPE_STEP).defaultFloat, 0f)
        assertFalse(d.SPACE_SWIPE_ACCEL)
        assertFalse(FrontSettings.definitions.first { it.key == FrontKeys.SPACE_SWIPE_ACCEL }.defaultBool)
        assertEquals(1.0f, slider(FrontKeys.SPACE_SWIPE_ACCEL_STRENGTH).defaultFloat, 0f)
        assertEquals(0.5f, slider(FrontKeys.SPACE_SWIPE_ACCEL_STRENGTH).min, 0f)
        assertEquals(3.0f, slider(FrontKeys.SPACE_SWIPE_ACCEL_STRENGTH).max, 0f)
        assertEquals(0.8f, slider(FrontKeys.HEIGHT_SCALE).min, 0f)
        assertEquals(1.3f, slider(FrontKeys.HEIGHT_SCALE).max, 0f)
        assertEquals(0.05f, slider(FrontKeys.HEIGHT_SCALE).step, 0f)
        assertEquals(0.5f, slider(FrontKeys.FLICK_SENSITIVITY).min, 0f)
        assertEquals(2.0f, slider(FrontKeys.FLICK_SENSITIVITY).max, 0f)
        assertEquals(4f, slider(FrontKeys.FLICK_THRESHOLD).min, 0f)
        assertEquals(40f, slider(FrontKeys.FLICK_THRESHOLD).max, 0f)
        assertEquals(150f, slider(FrontKeys.FLICK_LONG_PRESS).min, 0f)
        assertEquals(1000f, slider(FrontKeys.FLICK_LONG_PRESS).max, 0f)
        assertEquals(350f, slider(FrontKeys.FLICK_LONG_PRESS).defaultFloat, 0f)
        assertEquals(0.6f, slider(FrontKeys.ONE_HANDED_WIDTH).min, 0f)
        assertEquals(0.9f, slider(FrontKeys.ONE_HANDED_WIDTH).max, 0f)
        assertEquals(0f, slider(FrontKeys.KEY_SPACING).min, 0f)
        assertEquals(8f, slider(FrontKeys.KEY_SPACING).max, 0f)
        assertEquals(16f, slider(FrontKeys.KEY_CORNER).max, 0f)
    }

    @Test
    fun スライダーの既定値は範囲の中で目盛りに乗る() {
        for (def in FrontSettings.definitions.filter { it.kind == FrontKind.SLIDER }) {
            assertTrue(def.key, def.defaultFloat in def.min..def.max)
            assertTrue(def.key, def.step > 0f)
            val p = FrontSettings.progressOf(def, def.defaultFloat)
            assertEquals(def.key, def.defaultFloat, FrontSettings.valueAt(def, p), 1e-4f)
            assertEquals(def.key, def.min, FrontSettings.valueAt(def, 0), 0f)
            assertEquals(def.key, def.max, FrontSettings.valueAt(def, FrontSettings.steps(def)), 1e-4f)
        }
    }

    @Test
    fun 既定値のスイッチと選択は存在する選択肢() {
        for (def in FrontSettings.definitions.filter { it.kind == FrontKind.CHOICE || it.kind == FrontKind.HEIGHT_PRESET }) {
            assertTrue(def.key, def.choices.any { it.first == def.defaultChoice })
        }
        fun d(key: String) = FrontSettings.definitions.first { it.key == key }
        assertTrue(d(FrontKeys.COMPOSE_NUMBERS).defaultBool)
        assertTrue(d(FrontKeys.KEEP_PREEDIT_ON_SYMBOLS).defaultBool)
        assertFalse(d(FrontKeys.KEEP_PREEDIT_ON_ENGLISH).defaultBool)
        assertTrue(d(FrontKeys.FLICK_HINTS).defaultBool)
        assertTrue(d(FrontKeys.LAYOUT_BY_FIELD).defaultBool)
        assertEquals("flick-kana", d(FrontKeys.LAYOUT_KANA).defaultChoice)
        assertEquals("qwerty-alpha", d(FrontKeys.LAYOUT_ALPHA).defaultChoice)
    }

    @Test
    fun キー入力の音は選択式で既定はクリック音あり() {
        val def = FrontSettings.definitions.first { it.key == FrontKeys.SOUND }
        assertEquals(FrontKind.CHOICE, def.kind)
        assertEquals(listOf("off" to "オフ", "compat" to "クリック音"), def.choices)
        assertEquals("compat", def.defaultChoice)
        // 保存は従来の boolean
        assertEquals("compat", FrontSettings.soundValue(true))
        assertEquals("off", FrontSettings.soundValue(false))
        assertTrue(FrontSettings.soundEnabled("compat"))
        assertFalse(FrontSettings.soundEnabled("off"))
        assertEquals(FrontSettings.soundValue(KeyboardDefaults.KEY_SOUND), def.defaultChoice)
        assertEquals("クリック音", FrontSettings.defaultText(def))
    }

    @Test
    fun 高さのプリセットは低め_標準_高めで倍率はandroidの値() {
        val def = FrontSettings.definitions.first { it.key == FrontKeys.HEIGHT }
        assertEquals(FrontKind.HEIGHT_PRESET, def.kind)
        assertEquals(listOf("compact", "standard", "tall"), def.choices.map { it.first })
        assertEquals(listOf("低め", "標準", "高め"), def.choices.map { it.second.substringBefore('（') })
        assertEquals(listOf(0.9f, 1.0f, 1.1f), HeightPreset.values().map { it.scale })
        assertEquals("standard", def.defaultChoice)
    }

    @Test
    fun ラベルはiosの文言と単位の規則に揃っている() {
        fun label(key: String) = FrontSettings.definitions.first { it.key == key }.label
        assertEquals("フリック感度", label(FrontKeys.FLICK_SENSITIVITY))
        assertEquals("フリックの案内を表示", label(FrontKeys.FLICK_POPUP))
        assertEquals("フリックの判定距離（pt）", label(FrontKeys.FLICK_THRESHOLD))
        assertEquals("⌫の連打を始めるまで（ミリ秒）", label(FrontKeys.KEY_REPEAT_DELAY))
        assertEquals("⌫の連打の間隔（ミリ秒）", label(FrontKeys.KEY_REPEAT))
        assertEquals("カーソル移動 1 文字あたりの距離（pt）", label(FrontKeys.SPACE_SWIPE_STEP))
        assertEquals("⌫の左スワイプで単語を削除", label(FrontKeys.BACKSPACE_SWIPE))
        assertEquals("キー入力の触覚", label(FrontKeys.HAPTIC))
        assertEquals("キー入力の音", label(FrontKeys.SOUND))
        assertEquals("キーの間隔（pt）", label(FrontKeys.KEY_SPACING))
        assertEquals("キーの角丸（pt）", label(FrontKeys.KEY_CORNER))
        assertEquals("キーボードの高さ", label(FrontKeys.HEIGHT))
        assertEquals("高さの倍率", label(FrontKeys.HEIGHT_SCALE))
        assertEquals("入力欄の種類でレイアウトを切り替える", label(FrontKeys.LAYOUT_BY_FIELD))
        assertEquals("かなモードのレイアウト", label(FrontKeys.LAYOUT_KANA))
        assertEquals("英字モードのレイアウト", label(FrontKeys.LAYOUT_ALPHA))
        assertEquals("アップデートを自動で確認", label(FrontKeys.UPDATE_AUTO_CHECK))
        assertEquals("学習データをすべて削除", label(FrontKeys.LEARNING_CLEAR))
        assertEquals("開発者モード", label(FrontKeys.DEVELOPER_MODE))
        val theme = FrontSettings.definitions.first { it.key == FrontKeys.THEME }
        assertEquals(listOf("システムに合わせる", "ライト", "ダーク"), theme.choices.map { it.second })
        // 単位の規則: 距離は（pt）、時間は（ミリ秒）。dp / ms の表記は使わない
        for (def in FrontSettings.definitions) {
            assertFalse(def.label, def.label.contains("dp") || def.label.contains(" ms"))
            assertTrue(def.key, def.label.isNotBlank())
            if (def.key.endsWith("_pt")) assertTrue(def.label, def.label.endsWith("（pt）"))
            if (def.key.endsWith("_ms")) assertTrue(def.label, def.label.endsWith("（ミリ秒）"))
        }
    }

    @Test
    fun スライダーの値の表示() {
        assertEquals("14", FrontSettings.format(FrontUnit.INT, 14f))
        assertEquals("6.0", FrontSettings.format(FrontUnit.DEC1, 6f))
        assertEquals("×1.0", FrontSettings.format(FrontUnit.TIMES1, 1f))
        assertEquals("×0.85", FrontSettings.format(FrontUnit.TIMES2, 0.85f))
    }

    @Test
    fun 既定値の表示() {
        fun text(key: String) = FrontSettings.defaultText(FrontSettings.definitions.first { it.key == key })
        assertEquals("オン", text(FrontKeys.FLICK_POPUP))
        assertEquals("オフ", text(FrontKeys.BACKSPACE_SWIPE))
        assertEquals("450", text(FrontKeys.KEY_REPEAT_DELAY))
        assertEquals("標準（×1.0）", text(FrontKeys.HEIGHT))
        assertEquals("オフ", text(FrontKeys.ONE_HANDED))
        assertEquals("弱", text(FrontKeys.HAPTIC))
        assertEquals("システムに合わせる", text(FrontKeys.THEME))
        assertEquals("flick-kana", text(FrontKeys.LAYOUT_KANA))
    }

    @Test
    fun 定義の説明が空でない() {
        for (def in FrontSettings.definitions) assertNotNull(def.key, def.description)
    }
}
