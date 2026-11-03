package com.shiki01.nagi

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.StandardCopyOption

/**
 * データ資産（辞書・モデルなど）の差し替え。マニフェストの `data_assets` のうち、本体アプリの版に
 * 対応する（min_app / max_app）ものだけを対象にする。`.partial` → SHA-256 → ed25519 → rename の順で、
 * どれかに失敗したら旧データのまま残す
 */
internal object DataUpdater {
    private const val PREFS = "nagi_data_assets"
    private val main = Handler(Looper.getMainLooper())

    /** すべてメインスレッドで呼ばれる */
    interface Listener {
        /** total は不明なら -1 */
        fun onProgress(name: String, done: Long, total: Long)

        fun onAssetResult(name: String, success: Boolean, message: String)

        /** [updated] 件を差し替え、[failed] 件が失敗した。キャンセル時も呼ばれる */
        fun onFinished(updated: Int, failed: Int)
    }

    /** 入れてあるデータ資産の版。取得したことがなければ "0.0.0"（APK 同梱のものも含む） */
    fun installedVersion(ctx: Context, name: String): String =
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("version_$name", null) ?: "0.0.0"

    /** 更新できるデータ資産（互換範囲を満たし、入っている版より新しいもの） */
    fun pending(ctx: Context, manifest: Manifest): List<DataAsset> {
        val appVersion = UpdateChecker.currentVersion(ctx)
        return manifest.dataAssets.filter {
            it.isCompatible(appVersion) && Version.isNewer(it.version, installedVersion(ctx, it.name))
        }
    }

    /**
     * [assets] を順に差し替える。1 件でも差し替えたら最後に [onNeedRestart] をメインスレッドで呼ぶ
     * （コアは辞書やモデルを起動時に読むため、エンジンの再起動が必要）
     */
    fun update(ctx: Context, assets: List<DataAsset>, listener: Listener, onNeedRestart: () -> Unit): CancelToken {
        val app = ctx.applicationContext
        val token = CancelToken()
        Thread {
            var updated = 0
            var failed = 0
            for (asset in assets) {
                if (token.cancelled) break
                val dest = File(DataInstaller.dataDir(app), asset.dest)
                val partial = File(dest.parentFile, dest.name + ".partial")
                try {
                    if (!ManifestParser.isSafeDest(asset.dest)) throw UpdateException("配置先が不正です")
                    UpdateVerify.requirePublicKey(app)
                    val sha = Downloader.download(asset.url, partial, token) { done, total ->
                        main.post { listener.onProgress(asset.name, done, total) }
                    }
                    UpdateVerify.verify(app, partial, sha, asset.sha256, asset.signature)
                    if (token.cancelled) throw CancelledException()
                    replace(partial, dest)
                    app.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                        .putString("version_${asset.name}", asset.version).apply()
                    updated++
                    main.post { listener.onAssetResult(asset.name, true, "更新しました（${asset.version}）") }
                } catch (e: CancelledException) {
                    partial.delete()
                    break
                } catch (e: Exception) {
                    partial.delete()
                    failed++
                    val msg = (e as? UpdateException)?.message ?: "更新に失敗しました: ${e.message ?: e.javaClass.simpleName}"
                    main.post { listener.onAssetResult(asset.name, false, msg) }
                }
            }
            main.post {
                if (updated > 0) onNeedRestart()
                listener.onFinished(updated, failed)
            }
        }.start()
        return token
    }

    /** 同じディレクトリ内の rename で差し替える（読み込み中の旧ファイルは unlink されるだけで壊れない） */
    private fun replace(partial: File, dest: File) {
        try {
            Files.move(
                partial.toPath(),
                dest.toPath(),
                StandardCopyOption.REPLACE_EXISTING,
                StandardCopyOption.ATOMIC_MOVE,
            )
        } catch (e: IOException) {
            throw UpdateException("ファイルを差し替えられませんでした")
        }
    }
}
