package com.shiki01.nagi

import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest

/**
 * データ資産（モデル・任意データ）のダウンロード + SHA-256 検証 + 進捗通知。
 * 呼び出しは必ずバックグラウンドスレッドから行う（UI スレッド不可）。
 * `<dest>.partial` に書き、検証できたら [dest] に置き換える。失敗・キャンセル時は `.partial` を消し、既存の [dest] には触れない。
 * （アプリ更新用の `Downloader` とは別。あちらは署名検証つきの APK 取得専用）
 */
internal object DataDownloader {
    /** キャンセル用の旗。別スレッドから [cancel] する */
    class Token {
        @Volatile
        var cancelled = false
            private set

        fun cancel() {
            cancelled = true
        }
    }

    class CancelledException : IOException("キャンセルしました")

    private const val BUFFER = 64 * 1024
    private const val MAX_REDIRECTS = 8
    private const val PROGRESS_INTERVAL_MS = 100L

    /**
     * [url] を [dest] に取得する。[sha256]（小文字 16 進）と一致しなければ [IOException]。
     * [onProgress] は (取得済みバイト, 全体バイト or -1) をバックグラウンドスレッドから呼ぶ（間引き済み）。
     */
    fun download(
        url: String,
        dest: File,
        sha256: String?,
        token: Token,
        onProgress: (Long, Long) -> Unit,
    ) {
        dest.parentFile?.mkdirs()
        val partial = File(dest.parentFile, dest.name + ".partial")
        partial.delete()
        var conn: HttpURLConnection? = null
        try {
            conn = open(url)
            val total = conn.contentLengthLong
            val digest = MessageDigest.getInstance("SHA-256")
            var done = 0L
            var lastReport = 0L
            conn.inputStream.use { input ->
                partial.outputStream().use { out ->
                    val buf = ByteArray(BUFFER)
                    while (true) {
                        if (token.cancelled) throw CancelledException()
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        digest.update(buf, 0, n)
                        done += n
                        val now = System.currentTimeMillis()
                        if (now - lastReport >= PROGRESS_INTERVAL_MS) {
                            lastReport = now
                            onProgress(done, total)
                        }
                    }
                }
            }
            onProgress(done, if (total >= 0) total else done)
            if (token.cancelled) throw CancelledException()
            if (sha256 != null) {
                val actual = hex(digest.digest())
                if (!actual.equals(sha256, ignoreCase = true)) {
                    throw IOException("SHA-256 が一致しません（ファイルが壊れているか、取得元が変わっています）")
                }
            }
            replace(partial, dest)
        } catch (e: Throwable) {
            partial.delete()
            throw e
        } finally {
            conn?.disconnect()
        }
    }

    /** リダイレクトを自分で辿って接続する（http と https をまたぐ転送も追う。内容は SHA-256 で検証する） */
    private fun open(startUrl: String): HttpURLConnection {
        var current = startUrl
        for (hop in 0..MAX_REDIRECTS) {
            val c = URL(current).openConnection() as HttpURLConnection
            c.instanceFollowRedirects = false
            c.connectTimeout = 15_000
            c.readTimeout = 30_000
            c.setRequestProperty("User-Agent", "Nagi-Android")
            val code = c.responseCode
            if (code in 300..399) {
                val loc = c.getHeaderField("Location")
                c.disconnect()
                if (loc.isNullOrEmpty()) throw IOException("転送先が不明です（HTTP $code）")
                current = URL(URL(current), loc).toString()
                continue
            }
            if (code != HttpURLConnection.HTTP_OK) {
                c.disconnect()
                throw IOException("取得できませんでした（HTTP $code）")
            }
            return c
        }
        throw IOException("転送が多すぎます")
    }

    /** [src] を [dest] に置き換える（既存があれば上書き） */
    fun replace(src: File, dest: File) {
        Files.move(src.toPath(), dest.toPath(), StandardCopyOption.REPLACE_EXISTING)
    }

    fun hex(bytes: ByteArray): String {
        val digits = "0123456789abcdef"
        val sb = StringBuilder(bytes.size * 2)
        for (b in bytes) {
            val v = b.toInt() and 0xff
            sb.append(digits[v shr 4]).append(digits[v and 0x0f])
        }
        return sb.toString()
    }

    /** バイト数の表示（例: 70.4 MB） */
    fun formatBytes(n: Long): String = when {
        n >= 1024L * 1024 * 1024 -> String.format(java.util.Locale.ROOT, "%.1f GB", n / 1024.0 / 1024.0 / 1024.0)
        n >= 1024L * 1024 -> String.format(java.util.Locale.ROOT, "%.1f MB", n / 1024.0 / 1024.0)
        n >= 1024L -> String.format(java.util.Locale.ROOT, "%.0f KB", n / 1024.0)
        else -> "$n B"
    }
}
