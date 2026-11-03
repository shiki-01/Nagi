package com.shiki01.nagi.tabs

import android.content.Context
import android.text.InputType
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.LinearLayout
import com.shiki01.nagi.LP_WRAP
import com.shiki01.nagi.SettingDef
import com.shiki01.nagi.SettingType
import com.shiki01.nagi.SettingsSchema
import com.shiki01.nagi.UiSize
import com.shiki01.nagi.cWarning
import com.shiki01.nagi.dp
import com.shiki01.nagi.inputField
import com.shiki01.nagi.selectRow
import com.shiki01.nagi.stepperRow
import com.shiki01.nagi.switchRow
import com.shiki01.nagi.textButton
import com.shiki01.nagi.textFieldRow
import com.shiki01.nagi.textOf

/**
 * コアの記述子（[SettingDef]）から行を作る。型（スイッチ・選択・数値・文字列）ごとの操作部と、
 * 「次回起動で反映」「プロファイルに従う」の注記、値が明示されているときの「既定に戻す」を付ける。
 * 値の変更は [onChange]（null は既定に戻す）に渡す。検証と保存はコアが行う
 */
internal class CoreSettingRows(private val ctx: Context, private val onChange: (SettingDef, Any?) -> Unit) {
    fun build(d: SettingDef): View {
        val box = LinearLayout(ctx)
        box.orientation = LinearLayout.VERTICAL

        val control: View = when (d.type) {
            SettingType.BOOL -> ctx.switchRow(d.label, d.description, (d.value as? Boolean) ?: false) { onChange(d, it) }
            SettingType.CHOICE -> ctx.selectRow(
                d.label,
                d.description,
                d.choices.map { it.value to it.label },
                (d.value as? String) ?: (d.default as? String),
            ) { onChange(d, it) }
            SettingType.INT, SettingType.FLOAT -> numberRow(d)
            SettingType.TEXT -> textRow(d)
        }
        box.addView(control)

        val notes = ArrayList<String>()
        if (d.requiresRestart) notes.add("次回起動で反映")
        if (d.value == null) notes.add("プロファイルに従う")
        if (notes.isNotEmpty()) {
            val n = ctx.textOf(notes.joinToString(" / "), UiSize.NOTE_SP, ctx.cWarning)
            n.setPadding(ctx.dp(UiSize.ROW_PAD_DP), 0, ctx.dp(UiSize.ROW_PAD_DP), ctx.dp(8))
            box.addView(n)
        }

        if (d.explicit) {
            val reset = ctx.textButton("既定に戻す") { onChange(d, null) }
            val lp = LinearLayout.LayoutParams(LP_WRAP, LP_WRAP)
            lp.marginStart = ctx.dp(8)
            box.addView(reset, lp)
        }
        return box
    }

    private fun numberRow(d: SettingDef): View {
        val edit = ctx.inputField()
        edit.inputType = InputType.TYPE_CLASS_NUMBER or
            (if (d.type == SettingType.FLOAT) InputType.TYPE_NUMBER_FLAG_DECIMAL else 0) or
            (if (d.min < 0) InputType.TYPE_NUMBER_FLAG_SIGNED else 0)
        edit.setSingleLine(true)
        edit.imeOptions = EditorInfo.IME_ACTION_DONE
        edit.setText(SettingsSchema.formatNumber(d, SettingsSchema.numberOf(d)))

        fun commit(raw: Any?) {
            if (raw == null) {
                // 読めない入力は元に戻す
                edit.setText(SettingsSchema.formatNumber(d, SettingsSchema.numberOf(d)))
                return
            }
            onChange(d, raw)
        }

        fun commitText() {
            commit(SettingsSchema.parseNumberInput(d, edit.text.toString()))
        }

        fun step(dir: Int) {
            val cur = SettingsSchema.parseNumberInput(d, edit.text.toString())
                ?.let { (it as Number).toDouble() } ?: SettingsSchema.numberOf(d)
            val next = SettingsSchema.stepped(d, cur, dir)
            onChange(d, if (d.type == SettingType.INT) next.toInt() else next)
        }

        edit.setOnEditorActionListener { _, actionId, _ ->
            if (actionId == EditorInfo.IME_ACTION_DONE) {
                commitText()
                edit.clearFocus()
                true
            } else {
                false
            }
        }
        edit.setOnFocusChangeListener { _, hasFocus ->
            if (!hasFocus) {
                // 値が変わっていなければ送らない
                val parsed = SettingsSchema.parseNumberInput(d, edit.text.toString())
                val cur = SettingsSchema.numberOf(d)
                if (parsed == null || (parsed as Number).toDouble() != cur) commitText()
            }
        }

        val footnote = if (d.max > d.min) {
            "範囲 ${SettingsSchema.formatNumber(d, d.min)}〜${SettingsSchema.formatNumber(d, d.max)}" +
                "（刻み ${SettingsSchema.formatNumber(d, d.step)}）"
        } else {
            null
        }
        return ctx.stepperRow(d.label, d.description, edit, footnote) { dir -> step(dir) }
    }

    private fun textRow(d: SettingDef): View {
        val edit = ctx.inputField()
        edit.setSingleLine(true)
        edit.imeOptions = EditorInfo.IME_ACTION_DONE
        edit.setText((d.value as? String) ?: "")
        edit.hint = (d.default as? String) ?: ""
        fun commit() {
            val text = edit.text.toString()
            val current = (d.value as? String) ?: ""
            if (text == current) return
            onChange(d, if (text.isEmpty()) null else text)
        }
        edit.setOnEditorActionListener { _, actionId, _ ->
            if (actionId == EditorInfo.IME_ACTION_DONE) {
                commit()
                edit.clearFocus()
                true
            } else {
                false
            }
        }
        edit.setOnFocusChangeListener { _, hasFocus -> if (!hasFocus) commit() }
        return ctx.textFieldRow(d.label, d.description, edit)
    }
}
