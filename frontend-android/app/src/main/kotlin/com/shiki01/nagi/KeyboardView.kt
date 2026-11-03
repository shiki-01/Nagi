package com.shiki01.nagi

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import kotlin.math.hypot
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

    /** 入力中の文字を確定してから [block] を実行する（入力中でなければすぐ実行）。レイアウト切替・ショートカット用 */
    fun onCommitThen(block: () -> Unit)

    /** かな / 英字の入力モードを合わせる（レイアウト切替キー用。コアの入力モードも変える。画面の組み立てはしない） */
    fun onSetKana(kana: Boolean)

    /** 変換キー（`convert`）。入力中の文字の変換を始める */
    fun onConvert()

    /** キーボードを閉じる（`dismiss`） */
    fun onDismiss()

    /** クリップボードの文字を入れる（`paste`） */
    fun onPaste()

    /** カーソル手前の単語を削除する（`deleteWord`。入力中は何もしない） */
    fun onDeleteWord()

    /**
     * ガラケー式（toggle）の 1 打鍵。[previous] があれば直前に出した文字を消してから [text] を入れる。
     * [viaCore] が true ならコアを通す（かな）。false ならアプリへ直接入れる
     */
    fun onCycleText(text: String, previous: String?, viaCore: Boolean)
}

private const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

private fun lp(w: Int, h: Int, weight: Float = 0f) = LinearLayout.LayoutParams(w, h, weight)

/**
 * レイアウト（JSON。[LayoutStore] が読む）で組み立てるキーボード + 候補バー + 文節列 + 文節操作ツールバー
 * （プログラムで組み立てる View）。ロジックは持たず、操作を [KeyboardListener] に渡すだけにする。
 * JSON のレイアウトを 1 つも読めないときは、従来のコード組み立て（`legacy*`。[BuiltinLayouts]）に落ちる。
 */
internal class KeyboardView(context: Context, private val listener: KeyboardListener) : FrameLayout(context) {
    /** 設定画面で変えられる値のスナップショット。変わったら組み立て直す */
    private data class Prefs(
        val layout: String,
        val kanaId: String,
        val alphaId: String,
        val layoutsRevision: Long,
        val toggleInput: Boolean,
        val thresholdDp: Float,
        val popup: Boolean,
        val hints: Boolean,
        val longPressMs: Int,
        val haptic: Boolean,
        val heightScale: Float,
        val theme: String,
        val night: Boolean,
    )

    private enum class ShiftState { OFF, ONCE, CAPS }

    /** 直前のトグル入力（同じキーの連打で文字を置き換えるための記録） */
    private class ToggleState(
        val layoutId: String,
        val keyId: String,
        val index: Int,
        val at: Long,
        val text: String,
        val upper: Boolean,
    )

    /** 組み立てた JSON キーと、その View */
    private class KeyBinding(val face: KeyFace, val key: LayoutKey)

    private val uiHandler = Handler(Looper.getMainLooper())
    private var prefs: Prefs = readPrefs()
    private var colors: KeyboardColors = KeyboardColors.resolve(context)

    /** 明示的に開いているレイアウトの ID（数字・記号など）。null なら かな / 英字 の割り当てに従う */
    private var explicitId: String? = null

    /** 表示中の JSON レイアウト。null は組み込み（従来のコード組み立て）で表示している */
    private var current: LayoutModel? = null
    private val bindings = ArrayList<KeyBinding>()
    private var shiftState = ShiftState.OFF
    private var lastShiftTapAt = 0L
    private var toggleState: ToggleState? = null

    /** 入力欄の指定による改行キーの文字（検索・送信など。null ならレイアウトのまま） */
    private var enterOverride: String? = null
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
    private lateinit var longPopup: LongPressPopupView

    private val locA = IntArray(2)
    private val locB = IntArray(2)

    init {
        buildUi()
    }

    // ---- 外から呼ぶ ----

    /** かな入力か英数直接入力かの表示を切り替える（数字・記号の画面からは かな / 英数 の割り当て済みレイアウトに戻る） */
    fun setKanaMode(kana: Boolean) {
        kanaMode = kana
        explicitId = null
        buildKeyArea()
    }

    /** 数字・記号の画面を出す */
    fun showNumbers() {
        explicitId = LayoutIds.NUMBERS
        buildKeyArea()
    }

    /**
     * [id] のレイアウトを開く（入力欄の種類による自動選択用）。読めなければ何もせず null。
     * 読めたら、そのレイアウトの用途を返す
     */
    fun showLayoutId(id: String): LayoutMode? {
        val l = LayoutStore.load(context, id) ?: return null
        explicitId = id
        buildKeyArea()
        return l.mode
    }

    /** 入力欄の指定による改行キーの文字（検索・送信など）。null でレイアウトのまま */
    fun setEnterLabel(label: String?) {
        if (enterOverride == label) return
        enterOverride = label
        renderLabels()
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
            kanaId = KeyboardPrefs.kanaLayoutId(context),
            alphaId = KeyboardPrefs.alphaLayoutId(context),
            layoutsRevision = KeyboardPrefs.layoutsRevision(context),
            toggleInput = KeyboardPrefs.toggleInput(context),
            thresholdDp = KeyboardPrefs.flickThresholdDp(context),
            popup = KeyboardPrefs.flickPopup(context),
            hints = KeyboardPrefs.flickHints(context),
            longPressMs = KeyboardPrefs.flickLongPressMs(context),
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
        longPopup = LongPressPopupView(context, colors)
        addView(longPopup, FrameLayout.LayoutParams(MATCH, MATCH))

        buildKeyArea()
        renderCandidates()
        renderSegments()
    }

    private fun buildKeyArea() {
        keyArea.removeAllViews()
        letterKeys.clear()
        bindings.clear()
        spaceKey = null
        enterKey = null
        modeKey = null
        shiftKey = null
        shift = false
        shiftState = ShiftState.OFF
        toggleState = null
        if (::longPopup.isInitialized) longPopup.hide()
        val l = resolveLayout()
        current = l
        if (l != null) {
            buildFromLayout(l)
        } else {
            // JSON のレイアウトを読めない。従来のコード組み立てに落ちる（入力を失わない最後の砦）
            when (BuiltinLayouts.kindFor(explicitId, kanaMode, prefs.layout == "flick")) {
                BuiltinLayouts.Kind.NUMBERS -> legacyNumbers()
                BuiltinLayouts.Kind.FLICK -> legacyFlick()
                BuiltinLayouts.Kind.QWERTY -> legacyQwerty()
            }
        }
        renderLabels()
    }

    /** 表示するレイアウトを決める。明示指定 → かな / 英字の割り当て → そのモードの既定。どれも読めなければ null（組み込み） */
    private fun resolveLayout(): LayoutModel? {
        val explicit = explicitId
        if (explicit != null) {
            LayoutStore.load(context, explicit)?.let { return it }
            // 数字・記号・絵文字が読めないときは、割り当て済みのかな / 英字ではなく組み込みの数字画面にする
            if (BuiltinLayouts.kindFor(explicit, kanaMode, false) == BuiltinLayouts.Kind.NUMBERS) return null
        }
        val id = if (kanaMode) prefs.kanaId else prefs.alphaId
        return LayoutStore.load(context, id) ?: LayoutStore.load(context, LayoutIds.defaultFor(kanaMode))
    }

    private fun rowHeight(baseDp: Int): Int = (dp(baseDp) * prefs.heightScale).toInt()

    // ---- JSON レイアウトからの組み立て ----

    private fun buildFromLayout(l: LayoutModel) {
        // 12 キー（フリック）を含むレイアウトは従来どおり少し高く、それ以外は QWERTY と同じ高さ
        val hasFlick = l.rows.any { r -> r.keys.any { it.action is KeyAction.Flick } }
        val baseDp = if (hasFlick) 52 else 50
        if (l.mode == LayoutMode.EMOJI) keyArea.addView(buildEmojiGrid(), lp(MATCH, rowHeight(baseDp) * 3))
        for (row in l.rows) {
            val r = newRow((rowHeight(baseDp) * row.height).toInt())
            for (key in row.keys) addJsonKey(r, key, l)
        }
    }

    private fun buildEmojiGrid(): View {
        val scroll = ScrollView(context)
        val col = LinearLayout(context)
        col.orientation = LinearLayout.VERTICAL
        val perRow = 8
        for (chunk in EmojiData.ALL.chunked(perRow)) {
            val row = LinearLayout(context)
            row.orientation = LinearLayout.HORIZONTAL
            for (e in chunk) {
                val t = TextView(context)
                t.text = e
                t.textSize = 24f
                t.gravity = Gravity.CENTER
                t.setOnClickListener { listener.onDirectText(e) }
                row.addView(t, lp(0, dp(44), 1f))
            }
            // 最後の行は幅をそろえるため空の View で埋める
            for (i in chunk.size until perRow) row.addView(spacer(1f))
            col.addView(row, lp(MATCH, WRAP))
        }
        scroll.addView(col, FrameLayout.LayoutParams(MATCH, WRAP))
        return scroll
    }

    private fun addJsonKey(row: LinearLayout, key: LayoutKey, l: LayoutModel) {
        val a = key.action
        if (a is KeyAction.Spacer) {
            row.addView(spacer(key.width.toFloat()))
            return
        }
        // 12 キー名がコアの表にもフォールバック表にも無いときは、押せないので空白にする
        val flickDef = if (a is KeyAction.Flick) flickDefOf(key, a) else null
        if (a is KeyAction.Flick && flickDef == null) {
            row.addView(spacer(key.width.toFloat()))
            return
        }
        val face = KeyFace(context, colors)
        face.normalColor = roleColor(key.role)
        face.setTextColor(if (key.role == KeyRole.ACCENT) Color.WHITE else colors.fg)
        val text = if (a is KeyAction.Flick && key.label == null && flickDef != null) flickDef.center else LayoutLabels.staticLabel(key)
        face.text = text
        face.textSize = when {
            text.codePointCount(0, text.length) >= 3 -> 14f
            a is KeyAction.Flick || a is KeyAction.Text -> 20f
            else -> 16f
        }
        face.showPressed(false)
        if (flickDef != null && prefs.hints) {
            val hs = FlickHints(flickDef.left, flickDef.up, flickDef.right, flickDef.down)
            if (hs.any) face.hints = hs
        }
        // 長押しがあるキーは、最初の項目を右上に小さく出す（flick と ⌫ の longPress は動かないので出さない）
        if (prefs.hints && a !is KeyAction.Flick && !isBackspace(a)) face.cornerLabel = key.longPress.firstOrNull()?.label
        when {
            a is KeyAction.Flick && flickDef != null -> attachFlickKey(face, key, a, flickDef, l)
            isBackspace(a) -> attachRepeatKey(face)
            else -> attachJsonKey(face, key, l)
        }
        row.addView(face, keyParams(key.width.toFloat()))
        bindings.add(KeyBinding(face, key))
    }

    private fun isBackspace(a: KeyAction): Boolean = a is KeyAction.Act && a.name == "backspace"

    /** フリックキーの文字表。`kana` 指定はコアの表（無ければフォールバック表）、無指定は各方向の文字 */
    private fun flickDefOf(key: LayoutKey, a: KeyAction.Flick): KanaKeyDef? {
        val kk = a.kanaKey
        if (kk != null) {
            return keyTable.firstOrNull { it.key == kk } ?: KeyTables.FALLBACK.firstOrNull { it.key == kk }
        }
        val c = a.center ?: ""
        return KanaKeyDef(key.id, c, a.left ?: "", a.up ?: "", a.right ?: "", a.down ?: "", listOf(c))
    }

    // ---- JSON キーのタッチ処理 ----

    private fun breakToggle() {
        toggleState = null
    }

    /** タップ + 長押し（項目のポップアップ）。長押しが無いキーはタップだけ */
    private fun attachJsonKey(face: KeyFace, key: LayoutKey, l: LayoutModel) {
        val items = key.longPress
        var downX = 0f
        var downY = 0f
        var longShown = false
        var tapCancelled = false
        var keyRect = RectF()
        val longRun = Runnable {
            longShown = true
            buzz(face)
            longPopup.show(keyRect, items.map { it.label })
        }
        face.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    longShown = false
                    tapCancelled = false
                    face.showPressed(true)
                    buzz(view)
                    view.parent?.requestDisallowInterceptTouchEvent(true)
                    if (items.isNotEmpty()) {
                        keyRect = rectOf(view)
                        uiHandler.postDelayed(longRun, prefs.longPressMs.toLong())
                    }
                }
                MotionEvent.ACTION_MOVE -> {
                    val moved = hypot((ev.x - downX).toDouble(), (ev.y - downY).toDouble()).toFloat()
                    if (longShown) {
                        longPopup.setActive(longPopup.indexAt(keyRect.left + ev.x))
                    } else {
                        // 長押しの判定時間の前に大きく動いたら長押しにしない。さらに大きく動いたらタップにもしない
                        if (moved >= dp(LONG_PRESS_MOVE_DP)) uiHandler.removeCallbacks(longRun)
                        if (moved >= dp(TAP_MOVE_DP)) tapCancelled = true
                    }
                }
                MotionEvent.ACTION_UP -> {
                    uiHandler.removeCallbacks(longRun)
                    face.showPressed(false)
                    if (longShown) {
                        // ポップアップの項目を実行する。キーの下端より大きく下で離したら取り消し
                        val below = ev.y > view.height * (1f + LONG_PRESS_CANCEL_BELOW)
                        val idx = if (below) -1 else longPopup.indexAt(keyRect.left + ev.x)
                        longPopup.hide()
                        if (idx in items.indices) activate(items[idx].action, null, l)
                    } else if (!tapCancelled) {
                        activate(key.action, key, l)
                    }
                }
                MotionEvent.ACTION_CANCEL -> {
                    uiHandler.removeCallbacks(longRun)
                    longPopup.hide()
                    face.showPressed(false)
                }
            }
            true
        }
    }

    /** フリックのキー。押している間はポップアップを出し、離したときに方向を判定して送る */
    private fun attachFlickKey(face: KeyFace, key: LayoutKey, a: KeyAction.Flick, def: KanaKeyDef, l: LayoutModel) {
        var downX = 0f
        var downY = 0f
        var keyRect = RectF()
        face.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    face.showPressed(true)
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
                    face.showPressed(false)
                    val r = judge(ev.x - downX, ev.y - downY, view)
                    if (!r.cancelled) {
                        breakToggle()
                        val flick = r.direction != FlickDirection.TAP
                        if (a.kanaKey != null) {
                            listener.onKana(
                                def.key,
                                FlickLogic.gestureFor(r, prefs.toggleInput),
                                if (flick) r.angleDeg else null,
                                if (flick) r.distance else null,
                            )
                        } else {
                            // 文字を直接指定したフリック（kana を持たない）は、その文字をそのまま送る
                            val s = FlickLogic.charAt(def, r.direction)
                            if (s.isNotEmpty()) sendText(s, l)
                        }
                    }
                }
                MotionEvent.ACTION_CANCEL -> {
                    popup.hide()
                    face.showPressed(false)
                }
            }
            true
        }
    }

    /** ⌫: 押している間、一定間隔で繰り返す */
    private fun attachRepeatKey(face: KeyFace) {
        val repeat = object : Runnable {
            override fun run() {
                listener.onBackspace()
                uiHandler.postDelayed(this, 60)
            }
        }
        face.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    face.showPressed(true)
                    buzz(view)
                    breakToggle()
                    listener.onBackspace()
                    uiHandler.postDelayed(repeat, 400)
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    face.showPressed(false)
                    uiHandler.removeCallbacks(repeat)
                }
            }
            true
        }
    }

    // ---- キーの動作 ----

    /** [a] を実行する。[key] はトグル入力（キーの識別）に使う。長押しの項目では null */
    private fun activate(a: KeyAction, key: LayoutKey?, l: LayoutModel) {
        if (a !is KeyAction.Toggle) breakToggle()
        when (a) {
            is KeyAction.Text -> typeText(a, l)
            is KeyAction.Act -> performAct(a.name, l)
            is KeyAction.Shortcut -> runShortcut(a.steps, l)
            is KeyAction.LayoutSwitch -> switchLayout(a.target)
            is KeyAction.Toggle -> if (key != null) cycleText(key, a, l)
            is KeyAction.Flick -> Unit
            is KeyAction.Spacer -> Unit
        }
    }

    /** 文字を送る。かなのレイアウトはコア（ローマ字変換）へ、それ以外はアプリへ直接入れる */
    private fun sendText(text: String, l: LayoutModel) {
        if (l.mode == LayoutMode.KANA) listener.onLetter(text) else listener.onDirectText(text)
    }

    private fun typeText(a: KeyAction.Text, l: LayoutModel) {
        val shiftable = l.mode == LayoutMode.ALPHA
        val shiftOn = shiftable && shiftState != ShiftState.OFF
        sendText(LayoutLabels.shifted(a, shiftOn), l)
        if (shiftOn && shiftState == ShiftState.ONCE) {
            shiftState = ShiftState.OFF
            renderLabels()
        }
    }

    private fun cycleText(key: LayoutKey, a: KeyAction.Toggle, l: LayoutModel) {
        val now = SystemClock.uptimeMillis()
        val prev = toggleState
        val same = prev != null && prev.layoutId == l.id && prev.keyId == key.id
        val elapsed = if (prev != null) now - prev.at else 0L
        val continuing = prev != null && ToggleLogic.continues(a.cycle.size, same, elapsed)
        val idx = ToggleLogic.nextIndex(prev?.index, a.cycle.size, same, elapsed)
        var upper = if (continuing && prev != null) prev.upper else false
        var text = a.cycle[idx]
        if (!continuing && l.mode == LayoutMode.ALPHA && shiftState != ShiftState.OFF) {
            upper = true
            if (shiftState == ShiftState.ONCE) {
                shiftState = ShiftState.OFF
                renderLabels()
            }
        }
        if (upper) text = text.uppercase()
        toggleState = ToggleState(l.id, key.id, idx, now, text, upper)
        listener.onCycleText(text, if (continuing) prev?.text else null, l.mode == LayoutMode.KANA)
    }

    private fun performAct(name: String, l: LayoutModel?) {
        val kanaLayout = l?.mode == LayoutMode.KANA
        when (name) {
            "space" -> listener.onSpace()
            "enter" -> listener.onEnter()
            "backspace" -> listener.onBackspace()
            "shift" -> if (!kanaLayout) tapShift()
            "capslock" -> if (!kanaLayout) toggleCaps()
            "convert" -> listener.onConvert()
            "moveLeft" -> listener.onArrow(true)
            "moveRight" -> listener.onArrow(false)
            "dakuten", "smallKana" -> listener.onVariant()
            "modeSwitch" -> listener.onToggleMode()
            "globe" -> listener.onNextInputMethod()
            "dismiss" -> listener.onDismiss()
            "paste" -> listener.onPaste()
            "deleteWord" -> listener.onDeleteWord()
            else -> Unit // history など、Android では未対応の操作は無視する
        }
    }

    /** 入力中の文字を確定してから、手順を順に実行する。未知の action に当たったらそこで中断する */
    private fun runShortcut(steps: List<ShortcutStep>, l: LayoutModel) {
        listener.onCommitThen {
            for (step in steps) {
                val text = step.text
                val action = step.action
                if (text != null) {
                    listener.onDirectText(text)
                } else if (action != null && action in LayoutLimits.KNOWN_ACTIONS) {
                    performAct(action, l)
                } else {
                    break
                }
            }
        }
    }

    /** 別のレイアウトへ切り替える。入力中の文字は先に確定する。切替先が読めなければ何もしない */
    private fun switchLayout(target: String) {
        val next: LayoutModel = when (target) {
            LayoutSelect.TARGET_KANA -> LayoutStore.loadAssigned(context, true)
            LayoutSelect.TARGET_ALPHA -> LayoutStore.loadAssigned(context, false)
            else -> LayoutStore.load(context, target)
        } ?: return
        val wantKana = LayoutSelect.kanaModeOf(next.mode)
        val id = if (target == LayoutSelect.TARGET_KANA || target == LayoutSelect.TARGET_ALPHA) null else next.id
        listener.onCommitThen {
            if (wantKana != null) {
                listener.onSetKana(wantKana)
                kanaMode = wantKana
            }
            explicitId = id
            buildKeyArea()
        }
    }

    private fun tapShift() {
        val now = SystemClock.uptimeMillis()
        val double = now - lastShiftTapAt < DOUBLE_TAP_MS
        lastShiftTapAt = now
        shiftState = when (shiftState) {
            ShiftState.OFF -> ShiftState.ONCE
            ShiftState.ONCE -> if (double) ShiftState.CAPS else ShiftState.OFF
            ShiftState.CAPS -> ShiftState.OFF
        }
        renderLabels()
    }

    private fun toggleCaps() {
        shiftState = if (shiftState == ShiftState.CAPS) ShiftState.OFF else ShiftState.CAPS
        renderLabels()
    }

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

    // ---- 組み込み（従来のコード組み立て。JSON のレイアウトを読めないときのフォールバック） ----

    private fun legacyFlick() {
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

    private fun legacyQwerty() {
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

    private fun legacyNumbers() {
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

    /**
     * 候補バーの ⇄ ボタン。かなは 12 キー ⇄ QWERTY、英字は QWERTY ⇄ 12 キーを切り替え、割り当ても更新する。
     * 数字・記号などを開いているときは、かな / 英字の割り当て済みレイアウトに戻る
     */
    private fun toggleLayout() {
        val cur = current
        if (cur == null) {
            // 組み込み（従来）の表示
            val next = if (prefs.layout == "flick") "qwerty" else "flick"
            KeyboardPrefs.setLayout(context, next)
            prefs = readPrefs()
            explicitId = null
            buildKeyArea()
            return
        }
        val target = if (explicitId == null) LayoutSelect.toggleTarget(cur.mode, cur.id) else null
        if (target != null && LayoutStore.load(context, target) != null) {
            LayoutStore.assign(context, if (kanaMode) AssignSlot.KANA else AssignSlot.ALPHA, target)
            prefs = readPrefs()
        }
        explicitId = null
        buildKeyArea()
    }

    // ---- 表示の更新 ----

    private fun renderLabels() {
        spaceKey?.text = if (composing && kanaMode) "変換" else "空白"
        enterKey?.text = if (composing) "確定" else enterOverride ?: enterIdleLabel
        reconvertBtn.visibility = if (composing) View.GONE else View.VISIBLE
        val alpha = current?.mode == LayoutMode.ALPHA
        for (b in bindings) {
            val a = b.key.action
            val face = b.face
            when {
                a is KeyAction.Act && a.name == "space" ->
                    face.text = LayoutLabels.spaceLabel(composing, kanaMode, b.key.label)
                a is KeyAction.Act && a.name == "enter" ->
                    face.text = LayoutLabels.enterLabel(composing, enterOverride, b.key.label)
                a is KeyAction.Act && (a.name == "shift" || a.name == "capslock") -> {
                    if (b.key.label == null) face.text = if (shiftState == ShiftState.CAPS) "⇪" else "⇧"
                    face.normalColor = if (shiftState != ShiftState.OFF) colors.pressedBg else roleColor(b.key.role)
                    face.showPressed(false)
                }
                a is KeyAction.Text && alpha ->
                    face.text = LayoutLabels.textKeyFace(b.key, a, shiftState != ShiftState.OFF)
            }
        }
    }

    private fun roleColor(role: KeyRole): Int = when (role) {
        KeyRole.NORMAL -> colors.keyBg
        KeyRole.FUNCTION -> colors.funcBg
        KeyRole.ACCENT -> colors.accent
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
        /** 長押しの判定時間の前にこれ以上動いたら長押しにしない（dp） */
        const val LONG_PRESS_MOVE_DP = 20

        /** これ以上動かして離したらタップにしない（dp） */
        const val TAP_MOVE_DP = 40

        /** 長押しのポップアップで、キーの下端よりこの割合（キーの高さに対して）以上下で離したら取り消し */
        const val LONG_PRESS_CANCEL_BELOW = 0.3f

        /** Shift の二度押し（Caps Lock）とみなす間隔 */
        const val DOUBLE_TAP_MS = 500L

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
