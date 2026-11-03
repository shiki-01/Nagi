package com.shiki01.nagi

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.View
import android.widget.LinearLayout
import android.widget.Toast
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** アップデート画面。本体 APK とデータ資産（辞書など）の確認・取得・インストール */
class UpdateActivity : Activity() {
    private lateinit var currentRow: UiRow
    private lateinit var latestRow: UiRow
    private lateinit var lastCheckRow: UiRow
    private lateinit var notesButton: UiButton
    private lateinit var checkButton: UiButton
    private lateinit var installButton: UiButton
    private lateinit var progress: UiProgress
    private lateinit var assetsBox: LinearLayout
    private lateinit var messageBanner: UiBanner

    private var manifest: Manifest? = null
    private var token: CancelToken? = null
    private var busy = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("アップデート")
        val root = parts.content

        messageBanner = banner("", Tone.INFO)
        messageBanner.visibility = View.GONE
        root.addView(messageBanner)

        currentRow = infoRow("現在の版", UpdateChecker.currentVersion(this))
        latestRow = infoRow("最新の版", "未確認")
        lastCheckRow = infoRow("最終確認", "なし")
        root.addSection(
            "バージョン",
            card(currentRow, latestRow, lastCheckRow),
            "確認先: ${UpdateChecker.manifestUrl()}",
        )

        installButton = filledButton("ダウンロードしてインストール") { startInstall() }
        checkButton = tonalButton("今すぐ確認") { checkNow() }
        notesButton = textButton("リリースノートを開く") { openNotes() }
        val blp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        blp.topMargin = dp(16)
        root.addView(installButton, blp)
        root.addView(checkButton, LinearLayout.LayoutParams(LP_MATCH, LP_WRAP).also { it.topMargin = dp(8) })
        root.addView(notesButton, LinearLayout.LayoutParams(LP_MATCH, LP_WRAP).also { it.topMargin = dp(4) })

        progress = UiProgress(this, null) { token?.cancel() }
        root.addView(progress)

        root.addSection(
            "自動確認",
            card(
                switchRow(
                    "アップデートを自動で確認",
                    "24 時間に 1 回まで確認します",
                    KeyboardPrefs.autoCheckUpdates(this),
                ) { KeyboardPrefs.setAutoCheckUpdates(this, it) },
            ),
        )

        root.addView(sectionHeader("データ（辞書など）"))
        assetsBox = LinearLayout(this)
        assetsBox.orientation = LinearLayout.VERTICAL
        root.addView(assetsBox)

        setContentView(parts.root)

        render()
        // 自動確認が有効で間隔を過ぎていれば、開いたときに結果を出す
        UpdateChecker.checkIfDue(this) { info ->
            if (info != null && !isDestroyed && manifest == null) {
                manifest = info.manifest
                render()
            }
        }
    }

    override fun onDestroy() {
        // 検証前のダウンロードは止める（インストールの確認画面に渡した後は影響しない）
        token?.cancel()
        super.onDestroy()
    }

    private fun checkNow() {
        setBusy(true)
        progress.begin("確認しています…", false)
        UpdateChecker.checkNow(this) { r ->
            if (!isDestroyed) {
                progress.end()
                setBusy(false)
                when (r) {
                    is UpdateChecker.Result.Available -> {
                        manifest = r.info.manifest
                        messageBanner.show("新しい版 ${r.info.version} があります", Tone.INFO)
                    }
                    is UpdateChecker.Result.UpToDate -> {
                        manifest = r.manifest
                        messageBanner.show("アプリは最新です", Tone.SUCCESS)
                    }
                    is UpdateChecker.Result.Failed -> messageBanner.show(r.reason, Tone.ERROR)
                }
                render()
            }
        }
    }

    private fun startInstall() {
        val m = manifest ?: return
        if (!ApkInstaller.canRequestInstall(this)) {
            Toast.makeText(this, "このアプリからのインストールを許可してください", Toast.LENGTH_LONG).show()
            startActivity(ApkInstaller.unknownSourcesIntent(this))
            return
        }
        setBusy(true)
        progress.begin("ダウンロード中…", false)
        token = ApkInstaller.start(this, m, object : ApkInstaller.Listener {
            override fun onStage(message: String) {
                if (!isDestroyed) progress.textView.text = message
            }

            override fun onProgress(done: Long, total: Long) {
                if (!isDestroyed) showProgress("ダウンロード中", done, total)
            }

            override fun onFinished(success: Boolean, message: String) {
                if (isDestroyed) return
                token = null
                progress.end()
                setBusy(false)
                messageBanner.show(message, if (success) Tone.SUCCESS else Tone.ERROR)
                render()
            }
        })
        render()
    }

    private fun updateData(assets: List<DataAsset>) {
        if (assets.isEmpty()) return
        setBusy(true)
        progress.begin("データを取得しています…", false)
        messageBanner.hide()
        token = DataUpdater.update(
            this,
            assets,
            object : DataUpdater.Listener {
                override fun onProgress(name: String, done: Long, total: Long) {
                    if (!isDestroyed) showProgress(name, done, total)
                }

                override fun onAssetResult(name: String, success: Boolean, message: String) {
                    if (isDestroyed) return
                    if (messageBanner.visibility == View.VISIBLE) {
                        messageBanner.appendMessage("$name: $message")
                    } else {
                        messageBanner.show("$name: $message", Tone.INFO)
                    }
                }

                override fun onFinished(updated: Int, failed: Int) {
                    if (isDestroyed) return
                    token = null
                    progress.end()
                    setBusy(false)
                    val summary = "完了: 更新 $updated 件 / 失敗 $failed 件"
                    if (messageBanner.visibility == View.VISIBLE) {
                        messageBanner.appendMessage(summary)
                        messageBanner.setTone(if (failed > 0) Tone.ERROR else Tone.SUCCESS)
                    } else {
                        messageBanner.show(summary, if (failed > 0) Tone.ERROR else Tone.SUCCESS)
                    }
                    render()
                }
            },
            onNeedRestart = { restartEngine() },
        )
        render()
    }

    /** データ差し替え後にエンジンを再起動する（再起動の実体は NagiRuntime 側） */
    private fun restartEngine() {
        NagiRuntime.restart(this)
    }

    private fun openNotes() {
        val url = manifest?.notesUrl ?: return
        if (!url.startsWith("https://") && !url.startsWith("http://")) return
        try {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        } catch (e: ActivityNotFoundException) {
            Toast.makeText(this, "ブラウザが見つかりません", Toast.LENGTH_SHORT).show()
        }
    }

    private fun showProgress(label: String, done: Long, total: Long) {
        val text = if (total > 0) {
            "$label: ${done / 1024 / 1024} / ${total / 1024 / 1024} MB"
        } else {
            "$label: ${done / 1024 / 1024} MB"
        }
        progress.report(text, done, total)
        // 進捗が出ている間は、取り消せるときだけキャンセルを出す
        progress.cancelButton.visibility = if (token != null) View.VISIBLE else View.GONE
    }

    private fun setBusy(b: Boolean) {
        busy = b
        render()
    }

    /** 状態（manifest / busy）から表示を組み立て直す */
    private fun render() {
        val m = manifest
        val newer = m != null && Version.isNewer(m.version, UpdateChecker.currentVersion(this))
        if (m == null) {
            latestRow.setValue("未確認")
            latestRow.setSubtitle(null)
        } else if (newer) {
            latestRow.setValue(m.version, cAccent)
            latestRow.setSubtitle("新しい版があります")
        } else {
            latestRow.setValue(m.version, cTextSecondary)
            latestRow.setSubtitle("最新です")
        }
        val last = KeyboardPrefs.lastUpdateCheck(this)
        lastCheckRow.setValue(
            if (last > 0) SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.JAPAN).format(Date(last)) else "なし",
        )
        val notesOk = m?.notesUrl?.let { it.startsWith("https://") || it.startsWith("http://") } == true
        notesButton.visibility = if (notesOk) View.VISIBLE else View.GONE
        checkButton.isEnabled = !busy
        installButton.visibility = if (newer && m?.downloadUrl != null) View.VISIBLE else View.GONE
        installButton.isEnabled = !busy

        assetsBox.removeAllViews()
        val pending = if (m != null) DataUpdater.pending(this, m) else emptyList()
        val assetsCard = UiCard(this)
        if (m == null) {
            assetsCard.add(emptyState("「今すぐ確認」で更新を調べます"))
        } else if (pending.isEmpty()) {
            assetsCard.add(emptyState("更新できるデータはありません"))
        } else {
            for (a in pending) {
                val r = infoRow(a.name, null, "${DataUpdater.installedVersion(this, a.name)} → ${a.version}")
                val b = tonalButton("更新") { updateData(listOf(a)) }
                b.isEnabled = !busy
                r.setTrailing(b)
                assetsCard.add(r)
            }
        }
        assetsBox.addView(assetsCard)
        if (pending.size > 1) {
            val all = filledButton("すべて更新") { updateData(pending) }
            all.isEnabled = !busy
            assetsBox.addView(all, LinearLayout.LayoutParams(LP_MATCH, LP_WRAP).also { it.topMargin = dp(12) })
        }
    }
}
