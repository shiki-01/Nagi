package com.shiki01.nagi

import android.content.ClipboardManager
import android.content.Context
import android.inputmethodservice.InputMethodService
import android.os.Build
import android.os.Handler
import android.os.Looper
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
import kotlin.math.min

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

    /** 変換中の強調に使うアクセント色（設定 `accent`。キーボードを表示するたびに読み直す） */
    private var accentArgb: Int = KeyboardColors(false).accent

    /** 12 キーの表（コアから取れるまではフォールバック表） */
    private var keyTable: List<KanaKeyDef> = KeyTables.FALLBACK
    private var strip: CandidateStrip? = null
    private var stripShown = false

    /** 入力欄の種類によるレイアウトの自動選択で、かな / 英字のモードを変えたときの元のモード（戻すときに使う） */
    private var modeBeforeField: Boolean? = null

    /** 入力欄の種類によるレイアウトの自動選択で、かな / 英字以外（数字など）を開いている */
    private var fieldLayoutShown = false

    /** 物理キーを押し下げで受け取ったキー（離したときの [onKeyUp] も受け取って OS に流さない） */
    private val handledKeys = HashSet<Int>()

    /** 環境（メモリ・熱・電源）の監視（OT-01〜03）。サービスが生きている間だけ */
    private var envMonitor: EnvironmentMonitor? = null

    /** クリップボード履歴の取り込みと「貼り付け」ボタンの判定（キーボードを表示している間だけ変化を受ける） */
    private var clipboard: ClipboardCapture? = null

    /** コアの障害からの復旧（OT-04）の状態と、予約した作り直し */
    private val recovery = RecoveryState()
    private val mainHandler = Handler(Looper.getMainLooper())
    private var restartTask: Runnable? = null

    override fun onCreate() {
        super.onCreate()
        engine = NagiRuntime.get(this)
        engine.onNotification = { r -> applyNotification(r) }
        envMonitor = EnvironmentMonitor(this) { engine }.also { it.start() }
        clipboard = ClipboardCapture(this)
        // 起動後に 12 キーの表を 1 回取る（取れなければフォールバック表のまま）
        engine.withKeyTable { t ->
            if (t != null) {
                keyTable = t
                keyboard?.setKeyTable(t)
            }
        }
    }

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        envMonitor?.onTrimMemory(level)
    }

    override fun onDestroy() {
        clipboard?.stop()
        clipboard = null
        envMonitor?.stop()
        envMonitor = null
        restartTask?.let { mainHandler.removeCallbacks(it) }
        restartTask = null
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
        // 設定画面で変えた値（配置・高さ・テーマ・アクセント等）を反映する
        keyboard?.reloadPrefs()
        accentArgb = KeyboardColors.resolve(this).accent
        startClipboardWatch()
        // 同じ入力欄への再表示（restarting）では、利用者が選んだレイアウトを変えない
        if (!restarting) applyFieldLayout(info)
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        // キーボードを隠したら、クリップボードの変化は受けない（読めるのは表示している間だけ）
        clipboard?.stop()
        super.onFinishInputView(finishingInput)
    }

    /** 変化の通知を受け始め、いまのクリップボードで「貼り付け」の表示を決めて、履歴がオンなら取り込む */
    private fun startClipboardWatch() {
        val cap = clipboard ?: return
        cap.start { onClipboardChanged() }
        onClipboardChanged()
    }

    private fun onClipboardChanged() {
        val cap = clipboard ?: return
        keyboard?.setClipboardHasText(cap.hasText())
        if (cap.captureCurrent(secure)) keyboard?.refreshClipboardHistory()
    }

    /**
     * 入力欄の種類（メール・URL・数字など）に合わせてレイアウトを自動で選び、改行キーの文字を変える
     * （設定 `layoutByField` が有効なときだけ）。既に同じモードなら何もしない。
     * 指定のない欄に移ったら、自動で切り替えていた場合だけ元のモードに戻す
     */
    private fun applyFieldLayout(info: EditorInfo?) {
        val kb = keyboard ?: return
        if (!KeyboardPrefs.layoutByField(this)) {
            kb.setEnterLabel(null)
            restoreFromField(kb)
            return
        }
        kb.setEnterLabel(LayoutSelect.enterLabel(info?.imeOptions ?: 0))
        val kind = FieldKind.fromInputType(info?.inputType ?: 0)
        val target = LayoutStore.fieldTarget(this, kind)
        if (target == null) {
            restoreFromField(kb)
            return
        }
        val wantKana = LayoutSelect.kanaModeOfTarget(target)
        if (target == LayoutSelect.TARGET_KANA || target == LayoutSelect.TARGET_ALPHA) {
            // 割り当て済みのかな / 英字。今のモードと同じなら、利用者が選んだ同じモードの別レイアウトも変えない
            if (wantKana == kana && !fieldLayoutShown) return
            if (modeBeforeField == null) modeBeforeField = kana
            fieldLayoutShown = false
            changeKana(wantKana == true)
            kb.setKanaMode(kana)
            return
        }
        val mode = kb.showLayoutId(target) ?: return
        val kanaOfMode = LayoutSelect.kanaModeOf(mode)
        if (kanaOfMode != null) {
            if (modeBeforeField == null) modeBeforeField = kana
            changeKana(kanaOfMode)
            fieldLayoutShown = false
        } else {
            fieldLayoutShown = true
        }
    }

    /** 自動選択で変えたモード・レイアウトを元に戻す */
    private fun restoreFromField(kb: KeyboardView) {
        val before = modeBeforeField
        if (before == null && !fieldLayoutShown) return
        modeBeforeField = null
        fieldLayoutShown = false
        if (before != null) changeKana(before)
        kb.setKanaMode(kana)
    }

    /** 入力モード（かな / 英字）を変える。画面の組み立てはしない */
    private fun changeKana(value: Boolean) {
        if (kana == value) return
        kana = value
        sendCoreMode()
    }

    /**
     * コアの入力モードを合わせる。入力中の文字列を確定せずに英字へ切り替えたとき（設定
     * `keep_preedit_on_english`）は、コアのモードを変えない（変えると入力中の文字列が確定されうるため）
     */
    private fun sendCoreMode() {
        if (composing && !kana) return
        engine.action("set_mode", if (kana) "hiragana" else "alphanumeric") { }
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        secure = isSecureField(attribute)
        composing = false
        clearPreedit()
        lastCandidates = null
        recovery.reset()
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
        if (!viaCore()) {
            ic.commitText(text, 1)
            return
        }
        // コアが使えないときは、押した文字をそのまま入れる（入力を失わない）
        engine.insertText(text) { r -> apply(r) { currentInputConnection?.commitText(text, 1) } }
    }

    override fun onSpace() {
        if (!viaCore()) {
            currentInputConnection?.commitText(" ", 1)
            return
        }
        engine.action("space") { r -> apply(r) { currentInputConnection?.commitText(" ", 1) } }
    }

    override fun onEnter() {
        if (!viaCore()) {
            sendEnter()
            return
        }
        engine.action("enter") { r -> apply(r) { sendEnter() } }
    }

    override fun onBackspace() {
        if (!viaCore()) {
            sendDelete()
            return
        }
        engine.action("backspace") { r -> apply(r) { sendDelete() } }
    }

    override fun onToggleMode() {
        val from = if (kana) LayoutMode.KANA else LayoutMode.ALPHA
        val to = if (kana) LayoutMode.ALPHA else LayoutMode.KANA
        // 入力中の文字を（設定に従って）確定してから切り替える
        switchThen(from, to) { switchMode() }
    }

    /** かな入力の流れ、または設定 `keep_preedit_on_english` で英字キーも入力中の文字列に続けるときは true */
    private fun viaCore(): Boolean =
        kana || EnglishCompose.shouldCompose(KeyboardPrefs.keepPreeditOnEnglish(this), secure, composing)

    /** レイアウト切替で入力中の文字列を確定せずに保つか（設定 `keep_preedit_on_symbols` / `_on_english`） */
    private fun keepOnSwitch(from: LayoutMode, to: LayoutMode): Boolean = LayoutSwitchKeep.shouldKeep(
        from = from,
        to = to,
        hasPreedit = composing,
        isSecure = secure,
        keepOnSymbols = KeyboardPrefs.keepPreeditOnSymbols(this),
        keepOnEnglish = KeyboardPrefs.keepPreeditOnEnglish(this),
        composeNumbers = KeyboardPrefs.composeNumbers(this),
    )

    /** 入力中なら、保たない設定のときだけ先に確定してから [block] を実行する */
    private fun switchThen(from: LayoutMode, to: LayoutMode, block: () -> Unit) {
        if (composing && !keepOnSwitch(from, to)) {
            engine.action("commit") { r ->
                apply(r, null)
                block()
            }
        } else {
            block()
        }
    }

    override fun onLayoutSwitch(from: String, to: String, block: () -> Unit) {
        // 知らない種類は「かな」でも「英字」でもないものとして、常に確定する側（CUSTOM）に倒す
        switchThen(LayoutMode.fromJson(from) ?: LayoutMode.CUSTOM, LayoutMode.fromJson(to) ?: LayoutMode.CUSTOM, block)
    }

    override fun onNumberText(text: String) {
        val compose = NumberCompose.shouldCompose(KeyboardPrefs.composeNumbers(this), secure, kana, composing)
        if (compose) {
            // コアの入力中の文字列に入れる（電卓・日付の候補が出る）。コアが使えないときはそのまま入れる
            engine.insertText(text) { r -> apply(r) { currentInputConnection?.commitText(text, 1) } }
        } else {
            currentInputConnection?.commitText(text, 1)
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
            !viaCore() -> sendDownUpKeyEvents(code)
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
        // 入力中の文字を（設定に従って）確定してから数字・記号の画面を開く
        val from = if (kana) LayoutMode.KANA else LayoutMode.ALPHA
        switchThen(from, LayoutMode.NUMBER) { keyboard?.showNumbers() }
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

    override fun onCommitThen(block: () -> Unit) {
        if (composing) {
            engine.action("commit") { r ->
                apply(r, null)
                block()
            }
        } else {
            block()
        }
    }

    override fun onSetKana(kana: Boolean) {
        if (kana == this.kana) return
        changeKana(kana)
        updateKeyboard(null)
    }

    override fun onConvert() {
        if (!viaCore()) return
        engine.action("start_convert") { r -> apply(r, null) }
    }

    override fun onDismiss() {
        requestHideSelf(0)
    }

    override fun onPaste() {
        val cm = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
        val clip = cm.primaryClip ?: return
        if (clip.itemCount == 0) return
        val text = clip.getItemAt(0).coerceToText(this)?.toString()
        if (text.isNullOrEmpty()) return
        onCommitThen { currentInputConnection?.commitText(text, 1) }
    }

    override fun onClipboardInsert(text: String) {
        // パスワード欄では履歴を使わない（ページの状態が古くても入れない）。内容はログに出さない
        if (secure || text.isEmpty()) return
        onCommitThen { currentInputConnection?.commitText(text, 1) }
    }

    override fun isSecureInput(): Boolean = secure

    override fun onDeleteWord() {
        if (composing) return
        deleteWordInHost()
    }

    /** カーソル手前の単語を入力欄から消す。パスワード欄などでは周辺の文字を読まない（何もしない） */
    private fun deleteWordInHost() {
        if (secure) return
        val ic = currentInputConnection ?: return
        val before = ic.getTextBeforeCursor(WORD_CONTEXT_UNITS, 0) ?: return
        val n = WordBoundary.deleteLength(before)
        if (n > 0) ic.deleteSurroundingText(n, 0)
    }

    override fun onBackspaceWord() {
        when {
            // 変換中は注目文節の扱いが絡むので何もしない（押した瞬間の 1 回の backspace は済んでいる）
            focusIndex >= 0 -> Unit
            // 入力中: コアの backspace を単語の長さぶん続けて送る
            kana && composing -> {
                val p = lastPreedit ?: return
                val upTo = p.cursor.coerceIn(0, p.text.length)
                val n = WordBoundary.deleteCount(p.text.substring(0, upTo))
                if (n > 0) {
                    engine.actionRepeat("backspace", null, n) { r -> apply(r) { repeat(n) { sendDelete() } } }
                }
            }
            else -> deleteWordInHost()
        }
    }

    override fun onSpaceSwipe(steps: Int) {
        if (steps == 0) return
        val left = steps < 0
        val dir = if (left) "left" else "right"
        val code = if (left) KeyEvent.KEYCODE_DPAD_LEFT else KeyEvent.KEYCODE_DPAD_RIGHT
        val n = abs(steps)
        // 加速オン（touch.space_swipe_accel）のときは 1 回の上限を緩める（高速スライドで切り捨てて指の位置とずれないように）
        val accel = KeyboardPrefs.spaceSwipeAccel(this)
        val coreMax = if (accel) CORE_CURSOR_MAX_ACCEL else CORE_CURSOR_MAX
        when {
            !viaCore() -> moveHostCursor(code, n, accel)
            // 変換中は注目文節、入力中は入力中の文字列内のカーソル（← → と同じ操作）
            focusIndex >= 0 -> engine.actionRepeat("move_focus", dir, min(n, coreMax)) { r -> apply(r, null) }
            composing -> engine.actionRepeat("move_cursor", dir, min(n, coreMax)) { r -> apply(r, null) }
            else -> moveHostCursor(code, n, accel)
        }
    }

    /** 入力欄のカーソルを [n] 文字動かす（方向キーを送る。エディタの種類によらず動く） */
    private fun moveHostCursor(code: Int, n: Int, accel: Boolean = false) {
        repeat(min(n, if (accel) HOST_CURSOR_MAX_ACCEL else HOST_CURSOR_MAX)) { sendDownUpKeyEvents(code) }
    }

    override fun onCycleText(text: String, previous: String?, viaCore: Boolean) {
        val n = previous?.let { it.codePointCount(0, it.length) } ?: 0
        if (viaCore && kana) {
            // 直前の文字をコアの backspace で消してから入れ直す（コアが使えなければアプリへ直接）
            for (i in 0 until n) engine.action("backspace") { r -> apply(r) { sendDelete() } }
            engine.insertText(text) { r -> apply(r) { currentInputConnection?.commitText(text, 1) } }
        } else {
            for (i in 0 until n) sendDelete()
            currentInputConnection?.commitText(text, 1)
        }
    }

    private fun switchMode() {
        setKana(!kana)
        // コアの入力モードも合わせる（物理キーの英数 / 全角半角が正しく切り替わるように）
        sendCoreMode()
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

    // ---- コアの障害からの復旧（OT-04） ----

    /**
     * コアの呼び出しが失敗した（例外・タイムアウト・null 応答）。続けて失敗したら、入力中の文字はホストに残したまま
     * バックオフを置いてコアを作り直す。[typing] は、この失敗のあと押した文字を直接入れる（縮退入力）とき:
     * 入力中の文字が composing のままだと直接入力が置き換えてしまうので、先に下線を外してホストに残す
     */
    private fun onCoreFailure(typing: Boolean, force: Boolean = false) {
        if (typing && composing) {
            currentInputConnection?.finishComposingText()
            composing = false
            clearPreedit()
            DiagLog.event(this, "recovery", "入力中の文字をホストに残して縮退入力へ")
            updateKeyboard(null)
        }
        when (val action = recovery.onFailure(force)) {
            is RecoveryAction.Restart -> {
                DiagLog.event(this, "recovery", "コアを作り直す予約 attempt=${action.attempt} delay=${action.delayMs}ms composing=$composing")
                val task = Runnable { restartCore() }
                restartTask = task
                mainHandler.postDelayed(task, action.delayMs)
            }
            RecoveryAction.GiveUp -> DiagLog.event(this, "recovery", "作り直しの上限に達した。縮退入力のまま")
            RecoveryAction.None -> Unit
        }
    }

    /** コアを作り直し、残してあった読みを `restore_session` で戻す。戻せなくても確定せず composing を残す */
    private fun restartCore() {
        restartTask = null
        recovery.onRestartDone()
        val plan = RestoreLogic.plan(lastPreedit, composing)
        // 他で（モデルの変更などで）既に作り直されていればそれを使う
        val current = NagiRuntime.peek()
        engine = if (current != null && current !== engine) current else NagiRuntime.restart(this)
        engine.onNotification = { r -> applyNotification(r) }
        lastRevision = 0 // 新しいコアの revision は 0 から数え直す
        engine.withKeyTable { t ->
            if (t != null) {
                keyTable = t
                keyboard?.setKeyTable(t)
            }
        }
        var before: String? = null
        if (!secure) before = currentInputConnection?.getTextBeforeCursor(30, 0)?.toString()
        engine.setContext(secure, before)
        engine.setEnvironment()
        engine.focusIn()
        if (plan == null) {
            DiagLog.event(this, "recovery", "コアを作り直した（復元する入力中の文字なし）")
            return
        }
        engine.restoreSession(plan) { r ->
            if (r == null) {
                // 復元できなかった。確定せず、ホストの composing をそのまま残す
                DiagLog.event(this, "recovery", "restore_session に失敗。入力中の文字は確定せず残す")
                onCoreFailure(typing = false, force = true)
            } else {
                recovery.onSuccess()
                DiagLog.event(this, "recovery", "restore_session 成功 segments=${plan.segments.size}")
                if (r.preedit != null) apply(r, null)
            }
        }
    }

    // ---- コアの描画差分を反映する ----

    /** `consumed == false` のときは [fallback] を実行する（コアが扱わなかった操作をアプリへ流す） */
    private fun apply(r: EngineResult?, fallback: (() -> Unit)?) {
        if (r == null) {
            // コアが使えない（辞書なしの起動失敗など）。入力は失わないよう直接入力に落とす
            onCoreFailure(typing = fallback != null)
            fallback?.invoke()
            return
        }
        recovery.onSuccess()
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
                BackgroundColorSpan(AccentColor.withAlpha(accentArgb, 0x33)),
                p.selectionStart,
                p.selectionEnd,
                Spannable.SPAN_EXCLUSIVE_EXCLUSIVE,
            )
        }
        if (p.focusStart in 0 until p.focusEnd && p.focusEnd <= p.text.length) {
            s.setSpan(
                BackgroundColorSpan(AccentColor.withAlpha(accentArgb, 0x55)),
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
        // 次語候補（入力中の読みが無い状態で届く学習候補）は見た目を分け、パスワード欄では出さない
        val bar = CandidateKinds.buildBar(lastCandidates, composing, secure)
        val items: List<String> = bar.items
        val selected = bar.selected
        // 変換中（注目文節がある）ときだけ文節列を出す
        val segs: List<String> =
            if (focusIndex >= 0) lastPreedit?.segments?.map { it.surface } ?: emptyList() else emptyList()
        keyboard?.let { kb ->
            kb.setCandidates(bar)
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

        /** 単語削除のためにカーソル手前から読む長さ（UTF-16 コード単位。最大 50 文字ぶんの書記素に足りる長さ） */
        private const val WORD_CONTEXT_UNITS = 160

        /** スペースのスワイプ 1 回の通知で、コアのカーソル（文節）を動かす最大回数（iOS と同じ 20） */
        private const val CORE_CURSOR_MAX = 20

        /** スペースのスワイプ 1 回の通知で、入力欄のカーソルを動かす最大の文字数 */
        private const val HOST_CURSOR_MAX = 64

        /** 加速オン（`touch.space_swipe_accel`）のときの上限。高速スライドで切り捨てない */
        private const val CORE_CURSOR_MAX_ACCEL = 200
        private const val HOST_CURSOR_MAX_ACCEL = 256
    }
}
