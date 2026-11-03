package com.shiki01.nagi

import android.content.Context
import android.graphics.Typeface
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView

/**
 * 下端のタブバー。アイコンは文字のグリフ（[MainTab.glyph]）。選択中のタブはアクセント色。
 * 下端のナビゲーションバー分の余白は [setBottomInset] で足す（エッジ・トゥ・エッジ）
 */
internal class TabBarView(context: Context, private val onSelect: (MainTab) -> Unit) : LinearLayout(context) {
    private class Item(val box: LinearLayout, val glyph: TextView, val label: TextView)

    private val items = LinkedHashMap<MainTab, Item>()

    init {
        orientation = VERTICAL
        setBackgroundColor(context.cCard)

        val line = View(context)
        line.setBackgroundColor(context.cDivider)
        addView(line, LayoutParams(LP_MATCH, Math.max(1, context.dp(1) / 2)))

        val row = LinearLayout(context)
        row.orientation = HORIZONTAL
        for (tab in MainTab.ordered) {
            val item = makeItem(context, tab)
            items[tab] = item
            row.addView(item.box, LayoutParams(0, LP_WRAP, 1f))
        }
        addView(row, LayoutParams(LP_MATCH, LP_WRAP))
        select(MainTab.DEFAULT)
    }

    private fun makeItem(context: Context, tab: MainTab): Item {
        val box = LinearLayout(context)
        box.orientation = VERTICAL
        box.gravity = Gravity.CENTER
        box.minimumHeight = context.dp(UiSize.MIN_TOUCH_DP + 8)
        box.setPadding(0, context.dp(6), 0, context.dp(6))
        box.isClickable = true
        box.isFocusable = true
        box.contentDescription = tab.label
        box.background = context.rippleOver(null, 0f)
        box.setOnClickListener { onSelect(tab) }

        val glyph = context.textOf(tab.glyph, 20f, context.cTextSecondary)
        glyph.gravity = Gravity.CENTER
        box.addView(glyph, LayoutParams(LP_WRAP, LP_WRAP))
        val label = context.textOf(tab.label, 11f, context.cTextSecondary)
        label.gravity = Gravity.CENTER
        label.maxLines = 1
        box.addView(label, LayoutParams(LP_WRAP, LP_WRAP))
        return Item(box, glyph, label)
    }

    /** 選択中のタブを示す（アクセント色 + 太字）。選び直しても何も起こさない（呼び出し側が切り替える） */
    fun select(selected: MainTab) {
        for ((tab, isSelected) in MainTab.withSelection(selected)) {
            val item = items[tab] ?: continue
            val color = if (isSelected) context.cAccent else context.cTextSecondary
            item.glyph.setTextColor(color)
            item.label.setTextColor(color)
            item.label.setTypeface(null, if (isSelected) Typeface.BOLD else Typeface.NORMAL)
            item.box.isSelected = isSelected
        }
    }

    /** 下端のシステムバー（ナビゲーションバー）の高さ。バーの背景はそこまで伸ばし、項目はその上に置く */
    fun setBottomInset(px: Int) {
        setPadding(0, 0, 0, px)
    }
}
