package com.shiki01.nagi

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.util.TypedValue
import android.view.View

/**
 * フリックキーを押している間、キーの周りに 中央・上下左右の 5 文字を描くオーバーレイ。
 * キーボード全体に重ねて置き、タッチは受けない。
 */
internal class FlickPopupView(context: Context, private val colors: KeyboardColors) : View(context) {
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val text = Paint(Paint.ANTI_ALIAS_FLAG)
    private val radius = context.dpToPx(8f).toFloat()

    private var key: RectF? = null
    private var def: KanaKeyDef? = null
    private var active = FlickDirection.TAP

    init {
        text.textAlign = Paint.Align.CENTER
        text.textSize = TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_SP,
            22f,
            resources.displayMetrics,
        )
    }

    /** [keyRect]（このビューの座標）のキーに対するポップアップを出す。[active] の方向を強調する */
    fun show(keyRect: RectF, keyDef: KanaKeyDef, activeDirection: FlickDirection) {
        key = keyRect
        def = keyDef
        active = activeDirection
        invalidate()
    }

    fun hide() {
        if (key == null) return
        key = null
        def = null
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        val k = key ?: return
        val d = def ?: return
        val w = k.width()
        val h = k.height()
        cell(canvas, RectF(k.left - w, k.top, k.left, k.bottom), d.left, active == FlickDirection.LEFT)
        cell(canvas, RectF(k.right, k.top, k.right + w, k.bottom), d.right, active == FlickDirection.RIGHT)
        cell(canvas, RectF(k.left, k.top - h, k.right, k.top), d.up, active == FlickDirection.UP)
        cell(canvas, RectF(k.left, k.bottom, k.right, k.bottom + h), d.down, active == FlickDirection.DOWN)
        // 中央は最後に描く（上のセルが画面外に出て押し戻されたとき、重なりの上に来るようにする）
        cell(canvas, RectF(k), d.center, active == FlickDirection.TAP)
    }

    private fun cell(canvas: Canvas, r: RectF, label: String, highlighted: Boolean) {
        if (label.isEmpty()) return
        // 画面の外にはみ出すセルは内側へ寄せる
        var dx = 0f
        var dy = 0f
        if (r.left < 0f) dx = -r.left else if (r.right > width) dx = width - r.right
        if (r.top < 0f) dy = -r.top else if (r.bottom > height) dy = height - r.bottom
        r.offset(dx, dy)
        fill.color = if (highlighted) colors.accent else colors.keyBg
        canvas.drawRoundRect(r, radius, radius, fill)
        text.color = if (highlighted) android.graphics.Color.WHITE else colors.fg
        val y = r.centerY() - (text.ascent() + text.descent()) / 2f
        canvas.drawText(label, r.centerX(), y, text)
    }
}
