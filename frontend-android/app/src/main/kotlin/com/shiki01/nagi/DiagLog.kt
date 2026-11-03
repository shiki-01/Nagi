package com.shiki01.nagi

import android.content.Context
import android.os.Build
import java.io.File
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** 診断ログの書式と切り詰め（android.* に依存しない純粋関数） */
internal object DiagFormat {
    /** ファイルの上限（バイト）。超えたら後半の [KEEP_BYTES] だけを残す */
    const val MAX_BYTES = 64 * 1024
    const val KEEP_BYTES = 32 * 1024

    /** 1 イベントのメッセージの最大文字数 */
    const val MAX_MESSAGE_CHARS = 300

    /** 改行・タブを空白にして長さを切り、1 行に収める */
    fun oneLine(s: String): String =
        s.replace('\r', ' ').replace('\n', ' ').replace('\t', ' ').take(MAX_MESSAGE_CHARS)

    /** `HH:mm:ss.SSS kind message`（1 行 1 イベント） */
    fun line(time: String, kind: String, message: String): String =
        "$time ${oneLine(kind).ifEmpty { "-" }.replace(' ', '_')} ${oneLine(message)}"

    /**
     * [bytes] が [maxBytes] を超えたら、後半の約 [keepBytes] を行の頭から残す。超えていなければそのまま返す。
     * 行の途中・UTF-8 の文字の途中から始まらないようにする
     */
    fun trimTail(bytes: ByteArray, maxBytes: Int = MAX_BYTES, keepBytes: Int = KEEP_BYTES): ByteArray {
        if (bytes.size <= maxBytes) return bytes
        val nl = '\n'.code.toByte()
        var start = (bytes.size - keepBytes).coerceAtLeast(0)
        if (start > 0 && bytes[start - 1] != nl) {
            var i = start
            while (i < bytes.size && bytes[i] != nl) i++
            if (i < bytes.size) {
                start = i + 1
            } else {
                // 改行が無い: UTF-8 の継続バイトを飛ばして文字の頭から
                var j = start
                while (j < bytes.size && (bytes[j].toInt() and 0xC0) == 0x80) j++
                start = j
            }
        }
        return bytes.copyOfRange(start, bytes.size)
    }
}

/**
 * 診断ログ（DG-01）。既定オフ。端末内の `filesDir/diag.log` にだけ追記し、外部へは送らない。
 * 入力内容は記録しない。例外は、開発者モードで「入力内容を含める」をオンにしたときの [detail] だけ
 * （パスワード欄では常に記録しない）。どの関数も例外を投げない。
 */
internal object DiagLog {
    private const val FILE_NAME = "diag.log"

    private val timeFormat = SimpleDateFormat("HH:mm:ss.SSS", Locale.ROOT)

    fun file(ctx: Context): File = File(ctx.applicationContext.filesDir, FILE_NAME)

    /** 1 イベントを記録する（診断ログがオフなら何もしない）。[message] に入力内容を入れないこと */
    fun event(ctx: Context, kind: String, message: String) {
        try {
            if (!AppPrefs.diagEnabled(ctx)) return
            append(ctx, kind, message)
        } catch (e: Throwable) {
            // ログの失敗で本処理を止めない
        }
    }

    /**
     * 入力内容を含むイベントを記録する。診断ログがオン、開発者モードがオン、「入力内容を含める」がオン、
     * かつ [secure]（パスワード欄など）でないときだけ記録する
     */
    fun detail(ctx: Context, kind: String, text: String, secure: Boolean) {
        try {
            if (secure) return
            if (!AppPrefs.diagEnabled(ctx) || !AppPrefs.diagIncludeContent(ctx)) return
            append(ctx, kind, text)
        } catch (e: Throwable) {
            // 同上
        }
    }

    @Synchronized
    private fun append(ctx: Context, kind: String, message: String) {
        val f = file(ctx)
        val text = DiagFormat.line(timeFormat.format(Date()), kind, message) + "\n"
        FileOutputStream(f, true).use { it.write(text.toByteArray(Charsets.UTF_8)) }
        if (f.length() > DiagFormat.MAX_BYTES) {
            val trimmed = DiagFormat.trimTail(f.readBytes())
            f.writeBytes(trimmed)
        }
    }

    /** ログ全体（古い順）。無ければ空文字 */
    @Synchronized
    fun read(ctx: Context): String = try {
        val f = file(ctx)
        if (f.exists()) String(f.readBytes(), Charsets.UTF_8) else ""
    } catch (e: Throwable) {
        ""
    }

    @Synchronized
    fun clear(ctx: Context) {
        try {
            file(ctx).delete()
        } catch (e: Throwable) {
            // 同上
        }
    }

    /** ファイルの大きさ（バイト）。無ければ 0 */
    fun sizeBytes(ctx: Context): Long = try {
        val f = file(ctx)
        if (f.exists()) f.length() else 0L
    } catch (e: Throwable) {
        0L
    }

    /** 共有・コピー用の文字列。端末と版の見出し付き */
    fun exportText(ctx: Context): String {
        val version = UpdateChecker.currentVersion(ctx)
        val head = "Nagi $version / Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}) / ${Build.MODEL}\n" +
            "入力内容を含める: ${if (AppPrefs.diagIncludeContent(ctx)) "はい" else "いいえ"}\n---\n"
        return head + read(ctx)
    }
}
