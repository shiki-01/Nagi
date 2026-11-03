package com.shiki01.nagi

import android.content.Context
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

// 本体アプリの各画面で共通の、小さな関数（Compose / AppCompat は使わない）。
// デザインシステムの本体（色・カード・行・ボタン・画面の骨組み）は Ui.kt。ここには寸法変換と、旧来の呼び名の互換だけを置く

internal fun Context.dp(v: Int): Int = (v * resources.displayMetrics.density + 0.5f).toInt()

internal fun Context.toast(msg: String) {
    Toast.makeText(this, msg, Toast.LENGTH_LONG).show()
}

internal fun Context.row(vararg views: View): LinearLayout {
    val r = LinearLayout(this)
    r.orientation = LinearLayout.HORIZONTAL
    r.gravity = Gravity.CENTER_VERTICAL
    for (v in views) r.addView(v)
    return r
}

/** 行内で残りの幅を取る LayoutParams */
internal fun weight1(): LinearLayout.LayoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)

/** 旧来の名前。セクション見出し（[sectionHeader]）と同じ */
internal fun Context.heading(text: String): TextView = sectionHeader(text)

/** 旧来の名前。淡い塗りのボタン（[tonalButton]）を返す */
internal fun Context.button(label: String, onClick: () -> Unit): UiButton = tonalButton(label, onClick)

/** 旧来の名前。[selectRow] と同じ */
internal fun Context.spinnerRow(
    label: String,
    description: String?,
    options: List<Pair<String, String>>,
    selected: String?,
    onSelect: (String) -> Unit,
): UiRow = selectRow(label, description, options, selected, onSelect)

/** 旧来の名前。細い区切り線 */
internal fun Context.divider(): View {
    val v = View(this)
    v.setBackgroundColor(cDivider)
    v.layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, Math.max(1, dp(1) / 2)).also {
        it.topMargin = dp(8)
        it.bottomMargin = dp(8)
    }
    return v
}
