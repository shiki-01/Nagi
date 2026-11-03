package com.shiki01.nagi

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.io.ByteArrayOutputStream
import java.io.IOException

/** 更新確認。マニフェスト（latest.json）を取得して、本体の新版とデータ資産の更新を調べる */
internal object UpdateChecker {
    /** コアの `update_check::default_check_url`（os = android）と同じ規約 */
    const val DEFAULT_URL = "https://nagi.shiki-01.com/updates/android/latest.json"

    /** 自動確認の最短間隔（24 時間） */
    private const val MIN_INTERVAL_MS = 24L * 60 * 60 * 1000
    private const val MAX_MANIFEST_BYTES = 1 shl 20

    sealed class Result {
        /** 本体の新版がある */
        data class Available(val info: UpdateInfo) : Result()

        /** 本体は最新。データ資産の更新確認には [manifest] を使う */
        data class UpToDate(val manifest: Manifest) : Result()

        data class Failed(val reason: String) : Result()
    }

    private val main = Handler(Looper.getMainLooper())

    fun manifestUrl(): String = DEFAULT_URL

    fun currentVersion(ctx: Context): String = try {
        ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName ?: "0.0.0"
    } catch (e: Exception) {
        "0.0.0"
    }

    /**
     * 自動確認が有効で、前回の確認から 24 時間以上経っているときだけネットワークに出る。
     * 本体の新版があれば [onResult] にその情報、それ以外（間引き・最新・失敗）は null。必ずメインスレッドで呼ぶ
     */
    fun checkIfDue(ctx: Context, onResult: (UpdateInfo?) -> Unit) {
        val app = ctx.applicationContext
        val now = System.currentTimeMillis()
        val last = KeyboardPrefs.lastUpdateCheck(app)
        val due = KeyboardPrefs.autoCheckUpdates(app) && (last > now || now - last >= MIN_INTERVAL_MS)
        if (!due) {
            main.post { onResult(null) }
            return
        }
        fetch(app) { r -> onResult((r as? Result.Available)?.info) }
    }

    /** 間隔や設定にかかわらず今すぐ確認する。[onResult] はメインスレッド */
    fun checkNow(ctx: Context, onResult: (Result) -> Unit) {
        fetch(ctx.applicationContext, onResult)
    }

    private fun fetch(app: Context, onResult: (Result) -> Unit) {
        Thread {
            val result = try {
                val manifest = ManifestParser.parse(download(manifestUrl()))
                    ?: throw UpdateException("マニフェストを読めませんでした")
                KeyboardPrefs.setLastUpdateCheck(app, System.currentTimeMillis())
                if (Version.isNewer(manifest.version, currentVersion(app))) {
                    Result.Available(UpdateInfo(manifest.version, manifest.notesUrl, manifest))
                } else {
                    Result.UpToDate(manifest)
                }
            } catch (e: UpdateException) {
                Result.Failed(e.message ?: "確認に失敗しました")
            } catch (e: Exception) {
                // ネットワーク失敗は黙って失敗扱い（呼び出し側は表示するか無視するかを決める）
                Result.Failed("確認に失敗しました")
            }
            main.post { onResult(result) }
        }.start()
    }

    private fun download(url: String): String {
        val conn = Downloader.openConnection(url)
        try {
            if (conn.responseCode != 200) throw UpdateException("マニフェストを取得できませんでした（HTTP ${conn.responseCode}）")
            val out = ByteArrayOutputStream()
            val buf = ByteArray(8192)
            conn.inputStream.use { input ->
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    if (out.size() > MAX_MANIFEST_BYTES) throw UpdateException("マニフェストが大きすぎます")
                }
            }
            return out.toString("UTF-8")
        } catch (e: IOException) {
            throw UpdateException("通信に失敗しました")
        } finally {
            conn.disconnect()
        }
    }
}
