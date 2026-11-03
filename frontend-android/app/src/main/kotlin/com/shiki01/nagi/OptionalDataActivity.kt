package com.shiki01.nagi

import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.LinearLayout
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
    private lateinit var progress: UiProgress

    private var sources: List<OptionalSource> = emptyList()
    private var loadError: String? = null
    private var token: DataDownloader.Token? = null

    @Volatile
    private var busy = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("英語・カタカナ語データ")
        val root = parts.content
        root.addView(
            body(
                "英単語・カタカナ語（外来語）の変換を強くする、任意のデータです。取得しなくても通常の変換は使えます。" +
                    "取得元から直接ダウンロードし、内容を検証してからこの端末の中で索引を作ります。",
                secondary = true,
            ),
        )

        progress = UiProgress(this, "処理中はこの画面を閉じないでください。") { token?.cancel() }
        root.addView(progress)

        listBox = LinearLayout(this)
        listBox.orientation = LinearLayout.VERTICAL
        root.addView(listBox)

        setContentView(parts.root)
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
            listBox.addView(banner(err, Tone.ERROR))
            return
        }
        for (s in sources) {
            val idx = indexFileOf(s)
            val have = idx.exists()

            val head = LinearLayout(this)
            head.orientation = LinearLayout.HORIZONTAL
            head.gravity = android.view.Gravity.CENTER_VERTICAL
            head.addView(textOf(titleOf(s), UiSize.BODY_SP, cText, bold = true), LinearLayout.LayoutParams(0, LP_WRAP, 1f))
            head.addView(
                if (have) badge("取得済み", Tone.SUCCESS) else badge("未取得", Tone.NEUTRAL),
            )

            val desc = textOf(description(s), UiSize.NOTE_SP, cTextSecondary)
            desc.setPadding(0, dp(6), 0, 0)

            val get = (if (have) tonalButton("再取得") { startInstall(s) } else filledButton("取得") { startInstall(s) })
            get.isEnabled = !busy
            val buttons = ArrayList<View>()
            buttons.add(get)
            if (have) {
                val del = destructiveButton("削除") { confirmDelete(s) }
                del.isEnabled = !busy
                buttons.add(del)
            }
            val actions = buttonRow(*buttons.toTypedArray())
            val alp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
            alp.topMargin = dp(12)

            val box = cardBody(head, desc)
            if (have) {
                val size = textOf("サイズ ${DataDownloader.formatBytes(idx.length())}", UiSize.NOTE_SP, cTextSecondary)
                size.setPadding(0, dp(4), 0, 0)
                box.addView(size)
            }
            box.addView(actions, alp)

            val c = UiCard(this)
            c.add(box)
            val clp = c.layoutParams as LinearLayout.LayoutParams
            clp.topMargin = dp(12)
            listBox.addView(c)
        }
    }

    private fun confirmDelete(s: OptionalSource) {
        confirmDialog(
            "${titleOf(s)}を削除",
            "取得したデータを削除します。あとで再取得できます。",
            "削除",
            destructive = true,
        ) {
            indexFileOf(s).delete()
            File(File(DataInstaller.dataDir(this), s.dir), s.file).delete()
            NagiRuntime.restart(this)
            toast("削除しました（エンジンを再起動しました）")
            render()
        }
    }

    private fun updateProgress(label: String, done: Long, total: Long) {
        if (isDestroyed || !busy) return
        progress.report(
            if (total > 0) {
                "$label ${DataDownloader.formatBytes(done)} / ${DataDownloader.formatBytes(total)}"
            } else {
                "$label ${DataDownloader.formatBytes(done)}"
            },
            done,
            total,
        )
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
        progress.begin("${titleOf(s)} をダウンロード中…", true)
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
                        progress.begin("索引を作成中…（数十秒かかることがあります）", false)
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
        progress.end()
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
