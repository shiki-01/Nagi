package com.shiki01.nagi

import android.app.Activity
import android.app.AlertDialog
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import com.shiki01.nagi.core.NagiCore
import org.json.JSONObject
import java.io.File

/** 任意データ 1 種の取得元（コアの `nagi_optional_data_sources` の項目） */
internal data class OptionalSource(
    val kind: String,
    val dir: String,
    val file: String,
    val url: String,
    val sha256: String,
    val indexFile: String,
)

/**
 * 英語・カタカナ語データ（任意）。取得元はコアが返す。
 * ダウンロード → SHA-256 検証 → コアで索引化 → `<filesDir>/data/<dir>/<index_file>` に配置。
 */
class OptionalDataActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())

    private lateinit var listBox: LinearLayout
    private lateinit var progressPanel: LinearLayout
    private lateinit var progressText: TextView
    private lateinit var progressBar: ProgressBar
    private lateinit var cancelButton: Button

    private var sources: List<OptionalSource> = emptyList()
    private var loadError: String? = null
    private var token: DataDownloader.Token? = null

    @Volatile
    private var busy = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val (scroll, root) = screen("英語・カタカナ語データ")
        root.addView(
            body(
                "英単語・カタカナ語（外来語）の変換を強くする、任意のデータです。取得しなくても通常の変換は使えます。" +
                    "取得元から直接ダウンロードし、内容を検証してからこの端末の中で索引を作ります。",
                secondary = true,
            ),
        )

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

        setContentView(scroll)
        loadSources()
        render()
    }

    override fun onDestroy() {
        token?.cancel()
        super.onDestroy()
    }

    private fun loadSources() {
        try {
            sources = parseSources(NagiCore.nativeOptionalDataSources())
            if (sources.isEmpty()) loadError = "取得元の情報を読めませんでした"
        } catch (e: Throwable) {
            loadError = "コアから取得元を読めませんでした: ${e.message ?: e.javaClass.simpleName}"
        }
    }

    private fun parseSources(json: String): List<OptionalSource> {
        val o = JSONObject(json)
        val out = ArrayList<OptionalSource>()
        for (kind in listOf("english", "loanword")) {
            val s = o.optJSONObject(kind) ?: continue
            out.add(
                OptionalSource(
                    kind = kind,
                    dir = s.optString("dir"),
                    file = s.optString("file"),
                    url = s.optString("url"),
                    sha256 = s.optString("sha256"),
                    indexFile = s.optString("index_file"),
                ),
            )
        }
        return out.filter { it.dir.isNotEmpty() && it.file.isNotEmpty() && it.url.isNotEmpty() && it.indexFile.isNotEmpty() }
    }

    private fun titleOf(s: OptionalSource) = when (s.kind) {
        "english" -> "英語語彙（SCOWL）"
        "loanword" -> "カタカナ語（JMdict）"
        else -> s.kind
    }

    private fun description(s: OptionalSource) = when (s.kind) {
        "english" -> "英単語をカタカナ読みから変換したり、英語の綴りを補完したりするための単語リストです。"
        "loanword" -> "カタカナ語（外来語）とその英語表記を引くための辞書データです。"
        else -> ""
    }

    private fun indexFileOf(s: OptionalSource): File = File(File(DataInstaller.dataDir(this), s.dir), s.indexFile)

    private fun render() {
        listBox.removeAllViews()
        val err = loadError
        if (err != null) {
            listBox.addView(body(err, color = COLOR_ERROR))
            return
        }
        for (s in sources) {
            val idx = indexFileOf(s)
            listBox.addView(heading(titleOf(s)))
            listBox.addView(body(description(s), secondary = true))
            if (idx.exists()) {
                listBox.addView(body("状態: 取得済み（${DataDownloader.formatBytes(idx.length())}）"))
            } else {
                listBox.addView(body("状態: 未取得"))
            }
            val get = button(if (idx.exists()) "再取得" else "取得") { startInstall(s) }
            get.isEnabled = !busy
            val items = ArrayList<View>()
            items.add(get)
            if (idx.exists()) {
                val del = button("削除") { confirmDelete(s) }
                del.isEnabled = !busy
                items.add(del)
            }
            listBox.addView(row(*items.toTypedArray()))
            listBox.addView(divider())
        }
    }

    private fun confirmDelete(s: OptionalSource) {
        AlertDialog.Builder(this)
            .setTitle("${titleOf(s)}を削除")
            .setMessage("取得したデータを削除します。あとで再取得できます。")
            .setPositiveButton("削除") { _, _ ->
                indexFileOf(s).delete()
                File(File(DataInstaller.dataDir(this), s.dir), s.file).delete()
                NagiRuntime.restart(this)
                toast("削除しました（エンジンを再起動しました）")
                render()
            }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    private fun showProgress(text: String, cancellable: Boolean) {
        progressPanel.visibility = View.VISIBLE
        progressText.text = text
        progressBar.isIndeterminate = true
        cancelButton.visibility = if (cancellable) View.VISIBLE else View.GONE
    }

    private fun updateProgress(label: String, done: Long, total: Long) {
        if (isDestroyed || !busy) return
        if (total > 0) {
            progressBar.isIndeterminate = false
            progressBar.progress = (done * 1000 / total).toInt().coerceIn(0, 1000)
            progressText.text = "$label ${DataDownloader.formatBytes(done)} / ${DataDownloader.formatBytes(total)}"
        } else {
            progressBar.isIndeterminate = true
            progressText.text = "$label ${DataDownloader.formatBytes(done)}"
        }
    }

    private fun startInstall(s: OptionalSource) {
        if (busy) return
        val dir = File(DataInstaller.dataDir(this), s.dir)
        dir.mkdirs()
        if (dir.usableSpace < 300L * 1024 * 1024) {
            toast("空き容量が足りません（300MB 以上の空きを目安にしてください）")
            return
        }
        busy = true
        val t = DataDownloader.Token()
        token = t
        showProgress("${titleOf(s)} をダウンロード中…", true)
        render()
        val archive = File(dir, s.file)
        val out = indexFileOf(s)
        Thread {
            var error: String? = null
            var cancelled = false
            try {
                DataDownloader.download(s.url, archive, s.sha256, t) { d, total ->
                    main.post { updateProgress("ダウンロード中", d, total) }
                }
                // 検証済み。索引を作る（失敗しても既存の索引は残る: コアは一時ファイル経由で置き換える）
                main.post {
                    if (!isDestroyed) {
                        progressBar.isIndeterminate = true
                        progressText.text = "索引を作成中…（数十秒かかることがあります）"
                        cancelButton.visibility = View.GONE
                    }
                }
                val reason = NagiCore.nativeBuildOptionalIndex(s.kind, archive.absolutePath, out.absolutePath)
                if (reason != null) error = "索引を作れませんでした: $reason"
            } catch (e: DataDownloader.CancelledException) {
                cancelled = true
            } catch (e: Throwable) {
                error = e.message ?: e.javaClass.simpleName
            } finally {
                // アーカイブは索引の元になるだけなので、容量節約のため消す
                archive.delete()
            }
            main.post { onInstallDone(error, cancelled) }
        }.start()
    }

    private fun onInstallDone(error: String?, cancelled: Boolean) {
        busy = false
        token = null
        if (isDestroyed) return
        progressPanel.visibility = View.GONE
        when {
            cancelled -> toast("キャンセルしました")
            error != null -> toast("失敗しました: $error")
            else -> {
                NagiRuntime.restart(this)
                toast("取得しました（エンジンを再起動しました）")
            }
        }
        render()
    }
}
