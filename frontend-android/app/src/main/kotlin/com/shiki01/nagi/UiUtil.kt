package com.shiki01.nagi

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.view.View
import android.view.ViewGroup
import android.widget.AdapterView
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast

// 本体アプリの各画面で共通の、View を組み立てる小さな関数（Compose / AppCompat は使わない）

internal const val COLOR_SECONDARY = 0xFF666666.toInt()
internal const val COLOR_WARNING = 0xFFB35A00.toInt()
internal const val COLOR_ERROR = 0xFFC62828.toInt()

internal fun Context.dp(v: Int): Int = (v * resources.displayMetrics.density + 0.5f).toInt()

internal fun Context.toast(msg: String) {
    Toast.makeText(this, msg, Toast.LENGTH_LONG).show()
}

/** 縦スクロールの画面の骨組み。root に View を足していく。上下はシステムバー分の余白を取る */
@Suppress("DEPRECATION")
internal fun Context.screen(title: String): Pair<ScrollView, LinearLayout> {
    val pad = dp(16)
    val root = LinearLayout(this)
    root.orientation = LinearLayout.VERTICAL
    root.setPadding(pad, pad, pad, pad * 2)
    val scroll = ScrollView(this)
    scroll.clipToPadding = false
    scroll.addView(root)
    // targetSdk 35 では端から端まで描かれるため、システムバーの分だけ余白を足す
    scroll.setOnApplyWindowInsetsListener { v, insets ->
        v.setPadding(0, insets.systemWindowInsetTop, 0, insets.systemWindowInsetBottom)
        insets
    }
    val t = TextView(this)
    t.text = title
    t.textSize = 24f
    t.setTypeface(null, Typeface.BOLD)
    t.setPadding(0, pad, 0, pad / 2)
    root.addView(t)
    return Pair(scroll, root)
}

internal fun Context.heading(text: String): TextView {
    val t = TextView(this)
    t.text = text
    t.textSize = 17f
    t.setTypeface(null, Typeface.BOLD)
    t.setPadding(0, dp(20), 0, dp(4))
    return t
}

internal fun Context.body(text: String, secondary: Boolean = false, color: Int? = null): TextView {
    val t = TextView(this)
    t.text = text
    t.textSize = if (secondary) 13f else 15f
    if (color != null) t.setTextColor(color) else if (secondary) t.setTextColor(COLOR_SECONDARY)
    t.setPadding(0, dp(2), 0, dp(2))
    return t
}

internal fun Context.button(label: String, onClick: () -> Unit): Button {
    val b = Button(this)
    b.text = label
    b.isAllCaps = false
    b.setOnClickListener { onClick() }
    return b
}

internal fun Context.row(vararg views: View): LinearLayout {
    val r = LinearLayout(this)
    r.orientation = LinearLayout.HORIZONTAL
    r.gravity = android.view.Gravity.CENTER_VERTICAL
    for (v in views) r.addView(v)
    return r
}

/** 行内で残りの幅を取る LayoutParams */
internal fun weight1(): LinearLayout.LayoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)

/** ラベル + 説明 + スイッチ。[onChange] はユーザー操作のときだけ呼ぶ */
internal fun Context.switchRow(label: String, description: String?, checked: Boolean, onChange: (Boolean) -> Unit): LinearLayout {
    val col = LinearLayout(this)
    col.orientation = LinearLayout.VERTICAL
    col.addView(body(label))
    if (!description.isNullOrEmpty()) col.addView(body(description, secondary = true))
    val sw = Switch(this)
    sw.isChecked = checked
    sw.setOnCheckedChangeListener { btn, v -> if (btn.isPressed) onChange(v) }
    val r = row(col.also { it.layoutParams = weight1() }, sw)
    r.setPadding(0, dp(8), 0, dp(8))
    return r
}

/** ラベルとスピナー。選択肢は (値, 表示)。[onSelect] はユーザーが選び直したときだけ呼ぶ */
internal fun Context.spinnerRow(
    label: String,
    description: String?,
    options: List<Pair<String, String>>,
    selected: String?,
    onSelect: (String) -> Unit,
): LinearLayout {
    val col = LinearLayout(this)
    col.orientation = LinearLayout.VERTICAL
    col.setPadding(0, dp(8), 0, dp(8))
    col.addView(body(label))
    if (!description.isNullOrEmpty()) col.addView(body(description, secondary = true))
    val spinner = Spinner(this)
    spinner.adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, options.map { it.second })
    var current = options.indexOfFirst { it.first == selected }.coerceAtLeast(0)
    spinner.setSelection(current, false)
    spinner.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
        override fun onItemSelected(parent: AdapterView<*>?, view: View?, position: Int, id: Long) {
            if (position == current) return
            current = position
            onSelect(options[position].first)
        }

        override fun onNothingSelected(parent: AdapterView<*>?) {}
    }
    col.addView(spinner)
    return col
}

internal fun Context.divider(): View {
    val v = View(this)
    v.setBackgroundColor(Color.LTGRAY)
    v.layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 1).also {
        it.topMargin = dp(8)
        it.bottomMargin = dp(8)
    }
    return v
}
