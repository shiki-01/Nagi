package com.shiki01.nagi

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.util.TypedValue
import android.view.View

/**
 * レイアウトの簡易プレビュー（レイアウト管理画面の詳細用）。行の高さ比・キーの幅比に従って並べ、
 * キーの文字（label）と、小さなサブラベル（フリックの上下左右・長押しの先頭）だけを描く。
 * 色は役割（通常 / 機能 / アクセント）だけで決め、見た目の指定（look）は使わない。KeyboardView 本体は使わない。
 */
internal class PreviewView(context: Context) : View(context) {
    private var layoutModel: LayoutModel? = null

    private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val subPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val rect = RectF()

    private val baseRow = context.dpF(30f)
    private val gap = context.dpF(3f)
    private val radius = context.dpF(4f)
    private val pad = context.dpF(6f)
    private val textMax = sp(context, 13f)
    private val textMin = sp(context, 7f)
    private val subSize = sp(context, 7f)

    init {
        textPaint.textAlign = Paint.Align.CENTER
        subPaint.textAlign = Paint.Align.CENTER
        subPaint.textSize = subSize
        contentDescription = "レイアウトのプレビュー"
    }

    fun setLayout(layout: LayoutModel?) {
        layoutModel = layout
        requestLayout()
        invalidate()
    }

    private fun heights(): List<Double> = layoutModel?.rows?.map { it.height } ?: emptyList()

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val mode = MeasureSpec.getMode(widthMeasureSpec)
        val size = MeasureSpec.getSize(widthMeasureSpec)
        val w = if (mode == MeasureSpec.UNSPECIFIED) context.dp(300) else size
        val h = PreviewGeometry.totalHeight(heights(), baseRow, gap) + pad * 2
        setMeasuredDimension(w, Math.ceil(h.toDouble()).toInt())
    }

    override fun onDraw(canvas: Canvas) {
        val layout = layoutModel ?: return
        val ctx = context
        val bgColor = ctx.cBg
        val normal = ctx.cCard
        val function = blendColors(ctx.cBg, ctx.cText, 0.14f)
        val accent = ctx.cAccent

        fillPaint.color = bgColor
        rect.set(0f, 0f, width.toFloat(), height.toFloat())
        canvas.drawRoundRect(rect, radius * 2, radius * 2, fillPaint)

        val rowSpans = PreviewGeometry.rowSpans(heights(), baseRow, gap)
        val rowWidth = width - pad * 2
        for ((ri, row) in layout.rows.withIndex()) {
            val rs = rowSpans[ri]
            val top = pad + rs.start
            val keySpans = PreviewGeometry.keySpans(row.keys.map { it.width }, rowWidth, gap)
            for ((ki, key) in row.keys.withIndex()) {
                if (key.action == KeyAction.Spacer) continue
                val ks = keySpans.getOrNull(ki) ?: continue
                val left = pad + ks.start
                rect.set(left, top, left + ks.size, top + rs.size)
                val isAccent = key.role == KeyRole.ACCENT
                fillPaint.color = when (key.role) {
                    KeyRole.ACCENT -> accent
                    KeyRole.FUNCTION -> function
                    KeyRole.NORMAL -> normal
                }
                canvas.drawRoundRect(rect, radius, radius, fillPaint)
                drawLabels(canvas, key, if (isAccent) ctx.cOnAccent else ctx.cText, if (isAccent) ctx.cOnAccent else ctx.cTextSecondary)
            }
        }
    }

    /** [rect]（キーの矩形）の中に、メインの文字とサブラベルを描く */
    private fun drawLabels(canvas: Canvas, key: LayoutKey, mainColor: Int, subColor: Int) {
        val label = LayoutLabels.staticLabel(key)
        val inner = rect.width() - context.dpF(4f)
        if (label.isNotEmpty() && inner > 0f) {
            textPaint.color = mainColor
            var size = Math.min(textMax, rect.height() * 0.5f)
            textPaint.textSize = size
            while (size > textMin && textPaint.measureText(label) > inner) {
                size -= 0.5f
                textPaint.textSize = size
            }
            val shown = fit(label, inner)
            val fm = textPaint.fontMetrics
            val baseline = rect.centerY() - (fm.ascent + fm.descent) / 2f
            canvas.drawText(shown, rect.centerX(), baseline, textPaint)
        }
        subPaint.color = subColor
        val fm = subPaint.fontMetrics
        val subH = fm.descent - fm.ascent
        val edge = context.dpF(2f)
        val a = key.action
        if (a is KeyAction.Flick && a.kana == null) {
            a.up?.takeIf { it.isNotEmpty() }?.let { canvas.drawText(clip(it), rect.centerX(), rect.top + edge - fm.ascent, subPaint) }
            a.down?.takeIf { it.isNotEmpty() }?.let { canvas.drawText(clip(it), rect.centerX(), rect.bottom - edge - fm.descent, subPaint) }
            a.left?.takeIf { it.isNotEmpty() }?.let {
                canvas.drawText(clip(it), rect.left + edge + subPaint.measureText(clip(it)) / 2f, rect.centerY() + subH / 4f, subPaint)
            }
            a.right?.takeIf { it.isNotEmpty() }?.let {
                canvas.drawText(clip(it), rect.right - edge - subPaint.measureText(clip(it)) / 2f, rect.centerY() + subH / 4f, subPaint)
            }
        }
        // 長押しの先頭の項目は右上に小さく
        key.longPress.firstOrNull()?.label?.takeIf { it.isNotEmpty() }?.let {
            val t = clip(it)
            canvas.drawText(t, rect.right - edge - subPaint.measureText(t) / 2f, rect.top + edge - fm.ascent, subPaint)
        }
    }

    /** サブラベルは 1 文字だけ（狭いキーでも重ならないように） */
    private fun clip(s: String): String {
        val end = s.offsetByCodePoints(0, Math.min(1, s.codePointCount(0, s.length)))
        return s.substring(0, end)
    }

    /** 最小の文字サイズでも入らないときは末尾を省略する */
    private fun fit(s: String, maxWidth: Float): String {
        if (textPaint.measureText(s) <= maxWidth) return s
        var n = s.length
        while (n > 1 && textPaint.measureText(s, 0, n) + textPaint.measureText("…") > maxWidth) n--
        return s.substring(0, n) + "…"
    }

    private companion object {
        fun sp(context: Context, v: Float): Float =
            TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, v, context.resources.displayMetrics)
    }
}
