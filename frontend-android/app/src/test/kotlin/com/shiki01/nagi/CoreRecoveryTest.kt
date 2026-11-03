package com.shiki01.nagi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CoreRecoveryTest {
    @Test
    fun バックオフは0_5秒から倍で最大8秒() {
        assertEquals(listOf(500L, 1000L, 2000L, 4000L, 8000L), (1..5).map { RecoveryState.backoffMs(it) })
        assertEquals(8000L, RecoveryState.backoffMs(9))
    }

    @Test
    fun しきい値に満たない失敗では何もしない() {
        val s = RecoveryState()
        assertEquals(RecoveryAction.None, s.onFailure())
        assertEquals(RecoveryAction.None, s.onFailure())
        assertEquals(2, s.consecutiveFailures)
    }

    @Test
    fun 成功が挟まれば連続の数え直し() {
        val s = RecoveryState()
        s.onFailure()
        s.onFailure()
        s.onSuccess()
        assertEquals(RecoveryAction.None, s.onFailure())
        assertEquals(RecoveryAction.None, s.onFailure())
        assertTrue(s.onFailure() is RecoveryAction.Restart)
    }

    @Test
    fun 三回続けて失敗すると作り直しを予約する() {
        val s = RecoveryState()
        s.onFailure()
        s.onFailure()
        val a = s.onFailure()
        assertEquals(RecoveryAction.Restart(500L, 1), a)
        assertTrue(s.pending)
    }

    @Test
    fun 予約中の失敗では二重に予約しない() {
        val s = RecoveryState()
        repeat(3) { s.onFailure() }
        repeat(10) { assertEquals(RecoveryAction.None, s.onFailure()) }
    }

    @Test
    fun 試行ごとに待ち時間が延び五回で断念する() {
        val s = RecoveryState()
        val delays = ArrayList<Long>()
        repeat(5) {
            var a: RecoveryAction = RecoveryAction.None
            repeat(3) { a = s.onFailure() }
            val r = a as RecoveryAction.Restart
            delays.add(r.delayMs)
            s.onRestartDone()
        }
        assertEquals(listOf(500L, 1000L, 2000L, 4000L, 8000L), delays)
        var a: RecoveryAction = RecoveryAction.None
        repeat(3) { a = s.onFailure() }
        assertEquals(RecoveryAction.GiveUp, a)
        assertTrue(s.gaveUp)
        // 断念の通知は 1 回だけ
        repeat(3) { a = s.onFailure() }
        assertEquals(RecoveryAction.None, a)
    }

    @Test
    fun 成功すると試行回数と断念が戻る() {
        val s = RecoveryState()
        repeat(3) { s.onFailure() }
        s.onRestartDone()
        s.onSuccess()
        assertEquals(0, s.attempts)
        repeat(2) { s.onFailure() }
        assertEquals(RecoveryAction.Restart(500L, 1), s.onFailure())
    }

    @Test
    fun 復元の失敗はしきい値を待たずに次へ進む() {
        val s = RecoveryState()
        repeat(3) { s.onFailure() }
        s.onRestartDone()
        assertEquals(RecoveryAction.Restart(1000L, 2), s.onFailure(force = true))
    }

    @Test
    fun resetは予約を残して数え直す() {
        val s = RecoveryState()
        repeat(3) { s.onFailure() }
        s.reset()
        assertEquals(0, s.attempts)
        assertTrue(s.pending)
    }

    // ---- 復元の内容 ----

    private fun seg(reading: String, surface: String, start: Int, end: Int, kind: String = "japanese") =
        PreeditSegment(surface, reading, start, end, kind)

    private fun preedit(text: String, cursor: Int, vararg s: PreeditSegment, focus: Int = -1) =
        Preedit(text, cursor, -1, -1, s.toList(), focus)

    @Test
    fun 入力中でなければ復元しない() {
        assertNull(RestoreLogic.plan(null, true))
        assertNull(RestoreLogic.plan(preedit("あ", 1, seg("あ", "あ", 0, 1)), composing = false))
        assertNull(RestoreLogic.plan(preedit("", 0), composing = true))
    }

    @Test
    fun 入力中の読みをそのまま復元する() {
        val p = RestoreLogic.plan(preedit("かんじ", 3, seg("かんじ", "かんじ", 0, 3)), true)!!
        assertEquals(listOf(RestoreSeg("かんじ", "かんじ", "japanese")), p.segments)
        assertEquals(3, p.cursor)
        assertFalse(p.converting)
        assertNull(p.focus)
    }

    @Test
    fun 文節が無ければ全体を一つの日本語セグメントにする() {
        val p = RestoreLogic.plan(preedit("あい", 1), true)!!
        assertEquals(1, p.segments.size)
        assertEquals("あい", p.segments[0].reading)
        assertEquals(1, p.cursor)
    }

    @Test
    fun 変換中は注目文節つきで復元する() {
        val pre = preedit(
            "今日は",
            3,
            seg("きょう", "今日", 0, 2),
            seg("は", "は", 2, 3),
            focus = 1,
        )
        val p = RestoreLogic.plan(pre, true)!!
        assertTrue(p.converting)
        assertEquals(1, p.focus)
        assertEquals(listOf("きょう", "は"), p.segments.map { it.reading })
        // カーソルは読みの連結上（きょうは = 4 文字）
        assertEquals(4, p.cursor)
    }

    @Test
    fun 英数混じりの種別を保つ() {
        val pre = preedit("1+あ", 3, seg("1+", "1+", 0, 2, "literal"), seg("あ", "あ", 2, 3))
        val p = RestoreLogic.plan(pre, true)!!
        assertEquals("literal", p.segments[0].kind)
        assertEquals("japanese", p.segments[1].kind)
    }

    @Test
    fun カーソルは読みのUnicodeスカラー単位に直す() {
        val segs = listOf(seg("あ𠮷い", "あ𠮷い", 0, 4))
        // UTF-16 で 3（あ + 𠮷 の 2 単位）= スカラー 2
        assertEquals(2, RestoreLogic.readingCursor(segs, 3, 4))
        assertEquals(0, RestoreLogic.readingCursor(segs, 0, 4))
        assertEquals(3, RestoreLogic.readingCursor(segs, 4, 4))
    }

    @Test
    fun 変換済み文節の途中のカーソルは読みの末尾に寄せる() {
        val segs = listOf(seg("きょう", "今日", 0, 2), seg("は", "は", 2, 3))
        assertEquals(3, RestoreLogic.readingCursor(segs, 1, 3))
        assertEquals(0, RestoreLogic.readingCursor(segs, 0, 3))
        assertEquals(4, RestoreLogic.readingCursor(segs, 3, 3))
        assertNotNull(RestoreLogic.plan(preedit("今日は", 1, *segs.toTypedArray(), focus = 0), true))
    }
}
