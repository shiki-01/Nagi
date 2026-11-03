package com.shiki01.nagi

import android.app.Activity
import android.content.Intent
import android.graphics.Typeface
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import java.util.concurrent.Executors

/**
 * JSON を貼り付けて取り込む画面（iOS の `NagiLayoutPasteView` と同じ構成）。
 * 説明文・JSON の入力欄・「クリップボードから貼り付け」・「取り込む」。失敗はその場の赤いバナー、成功したら結果（[EXTRA_LAYOUT_ID]）を返して閉じる。
 * nagi:// のリンクが貼られたときもリンクとして扱う。回転では作り直さない（入力内容を保つ）
 */
class LayoutPasteActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor { r -> Thread(r, "nagi-layout-paste").apply { isDaemon = true } }

    private lateinit var scroll: ScrollView
    private lateinit var field: EditText
    private lateinit var errorBanner: UiBanner
    private lateinit var importButton: UiButton
    private var busy = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("JSON を貼り付け")
        scroll = parts.scroll
        val root = parts.content

        root.addView(
            body(
                "サイト（${LayoutClipboard.SITE_URL}）で作ったレイアウトの JSON を貼り付けて、「取り込む」を押してください。",
                secondary = true,
            ),
        )

        errorBanner = banner("", Tone.ERROR)
        errorBanner.visibility = View.GONE
        root.addView(errorBanner)

        field = inputField("レイアウトの JSON")
        field.minLines = 10
        field.gravity = Gravity.TOP or Gravity.START
        field.typeface = Typeface.MONOSPACE
        field.textSize = 13f
        field.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        field.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit

            override fun afterTextChanged(s: Editable?) {
                updateImportButton()
            }
        })
        val fieldLp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        fieldLp.topMargin = dp(8)
        root.addView(field, fieldLp)

        val pasteButton = tonalButton("クリップボードから貼り付け") { pasteFromClipboard() }
        val pasteLp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        pasteLp.topMargin = dp(12)
        root.addView(pasteButton, pasteLp)

        importButton = filledButton("取り込む") { startImport() }
        val importLp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        importLp.topMargin = dp(8)
        root.addView(importButton, importLp)

        setContentView(parts.root)
        updateImportButton()
    }

    private fun updateImportButton() {
        importButton.isEnabled = !busy && field.text.toString().isNotBlank()
    }

    private fun showError(message: String) {
        errorBanner.show(message, Tone.ERROR)
        scroll.smoothScrollTo(0, 0)
    }

    /** ボタンをタップしたときだけクリップボードを読む */
    private fun pasteFromClipboard() {
        val text = LayoutClipboardReader.readText(this)
        if (text.isNullOrBlank()) {
            errorBanner.show(LayoutClipboard.NOT_FOUND, Tone.WARNING)
            return
        }
        errorBanner.hide()
        field.setText(text)
        field.setSelection(field.text.length)
    }

    private fun <T> background(work: () -> T, done: (T) -> Unit) {
        busy = true
        updateImportButton()
        io.execute {
            val result = try {
                Result.success(work())
            } catch (e: Exception) {
                Result.failure(e)
            }
            main.post {
                if (isDestroyed) return@post
                busy = false
                updateImportButton()
                val value = result.getOrNull()
                if (result.isSuccess && value != null) {
                    done(value)
                } else {
                    showError("処理に失敗しました: ${result.exceptionOrNull()?.message ?: "不明なエラー"}")
                }
            }
        }
    }

    private fun startImport() {
        val text = field.text.toString()
        when (LayoutClipboard.classify(text)) {
            LayoutClipboardKind.EMPTY -> showError("テキストが空です")
            LayoutClipboardKind.LINK -> startLink(text)
            // JSON 以外に見える内容も、そのまま検証にかけて理由を出す
            LayoutClipboardKind.JSON, LayoutClipboardKind.OTHER ->
                if (LayoutIntake.looksLikeLink(text)) startLink(text) else inspectAndImport(text, viaLink = false)
        }
    }

    private fun startLink(text: String) {
        when (val r = LayoutIntake.classifyUrl(text)) {
            is IntakeResult.Reject -> showError(r.reason)
            IntakeResult.None -> showError("リンクが空です")
            is IntakeResult.Accept ->
                background({ LayoutImporter.parseDeepLink(r.url) }) { d ->
                    when (d) {
                        is DeepLinkResult.Err -> showError(d.reason)
                        is DeepLinkResult.Ok -> inspectAndImport(d.json, viaLink = true)
                    }
                }
        }
    }

    /** 検証 → （置き換え・上書き・リンク経由のときだけ確認）→ 保存 */
    private fun inspectAndImport(json: String, viaLink: Boolean) {
        background({ LayoutImporter.inspect(this, json, allowPresetId = !viaLink) }) { r ->
            when (r) {
                is InspectResult.Err -> showError(r.reason)
                is InspectResult.Ok -> {
                    val c = r.candidate
                    errorBanner.hide()
                    if (c.replaces || c.overridesPreset || viaLink || c.unsupported.isNotEmpty()) {
                        confirmDialog(
                            LayoutImportText.confirmTitle(viaLink),
                            LayoutImportText.confirmMessage(
                                c.layout.name,
                                c.layout.id,
                                c.layout.mode,
                                c.keyCount,
                                c.replaces,
                                c.overridesPreset,
                                viaLink,
                                c.unsupported,
                            ),
                            "取り込む",
                        ) { runImport(c.json, viaLink) }
                    } else {
                        runImport(c.json, viaLink)
                    }
                }
            }
        }
    }

    private fun runImport(json: String, viaLink: Boolean) {
        background({ LayoutImporter.importJson(this, json, allowPresetId = !viaLink) }) { r ->
            when (r) {
                is ImportResult.Success -> {
                    toast(LayoutImportText.success(r.candidate.layout.name, r.candidate.replaces) + r.iconNote)
                    setResult(RESULT_OK, Intent().putExtra(EXTRA_LAYOUT_ID, r.candidate.layout.id))
                    finish()
                }
                is ImportResult.Failure -> showError(r.reason)
            }
        }
    }

    companion object {
        /** 取り込んだレイアウトの id（結果の Intent） */
        const val EXTRA_LAYOUT_ID = "layout_id"
    }
}
