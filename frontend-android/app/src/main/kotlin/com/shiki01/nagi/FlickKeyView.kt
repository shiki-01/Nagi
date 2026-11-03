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

    /**
     * 高さは「親が決めた高さ」のときだけ使う。`wrap_content` の親の中でこのビューを `match_parent` で重ねると、
     * 既定の測り方は「使える最大の高さ」を要求し、キーボード全体が画面いっぱいに伸びてしまうため
     */
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val width = MeasureSpec.getSize(widthMeasureSpec)
        val height = if (MeasureSpec.getMode(heightMeasureSpec) == MeasureSpec.EXACTLY) {
            MeasureSpec.getSize(heightMeasureSpec)
        } else {
            0
        }
        setMeasuredDimension(width, height)
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
        // 5 セル全体の外接矩形を描画領域に収める共通の移動量。全セルに同じ量を足すので位置関係（上下左右・中央）は変わらない
        // （下端のキーは全体が上へ、上端は下へ、左右は内側へ）。文字のある方向だけを数える（iOS と同じ）
        val shift = PopupClamp.flickShift(
            key = BoxF(k.left, k.top, k.right, k.bottom),
            spacing = 0f,
            left = d.left.isNotEmpty(),
            up = d.up.isNotEmpty(),
            right = d.right.isNotEmpty(),
            down = d.down.isNotEmpty(),
            bounds = BoxF(0f, 0f, width.toFloat(), height.toFloat()),
        )
        val dx = shift.dx
        val dy = shift.dy
        cell(canvas, RectF(k.left - w + dx, k.top + dy, k.left + dx, k.bottom + dy), d.left, active == FlickDirection.LEFT)
        cell(canvas, RectF(k.right + dx, k.top + dy, k.right + w + dx, k.bottom + dy), d.right, active == FlickDirection.RIGHT)
        cell(canvas, RectF(k.left + dx, k.top - h + dy, k.right + dx, k.top + dy), d.up, active == FlickDirection.UP)
        cell(canvas, RectF(k.left + dx, k.bottom + dy, k.right + dx, k.bottom + h + dy), d.down, active == FlickDirection.DOWN)
        // 中央は最後に描く（ずらしたとき、他のセルとの重なりの上に来るようにする）
        cell(canvas, RectF(k.left + dx, k.top + dy, k.right + dx, k.bottom + dy), d.center, active == FlickDirection.TAP)
    }

    private fun cell(canvas: Canvas, r: RectF, label: String, highlighted: Boolean) {
        if (label.isEmpty()) return
        fill.color = if (highlighted) colors.accent else colors.keyBg
        canvas.drawRoundRect(r, radius, radius, fill)
        text.color = if (highlighted) android.graphics.Color.WHITE else colors.fg
        val y = r.centerY() - (text.ascent() + text.descent()) / 2f
        canvas.drawText(label, r.centerX(), y, text)
    }
}
