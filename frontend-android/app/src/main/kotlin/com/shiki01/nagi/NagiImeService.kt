package com.shiki01.nagi

import android.inputmethodservice.InputMethodService
import android.text.InputType
import android.text.Spannable
import android.text.SpannableString
import android.text.style.BackgroundColorSpan
import android.text.style.UnderlineSpan
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection

/**
 * Nagi の IME サービス。入力・変換のロジックはすべてコア（Rust）にあり、ここは
 * 「タップをコアに送る」「返ってきた描画差分を InputConnection に反映する」だけを行う。
 */
class NagiImeService : InputMethodService(), KeyboardListener {
    private lateinit var engine: NagiEngine
    private var keyboard: KeyboardView? = null

    /** かな入力（コア経由）か、英数の直接入力か */
    private var kana = true

    /** 入力中の文字列があるか（preedit が空でない） */
    private var composing = false
    private var lastRevision = 0
    private var secure = false

    override fun onCreate() {
        super.onCreate()
        engine = NagiRuntime.get(this)
        engine.onNotification = { r -> applyNotification(r) }
    }

    override fun onDestroy() {
        if (engine.onNotification != null) engine.onNotification = null
        super.onDestroy()
    }

    override fun onCreateInputView(): View {
        val kb = KeyboardView(this, this)
        kb.setKanaMode(kana)
        keyboard = kb
        return kb
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        secure = isSecureField(attribute)
        composing = false
        engine.reset()
        var before: String? = null
        if (!secure) {
            before = currentInputConnection?.getTextBeforeCursor(30, 0)?.toString()
        }
        engine.setContext(secure, before)
        engine.setEnvironment()
        engine.focusIn()
        updateKeyboard(null)
    }

    override fun onFinishInput() {
        // 入力中の文字は残す（入力を失わない）。コア側は破棄して学習を保存する
        currentInputConnection?.finishComposingText()
        composing = false
        engine.reset()
        engine.focusOut()
        super.onFinishInput()
    }

    override fun onUpdateSelection(
        oldSelStart: Int,
        oldSelEnd: Int,
        newSelStart: Int,
        newSelEnd: Int,
        candidatesStart: Int,
        candidatesEnd: Int,
    ) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        // 入力中にユーザーがカーソルを入力範囲の外へ動かしたら、入力を確定扱いにしてコアの状態を捨てる
        if (composing && candidatesStart >= 0 && (newSelStart < candidatesStart || newSelStart > candidatesEnd)) {
            composing = false
            currentInputConnection?.finishComposingText()
            engine.reset()
            updateKeyboard(null)
        }
    }

    // ---- KeyboardListener ----

    override fun onLetter(text: String) {
        val ic = currentInputConnection ?: return
        if (!kana) {
            ic.commitText(text, 1)
            return
        }
        engine.insertText(text) { r -> apply(r, null) }
    }

    override fun onSpace() {
        if (!kana) {
            currentInputConnection?.commitText(" ", 1)
            return
        }
        engine.action("space") { r -> apply(r) { currentInputConnection?.commitText(" ", 1) } }
    }

    override fun onEnter() {
        if (!kana) {
            sendEnter()
            return
        }
        engine.action("enter") { r -> apply(r) { sendEnter() } }
    }

    override fun onBackspace() {
        if (!kana) {
            sendDelete()
            return
        }
        engine.action("backspace") { r -> apply(r) { sendDelete() } }
    }

    override fun onToggleMode() {
        if (composing) {
            // 入力中の文字を確定してから切り替える
            engine.action("commit") { r ->
                apply(r, null)
                switchMode()
            }
        } else {
            switchMode()
        }
    }

    override fun onCandidate(index: Int) {
        engine.selectCandidate(index) { r -> apply(r, null) }
    }

    private fun switchMode() {
        kana = !kana
        keyboard?.setKanaMode(kana)
        updateKeyboard(null)
    }

    // ---- コアの描画差分を反映する ----

    /** `consumed == false` のときは [fallback] を実行する（コアが扱わなかった操作をアプリへ流す） */
    private fun apply(r: EngineResult?, fallback: (() -> Unit)?) {
        if (r == null) {
            // コアが使えない（辞書なしの起動失敗など）。入力は失わないよう直接入力に落とす
            fallback?.invoke()
            return
        }
        if (!r.consumed) {
            fallback?.invoke()
            return
        }
        if (r.revision > lastRevision) lastRevision = r.revision
        val ic = currentInputConnection ?: return
        ic.beginBatchEdit()
        try {
            if (r.deleteBefore > 0) ic.deleteSurroundingText(r.deleteBefore, 0)
            if (r.commit != null) {
                ic.commitText(r.commit, 1)
                // 確定した時点で入力中の文字は無くなる（続きの preedit は下で入れ直す）
                composing = false
            }
            r.preedit?.let { setPreedit(ic, it) }
        } finally {
            ic.endBatchEdit()
        }
        updateKeyboard(r)
    }

    private fun applyNotification(r: EngineResult) {
        // 古い通知は捨てる（同じ revision は通す）
        if (r.revision < lastRevision) return
        lastRevision = r.revision
        val ic = currentInputConnection ?: return
        ic.beginBatchEdit()
        try {
            r.preedit?.let { setPreedit(ic, it) }
        } finally {
            ic.endBatchEdit()
        }
        updateKeyboard(r)
    }

    private fun setPreedit(ic: InputConnection, p: Preedit) {
        if (p.text.isEmpty()) {
            ic.commitText("", 1)
            composing = false
            return
        }
        val s = SpannableString(p.text)
        s.setSpan(UnderlineSpan(), 0, p.text.length, Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
        if (p.focusStart in 0 until p.focusEnd && p.focusEnd <= p.text.length) {
            s.setSpan(
                BackgroundColorSpan(0x553B82F6),
                p.focusStart,
                p.focusEnd,
                Spannable.SPAN_EXCLUSIVE_EXCLUSIVE,
            )
        }
        ic.setComposingText(s, 1)
        composing = true
    }

    private var lastCandidates: Candidates? = null

    private fun updateKeyboard(r: EngineResult?) {
        val kb = keyboard ?: return
        if (r?.candidates != null) lastCandidates = r.candidates
        if (!composing) lastCandidates = if (r?.candidates?.visible == true) r.candidates else lastCandidates
        val c = lastCandidates
        if (c != null && c.visible && c.items.isNotEmpty()) kb.setCandidates(c.items, c.selected)
        else kb.setCandidates(emptyList(), 0)
        kb.setComposing(composing)
    }

    // ---- アプリへの既定動作 ----

    private fun sendEnter() {
        val ic = currentInputConnection ?: return
        val info = currentInputEditorInfo
        val action = (info?.imeOptions ?: 0) and EditorInfo.IME_MASK_ACTION
        val noEnterAction = ((info?.imeOptions ?: 0) and EditorInfo.IME_FLAG_NO_ENTER_ACTION) != 0
        if (!noEnterAction && action != EditorInfo.IME_ACTION_NONE && action != EditorInfo.IME_ACTION_UNSPECIFIED) {
            ic.performEditorAction(action)
        } else {
            ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER))
            ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER))
        }
    }

    private fun sendDelete() {
        val ic = currentInputConnection ?: return
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL))
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DEL))
    }

    /** パスワード欄・シークレットモード（学習停止の要求）なら true */
    private fun isSecureField(info: EditorInfo?): Boolean {
        if (info == null) return false
        val type = info.inputType
        val cls = type and InputType.TYPE_MASK_CLASS
        val variation = type and InputType.TYPE_MASK_VARIATION
        if (cls == InputType.TYPE_CLASS_TEXT) {
            if (variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD
            ) {
                return true
            }
        }
        if (cls == InputType.TYPE_CLASS_NUMBER && variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD) return true
        return (info.imeOptions and NO_PERSONALIZED_LEARNING) != 0
    }

    companion object {
        /** `EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING`（API 26+）と同じ値 */
        private const val NO_PERSONALIZED_LEARNING = 0x1000000
    }
}
