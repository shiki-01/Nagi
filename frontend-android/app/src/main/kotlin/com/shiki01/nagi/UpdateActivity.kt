package com.shiki01.nagi

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Typeface
import android.net.Uri
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** アップデート画面。本体 APK とデータ資産（辞書など）の確認・取得・インストール */
class UpdateActivity : Activity() {
    private lateinit var currentText: TextView
    private lateinit var latestText: TextView
    private lateinit var lastCheckText: TextView
    private lateinit var notesButton: Button
    private lateinit var checkButton: Button
    private lateinit var installButton: Button
    private lateinit var progressText: TextView
    private lateinit var progressBar: ProgressBar
    private lateinit var cancelButton: Button
    private lateinit var assetsBox: LinearLayout
    private lateinit var messageText: TextView

    private var manifest: Manifest? = null
    private var token: CancelToken? = null
    private var busy = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pad = (16 * resources.displayMetrics.density).toInt()
        val root = LinearLayout(this)
        root.orientation = LinearLayout.VERTICAL
        root.setPadding(pad, pad * 2, pad, pad)

        val title = TextView(this)
        title.text = "アップデート"
        title.textSize = 24f
        title.setTypeface(null, Typeface.BOLD)
        root.addView(title)

        currentText = TextView(this)
        currentText.setPadding(0, pad, 0, 0)
        currentText.text = "現在の版: ${UpdateChecker.currentVersion(this)}"
        root.addView(currentText)

        latestText = TextView(this)
        root.addView(latestText)

        lastCheckText = TextView(this)
        root.addView(lastCheckText)

        val urlText = TextView(this)
        urlText.text = "確認先: ${UpdateChecker.manifestUrl()}"
        urlText.textSize = 12f
        root.addView(urlText)

        notesButton = button("リリースノートを開く") { openNotes() }
        root.addView(notesButton)

        checkButton = button("今すぐ確認") { checkNow() }
        root.addView(checkButton)

        installButton = button("ダウンロードしてインストール") { startInstall() }
        root.addView(installButton)

        progressText = TextView(this)
        root.addView(progressText)
        progressBar = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal)
        progressBar.max = 100
        root.addView(progressBar)
        cancelButton = button("キャンセル") { token?.cancel() }
        root.addView(cancelButton)

        val auto = Switch(this)
        auto.text = "アップデートを自動で確認する（24 時間に 1 回まで）"
        auto.isChecked = KeyboardPrefs.autoCheckUpdates(this)
        auto.setOnCheckedChangeListener { _, checked -> KeyboardPrefs.setAutoCheckUpdates(this, checked) }
        auto.setPadding(0, pad, 0, pad)
        root.addView(auto)

        val assetsTitle = TextView(this)
        assetsTitle.text = "データ（辞書など）"
        assetsTitle.textSize = 18f
        assetsTitle.setTypeface(null, Typeface.BOLD)
        root.addView(assetsTitle)
        assetsBox = LinearLayout(this)
        assetsBox.orientation = LinearLayout.VERTICAL
        root.addView(assetsBox)

        messageText = TextView(this)
        messageText.setPadding(0, pad, 0, 0)
        root.addView(messageText)

        val scroll = ScrollView(this)
        scroll.addView(root)
        setContentView(scroll)

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
        messageText.text = "確認しています…"
        UpdateChecker.checkNow(this) { r ->
            if (!isDestroyed) {
                setBusy(false)
                when (r) {
                    is UpdateChecker.Result.Available -> {
                        manifest = r.info.manifest
                        messageText.text = "新しい版 ${r.info.version} があります"
                    }
                    is UpdateChecker.Result.UpToDate -> {
                        manifest = r.manifest
                        messageText.text = "アプリは最新です"
                    }
                    is UpdateChecker.Result.Failed -> messageText.text = r.reason
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
        resetProgress("ダウンロード中…")
        token = ApkInstaller.start(this, m, object : ApkInstaller.Listener {
            override fun onStage(message: String) {
                if (!isDestroyed) progressText.text = message
            }

            override fun onProgress(done: Long, total: Long) {
                if (!isDestroyed) showProgress("ダウンロード中", done, total)
            }

            override fun onFinished(success: Boolean, message: String) {
                if (isDestroyed) return
                token = null
                setBusy(false)
                progressText.text = ""
                messageText.text = message
                render()
            }
        })
        render()
    }

    private fun updateData(assets: List<DataAsset>) {
        if (assets.isEmpty()) return
        setBusy(true)
        resetProgress("データを取得しています…")
        token = DataUpdater.update(
            this,
            assets,
            object : DataUpdater.Listener {
                override fun onProgress(name: String, done: Long, total: Long) {
                    if (!isDestroyed) showProgress(name, done, total)
                }

                override fun onAssetResult(name: String, success: Boolean, message: String) {
                    if (!isDestroyed) messageText.append("\n$name: $message")
                }

                override fun onFinished(updated: Int, failed: Int) {
                    if (isDestroyed) return
                    token = null
                    setBusy(false)
                    progressText.text = ""
                    messageText.append("\n完了: 更新 $updated 件 / 失敗 $failed 件")
                    render()
                }
            },
            onNeedRestart = { restartEngine() },
        )
        messageText.text = ""
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

    private fun resetProgress(label: String) {
        progressText.text = label
        progressBar.isIndeterminate = true
    }

    private fun showProgress(label: String, done: Long, total: Long) {
        if (total > 0) {
            progressBar.isIndeterminate = false
            progressBar.progress = (done * 100 / total).toInt().coerceIn(0, 100)
            progressText.text = "$label: ${done / 1024 / 1024} / ${total / 1024 / 1024} MB"
        } else {
            progressBar.isIndeterminate = true
            progressText.text = "$label: ${done / 1024 / 1024} MB"
        }
    }

    private fun setBusy(b: Boolean) {
        busy = b
        render()
    }

    /** 状態（manifest / busy）から表示を組み立て直す */
    private fun render() {
        val m = manifest
        val newer = m != null && Version.isNewer(m.version, UpdateChecker.currentVersion(this))
        latestText.text = when {
            m == null -> "最新の版: 未確認"
            newer -> "最新の版: ${m.version}（新しい版があります）"
            else -> "最新の版: ${m.version}（最新です）"
        }
        val last = KeyboardPrefs.lastUpdateCheck(this)
        lastCheckText.text = if (last > 0) {
            "最終確認: " + SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.JAPAN).format(Date(last))
        } else {
            "最終確認: なし"
        }
        val notesOk = m?.notesUrl?.let { it.startsWith("https://") || it.startsWith("http://") } == true
        notesButton.visibility = if (notesOk) View.VISIBLE else View.GONE
        checkButton.isEnabled = !busy
        installButton.visibility = if (newer && m?.downloadUrl != null) View.VISIBLE else View.GONE
        installButton.isEnabled = !busy
        progressText.visibility = if (busy) View.VISIBLE else View.GONE
        progressBar.visibility = if (busy) View.VISIBLE else View.GONE
        cancelButton.visibility = if (busy && token != null) View.VISIBLE else View.GONE

        assetsBox.removeAllViews()
        val pending = if (m != null) DataUpdater.pending(this, m) else emptyList()
        if (m == null) {
            assetsBox.addView(label("「今すぐ確認」で更新を調べます"))
        } else if (pending.isEmpty()) {
            assetsBox.addView(label("更新できるデータはありません"))
        } else {
            for (a in pending) {
                val b = button("${a.name} を更新（${DataUpdater.installedVersion(this, a.name)} → ${a.version}）") {
                    updateData(listOf(a))
                }
                b.isEnabled = !busy
                assetsBox.addView(b)
            }
            if (pending.size > 1) {
                val all = button("すべて更新") { updateData(pending) }
                all.isEnabled = !busy
                assetsBox.addView(all)
            }
        }
    }

    private fun label(text: String): TextView {
        val t = TextView(this)
        t.text = text
        return t
    }

    private fun button(label: String, onClick: () -> Unit): Button {
        val b = Button(this)
        b.text = label
        b.setOnClickListener { onClick() }
        return b
    }
}
