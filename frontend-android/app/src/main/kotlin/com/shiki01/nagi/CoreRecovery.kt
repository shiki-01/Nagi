package com.shiki01.nagi

// コアの障害からの復旧（OT-04）の判定。android.* に依存しない純粋なロジック（単体テスト対象）。
// 流れ: 失敗が続く → 入力中の文字はホストに残したまま、バックオフを置いてコアを作り直す →
// 作り直したコアへ残してあった読みを restore_session で戻す。

/** 失敗を受けたときの次の動作 */
internal sealed class RecoveryAction {
    /** 何もしない（しきい値に未達、または作り直しの予約済み・断念済み） */
    object None : RecoveryAction()

    /** [delayMs] 後にコアを作り直す。[attempt] は 1 から数える試行番号 */
    data class Restart(val delayMs: Long, val attempt: Int) : RecoveryAction()

    /** 試行の上限に達した。以後は縮退入力（押した文字をそのまま入れる）のまま。1 回だけ返す */
    object GiveUp : RecoveryAction()
}

/** 連続失敗の数え方と、作り直しの回数・待ち時間 */
internal class RecoveryState(
    private val threshold: Int = FAILURE_THRESHOLD,
    private val maxAttempts: Int = MAX_ATTEMPTS,
) {
    var consecutiveFailures = 0
        private set
    var attempts = 0
        private set
    var pending = false
        private set
    var gaveUp = false
        private set

    /** コアが正常に応答した。失敗の数と試行回数を戻す */
    fun onSuccess() {
        consecutiveFailures = 0
        attempts = 0
        gaveUp = false
    }

    /** コアの呼び出しが失敗した（例外・タイムアウト・null 応答） */
    fun onFailure(force: Boolean = false): RecoveryAction {
        // [force]: 作り直した直後の復元に失敗したなど、しきい値を待たずに次の作り直しへ進める
        consecutiveFailures = if (force) threshold else consecutiveFailures + 1
        if (pending || consecutiveFailures < threshold) return RecoveryAction.None
        if (attempts >= maxAttempts) {
            consecutiveFailures = 0
            if (gaveUp) return RecoveryAction.None
            gaveUp = true
            return RecoveryAction.GiveUp
        }
        attempts++
        consecutiveFailures = 0
        pending = true
        return RecoveryAction.Restart(backoffMs(attempts), attempts)
    }

    /** 予約した作り直しを実行した（次の失敗で再び予約できる） */
    fun onRestartDone() {
        pending = false
    }

    /** 入力欄が変わったなど、数え直す。予約済みの作り直し（[pending]）は残す（予約は実行時に [onRestartDone] で戻る） */
    fun reset() {
        consecutiveFailures = 0
        attempts = 0
        gaveUp = false
    }

    companion object {
        /** 何回続けて失敗したら作り直すか */
        const val FAILURE_THRESHOLD = 3
        const val MAX_ATTEMPTS = 5
        private const val BASE_MS = 500L
        private const val MAX_MS = 8000L

        /** 試行 [attempt]（1 から）の前の待ち時間: 0.5s → 1s → 2s → 4s → 8s（最大 8s） */
        fun backoffMs(attempt: Int): Long {
            val shift = (attempt - 1).coerceIn(0, 10)
            return minOf(BASE_MS shl shift, MAX_MS)
        }
    }
}

/** `restore_session` に渡す 1 セグメント。[kind] は `japanese` / `literal` / `fixed` / `styled` */
internal data class RestoreSeg(val reading: String, val surface: String, val kind: String)

/** `restore_session` の内容。[cursor] は読みの連結上の Unicode スカラー単位の位置 */
internal data class RestorePlan(
    val segments: List<RestoreSeg>,
    val cursor: Int,
    val focus: Int?,
    val converting: Boolean,
)

internal object RestoreLogic {
    /**
     * 入力中の文字列 [p] から復元の内容を作る。復元するものが無ければ（[composing] が false・空）null。
     * 文節が取れていなければ、全体を 1 つの日本語セグメントとして扱う
     */
    fun plan(p: Preedit?, composing: Boolean): RestorePlan? {
        if (p == null || !composing || p.text.isEmpty()) return null
        val segs = if (p.segments.isEmpty()) {
            listOf(PreeditSegment(p.text, p.text, 0, p.text.length))
        } else {
            p.segments
        }
        val converting = p.focus in segs.indices
        val restoreSegs = segs.map { RestoreSeg(it.reading, it.surface, it.kind) }
        return RestorePlan(
            segments = restoreSegs,
            cursor = readingCursor(segs, p.cursor, p.text.length),
            focus = if (converting) p.focus else null,
            converting = converting,
        )
    }

    /**
     * 表示上のカーソル（UTF-16 位置）を、読みの連結上の Unicode スカラー単位の位置にする。
     * 文節の途中にカーソルがあり、読みと表示が違う（変換済み）ときは、その文節の読みの末尾に寄せる
     */
    fun readingCursor(segs: List<PreeditSegment>, cursorUtf16: Int, textLength: Int): Int {
        val cur = cursorUtf16.coerceIn(0, textLength)
        var total = 0
        for (s in segs) {
            val len = s.reading.codePointCount(0, s.reading.length)
            val start = if (s.start >= 0) s.start else 0
            val end = if (s.end >= 0) s.end else start + s.surface.length
            when {
                cur >= end -> total += len
                cur > start -> {
                    if (s.reading == s.surface) {
                        val off = (cur - start).coerceIn(0, s.surface.length)
                        total += s.surface.codePointCount(0, off)
                    } else {
                        total += len
                    }
                    return total
                }
                else -> return total
            }
        }
        return total
    }
}
