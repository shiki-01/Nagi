package com.shiki01.nagi

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.ForegroundColorSpan
import android.text.style.RelativeSizeSpan
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.core.view.ViewCompat
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

    /**
     * 数字・記号の画面（または数字・記号を割り当てたキー）の文字。設定 `compose_numbers` が有効なら
     * コアの入力中の文字列に入れ、そうでなければ [onDirectText] と同じく直接入れる（判断はサービス）
     */
    fun onNumberText(text: String)

    /**
     * レイアウトを [from] から [to]（どちらも [LayoutMode.json]）へ切り替える。入力中の文字列は、設定に従って確定してから、
     * または確定せずに [block] を実行する（サービスが判断する）
     */
    fun onLayoutSwitch(from: String, to: String, block: () -> Unit)

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
     * スペースキーの横スワイプによるカーソル移動。[steps] は動かす文字数（右が正、左が負）。
     * 変換中は注目文節、入力中は入力中の文字列内のカーソル、未入力は入力欄のカーソルを動かす
     */
    fun onSpaceSwipe(steps: Int)

    /** ⌫ の左スワイプ。手前の単語をまとめて削除する（入力中はコアの backspace の連続） */
    fun onBackspaceWord()

    /**
     * ガラケー式（toggle）の 1 打鍵。[previous] があれば直前に出した文字を消してから [text] を入れる。
     * [viaCore] が true ならコアを通す（かな）。false ならアプリへ直接入れる
     */
    fun onCycleText(text: String, previous: String?, viaCore: Boolean)

    /** クリップボード履歴のカードの文字を入れる（入力中の文字を確定してから。内容はログに出さない） */
    fun onClipboardInsert(text: String)

    /** パスワード欄・学習しない指定の入力欄か（クリップボード履歴の内容を出さない判定に使う） */
    fun isSecureInput(): Boolean
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
        val sensitivity: Float,
        val popup: Boolean,
        val hints: Boolean,
        val longPressMs: Int,
        val haptic: HapticLevel,
        val keySound: Boolean,
        /** 今の向きで使う高さの倍率（縦は `height_scale`、横は `height_scale_landscape`） */
        val heightScale: Float,
        val theme: String,
        val accent: AccentColor,
        val night: Boolean,
        val spaceSwipe: Boolean,
        val spaceSwipeStepDp: Float,
        val spaceSwipeAccel: Boolean,
        val spaceSwipeAccelStrength: Float,
        val backspaceSwipe: Boolean,
        val repeatDelayMs: Int,
        val repeatMs: Int,
        val oneHanded: OneHanded,
        val oneHandedWidth: Float,
        val keySpacingDp: Float,
        val keyCornerDp: Float,
        /** 「アニメーションをオフ」でないか */
        val animations: Boolean,
        /** ツールバーの項目と並び順（`toolbar.items`）、「貼り付け」を出すか（`candbar.paste_button`） */
        val toolbarItems: List<ToolbarItem>,
        val pasteButton: Boolean,
    )

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
    private class KeyBinding(val face: KeyFace, val key: LayoutKey, val flickDef: KanaKeyDef?)

    private val uiHandler = Handler(Looper.getMainLooper())
    private var prefs: Prefs = readPrefs()
    private var colors: KeyboardColors = KeyboardColors.resolve(context)
    private val audio: AudioManager? = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager

    /** 明示的に開いているレイアウトの ID（数字・記号など）。null なら かな / 英字 の割り当てに従う */
    private var explicitId: String? = null

    /** 表示中の JSON レイアウト。null は組み込み（従来のコード組み立て）で表示している */
    private var current: LayoutModel? = null
    private val bindings = ArrayList<KeyBinding>()
    private var shiftState = ShiftState.OFF
    private var lastShiftTapAt: Long? = null
    private var toggleState: ToggleState? = null

    /** 入力欄の指定による改行キーの文字（検索・送信など。null ならレイアウトのまま） */
    private var enterOverride: String? = null
    private var kanaMode = true
    private var keyTable: List<KanaKeyDef> = KeyTables.FALLBACK
    private var composing = false

    private var bar: CandidateBar = CandidateBar.EMPTY
    private val candItems: List<String> get() = bar.items
    private val candSelected: Int get() = bar.selected
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

    /** 入力中でなく候補も無いときに、候補バーの場所へ出す操作の列（ツールバー） */
    private lateinit var actionScroll: HorizontalScrollView
    private lateinit var actionRow: LinearLayout

    /** いま actionRow に並べている項目（変わったときだけ作り直す）。null は未構築 */
    private var shownActions: List<ToolbarItem>? = null

    /** クリップボードに文字があるか（「貼り付け」を出す判定。サービスが [setClipboardHasText] で伝える） */
    private var clipboardHasText = false
    private var historyPanel: ClipboardHistoryPanel? = null
    private lateinit var candBar: LinearLayout
    private lateinit var toolbarScroll: HorizontalScrollView
    private lateinit var reconvertBtn: TextView
    private lateinit var expandBtn: TextView
    private lateinit var pinnedBtn: TextView
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
        renderToolbar()
    }

    /** クリップボードに文字があるか。「貼り付け」ボタンの出し分けが変わるときだけ描き直す */
    fun setClipboardHasText(has: Boolean) {
        if (clipboardHasText == has) return
        clipboardHasText = has
        renderToolbar()
    }

    /** クリップボード履歴のページを開いていれば、内容を読み直す（新しくコピーされたとき） */
    fun refreshClipboardHistory() {
        historyPanel?.reload()
    }

    /** 候補バーを更新する。空・非表示なら空にする（高さは保つ） */
    fun setCandidates(bar: CandidateBar) {
        this.bar = bar
        renderCandidates()
    }

    /** 変換中の文節列（[focus] は注目文節の番号。変換中でなければ -1 または空） */
    fun setSegments(texts: List<String>, focus: Int) {
        segTexts = texts
        focusIndex = focus
        renderSegments()
        renderToolbar()
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
        val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
        return Prefs(
            layout = KeyboardPrefs.layout(context),
            kanaId = KeyboardPrefs.kanaLayoutId(context),
            alphaId = KeyboardPrefs.alphaLayoutId(context),
            layoutsRevision = KeyboardPrefs.layoutsRevision(context),
            toggleInput = KeyboardPrefs.toggleInput(context),
            thresholdDp = KeyboardPrefs.flickThresholdDp(context),
            sensitivity = KeyboardPrefs.flickSensitivity(context),
            popup = KeyboardPrefs.flickPopup(context),
            hints = KeyboardPrefs.flickHints(context),
            longPressMs = KeyboardPrefs.flickLongPressMs(context),
            haptic = KeyboardPrefs.hapticLevel(context),
            keySound = KeyboardPrefs.keySound(context),
            heightScale = if (landscape) KeyboardPrefs.heightScaleLandscape(context) else KeyboardPrefs.heightScale(context),
            theme = KeyboardPrefs.theme(context),
            accent = KeyboardPrefs.accent(context),
            night = night,
            spaceSwipe = KeyboardPrefs.spaceSwipe(context),
            spaceSwipeStepDp = KeyboardPrefs.spaceSwipeStepDp(context),
            spaceSwipeAccel = KeyboardPrefs.spaceSwipeAccel(context),
            spaceSwipeAccelStrength = KeyboardPrefs.spaceSwipeAccelStrength(context),
            backspaceSwipe = KeyboardPrefs.backspaceSwipe(context),
            repeatDelayMs = KeyboardPrefs.keyRepeatDelayMs(context),
            repeatMs = KeyboardPrefs.keyRepeatMs(context),
            oneHanded = KeyboardPrefs.oneHanded(context),
            oneHandedWidth = KeyboardPrefs.oneHandedWidth(context),
            keySpacingDp = KeyboardPrefs.keySpacingDp(context),
            keyCornerDp = KeyboardPrefs.keyCornerDp(context),
            animations = MotionPrefs.animationsEnabled(context),
            toolbarItems = KeyboardPrefs.toolbarItems(context),
            pasteButton = KeyboardPrefs.pasteButton(context),
        )
    }

    private fun buildUi() {
        removeAllViews()
        uiHandler.removeCallbacksAndMessages(null)
        gridPanel = null
        historyPanel = null
        shownActions = null
        colors = KeyboardColors.resolve(context)
        setBackgroundColor(colors.bg)
        // 同梱のレイアウトが使う Iconify アイコンを（未取得なら）バックグラウンドで取得する。プロセスにつき 1 回
        IconifyResolver.prefetchPresetIcons(context)

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
        ViewCompat.setAccessibilityPaneTitle(segScroll, "文節列")
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
        // ツールバー（候補の無いときだけ candScroll の代わりに出す）
        actionRow = LinearLayout(context)
        actionRow.orientation = LinearLayout.HORIZONTAL
        actionRow.gravity = Gravity.CENTER_VERTICAL
        actionScroll = HorizontalScrollView(context)
        actionScroll.isHorizontalScrollBarEnabled = false
        actionScroll.isFillViewport = true
        actionScroll.addView(actionRow, FrameLayout.LayoutParams(WRAP, MATCH))
        actionScroll.visibility = View.GONE
        ViewCompat.setAccessibilityPaneTitle(actionScroll, "ツールバー")
        candRow.addView(actionScroll, lp(0, MATCH, 1f))
        reconvertBtn = barButton("再変換", "選択した文字を再変換") { listener.onReconvert() }
        expandBtn = barButton("▾", "全候補を開く") { toggleGrid() }
        val layoutBtn = barButton("⇄", "12キーとローマ字入力の切り替え") { toggleLayout() }
        // 補正候補（もしかして）の最有力を右端に固定する。展開・再変換ボタンの左
        pinnedBtn = TextView(context)
        pinnedBtn.textSize = 16f
        pinnedBtn.gravity = Gravity.CENTER
        pinnedBtn.maxLines = 2
        pinnedBtn.setPadding(dp(10), 0, dp(10), 0)
        pinnedBtn.setTextColor(colors.fg)
        pinnedBtn.visibility = View.GONE
        val pinnedLp = lp(WRAP, MATCH)
        pinnedLp.setMargins(dp(2), dp(4), dp(2), dp(4))
        candRow.addView(pinnedBtn, pinnedLp)
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
        ViewCompat.setAccessibilityPaneTitle(toolbarScroll, "文節の操作")
        for (t in TOOLS) {
            val b = TextView(context)
            b.text = t.label
            b.contentDescription = t.desc
            b.textSize = 13f
            b.setTextColor(colors.fg)
            b.gravity = Gravity.CENTER
            b.setPadding(dp(10), 0, dp(10), 0)
            b.background = roundRect(colors.funcBg, cornerPx)
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

        addBody(root)
        popup = FlickPopupView(context, colors)
        addView(popup, FrameLayout.LayoutParams(MATCH, MATCH))
        longPopup = LongPressPopupView(context, colors)
        addView(longPopup, FrameLayout.LayoutParams(MATCH, MATCH))

        buildKeyArea()
        renderCandidates()
        renderSegments()
    }

    /**
     * キーボード本体 [root] を置く。片手モードのときは幅を絞って寄せ、空いた側に「⇄」（反対側へ寄せる）と
     * 「↔」（通常に戻す）の小さなボタンを縦に並べる
     */
    private fun addBody(root: LinearLayout) {
        val mode = prefs.oneHanded
        if (mode == OneHanded.OFF) {
            addView(root, FrameLayout.LayoutParams(MATCH, WRAP))
            return
        }
        val frac = OneHandedLogic.bodyFraction(mode, prefs.oneHandedWidth)
        val row = LinearLayout(context)
        row.orientation = LinearLayout.HORIZONTAL
        val side = oneHandedButtons(mode)
        // 重みで幅を按分する（本体 : 空き側 = frac : 1 - frac）
        val bodyLp = LinearLayout.LayoutParams(0, WRAP, frac)
        val sideLp = LinearLayout.LayoutParams(0, MATCH, 1f - frac)
        if (mode == OneHanded.LEFT) {
            row.addView(root, bodyLp)
            row.addView(side, sideLp)
        } else {
            row.addView(side, sideLp)
            row.addView(root, bodyLp)
        }
        addView(row, FrameLayout.LayoutParams(MATCH, WRAP))
    }

    /** 片手モードで空いた側のボタン列 */
    private fun oneHandedButtons(mode: OneHanded): View {
        val col = LinearLayout(context)
        col.orientation = LinearLayout.VERTICAL
        col.gravity = Gravity.CENTER
        fun sideButton(label: String, desc: String, onTap: () -> Unit): TextView {
            val t = TextView(context)
            t.text = label
            t.contentDescription = desc
            t.textSize = 18f
            t.gravity = Gravity.CENTER
            t.setTextColor(colors.fg)
            t.background = roundRect(colors.funcBg, cornerPx)
            t.setOnClickListener {
                buzz(t)
                onTap()
            }
            return t
        }
        val flip = sideButton("⇄", "反対側に寄せる") { applyOneHanded(mode.flipped()) }
        val off = sideButton("↔", "片手モードを解除") { applyOneHanded(OneHanded.OFF) }
        val lp1 = LinearLayout.LayoutParams(dp(30), dp(36))
        lp1.bottomMargin = dp(10)
        col.addView(flip, lp1)
        col.addView(off, LinearLayout.LayoutParams(dp(30), dp(36)))
        return col
    }

    /** 片手モードを保存して組み立て直す（ボタンのタップの最中に View を作り直さないよう、次のループで行う） */
    private fun applyOneHanded(mode: OneHanded) {
        KeyboardPrefs.setOneHanded(context, mode)
        post { reloadPrefs() }
    }

    private fun buildKeyArea() {
        keyArea.removeAllViews()
        letterKeys.clear()
        bindings.clear()
        spaceKey = null
        enterKey = null
        modeKey = null
        shiftKey = null
        shiftState = ShiftState.OFF
        lastShiftTapAt = null
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
        val face = KeyFace(context, colors, cornerPx)
        face.normalColor = roleColor(key.role)
        face.setTextColor(if (key.role == KeyRole.ACCENT) Color.WHITE else colors.fg)
        val text = if (a is KeyAction.Flick && key.label == null && flickDef != null) flickDef.center else LayoutLabels.staticLabel(key)
        face.text = text
        // 取り込み済みの Iconify アイコンがあれば、文字の代わりに描く（無ければ従来どおり label・代替文字）
        IconifyLogic.parse(key.icon)?.let { face.icon = IconStore.load(context, it) }
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
            isSpace(a) && prefs.spaceSwipe && key.longPress.isEmpty() -> attachSpaceKey(face)
            else -> attachJsonKey(face, key, l)
        }
        // TalkBack の「ダブルタップで有効にする」は click として届く（通常のタッチは onTouch が処理する）
        face.setOnClickListener { activateByAccessibility(key, a, flickDef, l) }
        val binding = KeyBinding(face, key, flickDef)
        face.contentDescription = describe(binding)
        row.addView(face, keyParams(key.width.toFloat()))
        bindings.add(binding)
    }

    private fun describe(b: KeyBinding): String =
        KeyDescriptions.forKey(b.key, b.flickDef, composing, kanaMode, enterOverride, shiftState)

    /** TalkBack などの操作でキーを有効にしたとき。タップと同じ動作を 1 回行う */
    private fun activateByAccessibility(key: LayoutKey, a: KeyAction, def: KanaKeyDef?, l: LayoutModel) {
        when {
            a is KeyAction.Flick && def != null -> {
                breakToggle()
                if (a.kanaKey != null) {
                    listener.onKana(def.key, if (prefs.toggleInput) "toggle" else "tap", null, null)
                } else if (def.center.isNotEmpty()) {
                    sendText(def.center, l)
                }
            }
            else -> activate(a, key, l)
        }
    }

    private fun isBackspace(a: KeyAction): Boolean = a is KeyAction.Act && a.name == "backspace"

    private fun isSpace(a: KeyAction): Boolean = a is KeyAction.Act && a.name == "space"

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

    /**
     * ⌫ の連打。押した瞬間に 1 文字消し、押し続けると [RepeatSchedule] に従って加速しながら繰り返し、離すと止まる。
     * 設定で有効なら、左へ約 40dp スワイプしたときに連打を止めて手前の単語をまとめて消す
     */
    private inner class BackspaceRepeater(private val onStep: () -> Unit) {
        private var interval = 0
        private var count = 0
        private val task = object : Runnable {
            override fun run() {
                onStep()
                count++
                interval = RepeatSchedule.nextInterval(interval, prefs.repeatMs, count)
                if (count < RepeatSchedule.MAX_REPEATS) uiHandler.postDelayed(this, interval.toLong())
            }
        }

        fun start() {
            stop()
            interval = prefs.repeatMs
            count = 0
            uiHandler.postDelayed(task, prefs.repeatDelayMs.toLong())
        }

        fun stop() {
            uiHandler.removeCallbacks(task)
        }
    }

    /** JSON レイアウトの ⌫ キー */
    private fun attachRepeatKey(face: KeyFace) {
        val rep = BackspaceRepeater { listener.onBackspace() }
        var downX = 0f
        var downY = 0f
        var swiped = false
        face.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    swiped = false
                    face.showPressed(true)
                    buzz(view, AudioManager.FX_KEYPRESS_DELETE)
                    breakToggle()
                    view.parent?.requestDisallowInterceptTouchEvent(true)
                    listener.onBackspace()
                    rep.start()
                }
                MotionEvent.ACTION_MOVE -> {
                    if (prefs.backspaceSwipe && !swiped && swipedLeft(ev.x - downX, ev.y - downY)) {
                        swiped = true
                        rep.stop()
                        buzz(view, AudioManager.FX_KEYPRESS_DELETE)
                        listener.onBackspaceWord()
                    }
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    face.showPressed(false)
                    rep.stop()
                }
            }
            true
        }
    }

    /** 単語削除とみなす左への移動か（約 40dp 以上左へ、かつ縦より横の動きが大きい） */
    private fun swipedLeft(dx: Float, dy: Float): Boolean =
        SwipeLogic.isLeftSwipe(dx, dy, context.dpF(BACKSPACE_SWIPE_DP))

    /**
     * 空白キーのスワイプ。タップは空白（または変換）。押したまま左右に 1 ステップ以上動かすとカーソル移動に入り、
     * 動かした距離に比例してカーソルを動かす。カーソル移動に入ったときは、離しても空白を入れない
     */
    private fun attachSpaceKey(face: KeyFace) = attachSpace(face) { face.showPressed(it) }

    private fun attachSpace(view: View, setPressed: (Boolean) -> Unit) {
        var tracker = SpaceSwipeTracker(
            context.dpF(prefs.spaceSwipeStepDp), prefs.spaceSwipeAccel, prefs.spaceSwipeAccelStrength,
        )
        var downX = 0f
        var downY = 0f
        view.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    tracker = SpaceSwipeTracker(
                        context.dpF(prefs.spaceSwipeStepDp), prefs.spaceSwipeAccel, prefs.spaceSwipeAccelStrength,
                    )
                    setPressed(true)
                    buzz(view, AudioManager.FX_KEYPRESS_SPACEBAR)
                    view.parent?.requestDisallowInterceptTouchEvent(true)
                }
                MotionEvent.ACTION_MOVE -> {
                    val wasActive = tracker.isActive
                    val delta = tracker.update(ev.x - downX)
                    if (tracker.isActive && !wasActive) {
                        breakToggle()
                        buzz(view, AudioManager.FX_KEYPRESS_SPACEBAR)
                    }
                    if (delta != 0) listener.onSpaceSwipe(delta)
                }
                MotionEvent.ACTION_UP -> {
                    setPressed(false)
                    // カーソル移動に入っていたら空白は入れない。ほとんど動いていないとき（単純なタップ）だけ空白
                    val moved = hypot((ev.x - downX).toDouble(), (ev.y - downY).toDouble())
                    if (!tracker.isActive && moved < dp(SPACE_TAP_SLOP_DP)) {
                        breakToggle()
                        listener.onSpace()
                    }
                }
                MotionEvent.ACTION_CANCEL -> setPressed(false)
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
        when {
            l.mode == LayoutMode.KANA -> listener.onLetter(text)
            NumberCompose.isNumberOrSymbol(l.mode) -> listener.onNumberText(text)
            else -> listener.onDirectText(text)
        }
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
            "history" -> toggleClipboardHistory()
            else -> Unit // Android では未対応の操作は無視する
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
        val from = current?.mode ?: if (kanaMode) LayoutMode.KANA else LayoutMode.ALPHA
        listener.onLayoutSwitch(from.json, next.mode.json) {
            if (wantKana != null) {
                listener.onSetKana(wantKana)
                kanaMode = wantKana
            }
            explicitId = id
            buildKeyArea()
        }
    }

    /** Shift: 1 回で次の 1 文字だけ大文字、0.5 秒以内の 2 回連続で Caps Lock、Caps Lock 中の 1 回で解除 */
    private fun tapShift() {
        val r = ShiftLogic.tap(shiftState, lastShiftTapAt, SystemClock.uptimeMillis())
        shiftState = r.state
        lastShiftTapAt = r.lastTapAt
        renderLabels()
    }

    private fun toggleCaps() {
        shiftState = ShiftLogic.toggleCaps(shiftState)
        lastShiftTapAt = null
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
        val h = context.dpToPx(KeyMetrics.horizontalMarginDp(prefs.keySpacingDp))
        val v = context.dpToPx(KeyMetrics.verticalMarginDp(prefs.keySpacingDp))
        p.setMargins(h, v, h, v)
        return p
    }

    /** キーの角丸（px。設定 `key_corner_dp`） */
    private val cornerPx: Float get() = context.dpToPx(prefs.keyCornerDp).toFloat()

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
        bs.contentDescription = "削除"
        attachRepeat(bs) { listener.onBackspace() }
        row.addView(bs, keyParams(1f))

        row = newRow(h)
        row.addView(funcKey("→", { listener.onArrow(false) }), keyParams(1f))
        addFlick(row, "ta")
        addFlick(row, "na")
        addFlick(row, "ha")
        val space = legacySpace(funcKey("空白", null), colors.funcBg)
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
        t.contentDescription = KeyDescriptions.flick(def)
        t.setOnClickListener { listener.onKana(def.key, if (prefs.toggleInput) "toggle" else "tap", null, null) }
        var downX = 0f
        var downY = 0f
        var keyRect = RectF()
        t.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    view.background = roundRect(colors.pressedBg, cornerPx)
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
                    view.background = roundRect(colors.keyBg, cornerPx)
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
                    view.background = roundRect(colors.keyBg, cornerPx)
                }
            }
            true
        }
        return t
    }

    private fun judge(dx: Float, dy: Float, v: View): FlickResult =
        FlickLogic.judge(
            dx,
            dy,
            thresholdPx(),
            v.width.toFloat(),
            v.height.toFloat(),
        )

    /** フリックの判定距離（px）。設定の距離を感度の倍率で割る */
    private fun thresholdPx(): Float =
        context.dpF(FlickLogic.effectiveThresholdDp(prefs.thresholdDp, prefs.sensitivity))

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
        val sh = funcKey("⇧", { tapShift() })
        shiftKey = sh
        row.addView(sh, keyParams(1.5f))
        for (k in "zxcvbnm") row.addView(letterKey(k.toString()), keyParams(1f))
        val bs = funcKey("⌫", null)
        bs.contentDescription = "削除"
        attachRepeat(bs) { listener.onBackspace() }
        row.addView(bs, keyParams(1.5f))

        // 下段: ← → は 12 キー画面と同じく、入力中・変換中・未入力でカーソルや注目文節を動かす
        row = newRow(h)
        val mode = funcKey(if (kanaMode) "あ" else "A", { listener.onToggleMode() }, { toggleLayout() })
        mode.contentDescription = "かなと英字の切り替え"
        modeKey = mode
        row.addView(mode, keyParams(1.1f))
        val num = funcKey("123", { listener.onOpenNumbers() })
        num.textSize = 14f
        num.contentDescription = "数字と記号"
        row.addView(num, keyParams(1f))
        row.addView(punctKey("ー", "-"), keyParams(0.9f))
        val space = legacySpace(baseKey("空白", colors.keyBg), colors.keyBg)
        spaceKey = space
        row.addView(space, keyParams(2.4f))
        row.addView(punctKey("、", ","), keyParams(0.9f))
        row.addView(punctKey("。", "."), keyParams(0.9f))
        val left = funcKey("←", { listener.onArrow(true) })
        left.contentDescription = "カーソルを左へ"
        row.addView(left, keyParams(0.9f))
        val right = funcKey("→", { listener.onArrow(false) })
        right.contentDescription = "カーソルを右へ"
        row.addView(right, keyParams(0.9f))
        val enter = funcKey("⏎", { listener.onEnter() })
        enterKey = enter
        row.addView(enter, keyParams(1.1f))
    }

    /** 組み込み（従来）の空白キー。スワイプが有効なら [attachSpaceKey] で、そうでなければタップだけ */
    private fun legacySpace(t: TextView, normal: Int): TextView {
        t.textSize = 16f
        if (prefs.spaceSwipe) {
            attachSpace(t) { t.background = roundRect(if (it) colors.pressedBg else normal, cornerPx) }
            // TalkBack の「ダブルタップで有効にする」
            t.setOnClickListener { listener.onSpace() }
        } else {
            attachTap(t, normal, null) { listener.onSpace() }
        }
        return t
    }

    private fun letterKey(c: String): TextView {
        val t = baseKey(c, colors.keyBg)
        t.tag = c
        attachTap(t, colors.keyBg, null) {
            val on = shiftState != ShiftState.OFF
            listener.onLetter(if (on) c.uppercase() else c)
            // 次の 1 文字だけの Shift はここで戻す。Caps Lock は固定のまま
            if (shiftState == ShiftState.ONCE) {
                shiftState = ShiftState.OFF
                renderLabels()
            }
        }
        letterKeys.add(t)
        return t
    }

    private fun punctKey(label: String, text: String): TextView {
        val t = baseKey(label, colors.keyBg)
        t.contentDescription = KeyDescriptions.symbolName(label)
        attachTap(t, colors.keyBg, null) { listener.onLetter(text) }
        return t
    }

    /** 組み込み（従来）の QWERTY の Shift 表示: ⇧（オフ・次の 1 文字）/ ⇪（Caps Lock）と、文字キーの大文字 */
    private fun renderLegacyShift() {
        val on = shiftState != ShiftState.OFF
        shiftKey?.let { k ->
            k.text = if (shiftState == ShiftState.CAPS) "⇪" else "⇧"
            k.background = roundRect(if (on) colors.pressedBg else colors.funcBg, cornerPx)
            k.contentDescription = when (shiftState) {
                ShiftState.OFF -> "シフト"
                ShiftState.ONCE -> "シフト（次の1文字が大文字）"
                ShiftState.CAPS -> "シフト（キャプスロック中）"
            }
        }
        for (t in letterKeys) {
            val c = t.tag as? String ?: continue
            t.text = if (on) c.uppercase() else c
            t.contentDescription = t.text
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
        bs.contentDescription = "削除"
        attachRepeat(bs) { listener.onBackspace() }
        row.addView(bs, keyParams(1.5f))

        row = newRow(h)
        val back = funcKey(if (kanaMode) "あいう" else "ABC", { setKanaMode(kanaMode) })
        back.textSize = 14f
        row.addView(back, keyParams(1.5f))
        val space = legacySpace(baseKey("空白", colors.keyBg), colors.keyBg)
        spaceKey = space
        row.addView(space, keyParams(4.5f))
        val enter = funcKey("⏎", { listener.onEnter() })
        enterKey = enter
        row.addView(enter, keyParams(1.5f))
    }

    private fun directKey(text: String): TextView {
        val t = baseKey(text, colors.keyBg)
        t.contentDescription = KeyDescriptions.symbolName(text)
        attachTap(t, colors.keyBg, null) { listener.onNumberText(text) }
        return t
    }

    // ---- 部品 ----

    private fun funcKey(label: String, onTap: (() -> Unit)?, onLong: (() -> Unit)? = null): TextView {
        val t = baseKey(label, colors.funcBg)
        t.textSize = 16f
        t.contentDescription = KeyDescriptions.legacyFace(label)
        if (onTap != null) attachTap(t, colors.funcBg, onLong, onTap)
        return t
    }

    private fun baseKey(label: String, color: Int): TextView {
        val t = TextView(context)
        t.text = label
        t.gravity = Gravity.CENTER
        t.textSize = 20f
        t.setTextColor(colors.fg)
        t.background = roundRect(color, cornerPx)
        t.contentDescription = KeyDescriptions.legacyFace(label)
        return t
    }

    private fun barButton(label: String, desc: String, onTap: () -> Unit): TextView {
        val t = TextView(context)
        t.text = label
        t.contentDescription = desc
        t.textSize = 14f
        t.setTextColor(colors.subFg)
        t.gravity = Gravity.CENTER
        t.setPadding(dp(8), 0, dp(8), 0)
        t.setOnClickListener { onTap() }
        return t
    }

    /** キーを押したときの触覚（強さは設定）とクリック音（設定でオン、種類は [fx]） */
    private fun buzz(v: View, fx: Int = AudioManager.FX_KEYPRESS_STANDARD) {
        KeyFeedback.haptic(v, prefs.haptic)
        if (prefs.keySound) KeyFeedback.click(audio, fx)
    }

    /** 離したときに [onTap]、[onLong] があれば長押し（500ms）でそちらを呼ぶ */
    private fun attachTap(v: View, normal: Int, onLong: (() -> Unit)?, onTap: () -> Unit) {
        var longFired = false
        val longRun = Runnable {
            longFired = true
            buzz(v)
            onLong?.invoke()
        }
        // TalkBack の「ダブルタップで有効にする」は click として届く（通常のタッチは onTouch が処理する）
        v.setOnClickListener { onTap() }
        v.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    longFired = false
                    view.background = roundRect(colors.pressedBg, cornerPx)
                    buzz(view)
                    if (onLong != null) uiHandler.postDelayed(longRun, 500)
                }
                MotionEvent.ACTION_UP -> {
                    uiHandler.removeCallbacks(longRun)
                    view.background = roundRect(normal, cornerPx)
                    if (!longFired) onTap()
                }
                MotionEvent.ACTION_CANCEL -> {
                    uiHandler.removeCallbacks(longRun)
                    view.background = roundRect(normal, cornerPx)
                }
            }
            true
        }
    }

    /** 押している間、加速しながら繰り返す（組み込みの ⌫ 用。左スワイプの単語削除も [attachRepeatKey] と同じ） */
    private fun attachRepeat(v: View, action: () -> Unit) {
        val rep = BackspaceRepeater(action)
        var downX = 0f
        var downY = 0f
        var swiped = false
        v.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = ev.x
                    downY = ev.y
                    swiped = false
                    view.background = roundRect(colors.pressedBg, cornerPx)
                    buzz(view, AudioManager.FX_KEYPRESS_DELETE)
                    view.parent?.requestDisallowInterceptTouchEvent(true)
                    action()
                    rep.start()
                }
                MotionEvent.ACTION_MOVE -> {
                    if (prefs.backspaceSwipe && !swiped && swipedLeft(ev.x - downX, ev.y - downY)) {
                        swiped = true
                        rep.stop()
                        buzz(view, AudioManager.FX_KEYPRESS_DELETE)
                        listener.onBackspaceWord()
                    }
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    view.background = roundRect(colors.funcBg, cornerPx)
                    rep.stop()
                }
            }
            true
        }
        // TalkBack の「ダブルタップで有効にする」
        v.setOnClickListener { action() }
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
        spaceKey?.contentDescription = KeyDescriptions.space(composing, kanaMode)
        enterKey?.text = if (composing) "確定" else enterOverride ?: enterIdleLabel
        enterKey?.contentDescription = KeyDescriptions.enter(composing, enterOverride)
        reconvertBtn.visibility = if (composing) View.GONE else View.VISIBLE
        renderLegacyShift()
        val alpha = current?.mode == LayoutMode.ALPHA
        for (b in bindings) {
            val a = b.key.action
            val face = b.face
            when {
                a is KeyAction.Act && a.name == "space" -> {
                    face.text = LayoutLabels.spaceLabel(composing, kanaMode, b.key.label)
                    // 入力中の「変換」は文字で出す
                    face.iconVisible = !(composing && kanaMode)
                }
                a is KeyAction.Act && a.name == "enter" -> {
                    face.text = LayoutLabels.enterLabel(composing, enterOverride, b.key.label)
                    // 入力中の「確定」・入力欄の指定（検索・送信など）は文字で出す
                    face.iconVisible = !composing && enterOverride == null
                }
                a is KeyAction.Act && (a.name == "shift" || a.name == "capslock") -> {
                    if (b.key.label == null) face.text = if (shiftState == ShiftState.CAPS) "⇪" else "⇧"
                    face.normalColor = if (shiftState != ShiftState.OFF) colors.pressedBg else roleColor(b.key.role)
                    face.showPressed(false)
                }
                a is KeyAction.Text && alpha ->
                    face.text = LayoutLabels.textKeyFace(b.key, a, shiftState != ShiftState.OFF)
            }
            face.contentDescription = describe(b)
        }
    }

    private fun roleColor(role: KeyRole): Int = when (role) {
        KeyRole.NORMAL -> colors.keyBg
        KeyRole.FUNCTION -> colors.funcBg
        KeyRole.ACCENT -> colors.accent
    }

    /** 表記 [surface] に、補正候補なら小さな「もしかして: 読み」の行を足した表示 */
    private fun candidateText(surface: String, hint: String?, prefix: String = ""): CharSequence {
        if (hint == null) return prefix + surface
        val sb = SpannableStringBuilder(prefix + surface)
        val start = sb.length + 1
        sb.append('\n').append(hint)
        sb.setSpan(RelativeSizeSpan(0.6f), start, sb.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        sb.setSpan(ForegroundColorSpan(colors.subFg), start, sb.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        return sb
    }

    private fun renderCandidates() {
        closeGrid()
        candBar.removeAllViews()
        val nextWord = bar.kind == CandidateKind.NEXT_WORD
        val pinned = bar.pinned
        var selView: View? = null
        for ((i, s) in candItems.withIndex()) {
            if (i == pinned) continue
            val hint = bar.hints.getOrNull(i)
            val t = TextView(context)
            t.text = candidateText(s, hint)
            t.textSize = 18f
            t.setTextColor(colors.fg)
            t.gravity = Gravity.CENTER
            t.setPadding(dp(14), 0, dp(14), 0)
            if (hint != null) t.maxLines = 2
            if (i == candSelected) {
                t.background = roundRect(colors.accentSoft, cornerPx)
                selView = t
            } else if (nextWord) {
                // 次語候補は入力中の候補と区別できるよう、薄い背景の札にする
                t.background = roundRect(AccentColor.withAlpha(colors.fg, 0x14), cornerPx)
            }
            t.contentDescription = KeyDescriptions.candidate(s, i, candItems.size, i == candSelected)
            t.setOnClickListener { listener.onCandidate(i) }
            val p = lp(WRAP, MATCH)
            if (nextWord) p.setMargins(dp(3), dp(5), dp(3), dp(5))
            candBar.addView(t, p)
        }
        candScroll.scrollTo(0, 0)
        val sv = selView
        if (sv != null && candSelected > 0) {
            // 「アニメーションをオフ」のときは滑らせずに飛ばす
            candScroll.post {
                val x = max(0, sv.left - dp(48))
                if (prefs.animations) candScroll.smoothScrollTo(x, 0) else candScroll.scrollTo(x, 0)
            }
        }
        renderPinned(pinned)
        // 次語候補は件数が少なく、選ぶとそのまま挿入されるので展開しない
        expandBtn.visibility = if (candItems.size > 1 && !nextWord) View.VISIBLE else View.GONE
        expandBtn.contentDescription = if (gridPanel != null) "全候補を閉じる" else "全候補を開く"
        renderToolbar()
    }

    // ---- ツールバー ----

    /**
     * 入力中でなく、候補も文節列も無いときは、候補バーの場所にツールバーを出す（候補が出たら候補バーに戻る）。
     * 項目は設定 `toolbar.items`。「貼り付け」は `candbar.paste_button` がオンで、クリップボードに文字があるときだけ
     */
    private fun renderToolbar() {
        if (!::actionScroll.isInitialized) return
        val show = ToolbarLogic.shouldShow(composing, candItems.isEmpty() && bar.pinned == null, false) && segTexts.isEmpty()
        val items = if (show) ToolbarLogic.visible(prefs.toolbarItems, prefs.pasteButton, clipboardHasText) else emptyList()
        val showBar = show && items.isNotEmpty()
        actionScroll.visibility = if (showBar) View.VISIBLE else View.GONE
        candScroll.visibility = if (showBar) View.GONE else View.VISIBLE
        if (!showBar || shownActions == items) return
        shownActions = items
        actionRow.removeAllViews()
        for (item in items) {
            val b = TextView(context)
            b.text = item.glyph
            b.contentDescription = item.title
            b.textSize = 15f
            b.setTextColor(colors.fg)
            b.gravity = Gravity.CENTER
            b.setPadding(dp(6), 0, dp(6), 0)
            b.background = roundRect(AccentColor.withAlpha(colors.fg, 0x14), cornerPx)
            b.setOnClickListener {
                buzz(b)
                runToolbarItem(item)
            }
            val p = LinearLayout.LayoutParams(dp(TOOLBAR_BUTTON_MIN_DP), MATCH, 1f)
            p.setMargins(dp(3), dp(5), dp(3), dp(5))
            actionRow.addView(b, p)
        }
        actionScroll.scrollTo(0, 0)
    }

    /** ツールバーの項目の操作。既存のキー操作（[KeyboardListener]）を呼ぶだけ */
    private fun runToolbarItem(item: ToolbarItem) {
        when (item) {
            ToolbarItem.DISMISS -> listener.onDismiss()
            ToolbarItem.PASTE -> listener.onPaste()
            ToolbarItem.CURSOR_LEFT -> listener.onArrow(true)
            ToolbarItem.CURSOR_RIGHT -> listener.onArrow(false)
            ToolbarItem.NEWLINE -> listener.onEnter()
            ToolbarItem.SPACE -> listener.onSpace()
            ToolbarItem.DELETE_WORD -> listener.onDeleteWord()
            ToolbarItem.EMOJI -> switchLayout(LayoutIds.EMOJI)
            ToolbarItem.NEXT_KEYBOARD -> listener.onNextInputMethod()
            ToolbarItem.HISTORY -> toggleClipboardHistory()
        }
    }

    // ---- クリップボード履歴のページ ----

    private fun toggleClipboardHistory() {
        if (historyPanel != null) closeClipboardHistory() else openClipboardHistory()
    }

    private fun openClipboardHistory() {
        if (historyPanel != null) return
        closeGrid()
        val store = ClipboardHistoryStore(java.io.File(context.applicationContext.filesDir, ClipboardHistoryStore.FILE_NAME))
        val panel = ClipboardHistoryPanel(
            context = context,
            colors = colors,
            store = store,
            cornerPx = cornerPx,
            isSecure = { listener.isSecureInput() },
            onInsert = { text ->
                closeClipboardHistory()
                listener.onClipboardInsert(text)
            },
            onClose = { closeClipboardHistory() },
        )
        // 全候補の一覧と同じく、いまのキーボードの高さ・幅に固定する
        val body = rootView
        val bodyW = body?.width ?: 0
        val lpPanel = FrameLayout.LayoutParams(if (bodyW > 0) bodyW else MATCH, max(body?.height ?: 0, dp(200)))
        if (bodyW > 0) lpPanel.leftMargin = body?.left ?: 0
        addView(panel, lpPanel)
        historyPanel = panel
    }

    private fun closeClipboardHistory() {
        val p = historyPanel ?: return
        removeView(p)
        historyPanel = null
    }

    /** 右端に固定する補正候補（最有力の 1 件）。無ければ隠す */
    private fun renderPinned(pinned: Int?) {
        if (pinned == null || pinned !in candItems.indices) {
            pinnedBtn.visibility = View.GONE
            pinnedBtn.setOnClickListener(null)
            return
        }
        val surface = candItems[pinned]
        val hint = bar.hints.getOrNull(pinned)
        val selected = pinned == candSelected
        pinnedBtn.text = candidateText(surface, bar.readings.getOrNull(pinned)?.takeIf { it.isNotEmpty() && it != surface }, "\uD83D\uDCA1 ")
        pinnedBtn.background = roundRect(if (selected) colors.accentSoft else AccentColor.withAlpha(colors.fg, 0x14), cornerPx)
        pinnedBtn.contentDescription = CandidateKinds.pinnedDescription(surface, hint)
        pinnedBtn.setOnClickListener { listener.onCandidate(pinned) }
        pinnedBtn.visibility = View.VISIBLE
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
            t.contentDescription = KeyDescriptions.segment(s, i, segTexts.size, i == focusIndex)
            if (i == focusIndex) {
                t.setTextColor(Color.WHITE)
                t.background = roundRect(colors.accent, cornerPx)
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
        // match_parent だと wrap_content の親が画面いっぱいに伸びるので、いまのキーボードの高さに固定する。
        // 片手モードのときは、キーボード本体と同じ幅・同じ位置に出す
        val body = rootView
        val bodyW = body?.width ?: 0
        val lpGrid = FrameLayout.LayoutParams(if (bodyW > 0) bodyW else MATCH, max(body?.height ?: 0, dp(200)))
        if (bodyW > 0) lpGrid.leftMargin = body?.left ?: 0
        addView(panel, lpGrid)
        gridPanel = panel
        expandBtn.contentDescription = "全候補を閉じる"
    }

    private fun closeGrid() {
        val g = gridPanel ?: return
        removeView(g)
        gridPanel = null
        expandBtn.contentDescription = "全候補を開く"
    }

    private fun dp(v: Int): Int = context.dpToPx(v.toFloat())

    /** 文節・範囲操作ツールバーの 1 ボタン */
    private class Tool(val label: String, val desc: String, val type: String, val content: String?)

    private companion object {
        /** ツールバーのボタンの最小幅（dp。項目が多いときは横にスクロールする） */
        const val TOOLBAR_BUTTON_MIN_DP = 52

        /** 長押しの判定時間の前にこれ以上動いたら長押しにしない（dp） */
        const val LONG_PRESS_MOVE_DP = 20

        /** これ以上動かして離したらタップにしない（dp） */
        const val TAP_MOVE_DP = 40

        /** 長押しのポップアップで、キーの下端よりこの割合（キーの高さに対して）以上下で離したら取り消し */
        const val LONG_PRESS_CANCEL_BELOW = 0.3f

        /** ⌫ の左スワイプで単語を消すとみなす移動量（dp） */
        const val BACKSPACE_SWIPE_DP = 40f

        /** 空白キーを「ほとんど動かさずに離した」とみなす移動量（dp。これ未満ならタップ） */
        const val SPACE_TAP_SLOP_DP = 20

        val TOOLS = listOf(
            Tool("◀文節", "注目する文節を左へ", "move_focus", "left"),
            Tool("文節▶", "注目する文節を右へ", "move_focus", "right"),
            Tool("区切り縮", "文節の区切りを縮める", "resize_segment", "left"),
            Tool("区切り伸", "文節の区切りを伸ばす", "resize_segment", "right"),
            Tool("選択◀", "選択範囲を左へ広げる", "extend_selection", "left"),
            Tool("選択▶", "選択範囲を右へ広げる", "extend_selection", "right"),
            Tool("カタカナ", "カタカナに変換", "to_katakana", null),
            Tool("ひらがな", "ひらがなに変換", "to_hiragana", null),
            Tool("半角", "半角に変換", "to_hankaku", null),
            Tool("取消", "変換を取り消す", "cancel", null),
        )
    }
}
