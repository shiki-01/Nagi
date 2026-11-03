package com.shiki01.nagi

import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.hypot

/** フリックの方向。`TAP` は移動が小さい（タップ） */
internal enum class FlickDirection(val gesture: String) {
    TAP("tap"),
    LEFT("flick_left"),
    UP("flick_up"),
    RIGHT("flick_right"),
    DOWN("flick_down"),
}

/**
 * 指の動きの判定結果。[cancelled] なら何も送らない。
 * [angleDeg] は右 = 0・上 = 90・左 = 180・下 = 270（反時計回り、0 以上 360 未満）、
 * [distance] はキー幅を 1 とした移動量。
 */
internal data class FlickResult(
    val cancelled: Boolean,
    val direction: FlickDirection,
    val angleDeg: Double,
    val distance: Double,
)

/** フリック判定（Android の型に依存しない純粋関数） */
internal object FlickLogic {
    /** キーサイズのこの倍率を超えて動かしたら取り消し */
    const val CANCEL_FACTOR = 2f

    /**
     * 実際に使う判定距離（dp）。設定の距離 [thresholdDp] を感度の倍率 [sensitivity]（0.5〜2.0）で割る
     * （倍率が大きいほど短い移動で反応する）。範囲外・不正な倍率は丸める
     */
    fun effectiveThresholdDp(thresholdDp: Float, sensitivity: Float): Float {
        val s = if (sensitivity.isNaN() || sensitivity <= 0f) 1f else sensitivity.coerceIn(0.5f, 2.0f)
        return thresholdDp / s
    }

    /**
     * 押した位置からの移動量（[dx], [dy]。画面座標で下が +）を判定する。
     * 縦横とも [thresholdPx] 未満ならタップ、超えたら大きい軸の方向。
     */
    fun judge(dx: Float, dy: Float, thresholdPx: Float, keyWidth: Float, keyHeight: Float): FlickResult {
        val ax = abs(dx)
        val ay = abs(dy)
        val angle = angleDeg(dx, dy)
        val dist = if (keyWidth > 0f) hypot(dx.toDouble(), dy.toDouble()) / keyWidth else 0.0
        if (ax > keyWidth * CANCEL_FACTOR || ay > keyHeight * CANCEL_FACTOR) {
            return FlickResult(true, FlickDirection.TAP, angle, dist)
        }
        if (ax < thresholdPx && ay < thresholdPx) {
            return FlickResult(false, FlickDirection.TAP, angle, dist)
        }
        val dir = if (ax >= ay) {
            if (dx < 0f) FlickDirection.LEFT else FlickDirection.RIGHT
        } else {
            if (dy < 0f) FlickDirection.UP else FlickDirection.DOWN
        }
        return FlickResult(false, dir, angle, dist)
    }

    /** 右 = 0・上 = 90・左 = 180・下 = 270 の角度（度）。画面座標（下が +）の移動量から求める */
    fun angleDeg(dx: Float, dy: Float): Double {
        var a = Math.toDegrees(atan2(-dy.toDouble(), dx.toDouble()))
        if (a < 0.0) a += 360.0
        if (a >= 360.0) a -= 360.0
        return a
    }

    /** コアへ送るジェスチャー名。トグル入力が ON のとき、タップは `toggle` に読み替える */
    fun gestureFor(r: FlickResult, toggleInput: Boolean): String =
        if (r.direction == FlickDirection.TAP && toggleInput) "toggle" else r.direction.gesture

    /** [def] のこの方向の文字 */
    fun charAt(def: KanaKeyDef, dir: FlickDirection): String = when (dir) {
        FlickDirection.TAP -> def.center
        FlickDirection.LEFT -> def.left
        FlickDirection.UP -> def.up
        FlickDirection.RIGHT -> def.right
        FlickDirection.DOWN -> def.down
    }
}
