package com.shiki01.nagi

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInstaller
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.core.content.ContextCompat
import java.io.File

/**
 * 本体 APK の取得・検証・インストール。
 * 検証（SHA-256 + ed25519）に通ったものだけを PackageInstaller に渡す。失敗時は何もインストールせず旧版のまま動く
 */
internal object ApkInstaller {
    private const val ACTION_STATUS = "com.shiki01.nagi.APK_INSTALL_STATUS"
    private val main = Handler(Looper.getMainLooper())

    /** すべてメインスレッドで呼ばれる */
    interface Listener {
        fun onStage(message: String)

        /** total は不明なら -1 */
        fun onProgress(done: Long, total: Long)

        /** success = true は「インストールが完了した」（通常はその前にアプリが入れ替わる）。false は失敗・取り消し */
        fun onFinished(success: Boolean, message: String)
    }

    /** 「提供元不明のアプリ」のインストールが許可されているか */
    fun canRequestInstall(ctx: Context): Boolean = ctx.packageManager.canRequestPackageInstalls()

    /** このアプリへのインストール許可を求める設定画面 */
    fun unknownSourcesIntent(ctx: Context): Intent =
        Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${ctx.packageName}"))

    /** ダウンロード → 検証 → インストールを開始する。戻り値でキャンセルできる（検証後は取り消せない） */
    fun start(ctx: Context, manifest: Manifest, listener: Listener): CancelToken {
        val app = ctx.applicationContext
        val token = CancelToken()
        Thread {
            val dir = File(app.cacheDir, "update")
            val partial = File(dir, "nagi-update.apk.partial")
            val apk = File(dir, "nagi-update.apk")
            try {
                val url = manifest.downloadUrl ?: throw UpdateException("ダウンロード URL がマニフェストにありません")
                UpdateVerify.requirePublicKey(app)
                main.post { listener.onStage("ダウンロード中…") }
                val sha = Downloader.download(url, partial, token) { done, total ->
                    main.post { listener.onProgress(done, total) }
                }
                main.post { listener.onStage("検証中…") }
                UpdateVerify.verify(app, partial, sha, manifest.sha256, manifest.signature)
                if (token.cancelled) throw CancelledException()
                apk.delete()
                if (!partial.renameTo(apk)) throw UpdateException("ダウンロードしたファイルを移動できませんでした")
                main.post { listener.onStage("インストールを準備中…") }
                commit(app, apk, listener)
            } catch (e: CancelledException) {
                partial.delete()
                apk.delete()
                main.post { listener.onFinished(false, "キャンセルしました") }
            } catch (e: Exception) {
                partial.delete()
                apk.delete()
                val msg = (e as? UpdateException)?.message ?: "インストールの準備に失敗しました: ${e.message ?: e.javaClass.simpleName}"
                main.post { listener.onFinished(false, msg) }
            }
        }.start()
        return token
    }

    private fun commit(app: Context, apk: File, listener: Listener) {
        val installer = app.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setSize(apk.length())
        val sessionId = installer.createSession(params)
        val session = installer.openSession(sessionId)

        var finished = false
        lateinit var receiver: BroadcastReceiver
        val finish = fun(success: Boolean, message: String) {
            if (finished) return
            finished = true
            try {
                app.unregisterReceiver(receiver)
            } catch (e: IllegalArgumentException) {
                // 登録済みでなければ何もしない
            }
            apk.delete()
            listener.onFinished(success, message)
        }
        receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
                val detail = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
                when (status) {
                    PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                        val confirm: Intent? = if (Build.VERSION.SDK_INT >= 33) {
                            intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                        } else {
                            @Suppress("DEPRECATION")
                            intent.getParcelableExtra(Intent.EXTRA_INTENT)
                        }
                        if (confirm == null) {
                            finish(false, "インストールの確認画面を開けませんでした")
                        } else {
                            confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            try {
                                context.startActivity(confirm)
                            } catch (e: Exception) {
                                finish(false, "インストールの確認画面を開けませんでした")
                            }
                        }
                    }
                    PackageInstaller.STATUS_SUCCESS -> finish(true, "インストールしました")
                    PackageInstaller.STATUS_FAILURE_ABORTED -> finish(false, "インストールが取り消されました")
                    else -> finish(false, "インストールに失敗しました（${detail ?: "status=$status"}）")
                }
            }
        }

        try {
            session.openWrite("nagi.apk", 0, apk.length()).use { out ->
                apk.inputStream().use { it.copyTo(out, 1 shl 16) }
                session.fsync(out)
            }
            ContextCompat.registerReceiver(app, receiver, IntentFilter(ACTION_STATUS), ContextCompat.RECEIVER_NOT_EXPORTED)
            // システムが結果の extras を埋めるため FLAG_MUTABLE。暗黙 Intent は不可なので package を固定する
            val statusIntent = Intent(ACTION_STATUS).setPackage(app.packageName)
            val pending = PendingIntent.getBroadcast(
                app,
                sessionId,
                statusIntent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
            )
            session.commit(pending.intentSender)
        } catch (e: Exception) {
            session.abandon()
            main.post { finish(false, "インストールの準備に失敗しました: ${e.message ?: e.javaClass.simpleName}") }
        } finally {
            session.close()
        }
    }
}
