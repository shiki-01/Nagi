package com.shiki01.nagi

import android.app.Activity
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import java.util.Locale
import kotlin.math.abs

/**
 * 入力設定。Android フロント固有の設定（[KeyboardPrefs]）を編集する。キーボード側は次に表示されたときに読む。
 * 各項目の右には、既定値と違うときだけ「既定に戻す」を出す（行の長押しでも戻せる）。画面の下の「すべて既定に戻す」は確認つき。
 * 既定値は [KeyboardDefaults] に 1 か所にまとめてある
 */
class KeyboardInputSettingsActivity : Activity() {
    /** 「既定に戻す」ボタンを持つ行。値が変わるたびに表示を更新する */
    private class ResetRow(val button: View, val isDefault: () -> Boolean)

    private val resetRows = ArrayList<ResetRow>()
    private var scrollHost: android.widget.ScrollView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        render()
    }

    /** 画面を組み立て直す。「既定に戻す」・プリセットの選択のあとに、スクロール位置を保って呼ぶ */
    private fun rebuild() {
        val y = scrollHost?.scrollY ?: 0
        val sv = render()
        sv.post { sv.scrollTo(0, y) }
    }

    private fun render(): android.widget.ScrollView {
        resetRows.clear()
        val parts = screenParts("入力設定")
        scrollHost = parts.scroll
        val root = parts.content

        root.addView(
            body(
                "キーボードの見た目と操作の設定です。値が既定と違う項目には「既定に戻す」が出ます（行の長押しでも戻せます）。",
                secondary = true,
            ),
        )

        root.addSection(
            "入力",
            card(
                resettable(
                    "レイアウト",
                    "12 キー（フリック）",
                    selectRow(
                        "レイアウト",
                        "12 キー（フリック）か、ローマ字入力（QWERTY）かを選びます。",
                        listOf("flick" to "12 キー（フリック）", "qwerty" to "QWERTY（ローマ字）"),
                        KeyboardPrefs.layout(this),
                    ) {
                        KeyboardPrefs.setLayout(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.kanaLayoutId(this) == LayoutIds.DEFAULT_KANA },
                    reset = { KeyboardPrefs.setKanaLayoutId(this, null) },
                ),
                resettable(
                    "トグル入力",
                    "オフ",
                    switchRow(
                        "トグル入力",
                        "同じキーを続けて押すと「あ→い→う…」と文字が変わります。オフのときは押すと行の先頭の文字になります。",
                        KeyboardPrefs.toggleInput(this),
                    ) {
                        KeyboardPrefs.setToggleInput(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.toggleInput(this) == KeyboardDefaults.TOGGLE_INPUT },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.TOGGLE_INPUT) },
                ),
                resettable(
                    "フリックの判定距離",
                    "14 dp",
                    seekRow(
                        "フリックの判定距離",
                        "フリックとみなす指の移動量です。小さいほど敏感です。感度の倍率で割った距離が実際の判定に使われます。",
                        min = KeyboardDefaults.FLICK_THRESHOLD_MIN,
                        max = KeyboardDefaults.FLICK_THRESHOLD_MAX,
                        step = 1f,
                        value = KeyboardPrefs.flickThresholdDp(this),
                        format = { String.format(Locale.ROOT, "%.0f dp", it) },
                    ) {
                        KeyboardPrefs.setFlickThresholdDp(this, it)
                        refreshResets()
                    },
                    isDefault = { same(KeyboardPrefs.flickThresholdDp(this), KeyboardDefaults.FLICK_THRESHOLD_DP) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.FLICK_THRESHOLD) },
                ),
                resettable(
                    "フリックの感度",
                    "×1.0",
                    seekRow(
                        "フリックの感度",
                        "判定距離に掛ける倍率です。大きいほど短い移動で反応します。",
                        min = KeyboardDefaults.FLICK_SENSITIVITY_MIN,
                        max = KeyboardDefaults.FLICK_SENSITIVITY_MAX,
                        step = 0.1f,
                        value = KeyboardPrefs.flickSensitivity(this),
                        format = { String.format(Locale.ROOT, "×%.1f", it) },
                    ) {
                        KeyboardPrefs.setFlickSensitivity(this, it)
                        refreshResets()
                    },
                    isDefault = { same(KeyboardPrefs.flickSensitivity(this), KeyboardDefaults.FLICK_SENSITIVITY) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.FLICK_SENSITIVITY) },
                ),
                resettable(
                    "フリックのポップアップ",
                    "オン",
                    switchRow(
                        "フリックのポップアップ",
                        "キーを押したとき、中央と上下左右の 5 文字を表示します。",
                        KeyboardPrefs.flickPopup(this),
                    ) {
                        KeyboardPrefs.setFlickPopup(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.flickPopup(this) == KeyboardDefaults.FLICK_POPUP },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.FLICK_POPUP) },
                ),
            ),
            RELOAD_NOTE,
        )

        root.addSection(
            "キー操作",
            card(
                resettable(
                    "スペースのスワイプ",
                    "オン",
                    switchRow(
                        "スペースキーのスワイプでカーソル移動",
                        "スペースキーを押したまま左右に動かすと、カーソルを動かします（離しても空白は入りません）。入力中は入力中の文字列の中を動きます。",
                        KeyboardPrefs.spaceSwipe(this),
                    ) {
                        KeyboardPrefs.setSpaceSwipe(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.spaceSwipe(this) == KeyboardDefaults.SPACE_SWIPE },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.SPACE_SWIPE) },
                ),
                resettable(
                    "スワイプで 1 文字動かす距離",
                    "12 dp",
                    seekRow(
                        "スワイプで 1 文字動かす距離",
                        "小さいほど少し動かすだけでカーソルが進みます。",
                        min = KeyboardDefaults.SPACE_SWIPE_STEP_MIN,
                        max = KeyboardDefaults.SPACE_SWIPE_STEP_MAX,
                        step = 1f,
                        value = KeyboardPrefs.spaceSwipeStepDp(this),
                        format = { String.format(Locale.ROOT, "%.0f dp", it) },
                    ) {
                        KeyboardPrefs.setSpaceSwipeStepDp(this, it)
                        refreshResets()
                    },
                    isDefault = { same(KeyboardPrefs.spaceSwipeStepDp(this), KeyboardDefaults.SPACE_SWIPE_STEP_DP) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.SPACE_SWIPE_STEP) },
                ),
                resettable(
                    "⌫ の左スワイプで単語削除",
                    "オフ",
                    switchRow(
                        "⌫ の左スワイプで単語削除",
                        "⌫ を押したまま左へ約 40 dp 動かすと、手前の単語をまとめて削除します。",
                        KeyboardPrefs.backspaceSwipe(this),
                    ) {
                        KeyboardPrefs.setBackspaceSwipe(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.backspaceSwipe(this) == KeyboardDefaults.BACKSPACE_SWIPE },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.BACKSPACE_SWIPE) },
                ),
                resettable(
                    "⌫ の連打を始めるまでの時間",
                    "400 ms",
                    seekRow(
                        "⌫ の連打を始めるまでの時間",
                        "⌫ を押し続けてから、繰り返し削除が始まるまでの時間です。",
                        min = KeyboardDefaults.KEY_REPEAT_DELAY_MIN.toFloat(),
                        max = KeyboardDefaults.KEY_REPEAT_DELAY_MAX.toFloat(),
                        step = 50f,
                        value = KeyboardPrefs.keyRepeatDelayMs(this).toFloat(),
                        format = { String.format(Locale.ROOT, "%.0f ms", it) },
                    ) {
                        KeyboardPrefs.setKeyRepeatDelayMs(this, it.toInt())
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.keyRepeatDelayMs(this) == KeyboardDefaults.KEY_REPEAT_DELAY_MS },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.KEY_REPEAT_DELAY) },
                ),
                resettable(
                    "⌫ の連打の間隔",
                    "60 ms",
                    seekRow(
                        "⌫ の連打の間隔",
                        "繰り返し削除の最初の間隔です。押し続けるほど少しずつ速くなります。",
                        min = KeyboardDefaults.KEY_REPEAT_MIN.toFloat(),
                        max = KeyboardDefaults.KEY_REPEAT_MAX.toFloat(),
                        step = 10f,
                        value = KeyboardPrefs.keyRepeatMs(this).toFloat(),
                        format = { String.format(Locale.ROOT, "%.0f ms", it) },
                    ) {
                        KeyboardPrefs.setKeyRepeatMs(this, it.toInt())
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.keyRepeatMs(this) == KeyboardDefaults.KEY_REPEAT_MS },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.KEY_REPEAT) },
                ),
            ),
            RELOAD_NOTE,
        )

        root.addSection(
            "見た目",
            card(
                resettable(
                    "テーマ",
                    "端末の設定に合わせる",
                    selectRow(
                        "テーマ",
                        null,
                        listOf("system" to "端末の設定に合わせる", "light" to "ライト", "dark" to "ダーク"),
                        KeyboardPrefs.theme(this),
                    ) {
                        KeyboardPrefs.setTheme(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.theme(this) == KeyboardDefaults.THEME },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.THEME) },
                ),
                resettable(
                    "アクセントカラー",
                    "ブルー",
                    selectRow(
                        "アクセントカラー",
                        "選択中の候補・変換中の強調・確定キーなどの色です。",
                        AccentColor.values().map { it.json to it.label },
                        KeyboardPrefs.accent(this).json,
                    ) {
                        KeyboardPrefs.setAccent(this, AccentColor.fromJson(it))
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.accent(this).json == KeyboardDefaults.ACCENT },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.ACCENT) },
                ),
                resettable(
                    "キーの間隔",
                    "4 dp",
                    seekRow(
                        "キーの間隔",
                        "キーとキーの間の余白です。",
                        min = KeyboardDefaults.KEY_SPACING_MIN,
                        max = KeyboardDefaults.KEY_SPACING_MAX,
                        step = 0.5f,
                        value = KeyboardPrefs.keySpacingDp(this),
                        format = { String.format(Locale.ROOT, "%.1f dp", it) },
                    ) {
                        KeyboardPrefs.setKeySpacingDp(this, it)
                        refreshResets()
                    },
                    isDefault = { same(KeyboardPrefs.keySpacingDp(this), KeyboardDefaults.KEY_SPACING_DP) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.KEY_SPACING) },
                ),
                resettable(
                    "キーの角丸",
                    "6 dp",
                    seekRow(
                        "キーの角丸",
                        "キーの角の丸さです。",
                        min = KeyboardDefaults.KEY_CORNER_MIN,
                        max = KeyboardDefaults.KEY_CORNER_MAX,
                        step = 1f,
                        value = KeyboardPrefs.keyCornerDp(this),
                        format = { String.format(Locale.ROOT, "%.0f dp", it) },
                    ) {
                        KeyboardPrefs.setKeyCornerDp(this, it)
                        refreshResets()
                    },
                    isDefault = { same(KeyboardPrefs.keyCornerDp(this), KeyboardDefaults.KEY_CORNER_DP) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.KEY_CORNER) },
                ),
            ),
            RELOAD_NOTE,
        )

        root.addSection(
            "フィードバック",
            card(
                resettable(
                    "触覚の強さ",
                    "弱",
                    selectRow(
                        "触覚の強さ",
                        "キーを押したときの振動の強さです。端末の「タッチ操作時の振動」がオフだと振動しません。",
                        HapticLevel.values().map { it.json to it.label },
                        KeyboardPrefs.hapticLevel(this).json,
                    ) {
                        KeyboardPrefs.setHapticLevel(this, HapticLevel.fromJson(it) ?: HapticLevel.LIGHT)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.hapticLevel(this).json == KeyboardDefaults.HAPTIC_LEVEL },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.HAPTIC) },
                ),
                resettable(
                    "キー入力のクリック音",
                    "オフ",
                    switchRow(
                        "キー入力のクリック音",
                        "キーを押したときに音を鳴らします。音量と鳴るかどうかは端末の「タッチ操作音」の設定に従います。",
                        KeyboardPrefs.keySound(this),
                    ) {
                        KeyboardPrefs.setKeySound(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.keySound(this) == KeyboardDefaults.KEY_SOUND },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.KEY_SOUND) },
                ),
            ),
            RELOAD_NOTE,
        )

        root.addSection(
            "片手・高さ",
            card(
                resettable(
                    "片手モード",
                    "オフ",
                    selectRow(
                        "片手モード",
                        "キーボードを左右どちらかに寄せます。空いた側の「⇄」で反対側へ、「↔」で通常に戻せます。",
                        OneHanded.values().map { it.json to it.label },
                        KeyboardPrefs.oneHanded(this).json,
                    ) {
                        KeyboardPrefs.setOneHanded(this, OneHanded.fromJson(it))
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.oneHanded(this).json == KeyboardDefaults.ONE_HANDED },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.ONE_HANDED) },
                ),
                resettable(
                    "片手モードの幅",
                    "×0.80",
                    seekRow(
                        "片手モードの幅",
                        "片手モードのときの、キーボードの幅の割合です。",
                        min = OneHandedLogic.WIDTH_MIN,
                        max = OneHandedLogic.WIDTH_MAX,
                        step = 0.05f,
                        value = KeyboardPrefs.oneHandedWidth(this),
                        format = { String.format(Locale.ROOT, "×%.2f", it) },
                    ) {
                        KeyboardPrefs.setOneHandedWidth(this, it)
                        refreshResets()
                    },
                    isDefault = { same(KeyboardPrefs.oneHandedWidth(this), KeyboardDefaults.ONE_HANDED_WIDTH) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.ONE_HANDED_WIDTH) },
                ),
                resettable(
                    "高さのプリセット",
                    "標準",
                    heightPresetRow(),
                    isDefault = { same(KeyboardPrefs.heightScale(this), KeyboardDefaults.HEIGHT_SCALE) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.HEIGHT_SCALE) },
                ),
                resettable(
                    "キーボードの高さ（縦画面）",
                    "×1.00",
                    seekRow(
                        "キーボードの高さ（縦画面）",
                        "標準の高さに対する倍率です。上のプリセットを選ぶと、この値も変わります。",
                        min = KeyboardDefaults.HEIGHT_SCALE_MIN,
                        max = KeyboardDefaults.HEIGHT_SCALE_MAX,
                        step = 0.05f,
                        value = KeyboardPrefs.heightScale(this),
                        format = { String.format(Locale.ROOT, "×%.2f", it) },
                    ) {
                        KeyboardPrefs.setHeightScale(this, it)
                        // プリセットの表示（「カスタム」かどうか）を合わせるため、組み立て直す（タッチの処理が終わってから）
                        scrollHost?.post { rebuild() }
                    },
                    isDefault = { same(KeyboardPrefs.heightScale(this), KeyboardDefaults.HEIGHT_SCALE) },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.HEIGHT_SCALE) },
                ),
                resettable(
                    "キーボードの高さ（横画面）",
                    "×1.00",
                    seekRow(
                        "キーボードの高さ（横画面）",
                        "横向きのときの高さの倍率です。",
                        min = KeyboardDefaults.HEIGHT_SCALE_LANDSCAPE_MIN,
                        max = KeyboardDefaults.HEIGHT_SCALE_LANDSCAPE_MAX,
                        step = 0.05f,
                        value = KeyboardPrefs.heightScaleLandscape(this),
                        format = { String.format(Locale.ROOT, "×%.2f", it) },
                    ) {
                        KeyboardPrefs.setHeightScaleLandscape(this, it)
                        refreshResets()
                    },
                    isDefault = {
                        same(KeyboardPrefs.heightScaleLandscape(this), KeyboardDefaults.HEIGHT_SCALE_LANDSCAPE)
                    },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.HEIGHT_SCALE_LANDSCAPE) },
                ),
            ),
            RELOAD_NOTE,
        )

        root.addSection(
            "アップデート",
            card(
                resettable(
                    "アップデートの自動確認",
                    "オン",
                    switchRow(
                        "アップデートを自動で確認",
                        "本体アプリを開いたとき、新しい版があるか確認します（24 時間に 1 回まで）。",
                        KeyboardPrefs.autoCheckUpdates(this),
                    ) {
                        KeyboardPrefs.setAutoCheckUpdates(this, it)
                        refreshResets()
                    },
                    isDefault = { KeyboardPrefs.autoCheckUpdates(this) == KeyboardDefaults.AUTO_CHECK_UPDATES },
                    reset = { KeyboardPrefs.reset(this, ResetKeys.AUTO_CHECK_UPDATES) },
                ),
            ),
        )

        root.addView(spacer(20))
        root.addView(
            destructiveButton("すべて既定に戻す") {
                confirmDialog(
                    "すべて既定に戻す",
                    "この画面の設定（キーボードの見た目・操作・片手モード・高さ・かな入力のレイアウトなど）をすべて既定の値に戻します。",
                    "戻す",
                    destructive = true,
                ) {
                    KeyboardPrefs.reset(this, ResetKeys.ALL)
                    KeyboardPrefs.setKanaLayoutId(this, null)
                    rebuild()
                }
            },
        )

        setContentView(parts.root)
        refreshResets()
        return parts.scroll
    }

    /** 高さのプリセット（コンパクト / 標準 / 高め）。選ぶと縦画面の高さの倍率を設定する。スライダーで中間の値にしていれば「カスタム」 */
    private fun heightPresetRow(): View {
        val current = HeightPreset.matching(KeyboardPrefs.heightScale(this))
        val options = ArrayList<Pair<String, String>>()
        for (p in HeightPreset.values()) options.add(p.json to p.label)
        if (current == null) options.add(CUSTOM to "カスタム（スライダーで設定）")
        return selectRow(
            "高さのプリセット",
            "縦画面のキーボードの高さを 3 段階から選びます。",
            options,
            current?.json ?: CUSTOM,
        ) { id ->
            val preset = HeightPreset.fromJson(id)
            if (preset != null) {
                KeyboardPrefs.setHeightScale(this, preset.scale)
                rebuild()
            }
        }
    }

    /**
     * [row] の右に「既定に戻す」ボタンを付ける。値が [isDefault] のときは隠す。行の長押しでも確認のうえ戻せる。
     * [defaultText] は既定値の表示（確認のメッセージ用）、[reset] は保存を消す処理（呼んだあと画面を組み立て直す）
     */
    private fun resettable(
        title: String,
        defaultText: String,
        row: View,
        isDefault: () -> Boolean,
        reset: () -> Unit,
    ): View {
        val wrapper = LinearLayout(this)
        wrapper.orientation = LinearLayout.HORIZONTAL
        wrapper.gravity = Gravity.CENTER_VERTICAL
        wrapper.addView(row, LinearLayout.LayoutParams(0, LP_WRAP, 1f))

        val doReset = {
            reset()
            rebuild()
        }
        val button: TextView = textOf("既定に戻す", 12f, cAccent, bold = true)
        button.gravity = Gravity.CENTER
        button.setPadding(dp(10), dp(10), dp(14), dp(10))
        button.minHeight = dp(UiSize.MIN_TOUCH_DP)
        button.isClickable = true
        button.isFocusable = true
        button.contentDescription = "$title を既定（$defaultText）に戻す"
        button.setOnClickListener { doReset() }
        wrapper.addView(button, LinearLayout.LayoutParams(LP_WRAP, LP_WRAP))

        row.setOnLongClickListener {
            if (!isDefault()) {
                confirmDialog("既定に戻す", "「$title」を既定（$defaultText）に戻しますか？", "戻す") { doReset() }
            }
            true
        }
        resetRows.add(ResetRow(button, isDefault))
        return wrapper
    }

    /** 値が既定のままの項目では「既定に戻す」を隠す */
    private fun refreshResets() {
        for (r in resetRows) r.button.visibility = if (r.isDefault()) View.GONE else View.VISIBLE
    }

    private fun same(a: Float, b: Float): Boolean = abs(a - b) < 1e-4f

    /** ラベル + 現在値 + シークバー。[onChange] は指を離したときに 1 回だけ呼ぶ */
    private fun seekRow(
        label: String,
        description: String,
        min: Float,
        max: Float,
        step: Float,
        value: Float,
        format: (Float) -> String,
        onChange: (Float) -> Unit,
    ): LinearLayout {
        val steps = Math.round((max - min) / step)
        fun valueAt(progress: Int): Float = (min + progress * step).coerceIn(min, max)
        return sliderRow(
            label,
            description,
            steps,
            Math.round((value - min) / step).coerceIn(0, steps),
            show = { format(valueAt(it)) },
            onCommit = { onChange(valueAt(it)) },
        )
    }

    private companion object {
        const val RELOAD_NOTE = "キーボードを開き直すと反映されます。"
        const val CUSTOM = "custom"
    }
}
