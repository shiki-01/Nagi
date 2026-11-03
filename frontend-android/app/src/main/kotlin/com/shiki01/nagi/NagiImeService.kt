package com.shiki01.nagi

import android.content.Context
import android.inputmethodservice.InputMethodService
import android.os.Build
import android.os.SystemClock
import android.text.InputType
import android.text.Spannable
import android.text.SpannableString
import android.text.style.BackgroundColorSpan
import android.text.style.UnderlineSpan
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import kotlin.math.abs

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

    /** 注目文節の番号。変換中でなければ -1 */
    private var focusIndex = -1
    private var lastPreedit: Preedit? = null

    /** 12 キーの表（コアから取れるまではフォールバック表） */
    private var keyTable: List<KanaKeyDef> = KeyTables.FALLBACK
    private var strip: CandidateStrip? = null
    private var stripShown = false

    /** 物理キーを押し下げで受け取ったキー（離したときの [onKeyUp] も受け取って OS に流さない） */
    private val handledKeys = HashSet<Int>()

    override fun onCreate() {
        super.onCreate()
        engine = NagiRuntime.get(this)
        engine.onNotification = { r -> applyNotification(r) }
        // 起動後に 12 キーの表を 1 回取る（取れなければフォールバック表のまま）
        engine.withKeyTable { t ->
            if (t != null) {
                keyTable = t
                keyboard?.setKeyTable(t)
            }
        }
    }

    override fun onDestroy() {
        if (engine.onNotification != null) engine.onNotification = null
        super.onDestroy()
    }

    override fun onCreateInputView(): View {
        val kb = KeyboardView(this, this)
        kb.setKeyTable(keyTable)
        kb.setKanaMode(kana)
        keyboard = kb
        // レイアウトパラメータが無いと、入力ビューの高さが MATCH_PARENT 扱いになり画面いっぱいに伸びる
        kb.layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        return kb
    }

    /** ソフトキーボードを出さないとき（物理キーボード接続時）の簡易な候補表示 */
    override fun onCreateCandidatesView(): View? {
        val s = CandidateStrip(this) { i -> onCandidate(i) }
        strip = s
        return s
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        // 設定画面で変えた値（配置・高さ・テーマ等）を反映する
        keyboard?.reloadPrefs()
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        secure = isSecureField(attribute)
        composing = false
        clearPreedit()
        handledKeys.clear()
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
        clearPreedit()
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
            clearPreedit()
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
        // コアが使えないときは、押した文字をそのまま入れる（入力を失わない）
        engine.insertText(text) { r -> apply(r) { currentInputConnection?.commitText(text, 1) } }
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

    override fun onKana(key: String, gesture: String, angleDeg: Double?, distance: Double?) {
        if (!kana) return
        engine.insertKey12(key, gesture, angleDeg, distance) { r ->
            // コアが使えないときは表の文字をそのまま入れる（入力を失わない）
            apply(r) {
                val def = keyTable.firstOrNull { it.key == key }
                if (def != null) currentInputConnection?.commitText(KeyTables.charFor(def, gesture), 1)
            }
        }
    }

    override fun onVariant() {
        if (!kana) return
        engine.insertVariant { r -> apply(r, null) }
    }

    override fun onArrow(left: Boolean) {
        val dir = if (left) "left" else "right"
        val code = if (left) KeyEvent.KEYCODE_DPAD_LEFT else KeyEvent.KEYCODE_DPAD_RIGHT
        when {
            !kana -> sendDownUpKeyEvents(code)
            focusIndex >= 0 -> engine.action("move_focus", dir) { r -> apply(r, null) }
            composing -> engine.action("move_cursor", dir) { r -> apply(r, null) }
            else -> sendDownUpKeyEvents(code)
        }
    }

    override fun onNextInputMethod() {
        val switched = Build.VERSION.SDK_INT >= 28 && switchToNextInputMethod(false)
        if (!switched) {
            (getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager)?.showInputMethodPicker()
        }
    }

    override fun onOpenNumbers() {
        if (composing) {
            // 入力中の文字を確定してから数字・記号の画面を開く
            engine.action("commit") { r ->
                apply(r, null)
                keyboard?.showNumbers()
            }
        } else {
            keyboard?.showNumbers()
        }
    }

    override fun onDirectText(text: String) {
        currentInputConnection?.commitText(text, 1)
    }

    override fun onFocusSegment(index: Int) {
        val diff = index - focusIndex
        if (focusIndex < 0 || diff == 0) return
        // 現在の注目との差の回数だけ move_focus を送り、応答は 1 つにまとめて反映する
        engine.actionRepeat("move_focus", if (diff < 0) "left" else "right", abs(diff)) { r -> apply(r, null) }
    }

    override fun onToolAction(type: String, content: String?) {
        engine.action(type, content) { r -> apply(r, null) }
    }

    override fun onReconvert() {
        val ic = currentInputConnection ?: return
        if (composing) return
        // 選択範囲があるときだけ（既存のテキストを消す処理が要るため、カーソル直前の取得はしない）
        val sel = ic.getSelectedText(0)?.toString()
        if (sel.isNullOrEmpty()) return
        // 返った preedit を setComposingText で入れると、選択範囲が置き換わる
        engine.reconvert(sel) { r -> apply(r, null) }
    }

    private fun switchMode() {
        setKana(!kana)
        // コアの入力モードも合わせる（物理キーの英数 / 全角半角が正しく切り替わるように）
        engine.action("set_mode", if (kana) "hiragana" else "alphanumeric") { }
        updateKeyboard(null)
    }

    private fun setKana(value: Boolean) {
        kana = value
        keyboard?.setKanaMode(value)
    }

    // ---- 物理キーボード ----

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (event != null && handlePhysicalKey(keyCode, event)) return true
        return super.onKeyDown(keyCode, event)
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent?): Boolean {
        if (handledKeys.remove(keyCode)) return true
        return super.onKeyUp(keyCode, event)
    }

    /**
     * 物理キーをコアの `process_key` へ送る。応答は非同期なので、ここでは「コアへ送るキー」は
     * いったん消費し、コアが消費しなかったとき（`consumed == false`）に OS 既定の動作を再現する。
     * コアへ送らない（OS に任せる）なら false。
     */
    private fun handlePhysicalKey(keyCode: Int, event: KeyEvent): Boolean {
        if (PhysicalKeys.isModifierKey(keyCode)) return false
        if (currentInputConnection == null) return false
        val meta = event.metaState
        val unicode = event.getUnicodeChar(PhysicalKeys.metaForChar(meta))
        val sym = PhysicalKeys.keysymFor(keyCode, unicode)
        if (sym == 0) return false
        val modeKey = PhysicalKeys.isModeKeysym(sym)
        // 英数モード（コアを通さない直接入力）では、モード切替キー以外は OS に任せる
        if (!kana && !modeKey) return false
        val text = PhysicalKeys.textFor(keyCode, unicode)
        val key = PhysicalKeyEvent(sym, keyCode, text, PhysicalKeys.modifiersFrom(meta))
        handledKeys.add(keyCode)
        engine.processKey(key) { r -> apply(r) { replayKey(keyCode, meta, text) } }
        return true
    }

    /** コアが消費しなかった物理キーを、アプリへそのまま渡す */
    private fun replayKey(keyCode: Int, meta: Int, text: String) {
        val ic = currentInputConnection ?: return
        if (keyCode == KeyEvent.KEYCODE_ENTER || keyCode == KeyEvent.KEYCODE_NUMPAD_ENTER) {
            sendEnter()
        } else if (text.isNotEmpty() && !PhysicalKeys.hasCommandModifier(meta)) {
            ic.commitText(text, 1)
        } else {
            val now = SystemClock.uptimeMillis()
            ic.sendKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_DOWN, keyCode, 0, meta))
            ic.sendKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_UP, keyCode, 0, meta))
        }
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
        // 物理キー（英数 / 全角半角など）で入力モードが変わった
        r.mode?.let { m -> if ((m != "alphanumeric") != kana) setKana(m != "alphanumeric") }
        updateKeyboard(r)
        // 変換キーなどで選択中のテキストの再変換を求められた
        if (r.reconvertSelection) onReconvert()
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
            clearPreedit()
            return
        }
        lastPreedit = p
        focusIndex = p.focus
        val s = SpannableString(p.text)
        s.setSpan(UnderlineSpan(), 0, p.text.length, Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
        if (p.selectionStart in 0 until p.selectionEnd && p.selectionEnd <= p.text.length) {
            // 複数文節の選択範囲
            s.setSpan(
                BackgroundColorSpan(0x333B82F6),
                p.selectionStart,
                p.selectionEnd,
                Spannable.SPAN_EXCLUSIVE_EXCLUSIVE,
            )
        }
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

    private fun clearPreedit() {
        lastPreedit = null
        focusIndex = -1
    }

    private var lastCandidates: Candidates? = null

    private fun updateKeyboard(r: EngineResult?) {
        if (r?.candidates != null) lastCandidates = r.candidates
        if (!composing) lastCandidates = if (r?.candidates?.visible == true) r.candidates else lastCandidates
        val c = lastCandidates
        val visible = c != null && c.visible && c.items.isNotEmpty()
        val items: List<String> = if (visible && c != null) c.items else emptyList()
        val selected = if (visible && c != null) c.selected else 0
        // 変換中（注目文節がある）ときだけ文節列を出す
        val segs: List<String> =
            if (focusIndex >= 0) lastPreedit?.segments?.map { it.surface } ?: emptyList() else emptyList()
        keyboard?.let { kb ->
            kb.setCandidates(items, selected)
            kb.setSegments(segs, focusIndex)
            kb.setComposing(composing)
        }
        // ソフトキーボードを出さない設定のとき（物理キーボード）だけ、簡易の候補表示を使う。
        // setCandidatesViewShown はウィンドウの表示にも触るので、ソフトキーボードがあるときは呼ばない
        strip?.setItems(items, selected)
        val wantStrip = items.isNotEmpty() && composing && !onEvaluateInputViewShown()
        if (wantStrip != stripShown) {
            stripShown = wantStrip
            setCandidatesViewShown(wantStrip)
        }
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
