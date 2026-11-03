package com.shiki01.nagi

import android.app.Activity
import android.app.ActivityManager
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import org.json.JSONObject
import java.io.File

/**
 * 変換モデル（LLM）の管理。モデルは `<filesDir>/data/models/` に置く。
 * 一覧・使用モデルの選択（`conversion.model` に絶対パス）・ダウンロード（SHA-256 検証）・インポート（.gguf）・削除。
 * モデルの変更後はエンジンを再起動する（`NagiRuntime.restart`）。
 */
class ModelActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())

    private lateinit var statusView: TextView
    private lateinit var llmBox: LinearLayout
    private lateinit var listBox: LinearLayout
    private lateinit var catalogBox: LinearLayout
    private lateinit var progressPanel: LinearLayout
    private lateinit var progressText: TextView
    private lateinit var progressBar: ProgressBar
    private lateinit var cancelButton: Button
    private lateinit var importButton: Button

    private var token: DataDownloader.Token? = null

    @Volatile
    private var busy = false

    private var useLlm = true
    private var configuredModel: String? = null
    private var llmRequiresRestart = false
    private var settingsLoaded = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val (scroll, root) = screen("変換モデル（LLM）")

        root.addView(
            body(
                "LLM（zenz）を使うと、文脈を考えた精度の高い変換になります。モデルが無い・メモリが足りないときは、" +
                    "辞書のみで動作します。",
                secondary = true,
            ),
        )
        statusView = body("")
        root.addView(statusView)

        llmBox = LinearLayout(this)
        llmBox.orientation = LinearLayout.VERTICAL
        root.addView(llmBox)

        root.addView(heading("インストール済みのモデル"))
        listBox = LinearLayout(this)
        listBox.orientation = LinearLayout.VERTICAL
        root.addView(listBox)

        progressPanel = LinearLayout(this)
        progressPanel.orientation = LinearLayout.VERTICAL
        progressPanel.visibility = View.GONE
        progressText = body("")
        progressBar = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal)
        progressBar.max = 1000
        cancelButton = button("キャンセル") { token?.cancel() }
        progressPanel.addView(progressText)
        progressPanel.addView(progressBar)
        progressPanel.addView(cancelButton)
        progressPanel.addView(body("処理中はこの画面を閉じないでください。", secondary = true))
        root.addView(progressPanel)

        root.addView(heading("ダウンロード"))
        catalogBox = LinearLayout(this)
        catalogBox.orientation = LinearLayout.VERTICAL
        root.addView(catalogBox)

        root.addView(heading("ファイルから追加"))
        root.addView(
            body("zenz の GGUF ファイル（.gguf）を選んで取り込みます。同じ名前のファイルは置き換えます。", secondary = true),
        )
        importButton = button("モデルをインポート（.gguf を選択）") { pickFile() }
        root.addView(importButton)

        root.addView(heading("再起動"))
        root.addView(
            body("モデルを変更すると自動でエンジンを再起動します。反映されないときは手動で再起動できます。", secondary = true),
        )
        root.addView(
            button("エンジンを再起動") {
                NagiRuntime.restart(this)
                toast("エンジンを再起動しました")
            },
        )

        setContentView(scroll)
        renderAll()
    }

    override fun onResume() {
        super.onResume()
        loadSettings()
    }

    override fun onDestroy() {
        token?.cancel()
        super.onDestroy()
    }

    // ---- 状態 ----

    private fun budgetMb(): Long {
        val am = getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val info = ActivityManager.MemoryInfo()
        am.getMemoryInfo(info)
        return ModelRecommendation.budgetMbFromAvail(info.availMem)
    }

    private fun loadSettings() {
        NagiRuntime.get(this).call("get_settings") { r ->
            if (r != null) {
                val items = SettingsSchema.parseItems(r)
                configuredModel = items.firstOrNull { it.key == "conversion.model" }?.value as? String
                val llm = items.firstOrNull { it.key == "conversion.use_llm" }
                useLlm = (llm?.value as? Boolean) ?: true
                llmRequiresRestart = llm?.requiresRestart ?: false
                settingsLoaded = true
            }
            renderAll()
        }
    }

    private fun renderAll() {
        val installed = ModelStore.list(this)
        val effective = ModelStore.effective(this, configuredModel)
        val budget = budgetMb()

        // 状態の要約
        val sb = StringBuilder()
        sb.append("この端末のメモリ予算: ").append(budget).append("MB（空きメモリの半分が目安）\n")
        val summary = ModelRecommendation.summary(budget, ModelRecommendation.CATALOG.map { ModelSpec(it.fileName, it.sizeBytes) })
        if (summary.isNotEmpty()) sb.append(summary).append('\n')
        sb.append("使用中のモデル: ").append(effective?.let { ModelRecommendation.label(it.name) } ?: "なし")
        statusView.text = sb.toString()
        val dictOnly = ModelRecommendation.dictionaryOnlyReason(effective != null, useLlm, budget)
        if (dictOnly != null) {
            statusView.append("\n" + dictOnly)
            statusView.setTextColor(COLOR_WARNING)
        } else {
            statusView.setTextColor(0xFF000000.toInt())
        }

        llmBox.removeAllViews()
        if (settingsLoaded) {
            llmBox.addView(
                switchRow("LLM を使う", "オフにすると、モデルがあっても辞書のみで変換します。", useLlm) { v ->
                    updateSetting("conversion.use_llm", SettingType.BOOL, v) { ok ->
                        if (ok) {
                            useLlm = v
                            if (llmRequiresRestart) NagiRuntime.restart(this)
                        }
                        renderAll()
                    }
                },
            )
        }

        // インストール済み
        listBox.removeAllViews()
        if (installed.isEmpty()) {
            listBox.addView(body("モデルはありません（辞書のみで動作します）。", secondary = true))
        }
        for (m in installed) {
            val inUse = effective?.file?.absolutePath == m.file.absolutePath
            val title = ModelRecommendation.label(m.name) + if (inUse) "（使用中）" else ""
            listBox.addView(body(title))
            listBox.addView(body("${m.name} / ${DataDownloader.formatBytes(m.sizeBytes)}", secondary = true))
            if (!ModelRecommendation.fits(m.spec, budget)) {
                listBox.addView(body("この端末の現在のメモリでは余裕がない見込みです。", secondary = true, color = COLOR_WARNING))
            }
            val buttons = ArrayList<View>()
            if (!inUse) buttons.add(button("使う") { selectModel(m) })
            buttons.add(button("削除") { confirmDelete(m) })
            val line = row(*buttons.toTypedArray())
            listBox.addView(line)
            listBox.addView(divider())
        }

        // ダウンロード
        catalogBox.removeAllViews()
        for (src in ModelRecommendation.CATALOG) {
            val have = installed.any { it.name == src.fileName }
            catalogBox.addView(body(src.label))
            catalogBox.addView(body("${src.fileName} / 約 ${DataDownloader.formatBytes(src.sizeBytes)}", secondary = true))
            val b = button(if (have) "再取得" else "ダウンロード") { startDownload(src) }
            b.isEnabled = !busy
            catalogBox.addView(b)
        }
        importButton.isEnabled = !busy
    }

    // ---- 設定の更新 ----

    /** `update_settings` を 1 項目送る。[value] が null なら既定へ戻す。[done] には「適用できたか」を返す */
    private fun updateSetting(key: String, type: SettingType, value: Any?, done: (Boolean) -> Unit) {
        val params = SettingsSchema.updateParams(listOf(Triple(key, type, value)))
        NagiRuntime.get(this).call("update_settings", params) { r: JSONObject? ->
            if (r == null) {
                toast("設定を保存できませんでした（コアが応答しません）")
                done(false)
                return@call
            }
            val errors = r.optJSONArray("errors")
            if (errors != null && errors.length() > 0) {
                val msgs = (0 until errors.length()).mapNotNull { errors.optJSONObject(it)?.optString("message") }
                toast(msgs.joinToString("\n"))
            }
            val applied = r.optJSONArray("applied")
            done(applied != null && applied.length() > 0)
        }
    }

    private fun selectModel(m: InstalledModel) {
        updateSetting("conversion.model", SettingType.TEXT, m.file.absolutePath) { ok ->
            if (ok) {
                configuredModel = m.file.absolutePath
                NagiRuntime.restart(this)
                toast("${ModelRecommendation.label(m.name)} を使います（エンジンを再起動しました）")
            }
            renderAll()
        }
    }

    private fun confirmDelete(m: InstalledModel) {
        AlertDialog.Builder(this)
            .setTitle("モデルを削除")
            .setMessage("${m.name}（${DataDownloader.formatBytes(m.sizeBytes)}）を削除します。")
            .setPositiveButton("削除") { _, _ -> deleteModel(m) }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    private fun deleteModel(m: InstalledModel) {
        val wasConfigured = configuredModel != null &&
            File(configuredModel!!).absolutePath == m.file.absolutePath
        val finish = {
            ModelStore.delete(this, m.name)
            NagiRuntime.restart(this)
            toast("削除しました（エンジンを再起動しました）")
            renderAll()
        }
        if (wasConfigured) {
            // 消えるファイルを指したままにしない（既定へ戻す）
            updateSetting("conversion.model", SettingType.TEXT, null) { _ ->
                configuredModel = null
                finish()
            }
        } else {
            finish()
        }
    }

    // ---- ダウンロード / インポート ----

    private fun showProgress(text: String, cancellable: Boolean) {
        progressPanel.visibility = View.VISIBLE
        progressText.text = text
        progressBar.isIndeterminate = true
        cancelButton.visibility = if (cancellable) View.VISIBLE else View.GONE
    }

    private fun updateProgress(label: String, done: Long, total: Long) {
        if (isDestroyed) return
        if (total > 0) {
            progressBar.isIndeterminate = false
            progressBar.progress = (done * 1000 / total).toInt().coerceIn(0, 1000)
            progressText.text = "$label ${DataDownloader.formatBytes(done)} / ${DataDownloader.formatBytes(total)}"
        } else {
            progressBar.isIndeterminate = true
            progressText.text = "$label ${DataDownloader.formatBytes(done)}"
        }
    }

    private fun startDownload(src: ModelSource) {
        if (busy) return
        val dir = ModelStore.modelsDir(this)
        dir.mkdirs()
        if (dir.usableSpace < src.sizeBytes + 32L * 1024 * 1024) {
            toast("空き容量が足りません（約 ${DataDownloader.formatBytes(src.sizeBytes)} 必要です）")
            return
        }
        busy = true
        val t = DataDownloader.Token()
        token = t
        showProgress("${ModelRecommendation.label(src.fileName)} をダウンロード中…", true)
        renderAll()
        val dest = File(dir, src.fileName)
        Thread {
            var error: String? = null
            var cancelled = false
            try {
                DataDownloader.download(src.url, dest, src.sha256, t) { d, total ->
                    main.post { updateProgress("ダウンロード中", d, total) }
                }
            } catch (e: DataDownloader.CancelledException) {
                cancelled = true
            } catch (e: Throwable) {
                error = e.message ?: e.javaClass.simpleName
            }
            main.post { finishInstall(dest, error, cancelled) }
        }.start()
    }

    private fun pickFile() {
        if (busy) return
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
        intent.addCategory(Intent.CATEGORY_OPENABLE)
        intent.type = "*/*"
        @Suppress("DEPRECATION")
        startActivityForResult(intent, REQUEST_IMPORT)
    }

    @Deprecated("Activity Result API を使わない素の Activity のため")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_IMPORT || resultCode != RESULT_OK) return
        val uri: Uri = data?.data ?: return
        startImport(uri)
    }

    private fun startImport(uri: Uri) {
        if (busy) return
        busy = true
        token = null
        showProgress("取り込み中…", false)
        renderAll()
        val app = applicationContext
        Thread {
            var error: String? = null
            var dest: File? = null
            try {
                dest = ModelStore.importFrom(app, uri) { copied ->
                    main.post { updateProgress("取り込み中", copied, -1L) }
                }
            } catch (e: Throwable) {
                error = e.message ?: e.javaClass.simpleName
            }
            main.post { finishInstall(dest, error, false) }
        }.start()
    }

    /** ダウンロード・インポートの完了処理。成功したら、使えるモデルが無ければそれを選び、エンジンを再起動する */
    private fun finishInstall(dest: File?, error: String?, cancelled: Boolean) {
        busy = false
        token = null
        if (isDestroyed) return
        progressPanel.visibility = View.GONE
        when {
            cancelled -> toast("キャンセルしました")
            error != null -> toast("失敗しました: $error")
            dest != null -> {
                val installed = ModelStore.list(this).firstOrNull { it.file.absolutePath == dest.absolutePath }
                if (installed != null && ModelStore.effective(this, configuredModel) == null) {
                    selectModel(installed)
                    return
                }
                NagiRuntime.restart(this)
                toast("モデルを追加しました（エンジンを再起動しました）")
            }
        }
        renderAll()
    }

    companion object {
        private const val REQUEST_IMPORT = 1001
    }
}
