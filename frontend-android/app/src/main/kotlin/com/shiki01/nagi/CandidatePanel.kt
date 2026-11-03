package com.shiki01.nagi

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.core.view.ViewCompat

/** キーボードの配色。設定のテーマ（system / light / dark）とアクセントカラーで決まる */
internal class KeyboardColors(val dark: Boolean, accentColor: AccentColor = AccentColor.BLUE) {
    val bg = if (dark) Color.rgb(32, 33, 36) else Color.rgb(232, 234, 237)
    val keyBg = if (dark) Color.rgb(66, 68, 72) else Color.WHITE
    val funcBg = if (dark) Color.rgb(48, 50, 54) else Color.rgb(207, 211, 216)
    val pressedBg = if (dark) Color.rgb(100, 104, 110) else Color.rgb(176, 190, 205)
    val fg = if (dark) Color.WHITE else Color.BLACK
    val candBg = if (dark) Color.rgb(44, 46, 50) else Color.rgb(246, 247, 248)
    val segBg = if (dark) Color.rgb(38, 40, 44) else Color.rgb(238, 240, 243)
    val accent = accentColor.argb(dark)

    /** 選択中の候補・変換中の強調の背景（アクセントを薄くしたもの） */
    val accentSoft = AccentColor.withAlpha(accent, if (dark) 0x66 else 0x40)
    val subFg = if (dark) Color.rgb(170, 174, 180) else Color.rgb(100, 104, 110)

    companion object {
        fun resolve(ctx: Context): KeyboardColors {
            val night = (ctx.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
                Configuration.UI_MODE_NIGHT_YES
            val dark = when (KeyboardPrefs.theme(ctx)) {
                "dark" -> true
                "light" -> false
                else -> night
            }
            return KeyboardColors(dark, KeyboardPrefs.accent(ctx))
        }
    }
}

internal fun Context.dpToPx(v: Float): Int = (v * resources.displayMetrics.density + 0.5f).toInt()

internal fun roundRect(color: Int, radiusPx: Float): GradientDrawable {
    val d = GradientDrawable()
    d.setColor(color)
    d.cornerRadius = radiusPx
    return d
}

/** 縦グリッドの列数。短い候補が多ければ 4、長い候補があれば 2 */
internal object CandidateGrid {
    fun columns(items: List<String>): Int {
        val longest = items.maxOfOrNull { it.length } ?: 0
        return when {
            longest <= 4 -> 4
            longest <= 8 -> 3
            else -> 2
        }
    }

    /** 全候補の縦グリッド（先頭に閉じる行）。選ぶと [onPick]、閉じると [onClose] */
    fun build(
        context: Context,
        colors: KeyboardColors,
        items: List<String>,
        selected: Int,
        onPick: (Int) -> Unit,
        onClose: () -> Unit,
    ): View {
        val panel = LinearLayout(context)
        panel.orientation = LinearLayout.VERTICAL
        panel.setBackgroundColor(colors.candBg)
        panel.isClickable = true // 下のキーにタッチを流さない
        ViewCompat.setAccessibilityPaneTitle(panel, "全候補の一覧")

        val head = TextView(context)
        head.text = "▴ 閉じる"
        head.textSize = 14f
        head.setTextColor(colors.subFg)
        head.gravity = Gravity.CENTER_VERTICAL or Gravity.END
        head.setPadding(0, 0, context.dpToPx(16f), 0)
        head.contentDescription = "全候補の一覧を閉じる"
        head.setOnClickListener { onClose() }
        panel.addView(head, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, context.dpToPx(44f)))

        val list = LinearLayout(context)
        list.orientation = LinearLayout.VERTICAL
        val cols = columns(items)
        for (chunk in items.withIndex().chunked(cols)) {
            val row = LinearLayout(context)
            row.orientation = LinearLayout.HORIZONTAL
            for (iv in chunk) {
                val cell = TextView(context)
                cell.text = iv.value
                cell.textSize = 18f
                cell.setTextColor(colors.fg)
                cell.gravity = Gravity.CENTER
                cell.maxLines = 1
                cell.ellipsize = TextUtils.TruncateAt.END
                cell.setPadding(context.dpToPx(6f), 0, context.dpToPx(6f), 0)
                if (iv.index == selected) cell.background = roundRect(colors.accentSoft, context.dpToPx(6f).toFloat())
                val idx = iv.index
                cell.contentDescription = KeyDescriptions.candidate(iv.value, idx, items.size, idx == selected)
                cell.setOnClickListener { onPick(idx) }
                row.addView(cell, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f))
            }
            // 最後の行の余りは空のセルで埋めて、列の幅をそろえる
            repeat(cols - chunk.size) {
                row.addView(View(context), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f))
            }
            list.addView(row, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, context.dpToPx(46f)))
        }
        val scroll = ScrollView(context)
        scroll.addView(list, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.WRAP_CONTENT))
        panel.addView(scroll, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        return panel
    }
}

/**
 * ソフトキーボードを出さないとき（物理キーボード接続時）の簡易な候補表示。
 * `onCreateCandidatesView` で使う。
 */
internal class CandidateStrip(context: Context, private val onPick: (Int) -> Unit) : HorizontalScrollView(context) {
    private val colors = KeyboardColors.resolve(context)
    private val bar = LinearLayout(context)

    init {
        isHorizontalScrollBarEnabled = false
        setBackgroundColor(colors.candBg)
        bar.orientation = LinearLayout.HORIZONTAL
        bar.gravity = Gravity.CENTER_VERTICAL
        addView(bar, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, context.dpToPx(44f)))
    }

    fun setItems(items: List<String>, selected: Int) {
        bar.removeAllViews()
        for ((i, s) in items.withIndex()) {
            val t = TextView(context)
            t.text = s
            t.textSize = 18f
            t.setTextColor(colors.fg)
            t.gravity = Gravity.CENTER
            t.setPadding(context.dpToPx(14f), 0, context.dpToPx(14f), 0)
            if (i == selected) t.background = roundRect(colors.accentSoft, context.dpToPx(6f).toFloat())
            t.contentDescription = KeyDescriptions.candidate(s, i, items.size, i == selected)
            t.setOnClickListener { onPick(i) }
            bar.addView(t, LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.MATCH_PARENT))
        }
        scrollTo(0, 0)
    }
}
