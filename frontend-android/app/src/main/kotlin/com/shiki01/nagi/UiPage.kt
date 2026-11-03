package com.shiki01.nagi

import android.content.Context
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

// タブの中身（Fragment）の骨組み。戻るボタンは無い（タブのルートなので）。
// 上は status bar（と切り欠き）の inset を足す。下はタブバー（MainActivity）が受け取るので 0。
// 左右の inset も MainActivity が受け取る。

/**
 * タブのページの骨組み: 大きな画面タイトルから始まるスクロール領域。
 * Fragment の `onCreateView` で `parts.root` を返し、`parts.content` に項目を足す。
 * [ScreenParts.scroll] は先頭へ戻すときなどに使う
 */
internal fun Context.pageParts(title: String): ScreenParts {
    val scroll = ScrollView(this)
    // 上の inset 分の余白でスクロール内容を切る（状態表示バーの下に文字が透けないように）
    scroll.clipToPadding = true
    scroll.isVerticalScrollBarEnabled = false
    scroll.isFillViewport = false

    val content = LinearLayout(this)
    content.orientation = LinearLayout.VERTICAL
    val side = dp(UiSize.SCREEN_PAD_DP)
    content.setPadding(side, dp(8), side, dp(32))
    scroll.addView(content)

    val titleView = textOf(title, UiSize.TITLE_SP, cText, bold = true)
    titleView.setPadding(dp(4), dp(4), dp(4), dp(12))
    content.addView(titleView)

    val root = FrameLayout(this)
    root.setBackgroundColor(cBg)
    root.addView(scroll, FrameLayout.LayoutParams(LP_MATCH, LP_MATCH))

    ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
        val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
        scroll.setPadding(0, bars.top, 0, 0)
        insets
    }
    // Fragment の View は、窓へ付く前に inset が配られ終わっていることがある。付いたら配り直してもらう
    root.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
        override fun onViewAttachedToWindow(v: View) {
            ViewCompat.requestApplyInsets(v)
        }

        override fun onViewDetachedFromWindow(v: View) {}
    })
    return ScreenParts(root, content, scroll)
}
