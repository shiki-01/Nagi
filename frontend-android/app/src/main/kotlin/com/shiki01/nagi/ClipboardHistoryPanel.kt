package com.shiki01.nagi

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.core.view.ViewCompat

/**
 * クリップボード履歴のページ（キーボード本体の上に重ねて出す。全候補の一覧と同じ位置・高さ）。
 * 上部に「閉じる」「履歴」「すべて削除（2 回押しで確認）」、本文は 2 列のカード（縦スクロール）。
 * カードのタップで入力、長押しでピン留めの切り替え、右上の × で個別削除。内容はログに出さない。
 */
internal class ClipboardHistoryPanel(
    context: Context,
    private val colors: KeyboardColors,
    private val store: ClipboardHistoryStore,
    private val cornerPx: Float,
    /** パスワード欄などか（内容を出さない） */
    private val isSecure: () -> Boolean,
    private val onInsert: (String) -> Unit,
    private val onClose: () -> Unit,
) : LinearLayout(context) {
    private val handler = Handler(Looper.getMainLooper())
    private val body = FrameLayout(context)
    private val clearBtn = TextView(context)

    /** 「すべて削除」を 1 回押した状態（もう一度押すと削除。数秒で戻る） */
    private var armedClear = false
    private val disarm = Runnable {
        armedClear = false
        renderClearButton()
    }

    init {
        orientation = VERTICAL
        setBackgroundColor(colors.candBg)
        isClickable = true // 下のキーにタッチを流さない
        ViewCompat.setAccessibilityPaneTitle(this, "クリップボード履歴")
        addView(buildHeader(), LayoutParams(LayoutParams.MATCH_PARENT, px(44f)))
        addView(body, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        reload()
    }

    override fun onDetachedFromWindow() {
        handler.removeCallbacksAndMessages(null)
        super.onDetachedFromWindow()
    }

    private fun px(dp: Float): Int = context.dpToPx(dp)

    private fun buildHeader(): View {
        val row = LinearLayout(context)
        row.orientation = HORIZONTAL
        row.gravity = Gravity.CENTER_VERTICAL

        val close = TextView(context)
        close.text = "◂ 閉じる"
        close.textSize = 14f
        close.setTextColor(colors.accent)
        close.gravity = Gravity.CENTER
        close.setPadding(px(14f), 0, px(14f), 0)
        close.contentDescription = "クリップボード履歴を閉じる"
        close.setOnClickListener { onClose() }
        row.addView(close, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))

        val title = TextView(context)
        title.text = "履歴"
        title.textSize = 15f
        title.setTextColor(colors.fg)
        title.gravity = Gravity.CENTER
        row.addView(title, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))

        clearBtn.textSize = 14f
        clearBtn.gravity = Gravity.CENTER
        clearBtn.setPadding(px(14f), 0, px(14f), 0)
        clearBtn.setOnClickListener { clearTapped() }
        row.addView(clearBtn, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        renderClearButton()
        return row
    }

    private fun renderClearButton() {
        clearBtn.text = if (armedClear) "もう一度で全削除" else "すべて削除"
        clearBtn.setTextColor(DANGER)
        clearBtn.contentDescription = if (armedClear) "もう一度押すと履歴をすべて削除" else "履歴をすべて削除"
        clearBtn.background = if (armedClear) roundRect(DANGER_SOFT, cornerPx) else null
    }

    private fun clearTapped() {
        handler.removeCallbacks(disarm)
        if (armedClear) {
            armedClear = false
            store.clear()
            reload()
        } else {
            armedClear = true
            handler.postDelayed(disarm, CONFIRM_MS)
        }
        renderClearButton()
    }

    /** 内容を読み直して描く。パスワード欄・設定オフのときは項目を持たない（内容を出さない） */
    fun reload() {
        val config = KeyboardPrefs.clipboardConfig(context)
        val secure = isSecure()
        var items: List<ClipboardEntry> = emptyList()
        if (config.enabled && !secure) {
            val now = System.currentTimeMillis()
            store.prune(now, config)
            items = store.items(now, config)
        }
        val state = ClipboardHistoryLogic.pageState(config.enabled, secure, items.size)
        body.removeAllViews()
        clearBtn.visibility = if (state == ClipboardHistoryLogic.PageState.ITEMS) View.VISIBLE else View.INVISIBLE
        val message = state.message
        if (message != null) {
            val t = TextView(context)
            t.text = message
            t.textSize = 14f
            t.setTextColor(colors.subFg)
            t.gravity = Gravity.CENTER
            t.setPadding(px(16f), 0, px(16f), 0)
            body.addView(t, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
            return
        }
        body.addView(buildGallery(items, config), FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
    }

    private fun buildGallery(items: List<ClipboardEntry>, config: ClipboardConfig): View {
        val now = System.currentTimeMillis()
        val list = LinearLayout(context)
        list.orientation = VERTICAL
        list.setPadding(px(4f), px(2f), px(4f), px(6f))
        for (chunk in items.chunked(COLUMNS)) {
            val row = LinearLayout(context)
            row.orientation = HORIZONTAL
            for (entry in chunk) {
                val card = buildCard(entry, config, now)
                val lp = LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f)
                lp.setMargins(px(3f), px(3f), px(3f), px(3f))
                row.addView(card, lp)
            }
            // 最後の行の余りは空のセルで埋めて、列の幅をそろえる
            repeat(COLUMNS - chunk.size) {
                val lp = LayoutParams(0, 1, 1f)
                lp.setMargins(px(3f), px(3f), px(3f), px(3f))
                row.addView(View(context), lp)
            }
            list.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        }
        val scroll = ScrollView(context)
        scroll.addView(list, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.WRAP_CONTENT))
        return scroll
    }

    /** カード 1 枚: 本文（最大 4 行・設定の文字数で省略）、下に種類・相対時刻・ピン留めのマーク、右上に × */
    private fun buildCard(entry: ClipboardEntry, config: ClipboardConfig, now: Long): View {
        val card = LinearLayout(context)
        card.orientation = VERTICAL
        card.background = roundRect(colors.keyBg, cornerPx)
        card.minimumHeight = px(76f)
        card.setPadding(px(8f), px(4f), px(2f), px(6f))
        val preview = ClipboardHistoryLogic.preview(entry.text, config.previewChars)
        card.contentDescription = (if (entry.pinned) "ピン留め済み。" else "") + "${entry.kind.title}: $preview。タップで入力、長押しでピン留めの切り替え"
        card.setOnClickListener { onInsert(entry.text) }
        card.setOnLongClickListener {
            if (!store.togglePin(entry.id) && !entry.pinned) {
                context.toast("ピン留めは ${ClipboardHistoryLogic.MAX_PINNED} 件までです")
            }
            reload()
            true
        }

        val top = LinearLayout(context)
        top.orientation = HORIZONTAL
        val text = TextView(context)
        text.text = preview
        text.textSize = 14f
        text.setTextColor(colors.fg)
        text.maxLines = 4
        text.ellipsize = TextUtils.TruncateAt.END
        text.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        top.addView(text, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        val del = TextView(context)
        del.text = "×"
        del.textSize = 16f
        del.setTextColor(colors.subFg)
        del.gravity = Gravity.CENTER
        del.contentDescription = "この履歴を削除"
        del.setOnClickListener {
            store.delete(entry.id)
            reload()
        }
        top.addView(del, LayoutParams(px(32f), px(32f)))
        card.addView(top, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))

        val meta = TextView(context)
        val pin = if (entry.pinned) "  📌" else ""
        meta.text = "${entry.kind.glyph}  ${ClipboardHistoryLogic.relativeTime(entry.copiedAt, now)}$pin"
        meta.textSize = 11f
        meta.setTextColor(colors.subFg)
        meta.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        card.addView(meta, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        return card
    }

    private companion object {
        const val COLUMNS = 2

        /** 「すべて削除」の確認表示が戻るまで（ミリ秒） */
        const val CONFIRM_MS = 3000L

        const val DANGER = 0xFFE5484D.toInt()
        const val DANGER_SOFT = 0x33E5484D
    }
}
