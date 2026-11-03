package com.shiki01.nagi

import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt

// 入力の操作感の判定ロジック（android.* に依存しない純粋な処理。単体テスト対象）。
// iOS の Keyboard/TouchLogic.swift・KeyboardModel.swift と同じ規則に揃える。

/**
 * スペースキーのスワイプによるカーソル移動の量を決める。
 * 押下位置からの横方向の移動量（右が正）を [update] に渡すと、前回から何文字ぶん動かすか（右が正、左が負）を返す。
 * 移動量が 1 ステップ（[step]。px）に届くまでは何もせず（= 単純タップのまま）、届いたらカーソル移動モードに入る。
 * 入ったあとはドラッグ距離に比例して動かし、指を戻せば戻る
 */
internal class SpaceSwipeTracker(step: Float) {
    private val step: Float = max(if (step.isNaN()) 1f else step, 1f)

    /** カーソル移動モードに入ったか（入ったら、離したときに空白を入力しない） */
    var isActive: Boolean = false
        private set
    private var movedSteps = 0

    /** 今回動かす文字数（右が正）。モードに入る前・変化なしは 0 */
    fun update(translationX: Float): Int {
        if (!translationX.isFinite()) return 0
        if (!isActive) {
            if (abs(translationX) < step) return 0
            isActive = true
        }
        // 0 に向かって丸める（Float.toInt() と同じ）
        val total = (translationX / step).toInt()
        val delta = total - movedSteps
        movedSteps = total
        return delta
    }
}

/** ⌫ の連打（押し続け）の間隔。最初は [start] ミリ秒おきで、続くほど速くする（iOS の RepeatKeyView と同じ） */
internal object RepeatSchedule {
    /** 暴走防止（離したイベントが届かなかった場合に、無限に消し続けない） */
    const val MAX_REPEATS = 150

    /** 加速の下限（最初の間隔の約 3 割、25ms 以上） */
    fun minInterval(startMs: Int): Int = max(25, startMs * 32 / 100)

    /** [count] 回実行した直後の、次の間隔。4 回ごとに 15% 縮め、下限で止める */
    fun nextInterval(currentMs: Int, startMs: Int, count: Int): Int {
        val floor = minInterval(startMs)
        return if (count % 4 == 0 && currentMs > floor) max(floor, currentMs * 85 / 100) else currentMs
    }
}

/** Shift の状態 */
internal enum class ShiftState { OFF, ONCE, CAPS }

/** Shift キーを 1 回押した結果。[lastTapAt] は次の 2 回押し判定に使う時刻（無ければ null） */
internal data class ShiftTap(val state: ShiftState, val lastTapAt: Long?)

/** Shift の Caps Lock 判定: 1 回で次の 1 文字だけ大文字、0.5 秒以内の 2 回連続で Caps Lock、Caps Lock 中の 1 回で解除 */
internal object ShiftLogic {
    /** 2 回押しとみなす間隔（ミリ秒）。iOS の `doubleTapInterval` */
    const val DOUBLE_TAP_MS = 500L

    fun isDoubleTap(lastTapAt: Long?, now: Long): Boolean {
        if (lastTapAt == null) return false
        val d = now - lastTapAt
        return d >= 0 && d < DOUBLE_TAP_MS
    }

    fun tap(state: ShiftState, lastTapAt: Long?, now: Long): ShiftTap = when (state) {
        ShiftState.OFF -> ShiftTap(ShiftState.ONCE, now)
        ShiftState.ONCE ->
            if (isDoubleTap(lastTapAt, now)) ShiftTap(ShiftState.CAPS, null) else ShiftTap(ShiftState.OFF, null)
        ShiftState.CAPS -> ShiftTap(ShiftState.OFF, null)
    }

    /** `capslock` キー: 固定 ⇄ 解除 */
    fun toggleCaps(state: ShiftState): ShiftState = if (state == ShiftState.CAPS) ShiftState.OFF else ShiftState.CAPS
}

/** 片手モード（`keyboard.one_handed`）の寄せ先 */
internal enum class OneHanded(val json: String, val label: String) {
    OFF("off", "オフ"),
    LEFT("left", "左に寄せる"),
    RIGHT("right", "右に寄せる"),
    ;

    /** 反対側へ寄せる（「⇄」）。オフはオフのまま */
    fun flipped(): OneHanded = when (this) {
        OFF -> OFF
        LEFT -> RIGHT
        RIGHT -> LEFT
    }

    companion object {
        fun fromJson(s: String?): OneHanded = values().firstOrNull { it.json == s } ?: OFF
    }
}

internal object OneHandedLogic {
    const val WIDTH_MIN = 0.6f
    const val WIDTH_MAX = 0.9f
    const val WIDTH_DEFAULT = 0.8f

    fun clampWidth(w: Float): Float = if (w.isNaN()) WIDTH_DEFAULT else w.coerceIn(WIDTH_MIN, WIDTH_MAX)

    /** キーボード本体の幅（全体を 1 とした比）。オフは 1 */
    fun bodyFraction(mode: OneHanded, width: Float): Float = if (mode == OneHanded.OFF) 1f else clampWidth(width)

    /** キーボード本体の幅（px）。端数は四捨五入 */
    fun bodyWidthPx(totalPx: Int, mode: OneHanded, width: Float): Int =
        (totalPx * bodyFraction(mode, width)).roundToInt().coerceIn(0, max(0, totalPx))
}

/** キーボードの高さのプリセット（選ぶと高さの倍率を設定する） */
internal enum class HeightPreset(val json: String, val label: String, val scale: Float) {
    COMPACT("compact", "低め（×0.9）", 0.9f),
    STANDARD("standard", "標準（×1.0）", 1.0f),
    TALL("tall", "高め（×1.1）", 1.1f),
    ;

    companion object {
        /** [scale] ちょうどのプリセット。スライダーで中間の値にしているときは null */
        fun matching(scale: Float): HeightPreset? = values().firstOrNull { abs(it.scale - scale) < 0.005f }

        fun fromJson(s: String?): HeightPreset? = values().firstOrNull { it.json == s }
    }
}

/** キーの間隔・角丸（`keyboard.key_spacing_pt` / `key_corner_pt`）から余白を決める */
internal object KeyMetrics {
    /** 左右の余白（dp。片側）。間隔の半分（4dp で 2dp = 従来の左右の余白） */
    fun horizontalMarginDp(spacingDp: Float): Float = spacingDp / 2f

    /** 上下の余白（dp。片側）。4dp で 3dp = 従来の上下の余白 */
    fun verticalMarginDp(spacingDp: Float): Float = spacingDp * 3f / 4f
}

/** ⌫ の左スワイプの判定 */
internal object SwipeLogic {
    /** 押した位置から [distancePx] 以上左へ動かし、かつ縦より横の動きが大きいか */
    fun isLeftSwipe(dx: Float, dy: Float, distancePx: Float): Boolean =
        dx.isFinite() && dy.isFinite() && dx < -distancePx && abs(dy) < abs(dx)
}
