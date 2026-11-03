package com.shiki01.nagi

import android.content.Context
import com.shiki01.nagi.core.NagiCore
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/** 更新処理の失敗（利用者に見せてよい日本語メッセージを持つ） */
internal class UpdateException(message: String) : Exception(message)

/** ダウンロード・インストールを途中でやめる */
internal class CancelledException : Exception("キャンセルしました")

internal class CancelToken {
    @Volatile
    var cancelled = false
        private set

    fun cancel() {
        cancelled = true
    }
}

/** 配布物のダウンロード（`.partial` に書き、SHA-256 を同時に計算する）。呼び出しはワーカースレッドで行う */
internal object Downloader {
    private const val CONNECT_TIMEOUT_MS = 15_000
    private const val READ_TIMEOUT_MS = 30_000
    private const val PROGRESS_INTERVAL_MS = 100L

    fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it.toInt() and 0xff) }

    fun openConnection(url: String): HttpURLConnection {
        if (!url.startsWith("https://")) throw UpdateException("https 以外の URL は使えません")
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = CONNECT_TIMEOUT_MS
        conn.readTimeout = READ_TIMEOUT_MS
        conn.instanceFollowRedirects = true
        conn.setRequestProperty("User-Agent", "Nagi-Android")
        return conn
    }

    /**
     * [url] を [partial] に書き出し、SHA-256（小文字 16 進）を返す。失敗・キャンセル時は [partial] を消す。
     * [onProgress] はワーカースレッドから間引いて呼ばれる（total は不明なら -1）
     */
    fun download(url: String, partial: File, token: CancelToken, onProgress: (done: Long, total: Long) -> Unit): String {
        partial.parentFile?.mkdirs()
        partial.delete()
        var conn: HttpURLConnection? = null
        try {
            conn = openConnection(url)
            val code = conn.responseCode
            if (code != HttpURLConnection.HTTP_OK) throw UpdateException("ダウンロードに失敗しました（HTTP $code）")
            val total = conn.contentLengthLong
            val md = MessageDigest.getInstance("SHA-256")
            var done = 0L
            var lastReport = 0L
            val buf = ByteArray(64 * 1024)
            conn.inputStream.use { input ->
                partial.outputStream().use { out ->
                    while (true) {
                        if (token.cancelled) throw CancelledException()
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        md.update(buf, 0, n)
                        done += n
                        val now = System.currentTimeMillis()
                        if (now - lastReport >= PROGRESS_INTERVAL_MS) {
                            lastReport = now
                            onProgress(done, total)
                        }
                    }
                }
            }
            if (total >= 0 && done != total) throw UpdateException("ダウンロードが途中で切れました")
            onProgress(done, total)
            return hex(md.digest())
        } catch (e: IOException) {
            partial.delete()
            throw UpdateException("通信に失敗しました: ${e.message ?: e.javaClass.simpleName}")
        } catch (e: Exception) {
            partial.delete()
            throw e
        } finally {
            conn?.disconnect()
        }
    }
}

/** 配布物の検証（SHA-256 照合 + ed25519 署名。署名対象は配布物のバイト列全体） */
internal object UpdateVerify {
    private const val KEY_ASSET = "update-public-key.txt"
    private val KEY_PATTERN = Regex("[0-9a-fA-F]{64}")

    /** assets の公開鍵（16 進 64 文字）。無い・壊れていれば null */
    fun publicKey(ctx: Context): String? = try {
        val text = ctx.assets.open(KEY_ASSET).bufferedReader().use { it.readText() }.trim()
        if (KEY_PATTERN.matches(text)) text else null
    } catch (e: IOException) {
        null
    }

    /** 検証鍵を返す。無ければ更新を拒否する */
    fun requirePublicKey(ctx: Context): String =
        publicKey(ctx) ?: throw UpdateException("更新の検証鍵がありません")

    /** [file] を検証する。通らなければ [UpdateException] */
    fun verify(ctx: Context, file: File, actualSha256: String, expectedSha256: String?, signature: String?) {
        val key = requirePublicKey(ctx)
        if (expectedSha256.isNullOrBlank() || signature.isNullOrBlank()) {
            throw UpdateException("マニフェストに SHA-256 または署名がありません")
        }
        if (!actualSha256.equals(expectedSha256.trim(), ignoreCase = true)) {
            throw UpdateException("SHA-256 が一致しません（ファイルが壊れているか改ざんされています）")
        }
        if (!NagiCore.nativeVerifyEd25519File(key, file.absolutePath, signature.trim())) {
            throw UpdateException("署名の検証に失敗しました")
        }
    }
}
