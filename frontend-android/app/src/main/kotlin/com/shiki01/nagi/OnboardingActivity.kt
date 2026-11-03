package com.shiki01.nagi

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.view.Gravity
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.LinearLayout

/** 使い始めの手順。各手順の達成状態を検出して色付きのバッジで示し、戻ってくるたびに更新する */
class OnboardingActivity : Activity() {
    private lateinit var stepsBox: LinearLayout
    private lateinit var testField: EditText
    private lateinit var closeButton: UiButton

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // 途中で閉じても、次回から自動では出さない（ホームの行からいつでも開き直せる）
        AppPrefs.setOnboardingDone(this, true)

        val parts = screenParts("使い始めの手順")
        val root = parts.content
        root.addView(body("Nagi を使えるようにするまでの手順です。済んだ手順には「済み」が付きます。", secondary = true))

        testField = inputField("ここで入力を試す")
        testField.minLines = 3
        testField.gravity = Gravity.TOP

        stepsBox = LinearLayout(this)
        stepsBox.orientation = LinearLayout.VERTICAL
        root.addView(stepsBox)

        closeButton = filledButton("あとで") { finish() }
        val lp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        lp.topMargin = dp(24)
        root.addView(closeButton, lp)

        setContentView(parts.root)
    }

    override fun onResume() {
        super.onResume()
        render()
    }

    private fun isEnabled(): Boolean {
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        return imm.enabledInputMethodList.any { it.packageName == packageName }
    }

    private fun isSelected(): Boolean {
        val id = Settings.Secure.getString(contentResolver, Settings.Secure.DEFAULT_INPUT_METHOD) ?: return false
        return id.contains(packageName)
    }

    private fun render() {
        val enabled = isEnabled()
        val selected = isSelected()
        val dictOk = DataInstaller.dictFile(this).exists()
        val modelOk = ModelStore.hasModel(this)

        // 入力欄は作り直さず付け替える（入力中の文字を保つ）
        (testField.parent as? ViewGroup)?.removeView(testField)
        stepsBox.removeAllViews()

        addStep(
            1, "キーボードを有効にする", "設定の入力方法の一覧で Nagi をオンにします。",
            done = enabled, optional = false,
            actionLabel = if (enabled) null else "設定を開く",
        ) { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) }
        addStep(
            2, "Nagi を選ぶ", "入力中のキーボードを Nagi に切り替えます。",
            done = selected, optional = false,
            actionLabel = if (selected) null else "入力方法を選ぶ",
        ) { (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).showInputMethodPicker() }
        addStep(
            3, "辞書の確認", "かな漢字変換に使う辞書です。通常はアプリに同梱されています。",
            done = dictOk, optional = false,
            actionLabel = if (dictOk) null else "アップデートで取得",
        ) { startActivity(Intent(this, UpdateActivity::class.java)) }
        addStep(
            4, "変換モデルを入れる（任意）", "入れると変換の精度が上がります。入れなくても辞書だけで動きます。",
            done = modelOk, optional = true,
            actionLabel = if (modelOk) null else "モデルの画面を開く",
        ) { startActivity(Intent(this, ModelActivity::class.java)) }
        addStep(
            5, "入力テスト", "Nagi に切り替えてから、ここで入力と変換を試します。",
            done = false, optional = true, actionLabel = null, extra = testField,
        ) {}

        closeButton.text = if (enabled && selected && dictOk) "完了" else "あとで"
    }

    private fun addStep(
        number: Int,
        title: String,
        description: String,
        done: Boolean,
        optional: Boolean,
        actionLabel: String?,
        extra: android.view.View? = null,
        onAction: () -> Unit,
    ) {
        val header = LinearLayout(this)
        header.orientation = LinearLayout.HORIZONTAL
        header.gravity = Gravity.CENTER_VERTICAL
        val num = textOf(number.toString(), UiSize.BODY_SP, cAccent, bold = true)
        num.gravity = Gravity.CENTER
        num.background = oval(cAccentSoft)
        header.addView(num, LinearLayout.LayoutParams(dp(28), dp(28)))
        val t = textOf(title, UiSize.BODY_SP, cText, bold = true)
        val tlp = LinearLayout.LayoutParams(0, LP_WRAP, 1f)
        tlp.marginStart = dp(12)
        tlp.marginEnd = dp(8)
        header.addView(t, tlp)
        val badgeView = when {
            done -> badge("済み", Tone.SUCCESS)
            optional -> badge("任意", Tone.NEUTRAL)
            else -> badge("未", Tone.WARNING)
        }
        // 入力テストは達成を検出できないので、バッジを出さない
        if (extra == null) header.addView(badgeView)

        val bodyBox = cardBody(header)
        val dlp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        dlp.topMargin = dp(8)
        bodyBox.addView(body(description, secondary = true), dlp)
        if (actionLabel != null) {
            val b = tonalButton(actionLabel) { onAction() }
            val blp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
            blp.topMargin = dp(12)
            bodyBox.addView(b, blp)
        }
        if (extra != null) {
            val elp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
            elp.topMargin = dp(12)
            bodyBox.addView(extra, elp)
        }
        if (stepsBox.childCount > 0) stepsBox.addView(spacer(12))
        stepsBox.addView(card(bodyBox))
    }
}
