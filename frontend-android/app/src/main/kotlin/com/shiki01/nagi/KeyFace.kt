package com.shiki01.nagi

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.TextView
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

/** フリック先の文字（キーの角に小さく出す。空文字は出さない） */
internal class FlickHints(val left: String, val up: String, val right: String, val down: String) {
    val any: Boolean get() = left.isNotEmpty() || up.isNotEmpty() || right.isNotEmpty() || down.isNotEmpty()
}

/**
 * JSON レイアウトの 1 キー。メインの文字は TextView として描き、フリック先のヒント（[hints]）と
 * 長押し項目の先頭（[cornerLabel]。右上）を小さく重ねて描く。
 */
internal class KeyFace(context: Context, private val colors: KeyboardColors) : TextView(context) {
    /** 押していないときの背景色 */
    var normalColor: Int = colors.keyBg

    var hints: FlickHints? = null

    /** 右上に小さく出す文字（長押しで出る項目のヒント） */
    var cornerLabel: String? = null

    private val hintPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val radius = context.dpToPx(6f).toFloat()
    private val pad = context.dpToPx(4f).toFloat()

    init {
        gravity = Gravity.CENTER
        hintPaint.textSize = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, 9f, resources.displayMetrics)
        hintPaint.color = colors.subFg
        hintPaint.textAlign = Paint.Align.CENTER
        showPressed(false)
    }

    fun showPressed(pressed: Boolean) {
        background = roundRect(if (pressed) colors.pressedBg else normalColor, radius)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val fm = hintPaint.fontMetrics
        val textH = fm.descent - fm.ascent
        val w = width.toFloat()
        val h = height.toFloat()
        fun baseline(centerY: Float): Float = centerY - (fm.ascent + fm.descent) / 2f
        val hs = hints
        if (hs != null) {
            if (hs.up.isNotEmpty()) canvas.drawText(hs.up, w / 2f, baseline(pad + textH / 2f), hintPaint)
            if (hs.down.isNotEmpty()) canvas.drawText(hs.down, w / 2f, baseline(h - pad - textH / 2f), hintPaint)
            if (hs.left.isNotEmpty()) {
                canvas.drawText(hs.left, pad + hintPaint.measureText(hs.left) / 2f, baseline(h / 2f), hintPaint)
            }
            if (hs.right.isNotEmpty()) {
                canvas.drawText(hs.right, w - pad - hintPaint.measureText(hs.right) / 2f, baseline(h / 2f), hintPaint)
            }
        }
        val c = cornerLabel
        if (!c.isNullOrEmpty()) {
            canvas.drawText(c, w - pad - hintPaint.measureText(c) / 2f, baseline(pad + textH / 2f), hintPaint)
        }
    }
}

/**
 * 長押しで出る項目のポップアップ（キーの上に横並び）。キーボード全体に重ねて置き、タッチは受けない。
 * 項目はキーの中央を基準に並べ、画面の左右からはみ出さないように寄せる。
 */
internal class LongPressPopupView(context: Context, private val colors: KeyboardColors) : View(context) {
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val text = Paint(Paint.ANTI_ALIAS_FLAG)
    private val radius = context.dpToPx(8f).toFloat()
    private val minCellW = context.dpToPx(34f).toFloat()

    private var labels: List<String> = emptyList()
    private var cells = RectF()
    private var cellW = 0f
    private var active = -1

    init {
        text.textAlign = Paint.Align.CENTER
        text.textSize = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, 18f, resources.displayMetrics)
    }

    /** 高さは「親が決めた高さ」のときだけ使う（FlickPopupView と同じ理由。wrap_content の親を伸ばさない） */
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val width = MeasureSpec.getSize(widthMeasureSpec)
        val height = if (MeasureSpec.getMode(heightMeasureSpec) == MeasureSpec.EXACTLY) {
            MeasureSpec.getSize(heightMeasureSpec)
        } else {
            0
        }
        setMeasuredDimension(width, height)
    }

    /** [keyRect]（このビューの座標）のキーの上に [items] を並べる。最初に強調する項目はキーの中央に最も近いもの */
    fun show(keyRect: RectF, items: List<String>) {
        if (items.isEmpty()) return
        labels = items
        val n = items.size
        val w = max(1, width).toFloat()
        cellW = min(max(keyRect.width(), minCellW), w / n)
        val total = cellW * n
        val left = min(max(keyRect.centerX() - total / 2f, 0f), max(0f, w - total))
        val h = max(keyRect.height(), minCellW)
        val top = max(0f, keyRect.top - h - radius / 2f)
        cells = RectF(left, top, left + total, top + h)
        active = indexAt(keyRect.centerX())
        invalidate()
    }

    /** このビューの x 座標に対する項目の番号（範囲外は -1） */
    fun indexAt(x: Float): Int {
        if (labels.isEmpty() || cellW <= 0f) return -1
        if (x < cells.left || x >= cells.right) return -1
        return min(labels.size - 1, floor((x - cells.left) / cellW).toInt())
    }

    /** 強調する項目を変える（-1 は無し）。変わったら true */
    fun setActive(index: Int): Boolean {
        if (index == active) return false
        active = index
        invalidate()
        return true
    }

    val activeIndex: Int get() = active

    fun hasItems(): Boolean = labels.isNotEmpty()

    fun hide() {
        if (labels.isEmpty()) return
        labels = emptyList()
        active = -1
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        if (labels.isEmpty()) return
        for ((i, label) in labels.withIndex()) {
            val r = RectF(cells.left + cellW * i, cells.top, cells.left + cellW * (i + 1), cells.bottom)
            r.inset(1f, 1f)
            val on = i == active
            fill.color = if (on) colors.accent else colors.keyBg
            canvas.drawRoundRect(r, radius, radius, fill)
            text.color = if (on) android.graphics.Color.WHITE else colors.fg
            val y = r.centerY() - (text.ascent() + text.descent()) / 2f
            canvas.drawText(label, r.centerX(), y, text)
        }
    }
}
