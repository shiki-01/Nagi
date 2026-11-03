package com.shiki01.nagi

import android.app.Activity
import android.app.ActivityManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.graphics.Typeface
import android.os.Bundle
import android.os.Debug
import android.view.Gravity
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView

/** 開発者画面: 診断ログ・応答時間・メモリ・入力テスト。ホームの「開発者」の行（開発者モードがオンのとき）から開く */
class DeveloperActivity : Activity() {
    private lateinit var logView: TextView
    private lateinit var logSizeRow: UiRow

    private lateinit var latCount: UiRow
    private lateinit var latP50: UiRow
    private lateinit var latP95: UiRow
    private lateinit var latMax: UiRow
    private lateinit var coreProfile: UiRow
    private lateinit var coreLlm: UiRow
    private lateinit var coreLearned: UiRow

    private lateinit var memAvail: UiRow
    private lateinit var memTotal: UiRow
    private lateinit var memThreshold: UiRow
    private lateinit var memBudget: UiRow
    private lateinit var memPss: UiRow
    private lateinit var memHeap: UiRow
    private lateinit var memNative: UiRow

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (!AppPrefs.developerMode(this)) {
            finish()
            return
        }
        val parts = screenParts("開発者")
        val root = parts.content

        // ---- 診断ログ ----
        root.addSection(
            "診断ログ",
            card(
                switchRow(
                    "診断ログを記録",
                    "コアの起動・エラー・再起動などを、この端末の中の diag.log にだけ記録します",
                    AppPrefs.diagEnabled(this),
                ) {
                    AppPrefs.setDiagEnabled(this, it)
                    DiagLog.event(this, "diag", if (it) "記録を開始" else "記録を停止")
                },
                switchRow(
                    "入力内容を含める",
                    "入力した文字もログに残します（パスワード欄では残しません）",
                    AppPrefs.diagIncludeContent(this),
                ) { AppPrefs.setDiagIncludeContent(this, it) },
            ),
        )
        root.addView(
            banner(
                "「入力内容を含める」をオンにすると、入力した文字がログに残ります。" +
                    "共有やコピーをする前に、内容を確認してください。外部には送信されません。",
                Tone.WARNING,
            ),
        )

        logView = textOf("", 11f, cText)
        logView.typeface = Typeface.MONOSPACE
        logView.setTextIsSelectable(true)
        logSizeRow = infoRow("ログの大きさ", "")
        root.addView(sectionHeader("ログ"))
        root.addView(card(logSizeRow, cardBody(logView)))
        val actions = buttonRow(
            tonalButton("更新") { refresh() },
            tonalButton("コピー") { copyLog() },
            tonalButton("共有") { shareLog() },
        )
        root.addView(actions, LinearLayout.LayoutParams(LP_MATCH, LP_WRAP).also { it.topMargin = dp(12) })
        root.addView(
            destructiveButton("ログを消去") { confirmClear() },
            LinearLayout.LayoutParams(LP_MATCH, LP_WRAP).also { it.topMargin = dp(8) },
        )

        // ---- 応答時間 ----
        latCount = infoRow("計測した回数", "")
        latP50 = infoRow("p50（中央値）", "")
        latP95 = infoRow("p95", "")
        latMax = infoRow("最大", "")
        coreProfile = infoRow("コアのプロファイル", "")
        coreLlm = infoRow("LLM", "")
        coreLearned = infoRow("学習・ユーザー辞書", "")
        root.addSection(
            "応答時間（コアの呼び出し 1 回あたり・直近 ${LatencyLog.CAPACITY} 件）",
            card(latCount, latP50, latP95, latMax),
        )
        root.addSection("コアの状態（get_status）", card(coreProfile, coreLlm, coreLearned))
        root.addView(
            textButton("応答時間の記録を消す") {
                LatencyLog.clear()
                refresh()
            },
        )

        // ---- メモリ ----
        memAvail = infoRow("端末の空き", "")
        memTotal = infoRow("端末の総量", "")
        memThreshold = infoRow("低メモリのしきい値", "")
        memBudget = infoRow("メモリ予算（コアに渡す目安）", "")
        memPss = infoRow("このプロセスの使用量（PSS）", "")
        memHeap = infoRow("Java ヒープ", "")
        memNative = infoRow("ネイティブヒープ", "")
        root.addSection("メモリ", card(memAvail, memTotal, memThreshold, memBudget, memPss, memHeap, memNative))

        // ---- 入力テスト ----
        val test: EditText = inputField("ここで入力を試す")
        test.minLines = 3
        test.gravity = Gravity.TOP
        root.addSection(
            "入力テスト",
            card(cardBody(test)),
            "入力欄の種類は、診断ログがオンのときキーボードが記録します。",
        )

        setContentView(parts.root)
    }

    override fun onResume() {
        super.onResume()
        if (isFinishing) return
        refresh()
    }

    private fun refresh() {
        val text = DiagLog.read(this)
        logView.text = if (text.isEmpty()) "（記録なし）" else tailOf(text)
        logSizeRow.setValue("${formatBytes(DiagLog.sizeBytes(this))} / 上限 ${DiagFormat.MAX_BYTES / 1024} KB")

        val s = LatencyLog.summary()
        if (s == null) {
            latCount.setValue("0")
            latP50.setValue("未計測")
            latP95.setValue("未計測")
            latMax.setValue("未計測")
        } else {
            latCount.setValue(s.count.toString())
            latP50.setValue(LatencyStats.formatMs(s.p50))
            latP95.setValue(LatencyStats.formatMs(s.p95))
            latMax.setValue(LatencyStats.formatMs(s.max))
        }

        NagiRuntime.get(this).call("get_status") { r ->
            if (isDestroyed) return@call
            if (r == null) {
                coreProfile.setValue("取得できません", cDanger)
                coreLlm.setValue("-")
                coreLearned.setValue("-")
            } else {
                coreProfile.setValue(r.optString("profile"), cTextSecondary)
                coreProfile.setSubtitle(r.optString("profile_reason").ifEmpty { null })
                coreLlm.setValue(if (r.optBoolean("llm")) "使用中" else "未使用")
                coreLlm.setSubtitle("バックエンド: ${r.optString("backend")}")
                coreLearned.setValue("${r.optInt("learned_entries")} / ${r.optInt("user_dict_entries")} 件")
            }
        }

        val am = getSystemService(ACTIVITY_SERVICE) as ActivityManager
        val mi = ActivityManager.MemoryInfo()
        am.getMemoryInfo(mi)
        memAvail.setValue(formatBytes(mi.availMem))
        memAvail.setSubtitle(if (mi.lowMemory) "端末は低メモリの状態です" else null)
        memTotal.setValue(formatBytes(mi.totalMem))
        memThreshold.setValue(formatBytes(mi.threshold))
        memBudget.setValue("${ModelRecommendation.budgetMbFromAvail(mi.availMem)} MB")
        val dbg = Debug.MemoryInfo()
        Debug.getMemoryInfo(dbg)
        memPss.setValue(formatBytes(dbg.totalPss * 1024L))
        val rt = Runtime.getRuntime()
        memHeap.setValue(formatBytes(rt.totalMemory() - rt.freeMemory()))
        memNative.setValue(formatBytes(Debug.getNativeHeapAllocatedSize()))
    }

    /** 画面に出すのは末尾だけ（全文はコピー・共有で取る）。先頭は行の頭にそろえる */
    private fun tailOf(text: String): String {
        if (text.length <= SHOW_CHARS) return text
        val cut = text.substring(text.length - SHOW_CHARS)
        val nl = cut.indexOf('\n')
        return "…（前半は省略。全文はコピー・共有で取れます）\n" + if (nl >= 0) cut.substring(nl + 1) else cut
    }

    private fun copyLog() {
        val cm = getSystemService(CLIPBOARD_SERVICE) as ClipboardManager
        cm.setPrimaryClip(ClipData.newPlainText("Nagi 診断ログ", DiagLog.exportText(this)))
        toast("診断ログをコピーしました")
    }

    private fun shareLog() {
        val send = Intent(Intent.ACTION_SEND)
        send.type = "text/plain"
        send.putExtra(Intent.EXTRA_SUBJECT, "Nagi 診断ログ")
        send.putExtra(Intent.EXTRA_TEXT, DiagLog.exportText(this))
        startActivity(Intent.createChooser(send, "診断ログを共有"))
    }

    private fun confirmClear() {
        confirmDialog("診断ログを消去", "記録した診断ログをすべて消します。元に戻せません。", "消去", destructive = true) {
            DiagLog.clear(this)
            refresh()
        }
    }

    companion object {
        private const val SHOW_CHARS = 6000
    }
}
