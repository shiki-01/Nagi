package com.shiki01.nagi.tabs

import android.content.Context
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import com.shiki01.nagi.AccentColor
import com.shiki01.nagi.AppPrefs
import com.shiki01.nagi.AssignSlot
import com.shiki01.nagi.FrontKeys
import com.shiki01.nagi.FrontKind
import com.shiki01.nagi.FrontSettingDef
import com.shiki01.nagi.FrontSettings
import com.shiki01.nagi.HapticLevel
import com.shiki01.nagi.HeightPreset
import com.shiki01.nagi.KeyboardDefaults
import com.shiki01.nagi.KeyboardPrefs
import com.shiki01.nagi.LP_WRAP
import com.shiki01.nagi.LayoutChoices
import com.shiki01.nagi.LayoutInfo
import com.shiki01.nagi.LayoutStore
import com.shiki01.nagi.OneHanded
import com.shiki01.nagi.ResetKeys
import com.shiki01.nagi.UiSize
import com.shiki01.nagi.cAccent
import com.shiki01.nagi.confirmDialog
import com.shiki01.nagi.dp
import com.shiki01.nagi.selectRow
import com.shiki01.nagi.sliderRow
import com.shiki01.nagi.switchRow
import com.shiki01.nagi.textOf
import com.shiki01.nagi.toast
import kotlin.math.abs
import kotlin.math.roundToInt

/** かな・英字のレイアウトの割り当て行に必要な情報（レイアウトの一覧は読み込みが重いのでバックグラウンドで集める） */
internal class LayoutSlots(val effective: Map<String, LayoutInfo>, val kana: String, val alpha: String) {
    companion object {
        /** ファイルを読むのでメインスレッドでは呼ばない */
        fun load(ctx: Context): LayoutSlots = LayoutSlots(
            LayoutChoices.effective(LayoutStore.listPresets(ctx), LayoutStore.listUser(ctx)),
            LayoutStore.assigned(ctx, AssignSlot.KANA),
            LayoutStore.assigned(ctx, AssignSlot.ALPHA),
        )
    }
}

/**
 * フロント固有の設定の行（[FrontSettingDef]）を作る。値の読み書きは [KeyboardPrefs] / [AppPrefs]。
 * 各行の右に、値が既定と違うときだけ「既定に戻す」を出す（行の長押しでも確認のうえ戻せる）
 */
internal class FrontSettingRows(private val ctx: Context, private val host: Host) {
    /** 画面側が受け持つこと */
    interface Host {
        /** 値が変わった。「既定に戻す」の表示を更新する */
        fun valueChanged()

        /** 画面を組み立て直す（スクロール位置は保つ）。呼び出し中のビューを作り直すので、実装側で post してから行う */
        fun rebuild()

        /** レイアウトの割り当てが変わった。レイアウトの一覧を読み直して組み立て直す */
        fun layoutsChanged()

        /** [work] をバックグラウンドで実行し、結果を [done] にメインスレッドで渡す */
        fun <T> background(work: () -> T, done: (T) -> Unit)
    }

    /** 「既定に戻す」ボタンを持つ行。値が変わるたびに表示を更新する */
    private class ResetEntry(val button: View, val isDefault: () -> Boolean)

    private val resets = ArrayList<ResetEntry>()

    /** 画面を組み立て直す前に呼ぶ */
    fun clear() {
        resets.clear()
    }

    /** 値が既定のままの項目では「既定に戻す」を隠す */
    fun refreshResets() {
        for (r in resets) r.button.visibility = if (r.isDefault()) View.GONE else View.VISIBLE
    }

    /** [def] の行を作る（[FrontKind.ACTION] は対象外）。[layouts] はレイアウト行の選択肢用（未読み込みなら null） */
    fun build(def: FrontSettingDef, layouts: LayoutSlots?): View = when (def.kind) {
        FrontKind.TOGGLE -> toggleRow(def)
        FrontKind.SLIDER -> sliderRowFor(def)
        FrontKind.CHOICE -> choiceRow(def)
        FrontKind.HEIGHT_PRESET -> heightRow(def)
        FrontKind.LAYOUT -> layoutRow(def, layouts)
        FrontKind.ACTION -> throw IllegalArgumentException("ACTION は行にしない: ${def.key}")
    }

    // ---- 値の読み書きの対応 ----

    private class BoolAccess(
        val get: () -> Boolean,
        val set: (Boolean) -> Unit,
        val reset: () -> Unit,
        /** 変更で他の行の出し分けが変わるとき */
        val rebuildOnChange: Boolean = false,
    )

    private class FloatAccess(
        val get: () -> Float,
        val set: (Float) -> Unit,
        val reset: () -> Unit,
        /** 変更で他の行の表示が変わるとき（高さの倍率 → 高さのプリセット） */
        val rebuildOnChange: Boolean = false,
    )

    private class ChoiceAccess(val get: () -> String, val set: (String) -> Unit, val reset: () -> Unit)

    private fun resetKeys(keys: List<String>): () -> Unit = { KeyboardPrefs.reset(ctx, keys) }

    private fun boolAccess(key: String): BoolAccess = when (key) {
        FrontKeys.TOGGLE_INPUT -> BoolAccess(
            { KeyboardPrefs.toggleInput(ctx) }, { KeyboardPrefs.setToggleInput(ctx, it) }, resetKeys(ResetKeys.TOGGLE_INPUT),
        )
        FrontKeys.COMPOSE_NUMBERS -> BoolAccess(
            { KeyboardPrefs.composeNumbers(ctx) }, { KeyboardPrefs.setComposeNumbers(ctx, it) },
            resetKeys(ResetKeys.COMPOSE_NUMBERS),
        )
        FrontKeys.KEEP_PREEDIT_ON_SYMBOLS -> BoolAccess(
            { KeyboardPrefs.keepPreeditOnSymbols(ctx) }, { KeyboardPrefs.setKeepPreeditOnSymbols(ctx, it) },
            resetKeys(ResetKeys.KEEP_PREEDIT_ON_SYMBOLS),
        )
        FrontKeys.KEEP_PREEDIT_ON_ENGLISH -> BoolAccess(
            { KeyboardPrefs.keepPreeditOnEnglish(ctx) }, { KeyboardPrefs.setKeepPreeditOnEnglish(ctx, it) },
            resetKeys(ResetKeys.KEEP_PREEDIT_ON_ENGLISH),
        )
        FrontKeys.FLICK_POPUP -> BoolAccess(
            { KeyboardPrefs.flickPopup(ctx) }, { KeyboardPrefs.setFlickPopup(ctx, it) }, resetKeys(ResetKeys.FLICK_POPUP),
        )
        FrontKeys.FLICK_HINTS -> BoolAccess(
            { KeyboardPrefs.flickHints(ctx) }, { KeyboardPrefs.setFlickHints(ctx, it) }, resetKeys(ResetKeys.FLICK_HINTS),
        )
        FrontKeys.SPACE_SWIPE -> BoolAccess(
            { KeyboardPrefs.spaceSwipe(ctx) }, { KeyboardPrefs.setSpaceSwipe(ctx, it) }, resetKeys(ResetKeys.SPACE_SWIPE),
        )
        FrontKeys.SPACE_SWIPE_ACCEL -> BoolAccess(
            { KeyboardPrefs.spaceSwipeAccel(ctx) }, { KeyboardPrefs.setSpaceSwipeAccel(ctx, it) },
            resetKeys(ResetKeys.SPACE_SWIPE_ACCEL),
        )
        FrontKeys.BACKSPACE_SWIPE -> BoolAccess(
            { KeyboardPrefs.backspaceSwipe(ctx) }, { KeyboardPrefs.setBackspaceSwipe(ctx, it) },
            resetKeys(ResetKeys.BACKSPACE_SWIPE),
        )
        FrontKeys.LAYOUT_BY_FIELD -> BoolAccess(
            { KeyboardPrefs.layoutByField(ctx) },
            {
                KeyboardPrefs.setLayoutByField(ctx, it)
                KeyboardPrefs.bumpLayoutsRevision(ctx)
            },
            {
                KeyboardPrefs.reset(ctx, ResetKeys.LAYOUT_BY_FIELD)
                KeyboardPrefs.bumpLayoutsRevision(ctx)
            },
        )
        FrontKeys.UPDATE_AUTO_CHECK -> BoolAccess(
            { KeyboardPrefs.autoCheckUpdates(ctx) }, { KeyboardPrefs.setAutoCheckUpdates(ctx, it) },
            resetKeys(ResetKeys.AUTO_CHECK_UPDATES),
        )
        FrontKeys.DEVELOPER_MODE -> BoolAccess(
            { AppPrefs.developerMode(ctx) }, { AppPrefs.setDeveloperMode(ctx, it) }, { AppPrefs.setDeveloperMode(ctx, false) },
            rebuildOnChange = true,
        )
        else -> throw IllegalArgumentException("未対応のスイッチ: $key")
    }

    private fun floatAccess(key: String): FloatAccess = when (key) {
        FrontKeys.FLICK_SENSITIVITY -> FloatAccess(
            { KeyboardPrefs.flickSensitivity(ctx) }, { KeyboardPrefs.setFlickSensitivity(ctx, it) },
            resetKeys(ResetKeys.FLICK_SENSITIVITY),
        )
        FrontKeys.FLICK_THRESHOLD -> FloatAccess(
            { KeyboardPrefs.flickThresholdDp(ctx) }, { KeyboardPrefs.setFlickThresholdDp(ctx, it) },
            resetKeys(ResetKeys.FLICK_THRESHOLD),
        )
        FrontKeys.FLICK_LONG_PRESS -> FloatAccess(
            { KeyboardPrefs.flickLongPressMs(ctx).toFloat() }, { KeyboardPrefs.setFlickLongPressMs(ctx, it.roundToInt()) },
            resetKeys(ResetKeys.FLICK_LONG_PRESS),
        )
        FrontKeys.KEY_REPEAT_DELAY -> FloatAccess(
            { KeyboardPrefs.keyRepeatDelayMs(ctx).toFloat() }, { KeyboardPrefs.setKeyRepeatDelayMs(ctx, it.roundToInt()) },
            resetKeys(ResetKeys.KEY_REPEAT_DELAY),
        )
        FrontKeys.KEY_REPEAT -> FloatAccess(
            { KeyboardPrefs.keyRepeatMs(ctx).toFloat() }, { KeyboardPrefs.setKeyRepeatMs(ctx, it.roundToInt()) },
            resetKeys(ResetKeys.KEY_REPEAT),
        )
        FrontKeys.SPACE_SWIPE_STEP -> FloatAccess(
            { KeyboardPrefs.spaceSwipeStepDp(ctx) }, { KeyboardPrefs.setSpaceSwipeStepDp(ctx, it) },
            resetKeys(ResetKeys.SPACE_SWIPE_STEP),
        )
        FrontKeys.SPACE_SWIPE_ACCEL_STRENGTH -> FloatAccess(
            { KeyboardPrefs.spaceSwipeAccelStrength(ctx) }, { KeyboardPrefs.setSpaceSwipeAccelStrength(ctx, it) },
            resetKeys(ResetKeys.SPACE_SWIPE_ACCEL_STRENGTH),
        )
        FrontKeys.HEIGHT_SCALE -> FloatAccess(
            { KeyboardPrefs.heightScale(ctx) }, { KeyboardPrefs.setHeightScale(ctx, it) }, resetKeys(ResetKeys.HEIGHT_SCALE),
            rebuildOnChange = true,
        )
        FrontKeys.HEIGHT_SCALE_LANDSCAPE -> FloatAccess(
            { KeyboardPrefs.heightScaleLandscape(ctx) }, { KeyboardPrefs.setHeightScaleLandscape(ctx, it) },
            resetKeys(ResetKeys.HEIGHT_SCALE_LANDSCAPE),
        )
        FrontKeys.ONE_HANDED_WIDTH -> FloatAccess(
            { KeyboardPrefs.oneHandedWidth(ctx) }, { KeyboardPrefs.setOneHandedWidth(ctx, it) },
            resetKeys(ResetKeys.ONE_HANDED_WIDTH),
        )
        FrontKeys.KEY_SPACING -> FloatAccess(
            { KeyboardPrefs.keySpacingDp(ctx) }, { KeyboardPrefs.setKeySpacingDp(ctx, it) }, resetKeys(ResetKeys.KEY_SPACING),
        )
        FrontKeys.KEY_CORNER -> FloatAccess(
            { KeyboardPrefs.keyCornerDp(ctx) }, { KeyboardPrefs.setKeyCornerDp(ctx, it) }, resetKeys(ResetKeys.KEY_CORNER),
        )
        else -> throw IllegalArgumentException("未対応のスライダー: $key")
    }

    private fun choiceAccess(key: String): ChoiceAccess = when (key) {
        FrontKeys.ONE_HANDED -> ChoiceAccess(
            { KeyboardPrefs.oneHanded(ctx).json },
            { KeyboardPrefs.setOneHanded(ctx, OneHanded.fromJson(it)) },
            resetKeys(ResetKeys.ONE_HANDED),
        )
        FrontKeys.HAPTIC -> ChoiceAccess(
            { KeyboardPrefs.hapticLevel(ctx).json },
            { KeyboardPrefs.setHapticLevel(ctx, HapticLevel.fromJson(it) ?: HapticLevel.LIGHT) },
            resetKeys(ResetKeys.HAPTIC),
        )
        FrontKeys.SOUND -> ChoiceAccess(
            { FrontSettings.soundValue(KeyboardPrefs.keySound(ctx)) },
            { KeyboardPrefs.setKeySound(ctx, FrontSettings.soundEnabled(it)) },
            resetKeys(ResetKeys.KEY_SOUND),
        )
        FrontKeys.THEME -> ChoiceAccess(
            { KeyboardPrefs.theme(ctx) }, { KeyboardPrefs.setTheme(ctx, it) }, resetKeys(ResetKeys.THEME),
        )
        FrontKeys.ACCENT -> ChoiceAccess(
            { KeyboardPrefs.accent(ctx).json },
            { KeyboardPrefs.setAccent(ctx, AccentColor.fromJson(it)) },
            resetKeys(ResetKeys.ACCENT),
        )
        else -> throw IllegalArgumentException("未対応の選択: $key")
    }

    // ---- 種類ごとの行 ----

    private fun toggleRow(def: FrontSettingDef): View {
        val a = boolAccess(def.key)
        val row = ctx.switchRow(def.label, def.description, a.get()) {
            a.set(it)
            if (a.rebuildOnChange) {
                // スイッチの切り替えの処理が終わってから組み立て直す
                host.rebuild()
            } else {
                host.valueChanged()
            }
        }
        return resettable(def, row, isDefault = { a.get() == def.defaultBool }, reset = a.reset)
    }

    private fun sliderRowFor(def: FrontSettingDef): View {
        val a = floatAccess(def.key)
        val row = ctx.sliderRow(
            def.label,
            def.description,
            FrontSettings.steps(def),
            FrontSettings.progressOf(def, a.get()),
            show = { FrontSettings.format(def.unit, FrontSettings.valueAt(def, it)) },
            onCommit = {
                a.set(FrontSettings.valueAt(def, it))
                if (a.rebuildOnChange) host.rebuild() else host.valueChanged()
            },
        )
        return resettable(def, row, isDefault = { same(a.get(), def.defaultFloat) }, reset = a.reset)
    }

    private fun choiceRow(def: FrontSettingDef): View {
        val a = choiceAccess(def.key)
        val row = ctx.selectRow(def.label, def.description, def.choices, a.get()) {
            a.set(it)
            host.valueChanged()
        }
        return resettable(def, row, isDefault = { a.get() == def.defaultChoice }, reset = a.reset)
    }

    /** 高さ（低め / 標準 / 高め）。選ぶと縦画面の倍率を設定する。倍率で中間の値にしていれば「カスタム」を足す */
    private fun heightRow(def: FrontSettingDef): View {
        val scale = KeyboardPrefs.heightScale(ctx)
        val current = HeightPreset.matching(scale)
        val options = ArrayList(def.choices)
        if (current == null) options.add(FrontSettings.HEIGHT_CUSTOM to "カスタム（高さの倍率で設定）")
        val row = ctx.selectRow(def.label, def.description, options, current?.json ?: FrontSettings.HEIGHT_CUSTOM) { id ->
            val preset = HeightPreset.fromJson(id)
            if (preset != null) {
                KeyboardPrefs.setHeightScale(ctx, preset.scale)
                host.rebuild()
            }
        }
        return resettable(
            def, row,
            isDefault = { same(KeyboardPrefs.heightScale(ctx), KeyboardDefaults.HEIGHT_SCALE) },
            reset = { KeyboardPrefs.reset(ctx, ResetKeys.HEIGHT_SCALE) },
        )
    }

    /** かな・英字のレイアウトの割り当て（レイアウト画面と同じ選択肢・同じ保存先） */
    private fun layoutRow(def: FrontSettingDef, layouts: LayoutSlots?): View {
        val slot = if (def.key == FrontKeys.LAYOUT_KANA) AssignSlot.KANA else AssignSlot.ALPHA
        val current = if (slot == AssignSlot.KANA) KeyboardPrefs.kanaLayoutId(ctx) else KeyboardPrefs.alphaLayoutId(ctx)
        val listed = if (layouts != null) LayoutChoices.slotOptions(slot, layouts.effective, current) else emptyList()
        // 一覧に無い（読み込み前・無効な）割り当ても、いまの値として見せる
        val options = if (listed.none { it.first == current }) listOf(current to current) + listed else listed
        val row = ctx.selectRow(def.label, def.description, options, current) { id ->
            host.background({ LayoutStore.assign(ctx, slot, id) }) { ok ->
                if (!ok) ctx.toast("このレイアウトは使えません")
                host.layoutsChanged()
            }
        }
        return resettable(
            def, row,
            isDefault = { current == def.defaultChoice },
            reset = {
                if (slot == AssignSlot.KANA) KeyboardPrefs.setKanaLayoutId(ctx, null) else KeyboardPrefs.setAlphaLayoutId(ctx, null)
                KeyboardPrefs.bumpLayoutsRevision(ctx)
            },
        )
    }

    private fun same(a: Float, b: Float): Boolean = abs(a - b) < 1e-4f

    /**
     * [row] の右に「既定に戻す」ボタンを付ける。値が [isDefault] のときは隠す。行の長押しでも確認のうえ戻せる。
     * [reset] は保存を消す処理（呼んだあと画面を組み立て直す）
     */
    private fun resettable(def: FrontSettingDef, row: View, isDefault: () -> Boolean, reset: () -> Unit): View {
        val wrapper = LinearLayout(ctx)
        wrapper.orientation = LinearLayout.HORIZONTAL
        wrapper.gravity = Gravity.CENTER_VERTICAL
        wrapper.addView(row, LinearLayout.LayoutParams(0, LP_WRAP, 1f))

        val defaultText = FrontSettings.defaultText(def)
        val doReset = {
            reset()
            host.rebuild()
        }
        val button: TextView = ctx.textOf("既定に戻す", 12f, ctx.cAccent, bold = true)
        button.gravity = Gravity.CENTER
        button.setPadding(ctx.dp(10), ctx.dp(10), ctx.dp(14), ctx.dp(10))
        button.minHeight = ctx.dp(UiSize.MIN_TOUCH_DP)
        button.isClickable = true
        button.isFocusable = true
        button.contentDescription = "${def.label} を既定（$defaultText）に戻す"
        button.setOnClickListener { doReset() }
        wrapper.addView(button, LinearLayout.LayoutParams(LP_WRAP, LP_WRAP))

        row.setOnLongClickListener {
            if (!isDefault()) {
                ctx.confirmDialog("既定に戻す", "「${def.label}」を既定（$defaultText）に戻しますか？", "戻す") { doReset() }
            }
            true
        }
        resets.add(ResetEntry(button, isDefault))
        return wrapper
    }
}
