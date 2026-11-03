package com.shiki01.nagi

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.math.max

/** キーボードの操作を IME サービスに伝える */
internal interface KeyboardListener {
    /** QWERTY の 1 文字（かなモードならローマ字としてコアへ、英数モードなら直接入力） */
    fun onLetter(text: String)

    fun onSpace()

    fun onEnter()

    fun onBackspace()

    fun onToggleMode()

    fun onCandidate(index: Int)

    /** 12 キーの 1 押下。フリックのときだけ [angleDeg] / [distance] が入る */
    fun onKana(key: String, gesture: String, angleDeg: Double?, distance: Double?)

    /** 濁点・小書きキー（゛゜小） */
    fun onVariant()

    /** ← / →。変換中は注目文節、入力中はカーソル、未入力はホストのカーソルを動かす */
    fun onArrow(left: Boolean)

    fun onNextInputMethod()

    /** 数字・記号の画面を開く（入力中の文字を確定してから [KeyboardView.showNumbers] を呼ぶのはサービスの役目） */
    fun onOpenNumbers()

    /** コアを通さず、そのまま入力欄に入れる文字（数字・記号の画面） */
    fun onDirectText(text: String)

    /** 文節列の [index] 番目をタップした */
    fun onFocusSegment(index: Int)

    /** 文節・範囲操作ツールバー（`perform_action` の type と、方向などの content） */
    fun onToolAction(type: String, content: String?)

    fun onReconvert()
}

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

private fun lp(w: Int, h: Int, weight: Float = 0f) = LinearLayout.LayoutParams(w, h, weight)

/**
 * 12 キー（フリック）/ QWERTY / 数字・記号 のキーボード + 候補バー + 文節列 + 文節操作ツールバー
 * （プログラムで組み立てる View）。ロジックは持たず、操作を [KeyboardListener] に渡すだけにする。
 */
internal class KeyboardView(context: Context, private val listener: KeyboardListener) : FrameLayout(context) {
    private enum class Page { KANA, NUMBERS }

    /** 設定画面で変えられる値のスナップショット。変わったら組み立て直す */
    private data class Prefs(
        val layout: String,
        val toggleInput: Boolean,
        val thresholdDp: Float,
        val popup: Boolean,
        val haptic: Boolean,
        val heightScale: Float,
        val theme: String,
        val night: Boolean,
    )

    private val uiHandler = Handler(Looper.getMainLooper())
    private var prefs: Prefs = readPrefs()
    private var colors: KeyboardColors = KeyboardColors.resolve(context)

    private var page = Page.KANA
    private var kanaMode = true
    private var keyTable: List<KanaKeyDef> = KeyTables.FALLBACK
    private var composing = false
    private var shift = false

    private var candItems: List<String> = emptyList()
    private var candSelected = 0
    private var segTexts: List<String> = emptyList()
    private var focusIndex = -1

    private val letterKeys = ArrayList<TextView>()
    private var spaceKey: TextView? = null
    private var enterKey: TextView? = null
    private var modeKey: TextView? = null
    private var shiftKey: TextView? = null
    private var enterIdleLabel = "⏎"
    private var gridPanel: View? = null
    /** キーボード本体（候補バー・ツールバー・キー）。全候補パネルの高さをこれに合わせる */
    private var rootView: LinearLayout? = null

    private lateinit var segScroll: HorizontalScrollView
    private lateinit var segBar: LinearLayout
    private lateinit var candScroll: HorizontalScrollView
    private lateinit var candBar: LinearLayout
    private lateinit var toolbarScroll: HorizontalScrollView
    private lateinit var reconvertBtn: TextView
    private lateinit var expandBtn: TextView
    private lateinit var keyArea: LinearLayout
    private lateinit var popup: FlickPopupView

    private val locA = IntArray(2)
    private val locB = IntArray(2)

    init {
        buildUi()
    }

    // ---- 外から呼ぶ ----

    /** かな入力か英数直接入力かの表示を切り替える（数字・記号の画面からは かな / 英数 の画面に戻る） */
    fun setKanaMode(kana: Boolean) {
        kanaMode = kana
        page = Page.KANA
        buildKeyArea()
    }

    /** 数字・記号の画面を出す */
    fun showNumbers() {
        page = Page.NUMBERS
        buildKeyArea()
    }

    /** コアから取った 12 キーの表を反映する（フォールバック表と同じなら何もしない） */
    fun setKeyTable(table: List<KanaKeyDef>) {
        if (table == keyTable) return
        keyTable = table
        buildKeyArea()
    }

    /** 入力中（変換できる状態）かどうかで Space と Enter の表示を変える */
    fun setComposing(composing: Boolean) {
        this.composing = composing
        renderLabels()
    }

    /** 候補バーを更新する。空・非表示なら空にする（高さは保つ） */
    fun setCandidates(items: List<String>, selected: Int) {
        candItems = items
        candSelected = selected
        renderCandidates()
    }

    /** 変換中の文節列（[focus] は注目文節の番号。変換中でなければ -1 または空） */
    fun setSegments(texts: List<String>, focus: Int) {
        segTexts = texts
        focusIndex = focus
        renderSegments()
    }

    /** 設定画面で変わった値を反映する（変わっていなければ何もしない） */
    fun reloadPrefs() {
        val n = readPrefs()
        if (n == prefs) return
        prefs = n
        buildUi()
    }

    override fun onConfigurationChanged(newConfig: Configuration?) {
        super.onConfigurationChanged(newConfig)
        reloadPrefs()
    }

    override fun onDetachedFromWindow() {
        uiHandler.removeCallbacksAndMessages(null)
        super.onDetachedFromWindow()
    }

    // ---- 組み立て ----

    private fun readPrefs(): Prefs {
        val night = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
            Configuration.UI_MODE_NIGHT_YES
        return Prefs(
            layout = KeyboardPrefs.layout(context),
            toggleInput = KeyboardPrefs.toggleInput(context),
            thresholdDp = KeyboardPrefs.flickThresholdDp(context),
            popup = KeyboardPrefs.flickPopup(context),
            haptic = KeyboardPrefs.haptic(context),
            heightScale = KeyboardPrefs.heightScale(context),
            theme = KeyboardPrefs.theme(context),
            night = night,
        )
    }

    private fun buildUi() {
        removeAllViews()
        uiHandler.removeCallbacksAndMessages(null)
        gridPanel = null
        colors = KeyboardColors.resolve(context)
        setBackgroundColor(colors.bg)

        val root = LinearLayout(context)
        rootView = root
        root.orientation = LinearLayout.VERTICAL
        root.setPadding(dp(4), 0, dp(4), dp(4))

        // 上段: 変換中の文節列
        segBar = LinearLayout(context)
        segBar.orientation = LinearLayout.HORIZONTAL
        segBar.gravity = Gravity.CENTER_VERTICAL
        segScroll = HorizontalScrollView(context)
        segScroll.isHorizontalScrollBarEnabled = false
        segScroll.setBackgroundColor(colors.segBg)
        segScroll.addView(segBar, FrameLayout.LayoutParams(WRAP, MATCH))
        segScroll.visibility = View.GONE
        root.addView(segScroll, lp(MATCH, dp(38)))

        // 候補バー + 右端のボタン
        val candRow = LinearLayout(context)
        candRow.orientation = LinearLayout.HORIZONTAL
        candRow.setBackgroundColor(colors.candBg)
        candBar = LinearLayout(context)
        candBar.orientation = LinearLayout.HORIZONTAL
        candBar.gravity = Gravity.CENTER_VERTICAL
        candScroll = HorizontalScrollView(context)
        candScroll.isHorizontalScrollBarEnabled = false
        candScroll.addView(candBar, FrameLayout.LayoutParams(WRAP, MATCH))
        candRow.addView(candScroll, lp(0, MATCH, 1f))
        reconvertBtn = barButton("再変換") { listener.onReconvert() }
        expandBtn = barButton("▾") { toggleGrid() }
        val layoutBtn = barButton("⇄") { toggleLayout() }
        candRow.addView(reconvertBtn, lp(WRAP, MATCH))
        candRow.addView(expandBtn, lp(dp(40), MATCH))
        candRow.addView(layoutBtn, lp(dp(40), MATCH))
        root.addView(candRow, lp(MATCH, dp(44)))

        // 文節・範囲操作ツールバー（変換中だけ）
        val toolbar = LinearLayout(context)
        toolbar.orientation = LinearLayout.HORIZONTAL
        toolbar.gravity = Gravity.CENTER_VERTICAL
        toolbarScroll = HorizontalScrollView(context)
        toolbarScroll.isHorizontalScrollBarEnabled = false
        toolbarScroll.setBackgroundColor(colors.segBg)
        toolbarScroll.addView(toolbar, FrameLayout.LayoutParams(WRAP, MATCH))
        for (t in TOOLS) {
            val b = TextView(context)
            b.text = t.label
            b.textSize = 13f
            b.setTextColor(colors.fg)
            b.gravity = Gravity.CENTER
            b.setPadding(dp(10), 0, dp(10), 0)
            b.background = roundRect(colors.funcBg, dp(6).toFloat())
            b.setOnClickListener { listener.onToolAction(t.type, t.content) }
            val p = lp(WRAP, MATCH)
            p.setMargins(dp(3), dp(3), dp(3), dp(3))
            toolbar.addView(b, p)
        }
        toolbarScroll.visibility = View.GONE
        root.addView(toolbarScroll, lp(MATCH, dp(38)))

        // キー
        keyArea = LinearLayout(context)
        keyArea.orientation = LinearLayout.VERTICAL
        root.addView(keyArea, lp(MATCH, WRAP))

        addView(root, FrameLayout.LayoutParams(MATCH, WRAP))
        popup = FlickPopupView(context, colors)
        addView(popup, FrameLayout.LayoutParams(MATCH, MATCH))

        buildKeyArea()
        renderCandidates()
        renderSegments()
    }

    private fun buildKeyArea() {
        keyArea.removeAllViews()
        letterKeys.clear()
        spaceKey = null
        enterKey = null
        modeKey = null
        shiftKey = null
        shift = false
        when {
            page == Page.NUMBERS -> buildNumbers()
            kanaMode && prefs.layout == "flick" -> buildFlick()
            else -> buildQwerty()
        }
        renderLabels()
    }

    private fun rowHeight(baseDp: Int): Int = (dp(baseDp) * prefs.heightScale).toInt()

    private fun newRow(heightPx: Int): LinearLayout {
        val row = LinearLayout(context)
        row.orientation = LinearLayout.HORIZONTAL
        keyArea.addView(row, lp(MATCH, heightPx))
        return row
    }

    private fun keyParams(weight: Float): LinearLayout.LayoutParams {
        val p = lp(0, MATCH, weight)
        p.setMargins(dp(2), dp(3), dp(2), dp(3))
        return p
    }

    private fun spacer(weight: Float): View {
        val v = View(context)
        v.layoutParams = lp(0, MATCH, weight)
        return v
    }

    // ---- 12 キー ----

    private fun buildFlick() {
        val h = rowHeight(52)
        enterIdleLabel = "改行"

        var row = newRow(h)
        row.addView(funcKey("←", { listener.onArrow(true) }), keyParams(1f))
        addFlick(row, "a")
        addFlick(row, "ka")
        addFlick(row, "sa")
        val bs = funcKey("⌫", null)
        attachRepeat(bs) { listener.onBackspace() }
        row.addView(bs, keyParams(1f))

        row = newRow(h)
        row.addView(funcKey("→", { listener.onArrow(false) }), keyParams(1f))
        addFlick(row, "ta")
        addFlick(row, "na")
        addFlick(row, "ha")
        val space = funcKey("空白", { listener.onSpace() })
        space.textSize = 16f
        spaceKey = space
        row.addView(space, keyParams(1f))

        row = newRow(h)
        val mode = funcKey(if (kanaMode) "あ" else "A", { listener.onToggleMode() }, { toggleLayout() })
        modeKey = mode
        row.addView(mode, keyParams(1f))
        addFlick(row, "ma")
        addFlick(row, "ya")
        addFlick(row, "ra")
        val enter = funcKey("改行", { listener.onEnter() })
        enter.textSize = 16f
        enterKey = enter
        row.addView(enter, keyParams(1f))

        row = newRow(h)
        row.addView(funcKey("🌐", { listener.onNextInputMethod() }), keyParams(1f))
        row.addView(funcKey("゛゜小", { listener.onVariant() }), keyParams(1f))
        addFlick(row, "wa")
        addFlick(row, "punct", "、。?!")
        row.addView(funcKey("☆123", { listener.onOpenNumbers() }), keyParams(1f))
    }

    private fun addFlick(row: LinearLayout, id: String, label: String? = null) {
        val def = keyTable.firstOrNull { it.key == id }
        if (def == null) {
            row.addView(spacer(1f))
            return
        }
        row.addView(flickKey(def, label ?: def.center), keyParams(1f))
    }

    /** タップ / フリックのキー。押している間はポップアップを出し、離したときに方向を判定して送る */
    private fun flickKey(def: KanaKeyDef, label: String): TextView {
        val t = baseKey(label, colors.keyBg)
        var downX = 0f
        var downY = 0f
        var keyRect = RectF()
        t.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    view.background = roundRect(colors.pressedBg, dp(6).toFloat())
                    buzz(view)
                    view.parent?.requestDisallowInterceptTouchEvent(true)
                    keyRect = rectOf(view)
                    if (prefs.popup) popup.show(keyRect, def, FlickDirection.TAP)
                }
                MotionEvent.ACTION_MOVE -> {
                    if (prefs.popup) {
                        val r = judge(ev.x - downX, ev.y - downY, view)
                        if (r.cancelled) popup.hide() else popup.show(keyRect, def, r.direction)
                    }
                }
                MotionEvent.ACTION_UP -> {
                    popup.hide()
                    view.background = roundRect(colors.keyBg, dp(6).toFloat())
                    val r = judge(ev.x - downX, ev.y - downY, view)
                    if (!r.cancelled) {
                        val flick = r.direction != FlickDirection.TAP
                        listener.onKana(
                            def.key,
                            FlickLogic.gestureFor(r, prefs.toggleInput),
                            if (flick) r.angleDeg else null,
                            if (flick) r.distance else null,
                        )
                    }
                }
                MotionEvent.ACTION_CANCEL -> {
                    popup.hide()
                    view.background = roundRect(colors.keyBg, dp(6).toFloat())
                }
            }
            true
        }
        return t
    }

    private fun judge(dx: Float, dy: Float, v: View): FlickResult =
        FlickLogic.judge(dx, dy, context.dpToPx(prefs.thresholdDp).toFloat(), v.width.toFloat(), v.height.toFloat())

    /** [v] の位置とサイズ（このビューの座標） */
    private fun rectOf(v: View): RectF {
        v.getLocationInWindow(locA)
        getLocationInWindow(locB)
        val x = (locA[0] - locB[0]).toFloat()
        val y = (locA[1] - locB[1]).toFloat()
        return RectF(x, y, x + v.width, y + v.height)
    }

    // ---- QWERTY ----

    private fun buildQwerty() {
        val h = rowHeight(50)
        enterIdleLabel = "⏎"

        var row = newRow(h)
        for (k in "qwertyuiop") row.addView(letterKey(k.toString()), keyParams(1f))

        row = newRow(h)
        row.addView(spacer(0.5f))
        for (k in "asdfghjkl") row.addView(letterKey(k.toString()), keyParams(1f))
        row.addView(spacer(0.5f))

        row = newRow(h)
        val sh = funcKey("⇧", { toggleShift() })
        shiftKey = sh
        row.addView(sh, keyParams(1.5f))
        for (k in "zxcvbnm") row.addView(letterKey(k.toString()), keyParams(1f))
        val bs = funcKey("⌫", null)
        attachRepeat(bs) { listener.onBackspace() }
        row.addView(bs, keyParams(1.5f))

        row = newRow(h)
        val mode = funcKey(if (kanaMode) "あ" else "A", { listener.onToggleMode() }, { toggleLayout() })
        modeKey = mode
        row.addView(mode, keyParams(1.2f))
        val num = funcKey("123", { listener.onOpenNumbers() })
        num.textSize = 14f
        row.addView(num, keyParams(1.1f))
        row.addView(punctKey("ー", "-"), keyParams(1f))
        val space = baseKey("空白", colors.keyBg)
        space.textSize = 16f
        attachTap(space, colors.keyBg, null) { listener.onSpace() }
        spaceKey = space
        row.addView(space, keyParams(3f))
        row.addView(punctKey("、", ","), keyParams(1f))
        row.addView(punctKey("。", "."), keyParams(1f))
        val enter = funcKey("⏎", { listener.onEnter() })
        enterKey = enter
        row.addView(enter, keyParams(1.2f))
    }

    private fun letterKey(c: String): TextView {
        val t = baseKey(c, colors.keyBg)
        t.tag = c
        attachTap(t, colors.keyBg, null) {
            val s = if (shift) c.uppercase() else c
            listener.onLetter(s)
            if (shift) toggleShift()
        }
        letterKeys.add(t)
        return t
    }

    private fun punctKey(label: String, text: String): TextView {
        val t = baseKey(label, colors.keyBg)
        attachTap(t, colors.keyBg, null) { listener.onLetter(text) }
        return t
    }

    private fun toggleShift() {
        shift = !shift
        shiftKey?.text = if (shift) "⇪" else "⇧"
        for (t in letterKeys) {
            val c = t.tag as? String ?: continue
            t.text = if (shift) c.uppercase() else c
        }
    }

    // ---- 数字・記号 ----

    private fun buildNumbers() {
        val h = rowHeight(50)
        enterIdleLabel = "⏎"

        var row = newRow(h)
        for (c in "1234567890") row.addView(directKey(c.toString()), keyParams(1f))

        row = newRow(h)
        for (c in listOf("-", "/", ":", ";", "(", ")", "¥", "&", "@", "\"")) {
            row.addView(directKey(c), keyParams(1f))
        }

        row = newRow(h)
        for (c in listOf(".", ",", "、", "。", "?", "!", "…", "~")) {
            row.addView(directKey(c), keyParams(1f))
        }
        val bs = funcKey("⌫", null)
        attachRepeat(bs) { listener.onBackspace() }
        row.addView(bs, keyParams(1.5f))

        row = newRow(h)
        val back = funcKey(if (kanaMode) "あいう" else "ABC", { setKanaMode(kanaMode) })
        back.textSize = 14f
        row.addView(back, keyParams(1.5f))
        val space = baseKey("空白", colors.keyBg)
        space.textSize = 16f
        attachTap(space, colors.keyBg, null) { listener.onSpace() }
        spaceKey = space
        row.addView(space, keyParams(4.5f))
        val enter = funcKey("⏎", { listener.onEnter() })
        enterKey = enter
        row.addView(enter, keyParams(1.5f))
    }

    private fun directKey(text: String): TextView {
        val t = baseKey(text, colors.keyBg)
        attachTap(t, colors.keyBg, null) { listener.onDirectText(text) }
        return t
    }

    // ---- 部品 ----

    private fun funcKey(label: String, onTap: (() -> Unit)?, onLong: (() -> Unit)? = null): TextView {
        val t = baseKey(label, colors.funcBg)
        t.textSize = 16f
        if (onTap != null) attachTap(t, colors.funcBg, onLong, onTap)
        return t
    }

    private fun baseKey(label: String, color: Int): TextView {
        val t = TextView(context)
        t.text = label
        t.gravity = Gravity.CENTER
        t.textSize = 20f
        t.setTextColor(colors.fg)
        t.background = roundRect(color, dp(6).toFloat())
        return t
    }

    private fun barButton(label: String, onTap: () -> Unit): TextView {
        val t = TextView(context)
        t.text = label
        t.textSize = 14f
        t.setTextColor(colors.subFg)
        t.gravity = Gravity.CENTER
        t.setPadding(dp(8), 0, dp(8), 0)
        t.setOnClickListener { onTap() }
        return t
    }

    private fun buzz(v: View) {
        if (prefs.haptic) v.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
    }

    /** 離したときに [onTap]、[onLong] があれば長押し（500ms）でそちらを呼ぶ */
    private fun attachTap(v: View, normal: Int, onLong: (() -> Unit)?, onTap: () -> Unit) {
        var longFired = false
        val longRun = Runnable {
            longFired = true
            buzz(v)
            onLong?.invoke()
        }
        v.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    longFired = false
                    view.background = roundRect(colors.pressedBg, dp(6).toFloat())
                    buzz(view)
                    if (onLong != null) uiHandler.postDelayed(longRun, 500)
                }
                MotionEvent.ACTION_UP -> {
                    uiHandler.removeCallbacks(longRun)
                    view.background = roundRect(normal, dp(6).toFloat())
                    if (!longFired) onTap()
                }
                MotionEvent.ACTION_CANCEL -> {
                    uiHandler.removeCallbacks(longRun)
                    view.background = roundRect(normal, dp(6).toFloat())
                }
            }
            true
        }
    }

    /** 押している間、一定間隔で繰り返す（⌫ 用） */
    private fun attachRepeat(v: View, action: () -> Unit) {
        val repeat = object : Runnable {
            override fun run() {
                action()
                uiHandler.postDelayed(this, 60)
            }
        }
        v.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    view.background = roundRect(colors.pressedBg, dp(6).toFloat())
                    buzz(view)
                    action()
                    uiHandler.postDelayed(repeat, 400)
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    view.background = roundRect(colors.funcBg, dp(6).toFloat())
                    uiHandler.removeCallbacks(repeat)
                }
            }
            true
        }
    }

    // ---- 12 キー ⇄ QWERTY ----

    private fun toggleLayout() {
        val next = if (prefs.layout == "flick") "qwerty" else "flick"
        KeyboardPrefs.setLayout(context, next)
        prefs = prefs.copy(layout = next)
        if (page == Page.NUMBERS) page = Page.KANA
        buildKeyArea()
    }

    // ---- 表示の更新 ----

    private fun renderLabels() {
        spaceKey?.text = if (composing && kanaMode) "変換" else "空白"
        enterKey?.text = if (composing) "確定" else enterIdleLabel
        reconvertBtn.visibility = if (composing) View.GONE else View.VISIBLE
    }

    private fun renderCandidates() {
        closeGrid()
        candBar.removeAllViews()
        var selView: View? = null
        for ((i, s) in candItems.withIndex()) {
            val t = TextView(context)
            t.text = s
            t.textSize = 18f
            t.setTextColor(colors.fg)
            t.gravity = Gravity.CENTER
            t.setPadding(dp(14), 0, dp(14), 0)
            if (i == candSelected) {
                t.background = roundRect(colors.pressedBg, dp(6).toFloat())
                selView = t
            }
            t.setOnClickListener { listener.onCandidate(i) }
            candBar.addView(t, lp(WRAP, MATCH))
        }
        candScroll.scrollTo(0, 0)
        val sv = selView
        if (sv != null && candSelected > 0) {
            candScroll.post { candScroll.scrollTo(max(0, sv.left - dp(48)), 0) }
        }
        expandBtn.visibility = if (candItems.size > 1) View.VISIBLE else View.GONE
    }

    private fun renderSegments() {
        segBar.removeAllViews()
        val converting = focusIndex >= 0 && segTexts.isNotEmpty()
        val vis = if (converting) View.VISIBLE else View.GONE
        segScroll.visibility = vis
        toolbarScroll.visibility = vis
        if (!converting) return
        var focusView: View? = null
        for ((i, s) in segTexts.withIndex()) {
            val t = TextView(context)
            t.text = s
            t.textSize = 17f
            t.gravity = Gravity.CENTER
            t.setPadding(dp(12), 0, dp(12), 0)
            if (i == focusIndex) {
                t.setTextColor(Color.WHITE)
                t.background = roundRect(colors.accent, dp(6).toFloat())
                focusView = t
            } else {
                t.setTextColor(colors.fg)
                t.paintFlags = t.paintFlags or Paint.UNDERLINE_TEXT_FLAG
            }
            t.setOnClickListener { listener.onFocusSegment(i) }
            val p = lp(WRAP, MATCH)
            p.setMargins(dp(2), dp(4), dp(2), dp(4))
            segBar.addView(t, p)
        }
        val fv = focusView
        if (fv != null) segScroll.post { segScroll.scrollTo(max(0, fv.left - dp(48)), 0) }
    }

    // ---- 全候補の縦グリッド ----

    private fun toggleGrid() {
        if (gridPanel != null) closeGrid() else openGrid()
    }

    private fun openGrid() {
        if (candItems.isEmpty()) return
        val panel = CandidateGrid.build(
            context = context,
            colors = colors,
            items = candItems,
            selected = candSelected,
            onPick = { i ->
                closeGrid()
                listener.onCandidate(i)
            },
            onClose = { closeGrid() },
        )
        // match_parent だと wrap_content の親が画面いっぱいに伸びるので、いまのキーボードの高さに固定する
        addView(panel, FrameLayout.LayoutParams(MATCH, max(rootView?.height ?: 0, dp(200))))
        gridPanel = panel
    }

    private fun closeGrid() {
        val g = gridPanel ?: return
        removeView(g)
        gridPanel = null
    }

    private fun dp(v: Int): Int = context.dpToPx(v.toFloat())

    /** 文節・範囲操作ツールバーの 1 ボタン */
    private class Tool(val label: String, val type: String, val content: String?)

    private companion object {
        val TOOLS = listOf(
            Tool("◀文節", "move_focus", "left"),
            Tool("文節▶", "move_focus", "right"),
            Tool("区切り縮", "resize_segment", "left"),
            Tool("区切り伸", "resize_segment", "right"),
            Tool("選択◀", "extend_selection", "left"),
            Tool("選択▶", "extend_selection", "right"),
            Tool("カタカナ", "to_katakana", null),
            Tool("ひらがな", "to_hiragana", null),
            Tool("半角", "to_hankaku", null),
            Tool("取消", "cancel", null),
        )
    }
}
