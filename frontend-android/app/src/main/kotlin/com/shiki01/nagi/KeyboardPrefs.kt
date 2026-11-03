package com.shiki01.nagi

import android.content.Context
import android.content.SharedPreferences

/**
 * 入力の操作感の設定の既定値と範囲（1 か所にまとめる。設定画面の「既定に戻す」・[KeyboardPrefs] の読み出しが共通で使う）。
 * iOS の実装（`IOSSettings.swift`）の同名の項目に揃えた値（Android 固有の違いは各行のコメント）。
 */
internal object KeyboardDefaults {
    // 入力
    const val TOGGLE_INPUT = false
    const val FLICK_THRESHOLD_DP = 14f
    const val FLICK_THRESHOLD_MIN = 4f
    const val FLICK_THRESHOLD_MAX = 40f
    const val FLICK_SENSITIVITY = 1.0f
    const val FLICK_SENSITIVITY_MIN = 0.5f
    const val FLICK_SENSITIVITY_MAX = 2.0f
    const val FLICK_POPUP = true
    const val FLICK_HINTS = true

    // 数字・記号・英字の入力中の扱い（iOS の input.compose_numbers / input.keep_preedit_on_* に揃える）
    const val COMPOSE_NUMBERS = true
    const val KEEP_PREEDIT_ON_SYMBOLS = true
    const val KEEP_PREEDIT_ON_ENGLISH = false
    const val FLICK_LONG_PRESS_MS = 350
    const val FLICK_LONG_PRESS_MIN = 150
    const val FLICK_LONG_PRESS_MAX = 1000

    // キー操作
    const val SPACE_SWIPE = true
    const val SPACE_SWIPE_STEP_DP = 14f
    const val SPACE_SWIPE_STEP_MIN = 4f
    const val SPACE_SWIPE_STEP_MAX = 40f
    const val SPACE_SWIPE_ACCEL = false
    const val SPACE_SWIPE_ACCEL_STRENGTH = 1.0f
    const val SPACE_SWIPE_ACCEL_STRENGTH_MIN = 0.5f
    const val SPACE_SWIPE_ACCEL_STRENGTH_MAX = 3.0f
    const val BACKSPACE_SWIPE = false
    const val KEY_REPEAT_DELAY_MS = 450
    const val KEY_REPEAT_DELAY_MIN = 200
    const val KEY_REPEAT_DELAY_MAX = 1000

    const val KEY_REPEAT_MS = 110
    const val KEY_REPEAT_MIN = 30
    const val KEY_REPEAT_MAX = 300

    // 見た目
    const val THEME = "system"
    const val ACCENT = "blue"
    const val KEY_SPACING_DP = 6f
    const val KEY_SPACING_MIN = 0f
    const val KEY_SPACING_MAX = 8f
    const val KEY_CORNER_DP = 6f
    const val KEY_CORNER_MIN = 0f
    const val KEY_CORNER_MAX = 16f

    // フィードバック
    const val HAPTIC_LEVEL = "light"
    /** iOS の `keyboard.sound` の既定（クリック音あり）に揃える。保存は従来どおり boolean */
    const val KEY_SOUND = true

    // アップデート
    const val AUTO_CHECK_UPDATES = true

    // 片手・高さ
    const val ONE_HANDED = "off"
    const val ONE_HANDED_WIDTH = 0.8f
    const val HEIGHT_SCALE = 1.0f
    const val HEIGHT_SCALE_MIN = 0.8f
    const val HEIGHT_SCALE_MAX = 1.3f
    const val HEIGHT_SCALE_LANDSCAPE = 1.0f
    const val HEIGHT_SCALE_LANDSCAPE_MIN = 0.7f
    const val HEIGHT_SCALE_LANDSCAPE_MAX = 1.4f
}

/** [KeyboardPrefs] の保存キー（SharedPreferences の名前）。設定画面の「既定に戻す」もこの名前で消す */
internal object PrefKeys {
    const val FLICK_LONG_PRESS_MS = "flick_long_press_ms"
    const val FLICK_HINTS = "flick_hints"
    const val COMPOSE_NUMBERS = "compose_numbers"
    const val KEEP_PREEDIT_ON_SYMBOLS = "keep_preedit_on_symbols"
    const val KEEP_PREEDIT_ON_ENGLISH = "keep_preedit_on_english"
    const val TOGGLE_INPUT = "toggle_input"
    const val FLICK_THRESHOLD_DP = "flick_threshold_dp"
    const val FLICK_POPUP = "flick_popup"
    const val HAPTIC_LEVEL = "haptic_level"
    const val HAPTIC = "haptic"
    const val HEIGHT_SCALE_LANDSCAPE = "height_scale_landscape"
    const val HEIGHT_SCALE = "height_scale"
    const val THEME = "theme"
    const val KEY_SOUND = "key_sound"
    const val FLICK_SENSITIVITY = "flick_sensitivity"
    const val SPACE_SWIPE_STEP_DP = "space_swipe_step_dp"
    const val SPACE_SWIPE = "space_swipe"
    const val SPACE_SWIPE_ACCEL = "space_swipe_accel"
    const val SPACE_SWIPE_ACCEL_STRENGTH = "space_swipe_accel_strength"
    const val BACKSPACE_SWIPE = "backspace_swipe"
    const val KEY_REPEAT_DELAY_MS = "key_repeat_delay_ms"
    const val KEY_REPEAT_MS = "key_repeat_ms"
    const val ONE_HANDED_WIDTH = "one_handed_width"
    const val ONE_HANDED = "one_handed"
    const val KEY_SPACING_DP = "key_spacing_dp"
    const val KEY_CORNER_DP = "key_corner_dp"
    const val ACCENT = "accent"
    const val AUTO_CHECK_UPDATES = "auto_check_updates"
    const val LAYOUT_BY_FIELD = "layout_by_field"

    // ツールバー・クリップボード履歴（iOS の toolbar.items / candbar.paste_button / clipboard.history.*）
    const val TOOLBAR_ITEMS = "toolbar_items"
    const val PASTE_BUTTON = "paste_button"
    const val CLIPBOARD_ENABLED = "clipboard_history_enabled"
    const val CLIPBOARD_RETENTION = "clipboard_history_retention"
    const val CLIPBOARD_MAX_ITEMS = "clipboard_history_max_items"
    const val CLIPBOARD_TYPES = "clipboard_history_types"
    const val CLIPBOARD_PREVIEW_CHARS = "clipboard_history_preview_chars"

    /** 最後に取り込んだクリップボードの時刻印（同じ内容を読み直さないため。設定ではないので「既定に戻す」の対象外） */
    const val CLIPBOARD_LAST_STAMP = "clipboard_last_stamp"
}

/**
 * 設定画面の 1 項目ぶんの保存キー。「既定に戻す」で [KeyboardPrefs.reset] に渡す。
 * 触覚は旧来の on / off のキーも一緒に消さないと、読み替えで「オフ」に戻ってしまう
 */
internal object ResetKeys {
    val TOGGLE_INPUT = listOf(PrefKeys.TOGGLE_INPUT)
    val FLICK_THRESHOLD = listOf(PrefKeys.FLICK_THRESHOLD_DP)
    val FLICK_SENSITIVITY = listOf(PrefKeys.FLICK_SENSITIVITY)
    val FLICK_POPUP = listOf(PrefKeys.FLICK_POPUP)
    val FLICK_HINTS = listOf(PrefKeys.FLICK_HINTS)
    val COMPOSE_NUMBERS = listOf(PrefKeys.COMPOSE_NUMBERS)
    val KEEP_PREEDIT_ON_SYMBOLS = listOf(PrefKeys.KEEP_PREEDIT_ON_SYMBOLS)
    val KEEP_PREEDIT_ON_ENGLISH = listOf(PrefKeys.KEEP_PREEDIT_ON_ENGLISH)
    val FLICK_LONG_PRESS = listOf(PrefKeys.FLICK_LONG_PRESS_MS)
    val SPACE_SWIPE = listOf(PrefKeys.SPACE_SWIPE)
    val SPACE_SWIPE_STEP = listOf(PrefKeys.SPACE_SWIPE_STEP_DP)
    val SPACE_SWIPE_ACCEL = listOf(PrefKeys.SPACE_SWIPE_ACCEL)
    val SPACE_SWIPE_ACCEL_STRENGTH = listOf(PrefKeys.SPACE_SWIPE_ACCEL_STRENGTH)
    val BACKSPACE_SWIPE = listOf(PrefKeys.BACKSPACE_SWIPE)
    val KEY_REPEAT_DELAY = listOf(PrefKeys.KEY_REPEAT_DELAY_MS)
    val KEY_REPEAT = listOf(PrefKeys.KEY_REPEAT_MS)
    val THEME = listOf(PrefKeys.THEME)
    val ACCENT = listOf(PrefKeys.ACCENT)
    val KEY_SPACING = listOf(PrefKeys.KEY_SPACING_DP)
    val KEY_CORNER = listOf(PrefKeys.KEY_CORNER_DP)
    val HAPTIC = listOf(PrefKeys.HAPTIC_LEVEL, PrefKeys.HAPTIC)
    val KEY_SOUND = listOf(PrefKeys.KEY_SOUND)
    val ONE_HANDED = listOf(PrefKeys.ONE_HANDED)
    val ONE_HANDED_WIDTH = listOf(PrefKeys.ONE_HANDED_WIDTH)
    val HEIGHT_SCALE = listOf(PrefKeys.HEIGHT_SCALE)
    val HEIGHT_SCALE_LANDSCAPE = listOf(PrefKeys.HEIGHT_SCALE_LANDSCAPE)
    val AUTO_CHECK_UPDATES = listOf(PrefKeys.AUTO_CHECK_UPDATES)
    val LAYOUT_BY_FIELD = listOf(PrefKeys.LAYOUT_BY_FIELD)
    val TOOLBAR_ITEMS = listOf(PrefKeys.TOOLBAR_ITEMS)
    val PASTE_BUTTON = listOf(PrefKeys.PASTE_BUTTON)
    val CLIPBOARD_ENABLED = listOf(PrefKeys.CLIPBOARD_ENABLED)
    val CLIPBOARD_RETENTION = listOf(PrefKeys.CLIPBOARD_RETENTION)
    val CLIPBOARD_MAX_ITEMS = listOf(PrefKeys.CLIPBOARD_MAX_ITEMS)
    val CLIPBOARD_TYPES = listOf(PrefKeys.CLIPBOARD_TYPES)
    val CLIPBOARD_PREVIEW_CHARS = listOf(PrefKeys.CLIPBOARD_PREVIEW_CHARS)

    /** 設定画面で扱う設定すべて（「すべて既定に戻す」）。かな入力のレイアウトの割り当ては別（[KeyboardPrefs.setKanaLayoutId]） */
    val ALL: List<String> = (
        TOGGLE_INPUT + FLICK_THRESHOLD + FLICK_SENSITIVITY + FLICK_POPUP +
            SPACE_SWIPE + SPACE_SWIPE_STEP + SPACE_SWIPE_ACCEL + SPACE_SWIPE_ACCEL_STRENGTH + BACKSPACE_SWIPE + KEY_REPEAT_DELAY + KEY_REPEAT + THEME + ACCENT +
            KEY_SPACING + KEY_CORNER + HAPTIC + KEY_SOUND + ONE_HANDED + ONE_HANDED_WIDTH + HEIGHT_SCALE +
            HEIGHT_SCALE_LANDSCAPE + AUTO_CHECK_UPDATES + COMPOSE_NUMBERS + KEEP_PREEDIT_ON_SYMBOLS +
            KEEP_PREEDIT_ON_ENGLISH + FLICK_HINTS + FLICK_LONG_PRESS + LAYOUT_BY_FIELD + TOOLBAR_ITEMS + PASTE_BUTTON +
            CLIPBOARD_ENABLED + CLIPBOARD_RETENTION + CLIPBOARD_MAX_ITEMS + CLIPBOARD_TYPES + CLIPBOARD_PREVIEW_CHARS
        ).distinct()
}

/**
 * Android フロント固有の設定（コアの `config.toml` に無いもの。iOS の `ios-settings.json` に相当）。
 * キーボード（入力側）が読み、設定画面が書く。値の範囲は読むときに丸める。
 */
internal object KeyboardPrefs {
    private const val NAME = "nagi_android"

    private fun sp(ctx: Context): SharedPreferences = ctx.getSharedPreferences(NAME, Context.MODE_PRIVATE)

    /**
     * 日本語入力のレイアウト（旧来の値）: "flick"（12 キー）/ "qwerty"（ローマ字）。
     * かな用のレイアウト ID（[kanaLayoutId]）から読み替える。ID が QWERTY 系なら "qwerty"、それ以外は "flick"
     */
    fun layout(ctx: Context): String = if (kanaLayoutId(ctx).startsWith("qwerty")) "qwerty" else "flick"

    /** 旧来の値で設定する。かな用のレイアウト ID も `flick-kana` / `qwerty-kana` に合わせる */
    fun setLayout(ctx: Context, v: String) {
        sp(ctx).edit()
            .putString("layout", v)
            .putString("layout_kana", if (v == "qwerty") "qwerty-kana" else LayoutIds.DEFAULT_KANA)
            .apply()
    }

    /** かな入力で使うレイアウトの ID。未設定なら旧来の "layout" の値から（flick-kana / qwerty-kana） */
    fun kanaLayoutId(ctx: Context): String {
        val v = sp(ctx).getString("layout_kana", null)
        if (v != null && LayoutIds.isValid(v)) return v
        return if (sp(ctx).getString("layout", "flick") == "qwerty") "qwerty-kana" else LayoutIds.DEFAULT_KANA
    }

    /** 英字入力で使うレイアウトの ID。未設定なら `qwerty-alpha` */
    fun alphaLayoutId(ctx: Context): String {
        val v = sp(ctx).getString("layout_alpha", null)
        return if (v != null && LayoutIds.isValid(v)) v else LayoutIds.DEFAULT_ALPHA
    }

    /** かな用のレイアウト ID を設定する（null で既定に戻す） */
    fun setKanaLayoutId(ctx: Context, id: String?) {
        val e = sp(ctx).edit()
        if (id == null) e.remove("layout_kana").remove("layout") else e.putString("layout_kana", id)
        e.apply()
    }

    /** 英字用のレイアウト ID を設定する（null で既定に戻す） */
    fun setAlphaLayoutId(ctx: Context, id: String?) {
        val e = sp(ctx).edit()
        if (id == null) e.remove("layout_alpha") else e.putString("layout_alpha", id)
        e.apply()
    }

    /** 入力欄の種類ごとに利用者が割り当てたレイアウト（`kana` / `alpha` / ID / "none"）。未設定は null（既定） */
    fun fieldLayoutId(ctx: Context, kind: FieldKind): String? {
        val v = sp(ctx).getString("layout_field_${kind.key}", null) ?: return null
        return if (v == LayoutSelect.FIELD_NONE || v == LayoutSelect.TARGET_KANA || v == LayoutSelect.TARGET_ALPHA ||
            LayoutIds.isValid(v)
        ) {
            v
        } else {
            null
        }
    }

    /** 入力欄の種類ごとのレイアウトを設定する（null で既定に戻す） */
    fun setFieldLayoutId(ctx: Context, kind: FieldKind, id: String?) {
        val e = sp(ctx).edit()
        if (id == null) e.remove("layout_field_${kind.key}") else e.putString("layout_field_${kind.key}", id)
        e.apply()
    }

    /** 利用者が割り当てた入力欄別のレイアウト（未設定の種類は含まない） */
    fun fieldAssignments(ctx: Context): Map<FieldKind, String> {
        val out = HashMap<FieldKind, String>()
        for (k in FieldKind.values()) fieldLayoutId(ctx, k)?.let { out[k] = it }
        return out
    }

    /** 入力欄の種類（メール・URL・数字など）に合わせてレイアウトを自動で選び、改行キーの文字を変える */
    fun layoutByField(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.LAYOUT_BY_FIELD, true)

    fun setLayoutByField(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.LAYOUT_BY_FIELD, v).apply()

    /** ツールバーの項目と並び順（`toolbar.items`）。未設定・空・読めない値は既定 */
    fun toolbarItems(ctx: Context): List<ToolbarItem> = ToolbarLogic.parse(sp(ctx).getString(PrefKeys.TOOLBAR_ITEMS, null))

    fun setToolbarItems(ctx: Context, items: List<ToolbarItem>) =
        sp(ctx).edit().putString(PrefKeys.TOOLBAR_ITEMS, ToolbarLogic.encode(items)).apply()

    /** ツールバーに「貼り付け」を出す（`candbar.paste_button`）。オフなら項目に入れていても出さない */
    fun pasteButton(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.PASTE_BUTTON, true)

    fun setPasteButton(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.PASTE_BUTTON, v).apply()

    fun clipboardEnabled(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.CLIPBOARD_ENABLED, ClipboardDefaults.ENABLED)

    fun setClipboardEnabled(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.CLIPBOARD_ENABLED, v).apply()

    /** 保存期間の選択肢の値（1h / 24h / 7d / 30d / forever） */
    fun clipboardRetention(ctx: Context): String {
        val v = sp(ctx).getString(PrefKeys.CLIPBOARD_RETENTION, null)
        return if (ClipboardDefaults.RETENTION_CHOICES.any { it.first == v }) v!! else ClipboardDefaults.RETENTION
    }

    fun setClipboardRetention(ctx: Context, v: String) = sp(ctx).edit().putString(PrefKeys.CLIPBOARD_RETENTION, v).apply()

    /** 保存する最大数の選択肢の値（10 / 30 / 50 / 100 / 200） */
    fun clipboardMaxItems(ctx: Context): String =
        ClipboardHistoryLogic.maxItems(sp(ctx).getString(PrefKeys.CLIPBOARD_MAX_ITEMS, null)).toString()

    fun setClipboardMaxItems(ctx: Context, v: String) = sp(ctx).edit().putString(PrefKeys.CLIPBOARD_MAX_ITEMS, v).apply()

    /** カードに表示する文字数の選択肢の値（20 / 40 / 80 / 120） */
    fun clipboardPreviewChars(ctx: Context): String {
        val v = sp(ctx).getString(PrefKeys.CLIPBOARD_PREVIEW_CHARS, null)
        return if (ClipboardDefaults.PREVIEW_CHOICES.any { it.first == v }) v!! else ClipboardDefaults.PREVIEW_CHARS.toString()
    }

    fun setClipboardPreviewChars(ctx: Context, v: String) =
        sp(ctx).edit().putString(PrefKeys.CLIPBOARD_PREVIEW_CHARS, v).apply()

    /** 保存する種類。未設定は全種類 */
    fun clipboardKinds(ctx: Context): Set<ClipboardKind> =
        ClipboardHistoryLogic.parseKinds(sp(ctx).getString(PrefKeys.CLIPBOARD_TYPES, null))

    fun setClipboardKinds(ctx: Context, v: Set<ClipboardKind>) =
        sp(ctx).edit().putString(PrefKeys.CLIPBOARD_TYPES, ClipboardHistoryLogic.encodeKinds(v)).apply()

    /** 履歴の設定をまとめたもの */
    fun clipboardConfig(ctx: Context): ClipboardConfig = ClipboardConfig(
        enabled = clipboardEnabled(ctx),
        retentionMs = ClipboardHistoryLogic.retentionMs(clipboardRetention(ctx)),
        maxItems = ClipboardHistoryLogic.maxItems(clipboardMaxItems(ctx)),
        kinds = clipboardKinds(ctx),
        previewChars = ClipboardHistoryLogic.previewChars(clipboardPreviewChars(ctx)),
    )

    /** 最後に取り込んだクリップボードの時刻印（0 は未記録） */
    fun clipboardLastStamp(ctx: Context): Long = sp(ctx).getLong(PrefKeys.CLIPBOARD_LAST_STAMP, 0L)

    fun setClipboardLastStamp(ctx: Context, v: Long) = sp(ctx).edit().putLong(PrefKeys.CLIPBOARD_LAST_STAMP, v).apply()

    /** 長押しと判定する時間（ミリ秒、150〜1000）。iOS の `flick.long_press_ms` */
    fun flickLongPressMs(ctx: Context): Int = sp(ctx).getInt(PrefKeys.FLICK_LONG_PRESS_MS, KeyboardDefaults.FLICK_LONG_PRESS_MS)
        .coerceIn(KeyboardDefaults.FLICK_LONG_PRESS_MIN, KeyboardDefaults.FLICK_LONG_PRESS_MAX)

    fun setFlickLongPressMs(ctx: Context, v: Int) = sp(ctx).edit().putInt(PrefKeys.FLICK_LONG_PRESS_MS, v).apply()

    /** フリック先の文字を、キーの角に小さく表示する。iOS の `flick.hints` */
    fun flickHints(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.FLICK_HINTS, KeyboardDefaults.FLICK_HINTS)

    fun setFlickHints(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.FLICK_HINTS, v).apply()

    /** 数字・記号のキーも入力中の文字列（コアの composing）に入れる。iOS の `input.compose_numbers`（フロント固有の設定） */
    fun composeNumbers(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.COMPOSE_NUMBERS, KeyboardDefaults.COMPOSE_NUMBERS)

    fun setComposeNumbers(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.COMPOSE_NUMBERS, v).apply()

    /** かな ⇄ 数字・記号の切替で入力中の文字列を確定しない。iOS の `input.keep_preedit_on_symbols` */
    fun keepPreeditOnSymbols(ctx: Context): Boolean =
        sp(ctx).getBoolean(PrefKeys.KEEP_PREEDIT_ON_SYMBOLS, KeyboardDefaults.KEEP_PREEDIT_ON_SYMBOLS)

    fun setKeepPreeditOnSymbols(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.KEEP_PREEDIT_ON_SYMBOLS, v).apply()

    /** かな ⇄ 英字の切替で入力中の文字列を確定しない。iOS の `input.keep_preedit_on_english` */
    fun keepPreeditOnEnglish(ctx: Context): Boolean =
        sp(ctx).getBoolean(PrefKeys.KEEP_PREEDIT_ON_ENGLISH, KeyboardDefaults.KEEP_PREEDIT_ON_ENGLISH)

    fun setKeepPreeditOnEnglish(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.KEEP_PREEDIT_ON_ENGLISH, v).apply()

    /** レイアウトの保存・削除・割り当てのたびに増える番号（キーボードが組み立て直すかの判定とキャッシュ破棄に使う） */
    fun layoutsRevision(ctx: Context): Long = sp(ctx).getLong("layouts_rev", 0L)

    fun bumpLayoutsRevision(ctx: Context) {
        val p = sp(ctx)
        p.edit().putLong("layouts_rev", p.getLong("layouts_rev", 0L) + 1L).apply()
    }

    /** 12 キーでキーを押したときトグル入力（あ→い→う…）にする（false なら tap = 行の先頭） */
    fun toggleInput(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.TOGGLE_INPUT, KeyboardDefaults.TOGGLE_INPUT)

    fun setToggleInput(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.TOGGLE_INPUT, v).apply()

    /** フリックとみなす移動量（dp）。小さいほど敏感 */
    fun flickThresholdDp(ctx: Context): Float = sp(ctx).getFloat(PrefKeys.FLICK_THRESHOLD_DP, KeyboardDefaults.FLICK_THRESHOLD_DP)
        .coerceIn(KeyboardDefaults.FLICK_THRESHOLD_MIN, KeyboardDefaults.FLICK_THRESHOLD_MAX)

    fun setFlickThresholdDp(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.FLICK_THRESHOLD_DP, v).apply()

    /** フリックの押下ポップアップ（中央と上下左右の 5 文字）を出す */
    fun flickPopup(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.FLICK_POPUP, KeyboardDefaults.FLICK_POPUP)

    fun setFlickPopup(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.FLICK_POPUP, v).apply()

    /**
     * キー押下時の触覚の強さ。未設定なら旧来の on / off（"haptic"）から読み替える（on = 弱、off = オフ）
     */
    fun hapticLevel(ctx: Context): HapticLevel {
        HapticLevel.fromJson(sp(ctx).getString(PrefKeys.HAPTIC_LEVEL, null))?.let { return it }
        return if (sp(ctx).getBoolean(PrefKeys.HAPTIC, true)) HapticLevel.LIGHT else HapticLevel.OFF
    }

    fun setHapticLevel(ctx: Context, v: HapticLevel) =
        sp(ctx).edit().putString(PrefKeys.HAPTIC_LEVEL, v.json).putBoolean(PrefKeys.HAPTIC, v != HapticLevel.OFF).apply()

    /** 触覚が有効か（旧来の呼び方。強さが「オフ」以外） */
    fun haptic(ctx: Context): Boolean = hapticLevel(ctx) != HapticLevel.OFF

    /** キー入力のクリック音（端末の「タッチ操作音」の設定に従う） */
    fun keySound(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.KEY_SOUND, KeyboardDefaults.KEY_SOUND)

    fun setKeySound(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.KEY_SOUND, v).apply()

    /** フリック感度の倍率（0.5〜2.0）。判定距離 = しきい値 ÷ 倍率 */
    fun flickSensitivity(ctx: Context): Float = sp(ctx).getFloat(PrefKeys.FLICK_SENSITIVITY, KeyboardDefaults.FLICK_SENSITIVITY)
        .coerceIn(KeyboardDefaults.FLICK_SENSITIVITY_MIN, KeyboardDefaults.FLICK_SENSITIVITY_MAX)

    fun setFlickSensitivity(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.FLICK_SENSITIVITY, v).apply()

    /** スペースキーの横スワイプでカーソルを動かす */
    fun spaceSwipe(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.SPACE_SWIPE, KeyboardDefaults.SPACE_SWIPE)

    fun setSpaceSwipe(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.SPACE_SWIPE, v).apply()

    /** スペースのスワイプで 1 文字動かす距離（dp、4〜40） */
    fun spaceSwipeStepDp(ctx: Context): Float = sp(ctx).getFloat(PrefKeys.SPACE_SWIPE_STEP_DP, KeyboardDefaults.SPACE_SWIPE_STEP_DP)
        .coerceIn(KeyboardDefaults.SPACE_SWIPE_STEP_MIN, KeyboardDefaults.SPACE_SWIPE_STEP_MAX)

    fun setSpaceSwipeStepDp(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.SPACE_SWIPE_STEP_DP, v).apply()

    /** スペースのスワイプで、開始点から離れるほどカーソルを速く動かす（`touch.space_swipe_accel`） */
    fun spaceSwipeAccel(ctx: Context): Boolean =
        sp(ctx).getBoolean(PrefKeys.SPACE_SWIPE_ACCEL, KeyboardDefaults.SPACE_SWIPE_ACCEL)

    fun setSpaceSwipeAccel(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.SPACE_SWIPE_ACCEL, v).apply()

    /** 加速の強さ（0.5〜3.0。`touch.space_swipe_accel_strength`） */
    fun spaceSwipeAccelStrength(ctx: Context): Float =
        sp(ctx).getFloat(PrefKeys.SPACE_SWIPE_ACCEL_STRENGTH, KeyboardDefaults.SPACE_SWIPE_ACCEL_STRENGTH)
            .coerceIn(KeyboardDefaults.SPACE_SWIPE_ACCEL_STRENGTH_MIN, KeyboardDefaults.SPACE_SWIPE_ACCEL_STRENGTH_MAX)

    fun setSpaceSwipeAccelStrength(ctx: Context, v: Float) =
        sp(ctx).edit().putFloat(PrefKeys.SPACE_SWIPE_ACCEL_STRENGTH, v).apply()

    /** ⌫ の左スワイプで手前の単語をまとめて削除する（`touch.backspace_swipe`） */
    fun backspaceSwipe(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.BACKSPACE_SWIPE, KeyboardDefaults.BACKSPACE_SWIPE)

    fun setBackspaceSwipe(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.BACKSPACE_SWIPE, v).apply()

    /** ⌫ の連打を始めるまでの時間（ミリ秒、200〜1000）。iOS の `touch.key_repeat_delay_ms` */
    fun keyRepeatDelayMs(ctx: Context): Int = sp(ctx).getInt(PrefKeys.KEY_REPEAT_DELAY_MS, KeyboardDefaults.KEY_REPEAT_DELAY_MS)
        .coerceIn(KeyboardDefaults.KEY_REPEAT_DELAY_MIN, KeyboardDefaults.KEY_REPEAT_DELAY_MAX)

    fun setKeyRepeatDelayMs(ctx: Context, v: Int) = sp(ctx).edit().putInt(PrefKeys.KEY_REPEAT_DELAY_MS, v).apply()

    /** ⌫ の連打の最初の間隔（ミリ秒、30〜300）。押し続けると加速する。iOS の `touch.key_repeat_ms` */
    fun keyRepeatMs(ctx: Context): Int = sp(ctx).getInt(PrefKeys.KEY_REPEAT_MS, KeyboardDefaults.KEY_REPEAT_MS)
        .coerceIn(KeyboardDefaults.KEY_REPEAT_MIN, KeyboardDefaults.KEY_REPEAT_MAX)

    fun setKeyRepeatMs(ctx: Context, v: Int) = sp(ctx).edit().putInt(PrefKeys.KEY_REPEAT_MS, v).apply()

    /** 片手モード: "off" / "left" / "right"（キーボード本体を寄せる側） */
    fun oneHanded(ctx: Context): OneHanded = OneHanded.fromJson(sp(ctx).getString(PrefKeys.ONE_HANDED, KeyboardDefaults.ONE_HANDED))

    fun setOneHanded(ctx: Context, v: OneHanded) = sp(ctx).edit().putString(PrefKeys.ONE_HANDED, v.json).apply()

    /** 片手モードのときの本体の幅の比（0.6〜0.9） */
    fun oneHandedWidth(ctx: Context): Float =
        OneHandedLogic.clampWidth(sp(ctx).getFloat(PrefKeys.ONE_HANDED_WIDTH, KeyboardDefaults.ONE_HANDED_WIDTH))

    fun setOneHandedWidth(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.ONE_HANDED_WIDTH, v).apply()

    /** 横画面でのキーボードの高さの倍率（0.7〜1.4） */
    fun heightScaleLandscape(ctx: Context): Float =
        sp(ctx).getFloat(PrefKeys.HEIGHT_SCALE_LANDSCAPE, KeyboardDefaults.HEIGHT_SCALE_LANDSCAPE)
            .coerceIn(KeyboardDefaults.HEIGHT_SCALE_LANDSCAPE_MIN, KeyboardDefaults.HEIGHT_SCALE_LANDSCAPE_MAX)

    fun setHeightScaleLandscape(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.HEIGHT_SCALE_LANDSCAPE, v).apply()

    /** キーの間隔（dp、0〜8） */
    fun keySpacingDp(ctx: Context): Float = sp(ctx).getFloat(PrefKeys.KEY_SPACING_DP, KeyboardDefaults.KEY_SPACING_DP)
        .coerceIn(KeyboardDefaults.KEY_SPACING_MIN, KeyboardDefaults.KEY_SPACING_MAX)

    fun setKeySpacingDp(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.KEY_SPACING_DP, v).apply()

    /** キーの角丸（dp、0〜16） */
    fun keyCornerDp(ctx: Context): Float = sp(ctx).getFloat(PrefKeys.KEY_CORNER_DP, KeyboardDefaults.KEY_CORNER_DP)
        .coerceIn(KeyboardDefaults.KEY_CORNER_MIN, KeyboardDefaults.KEY_CORNER_MAX)

    fun setKeyCornerDp(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.KEY_CORNER_DP, v).apply()

    /** アクセントカラー（blue / green / orange / pink / gray）。ホームのアクセントとは別 */
    fun accent(ctx: Context): AccentColor = AccentColor.fromJson(sp(ctx).getString(PrefKeys.ACCENT, KeyboardDefaults.ACCENT))

    fun setAccent(ctx: Context, v: AccentColor) = sp(ctx).edit().putString(PrefKeys.ACCENT, v.json).apply()

    /**
     * 指定した設定を保存から消して既定に戻す（設定画面の「既定に戻す」）。[ResetKeys] のリストを渡す。
     * 触覚の旧来のキー（"haptic"）も一緒に消す
     */
    fun reset(ctx: Context, keys: List<String>) {
        val e = sp(ctx).edit()
        for (k in keys) e.remove(k)
        e.apply()
    }

    /** キーボードの高さの倍率（0.8〜1.3） */
    fun heightScale(ctx: Context): Float = sp(ctx).getFloat(PrefKeys.HEIGHT_SCALE, KeyboardDefaults.HEIGHT_SCALE)
        .coerceIn(KeyboardDefaults.HEIGHT_SCALE_MIN, KeyboardDefaults.HEIGHT_SCALE_MAX)

    fun setHeightScale(ctx: Context, v: Float) = sp(ctx).edit().putFloat(PrefKeys.HEIGHT_SCALE, v).apply()

    /** テーマ: "system" / "light" / "dark" */
    fun theme(ctx: Context): String = sp(ctx).getString(PrefKeys.THEME, KeyboardDefaults.THEME) ?: KeyboardDefaults.THEME

    fun setTheme(ctx: Context, v: String) = sp(ctx).edit().putString(PrefKeys.THEME, v).apply()

    /** アップデートの自動確認（本体アプリを開いたとき。24 時間に 1 回まで） */
    fun autoCheckUpdates(ctx: Context): Boolean = sp(ctx).getBoolean(PrefKeys.AUTO_CHECK_UPDATES, KeyboardDefaults.AUTO_CHECK_UPDATES)

    fun setAutoCheckUpdates(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean(PrefKeys.AUTO_CHECK_UPDATES, v).apply()

    /** 最後にアップデートを確認した時刻（epoch ミリ秒） */
    fun lastUpdateCheck(ctx: Context): Long = sp(ctx).getLong("last_update_check", 0L)

    fun setLastUpdateCheck(ctx: Context, v: Long) = sp(ctx).edit().putLong("last_update_check", v).apply()
}
