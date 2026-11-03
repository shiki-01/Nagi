package com.shiki01.nagi

import android.app.Activity
import android.os.Bundle
import android.text.InputType
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.LinearLayout
import org.json.JSONObject

/**
 * 変換の設定。コアの `get_settings` が返す記述子から汎用フォームを作り、変更は `update_settings` で即時反映する。
 * 値の検証と `config.toml` への書き込みはコアが行う。
 */
class SettingsFormActivity : Activity() {
    private lateinit var content: LinearLayout
    private lateinit var errorBanner: UiBanner
    private lateinit var restartButton: UiButton

    private var items: List<SettingDef> = emptyList()
    private var showDetail = false
    private var loaded = false
    private val rows = HashMap<String, View>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("変換の設定")
        val root = parts.content

        errorBanner = banner("", Tone.ERROR)
        errorBanner.visibility = View.GONE
        root.addView(errorBanner)

        root.addView(
            card(
                switchRow("詳細設定を表示", "ふだん変更する必要のない項目も表示します", false) {
                    showDetail = it
                    render()
                },
            ),
        )

        restartButton = filledButton("変更を反映する（エンジンを再起動）") {
            NagiRuntime.restart(this)
            restartButton.visibility = View.GONE
            toast("エンジンを再起動しました")
        }
        restartButton.visibility = View.GONE
        val rlp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        rlp.topMargin = dp(16)
        root.addView(restartButton, rlp)

        content = LinearLayout(this)
        content.orientation = LinearLayout.VERTICAL
        root.addView(content)
        content.addView(emptyState("読み込んでいます…"))

        setContentView(parts.root)
    }

    override fun onResume() {
        super.onResume()
        load()
    }

    private fun load() {
        NagiRuntime.get(this).call("get_settings") { r ->
            if (r == null) {
                content.removeAllViews()
                content.addView(
                    banner("コアを起動できないため、設定を読み込めませんでした。", Tone.ERROR),
                )
                return@call
            }
            applySnapshot(r)
            render()
        }
    }

    private fun applySnapshot(snapshot: JSONObject) {
        items = SettingsSchema.parseItems(snapshot)
        loaded = true
        val err = snapshot.optString("error", "")
        if (err.isNotEmpty()) {
            errorBanner.show("設定ファイルを読めませんでした（既定値を表示しています。ここでの変更は保存されません）\n$err", Tone.ERROR)
        } else {
            errorBanner.hide()
        }
    }

    private fun render() {
        content.removeAllViews()
        rows.clear()
        if (!loaded) return
        val shown = SettingsSchema.visible(items, showDetail)
        if (shown.isEmpty()) {
            content.addView(emptyState("表示できる設定がありません"))
            return
        }
        for ((group, defs) in SettingsSchema.groups(shown)) {
            val groupCard = UiCard(this)
            for (d in defs) {
                val v = buildRow(d)
                rows[d.key] = v
                groupCard.add(v)
            }
            content.addSection(group, groupCard)
        }
    }

    /** 1 項目を差し替える（スクロール位置を保つため全体は作り直さない） */
    private fun refreshRow(key: String) {
        val old = rows[key] ?: return
        val def = items.firstOrNull { it.key == key } ?: return
        val parent = old.parent as? LinearLayout ?: return
        val index = parent.indexOfChild(old)
        val v = buildRow(def)
        parent.removeViewAt(index)
        parent.addView(v, index)
        rows[key] = v
    }

    // ---- 変更の送信 ----

    /** [value] が null なら既定に戻す */
    private fun send(def: SettingDef, value: Any?) {
        val params = SettingsSchema.updateParams(listOf(Triple(def.key, def.type, value)))
        NagiRuntime.get(this).call("update_settings", params) { r ->
            if (r == null) {
                toast("設定を保存できませんでした（コアが応答しません）")
                refreshRow(def.key)
                return@call
            }
            val errors = r.optJSONArray("errors")
            if (errors != null && errors.length() > 0) {
                val msgs = (0 until errors.length()).mapNotNull { errors.optJSONObject(it)?.optString("message") }
                toast(msgs.joinToString("\n").ifEmpty { "設定を変更できませんでした" })
            }
            r.optJSONObject("snapshot")?.let { applySnapshot(it) }
            refreshRow(def.key)
            if (def.requiresRestart && r.optJSONArray("applied")?.length().let { it != null && it > 0 }) {
                restartButton.visibility = View.VISIBLE
            }
        }
    }

    // ---- 行の組み立て ----

    private fun buildRow(d: SettingDef): View {
        val box = LinearLayout(this)
        box.orientation = LinearLayout.VERTICAL

        val control: View = when (d.type) {
            SettingType.BOOL -> switchRow(d.label, d.description, (d.value as? Boolean) ?: false) { send(d, it) }
            SettingType.CHOICE -> selectRow(
                d.label,
                d.description,
                d.choices.map { it.value to it.label },
                (d.value as? String) ?: (d.default as? String),
            ) { send(d, it) }
            SettingType.INT, SettingType.FLOAT -> numberRow(d)
            SettingType.TEXT -> textRow(d)
        }
        box.addView(control)

        val notes = ArrayList<String>()
        if (d.requiresRestart) notes.add("次回起動で反映")
        if (d.value == null) notes.add("プロファイルに従う")
        if (notes.isNotEmpty()) {
            val n = textOf(notes.joinToString(" / "), UiSize.NOTE_SP, cWarning)
            n.setPadding(dp(UiSize.ROW_PAD_DP), 0, dp(UiSize.ROW_PAD_DP), dp(8))
            box.addView(n)
        }

        if (d.explicit) {
            val reset = textButton("既定に戻す") { send(d, null) }
            val lp = LinearLayout.LayoutParams(LP_WRAP, LP_WRAP)
            lp.marginStart = dp(8)
            box.addView(reset, lp)
        }
        return box
    }

    private fun numberRow(d: SettingDef): View {
        val edit = inputField()
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
            send(d, raw)
        }

        fun commitText() {
            commit(SettingsSchema.parseNumberInput(d, edit.text.toString()))
        }

        fun step(dir: Int) {
            val cur = SettingsSchema.parseNumberInput(d, edit.text.toString())
                ?.let { (it as Number).toDouble() } ?: SettingsSchema.numberOf(d)
            val next = SettingsSchema.stepped(d, cur, dir)
            send(d, if (d.type == SettingType.INT) next.toInt() else next)
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
        return stepperRow(d.label, d.description, edit, footnote) { dir -> step(dir) }
    }

    private fun textRow(d: SettingDef): View {
        val edit = inputField()
        edit.setSingleLine(true)
        edit.imeOptions = EditorInfo.IME_ACTION_DONE
        edit.setText((d.value as? String) ?: "")
        edit.hint = (d.default as? String) ?: ""
        fun commit() {
            val text = edit.text.toString()
            val current = (d.value as? String) ?: ""
            if (text == current) return
            send(d, if (text.isEmpty()) null else text)
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
        return textFieldRow(d.label, d.description, edit)
    }
}
