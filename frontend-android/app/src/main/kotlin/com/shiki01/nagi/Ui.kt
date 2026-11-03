package com.shiki01.nagi

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.ContextWrapper
import android.content.DialogInterface
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.os.Build
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.AbsListView
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.Switch
import android.widget.TextView
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

// 本体アプリ（設定まわりの画面）のデザインシステム。Compose / AppCompat / Material Components は使わない。
// 色は res/values/colors.xml と res/values-night/colors.xml（nagi_*）が正。ここでは Context から解決する。
// 画面は「薄いグレーの背景 + 角丸カード」で組み、項目はカードの中に縦に並べる。

// ---- 寸法・文字サイズ ----

internal object UiSize {
    const val CARD_RADIUS_DP = 16f
    const val BUTTON_RADIUS_DP = 12f
    const val FIELD_RADIUS_DP = 12f
    const val MIN_TOUCH_DP = 48

    /** 画面の左右の余白 */
    const val SCREEN_PAD_DP = 16

    /** カード内の左右の余白 */
    const val ROW_PAD_DP = 16

    const val TITLE_SP = 28f
    const val BAR_TITLE_SP = 17f
    const val SECTION_SP = 13f
    const val BODY_SP = 16f
    const val BANNER_SP = 14f
    const val NOTE_SP = 13f
    const val BUTTON_SP = 15f
}

internal const val LP_MATCH = ViewGroup.LayoutParams.MATCH_PARENT
internal const val LP_WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

// ---- 色（ライト / ダークは colors.xml の修飾子で切り替わる） ----

internal val Context.cBg: Int get() = getColor(R.color.nagi_bg)
internal val Context.cCard: Int get() = getColor(R.color.nagi_card)
internal val Context.cText: Int get() = getColor(R.color.nagi_text)
internal val Context.cTextSecondary: Int get() = getColor(R.color.nagi_text_secondary)
internal val Context.cDivider: Int get() = getColor(R.color.nagi_divider)
internal val Context.cAccent: Int get() = getColor(R.color.nagi_accent)
internal val Context.cOnAccent: Int get() = getColor(R.color.nagi_on_accent)
internal val Context.cAccentSoft: Int get() = getColor(R.color.nagi_accent_soft)
internal val Context.cDanger: Int get() = getColor(R.color.nagi_danger)
internal val Context.cDangerSoft: Int get() = getColor(R.color.nagi_danger_soft)
internal val Context.cSuccess: Int get() = getColor(R.color.nagi_success)
internal val Context.cSuccessSoft: Int get() = getColor(R.color.nagi_success_soft)
internal val Context.cWarning: Int get() = getColor(R.color.nagi_warning)
internal val Context.cWarningSoft: Int get() = getColor(R.color.nagi_warning_soft)
internal val Context.cNeutral: Int get() = getColor(R.color.nagi_neutral)
internal val Context.cRipple: Int get() = getColor(R.color.nagi_ripple)

internal val Context.isNight: Boolean
    get() = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES

/** 状態・メッセージの色味 */
internal enum class Tone { INFO, SUCCESS, WARNING, ERROR, NEUTRAL }

internal fun Context.toneColor(tone: Tone): Int = when (tone) {
    Tone.INFO -> cAccent
    Tone.SUCCESS -> cSuccess
    Tone.WARNING -> cWarning
    Tone.ERROR -> cDanger
    Tone.NEUTRAL -> cNeutral
}

internal fun Context.toneSoft(tone: Tone): Int = when (tone) {
    Tone.INFO -> cAccentSoft
    Tone.SUCCESS -> cSuccessSoft
    Tone.WARNING -> cWarningSoft
    Tone.ERROR -> cDangerSoft
    Tone.NEUTRAL -> cBg
}

internal fun Context.findActivity(): Activity? {
    var c: Context? = this
    while (c is ContextWrapper) {
        if (c is Activity) return c
        c = c.baseContext
    }
    return null
}

// ---- 描画の部品 ----

internal fun Context.dpF(v: Float): Float = v * resources.displayMetrics.density

internal fun Context.rounded(color: Int, radiusDp: Float): GradientDrawable {
    val d = GradientDrawable()
    d.shape = GradientDrawable.RECTANGLE
    d.setColor(color)
    d.cornerRadius = dpF(radiusDp)
    return d
}

internal fun Context.oval(color: Int): GradientDrawable {
    val d = GradientDrawable()
    d.shape = GradientDrawable.OVAL
    d.setColor(color)
    return d
}

/** 押下でリップルが出る背景。[base] は塗り（無ければ透明）、角丸は [radiusDp] でマスクする */
internal fun Context.rippleOver(base: Drawable?, radiusDp: Float, rippleColor: Int = cRipple): Drawable =
    RippleDrawable(ColorStateList.valueOf(rippleColor), base, rounded(Color.BLACK, radiusDp))

/** 上下だけ角丸にしたカード背景（一覧の行をグループとして見せる用）。リップル付き */
internal fun Context.groupedRowBackground(top: Boolean, bottom: Boolean, ripple: Boolean): Drawable {
    val r = dpF(UiSize.CARD_RADIUS_DP)
    val t = if (top) r else 0f
    val b = if (bottom) r else 0f
    val radii = floatArrayOf(t, t, t, t, b, b, b, b)
    val fill = GradientDrawable()
    fill.shape = GradientDrawable.RECTANGLE
    fill.setColor(cCard)
    fill.cornerRadii = radii
    if (!ripple) return fill
    val mask = GradientDrawable()
    mask.shape = GradientDrawable.RECTANGLE
    mask.setColor(Color.BLACK)
    mask.cornerRadii = radii
    return RippleDrawable(ColorStateList.valueOf(cRipple), fill, mask)
}

// ---- 文字 ----

internal fun Context.textOf(text: String, sizeSp: Float, color: Int, bold: Boolean = false): TextView {
    val t = TextView(this)
    t.text = text
    t.textSize = sizeSp
    t.setTextColor(color)
    if (bold) t.setTypeface(null, Typeface.BOLD)
    return t
}

/** 本文（16sp）。[secondary] なら補足（13sp・淡色）。ふつうはカードの外に置く説明文に使う */
internal fun Context.body(text: String, secondary: Boolean = false, color: Int? = null): TextView {
    val t = textOf(
        text,
        if (secondary) UiSize.NOTE_SP else UiSize.BODY_SP,
        color ?: if (secondary) cTextSecondary else cText,
    )
    t.setPadding(0, dp(2), 0, dp(2))
    return t
}

/** セクション見出し（13sp・太字・淡色）。カードの上に置く */
internal fun Context.sectionHeader(text: String): TextView {
    val t = textOf(text, UiSize.SECTION_SP, cTextSecondary, bold = true)
    t.letterSpacing = 0.04f
    t.setPadding(dp(UiSize.ROW_PAD_DP), dp(24), dp(UiSize.ROW_PAD_DP), dp(8))
    return t
}

/** カードの下に置く補足（13sp・淡色） */
internal fun Context.note(text: String, color: Int? = null): TextView {
    val t = textOf(text, UiSize.NOTE_SP, color ?: cTextSecondary)
    t.setPadding(dp(UiSize.ROW_PAD_DP), dp(8), dp(UiSize.ROW_PAD_DP), 0)
    return t
}

internal fun Context.spacer(heightDp: Int): View {
    val v = View(this)
    v.layoutParams = LinearLayout.LayoutParams(LP_MATCH, dp(heightDp))
    return v
}

// ---- ボタン ----

internal enum class ButtonKind { FILLED, TONAL, TEXT, DESTRUCTIVE }

/** 角丸のボタン（4 種）。押下でリップル、無効のときは薄く表示する。高さは 48dp 以上 */
internal class UiButton(context: Context, label: String, kind: ButtonKind, onClick: () -> Unit) : TextView(context) {
    init {
        text = label
        textSize = UiSize.BUTTON_SP
        setTypeface(null, Typeface.BOLD)
        gravity = Gravity.CENTER
        minHeight = context.dp(UiSize.MIN_TOUCH_DP)
        setPadding(context.dp(20), context.dp(10), context.dp(20), context.dp(10))
        isClickable = true
        isFocusable = true
        val fill: Int?
        val fg: Int
        var ripple = context.cRipple
        when (kind) {
            ButtonKind.FILLED -> {
                fill = context.cAccent
                fg = context.cOnAccent
                ripple = withAlpha(context.cOnAccent, 0x33)
            }
            ButtonKind.TONAL -> {
                fill = context.cAccentSoft
                fg = context.cAccent
            }
            ButtonKind.TEXT -> {
                fill = null
                fg = context.cAccent
            }
            ButtonKind.DESTRUCTIVE -> {
                fill = context.cDangerSoft
                fg = context.cDanger
            }
        }
        setTextColor(fg)
        val base = if (fill != null) context.rounded(fill, UiSize.BUTTON_RADIUS_DP) else null
        background = context.rippleOver(base, UiSize.BUTTON_RADIUS_DP, ripple)
        setOnClickListener { onClick() }
    }

    override fun setEnabled(enabled: Boolean) {
        super.setEnabled(enabled)
        alpha = if (enabled) 1f else 0.4f
    }
}

internal fun Context.uiButton(label: String, kind: ButtonKind = ButtonKind.FILLED, onClick: () -> Unit): UiButton =
    UiButton(this, label, kind, onClick)

internal fun Context.filledButton(label: String, onClick: () -> Unit): UiButton = uiButton(label, ButtonKind.FILLED, onClick)

internal fun Context.tonalButton(label: String, onClick: () -> Unit): UiButton = uiButton(label, ButtonKind.TONAL, onClick)

internal fun Context.textButton(label: String, onClick: () -> Unit): UiButton = uiButton(label, ButtonKind.TEXT, onClick)

internal fun Context.destructiveButton(label: String, onClick: () -> Unit): UiButton =
    uiButton(label, ButtonKind.DESTRUCTIVE, onClick)

/** ボタンを横に等幅で並べる（間は 8dp） */
internal fun Context.buttonRow(vararg buttons: View): LinearLayout {
    val r = LinearLayout(this)
    r.orientation = LinearLayout.HORIZONTAL
    buttons.forEachIndexed { i, b ->
        val lp = LinearLayout.LayoutParams(0, LP_WRAP, 1f)
        if (i > 0) lp.marginStart = dp(8)
        r.addView(b, lp)
    }
    return r
}

// ---- 描画で作る小さなビュー ----

/** 山形（›）または戻る矢印（‹）。線で描く */
internal class ChevronView(context: Context, private val left: Boolean, color: Int) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val path = Path()

    init {
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = context.dpF(2f)
        paint.strokeCap = Paint.Cap.ROUND
        paint.strokeJoin = Paint.Join.ROUND
        paint.color = color
    }

    override fun onDraw(canvas: Canvas) {
        val w = width.toFloat()
        val h = height.toFloat()
        val inset = paint.strokeWidth / 2f
        val x1 = if (left) w - inset else inset
        val x2 = if (left) inset else w - inset
        path.reset()
        path.moveTo(x1, inset)
        path.lineTo(x2, h / 2f)
        path.lineTo(x1, h - inset)
        canvas.drawPath(path, paint)
    }
}

/** 右下に浮かべる丸い追加ボタン（＋を描画する） */
internal class FabView(context: Context, description: String, onClick: () -> Unit) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)

    init {
        contentDescription = description
        background = context.oval(context.cAccent)
        foreground = RippleDrawable(
            ColorStateList.valueOf(withAlpha(context.cOnAccent, 0x33)),
            null,
            context.oval(Color.BLACK),
        )
        elevation = context.dpF(6f)
        isClickable = true
        isFocusable = true
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = context.dpF(2.5f)
        paint.strokeCap = Paint.Cap.ROUND
        paint.color = context.cOnAccent
        setOnClickListener { onClick() }
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val size = context.dp(56)
        setMeasuredDimension(size, size)
    }

    override fun onDraw(canvas: Canvas) {
        val cx = width / 2f
        val cy = height / 2f
        val arm = context.dpF(8f)
        canvas.drawLine(cx - arm, cy, cx + arm, cy, paint)
        canvas.drawLine(cx, cy - arm, cx, cy + arm, paint)
    }
}

internal fun Context.dot(color: Int, sizeDp: Int = 10): View {
    val v = View(this)
    v.background = oval(color)
    v.layoutParams = LinearLayout.LayoutParams(dp(sizeDp), dp(sizeDp))
    return v
}

// ---- 行とカード ----

/** 設定行。左にタイトル（と補足）、右に値・スイッチ・シェブロンなどを置く。高さは 56dp 以上 */
internal class UiRow(context: Context) : LinearLayout(context) {
    val titleView: TextView = context.textOf("", UiSize.BODY_SP, context.cText)
    val subtitleView: TextView = context.textOf("", UiSize.NOTE_SP, context.cTextSecondary)
    private val textCol = LinearLayout(context)
    private var dotView: View? = null
    private var valueView: TextView? = null
    private var chevron: View? = null

    init {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = context.dp(56)
        val p = context.dp(UiSize.ROW_PAD_DP)
        setPadding(p, context.dp(10), p, context.dp(10))
        textCol.orientation = VERTICAL
        textCol.addView(titleView)
        subtitleView.setPadding(0, context.dp(2), 0, 0)
        subtitleView.visibility = GONE
        textCol.addView(subtitleView)
        addView(textCol, LayoutParams(0, LP_WRAP, 1f))
    }

    fun setSubtitle(text: String?) {
        if (text.isNullOrEmpty()) {
            subtitleView.visibility = GONE
        } else {
            subtitleView.text = text
            subtitleView.visibility = VISIBLE
        }
    }

    /** 左端の色付きの丸（状態表示）。2 回目以降は色だけ変える */
    fun setDot(color: Int) {
        val d = dotView
        if (d != null) {
            d.background = context.oval(color)
            return
        }
        val nd = context.dot(color)
        (nd.layoutParams as LayoutParams).marginEnd = context.dp(12)
        addView(nd, 0)
        dotView = nd
    }

    /** 右側の値の文字。シェブロンがあればその手前に置く */
    fun setValue(text: String, color: Int? = null) {
        var v = valueView
        if (v == null) {
            v = context.textOf("", 15f, context.cTextSecondary)
            v.gravity = Gravity.END
            v.maxWidth = context.dp(180)
            val lp = LayoutParams(LP_WRAP, LP_WRAP)
            lp.marginStart = context.dp(12)
            val c = chevron
            addView(v, if (c != null) indexOfChild(c) else childCount, lp)
            valueView = v
        }
        v.text = text
        if (color != null) v.setTextColor(color)
    }

    fun setChevron() {
        if (chevron != null) return
        val c = ChevronView(context, left = false, color = context.cNeutral)
        val lp = LayoutParams(context.dp(9), context.dp(16))
        lp.marginStart = context.dp(8)
        addView(c, lp)
        chevron = c
    }

    fun setTrailing(v: View) {
        val lp = LayoutParams(LP_WRAP, LP_WRAP)
        lp.marginStart = context.dp(12)
        addView(v, lp)
    }

    /** 行全体をタップできるようにする（リップル付き） */
    fun onTap(action: () -> Unit) {
        isClickable = true
        isFocusable = true
        background = context.rippleOver(null, 0f)
        setOnClickListener { action() }
    }
}

/** 角丸のカード。[add] で項目を縦に並べ、項目の間に細い区切り線を入れる */
internal class UiCard(context: Context) : LinearLayout(context) {
    init {
        orientation = VERTICAL
        background = context.rounded(context.cCard, UiSize.CARD_RADIUS_DP)
        clipToOutline = true
        layoutParams = LayoutParams(LP_MATCH, LP_WRAP)
    }

    fun add(v: View): UiCard {
        if (childCount > 0) {
            val line = View(context)
            line.setBackgroundColor(context.cDivider)
            val lp = LayoutParams(LP_MATCH, Math.max(1, context.dp(1) / 2))
            lp.marginStart = context.dp(UiSize.ROW_PAD_DP)
            addView(line, lp)
        }
        addView(v)
        return this
    }

    fun clear() {
        removeAllViews()
    }
}

internal fun Context.card(vararg rows: View): UiCard {
    val c = UiCard(this)
    for (r in rows) c.add(r)
    return c
}

/** カードに自由な内容（文章・ボタンなど）を載せるための、余白付きの縦コンテナ */
internal fun Context.cardBody(vararg views: View): LinearLayout {
    val b = LinearLayout(this)
    b.orientation = LinearLayout.VERTICAL
    val p = dp(UiSize.ROW_PAD_DP)
    b.setPadding(p, p, p, p)
    for (v in views) b.addView(v)
    return b
}

/** 見出し + カード + 補足を 1 組として足す */
internal fun LinearLayout.addSection(title: String?, card: View, footnote: String? = null) {
    if (title != null) addView(context.sectionHeader(title))
    addView(card)
    if (footnote != null) addView(context.note(footnote))
}

/** タイトル + 補足 + 右に値（任意）。タップ不可の情報行 */
internal fun Context.infoRow(title: String, value: String?, subtitle: String? = null): UiRow {
    val r = UiRow(this)
    r.titleView.text = title
    r.setSubtitle(subtitle)
    if (value != null) r.setValue(value)
    return r
}

/** 色付きの丸 + タイトル + 値。状態の一覧に使う */
internal fun Context.statusRow(title: String, value: String, tone: Tone, subtitle: String? = null): UiRow {
    val r = UiRow(this)
    r.titleView.text = title
    r.setSubtitle(subtitle)
    r.setDot(toneColor(tone))
    r.setValue(value)
    return r
}

/** 画面遷移などの行（右にシェブロン。[value] があればその手前に値） */
internal fun Context.navRow(title: String, subtitle: String?, value: String? = null, onClick: () -> Unit): UiRow {
    val r = UiRow(this)
    r.titleView.text = title
    r.setSubtitle(subtitle)
    if (value != null) r.setValue(value)
    r.setChevron()
    r.onTap(onClick)
    return r
}

/** アクションの行（シェブロン無し）。[danger] なら赤字 */
internal fun Context.actionRow(title: String, subtitle: String?, danger: Boolean = false, onClick: () -> Unit): UiRow {
    val r = UiRow(this)
    r.titleView.text = title
    if (danger) r.titleView.setTextColor(cDanger)
    r.setSubtitle(subtitle)
    r.onTap(onClick)
    return r
}

/** アクセント色にそろえたスイッチ */
internal fun Context.styledSwitch(checked: Boolean): Switch {
    val sw = Switch(this)
    sw.isChecked = checked
    val states = arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf())
    sw.thumbTintList = ColorStateList(states, intArrayOf(cOnAccent, cNeutral))
    sw.trackTintList = ColorStateList(states, intArrayOf(cAccent, blendColors(cCard, cText, 0.2f)))
    return sw
}

/** ラベル + 説明 + スイッチ。行のどこをタップしても切り替わる。[onChange] は切り替えたときに呼ぶ */
internal fun Context.switchRow(label: String, description: String?, checked: Boolean, onChange: (Boolean) -> Unit): UiRow {
    val r = UiRow(this)
    r.titleView.text = label
    r.setSubtitle(description)
    val sw = styledSwitch(checked)
    // 初期値は listener を付ける前に入れてあるので、ここに来るのは切り替え操作のときだけ
    sw.setOnCheckedChangeListener { _, v -> onChange(v) }
    r.setTrailing(sw)
    r.onTap { sw.toggle() }
    return r
}

/** 選択行。現在の値を右に出し、タップでリスト選択ダイアログ。選択肢は (値, 表示)。[onSelect] は選び直したときだけ呼ぶ */
internal fun Context.selectRow(
    label: String,
    description: String?,
    options: List<Pair<String, String>>,
    selected: String?,
    onSelect: (String) -> Unit,
): UiRow {
    var current = options.indexOfFirst { it.first == selected }.coerceAtLeast(0)
    val r = UiRow(this)
    r.titleView.text = label
    r.setSubtitle(description)
    r.setValue(options.getOrNull(current)?.second ?: "", cAccent)
    r.setChevron()
    r.onTap {
        choiceDialog(label, options.map { it.second }, current) { i ->
            if (i != current && i in options.indices) {
                current = i
                r.setValue(options[i].second)
                onSelect(options[i].first)
            }
        }
    }
    return r
}

/** 入力欄（薄い背景の角丸）。カードの上でも使える */
internal fun Context.inputField(hint: String? = null): EditText {
    val e = EditText(this)
    e.background = rounded(cBg, UiSize.FIELD_RADIUS_DP)
    e.setTextColor(cText)
    e.setHintTextColor(withAlpha(cTextSecondary, 0xAA))
    e.textSize = UiSize.BODY_SP
    e.minHeight = dp(UiSize.MIN_TOUCH_DP)
    e.setPadding(dp(14), dp(10), dp(14), dp(10))
    if (hint != null) e.hint = hint
    return e
}

/** 入力欄と同じ見た目の選択欄。タップでリスト選択ダイアログ。選んだら表示を更新して [onSelect] を呼ぶ */
internal fun Context.selectField(title: String, labels: List<String>, selectedIndex: Int, onSelect: (Int) -> Unit): TextView {
    var current = selectedIndex.coerceIn(0, Math.max(0, labels.size - 1))
    val t = textOf(labels.getOrNull(current) ?: "", UiSize.BODY_SP, cText)
    t.gravity = Gravity.CENTER_VERTICAL
    t.minHeight = dp(UiSize.MIN_TOUCH_DP)
    t.setPadding(dp(14), dp(10), dp(14), dp(10))
    t.background = rippleOver(rounded(cBg, UiSize.FIELD_RADIUS_DP), UiSize.FIELD_RADIUS_DP)
    t.isClickable = true
    t.isFocusable = true
    t.setOnClickListener {
        choiceDialog(title, labels, current) { i ->
            current = i
            t.text = labels[i]
            onSelect(i)
        }
    }
    return t
}

/** タイトル + 説明 + 入力欄（縦並び） */
internal fun Context.textFieldRow(label: String, description: String?, field: EditText): LinearLayout {
    val col = LinearLayout(this)
    col.orientation = LinearLayout.VERTICAL
    val p = dp(UiSize.ROW_PAD_DP)
    col.setPadding(p, dp(12), p, dp(12))
    col.addView(textOf(label, UiSize.BODY_SP, cText))
    if (!description.isNullOrEmpty()) col.addView(textOf(description, UiSize.NOTE_SP, cTextSecondary))
    val lp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
    lp.topMargin = dp(8)
    col.addView(field, lp)
    return col
}

/** 数値ステッパー行。［−］入力欄［＋］を縦並びのタイトルの下に置く。[onStep] は −1 / +1 */
internal fun Context.stepperRow(
    label: String,
    description: String?,
    field: EditText,
    footnote: String?,
    onStep: (Int) -> Unit,
): LinearLayout {
    val col = LinearLayout(this)
    col.orientation = LinearLayout.VERTICAL
    val p = dp(UiSize.ROW_PAD_DP)
    col.setPadding(p, dp(12), p, dp(12))
    col.addView(textOf(label, UiSize.BODY_SP, cText))
    if (!description.isNullOrEmpty()) col.addView(textOf(description, UiSize.NOTE_SP, cTextSecondary))

    field.gravity = Gravity.CENTER
    val minus = tonalButton("−") { onStep(-1) }
    val plus = tonalButton("＋") { onStep(+1) }
    minus.contentDescription = "減らす"
    plus.contentDescription = "増やす"
    minus.minWidth = dp(56)
    plus.minWidth = dp(56)
    val line = LinearLayout(this)
    line.orientation = LinearLayout.HORIZONTAL
    line.gravity = Gravity.CENTER_VERTICAL
    line.addView(minus, LinearLayout.LayoutParams(LP_WRAP, LP_WRAP))
    val flp = LinearLayout.LayoutParams(0, LP_WRAP, 1f)
    flp.marginStart = dp(8)
    flp.marginEnd = dp(8)
    line.addView(field, flp)
    line.addView(plus, LinearLayout.LayoutParams(LP_WRAP, LP_WRAP))
    val llp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
    llp.topMargin = dp(8)
    col.addView(line, llp)
    if (!footnote.isNullOrEmpty()) {
        val n = textOf(footnote, UiSize.NOTE_SP, cTextSecondary)
        n.setPadding(0, dp(6), 0, 0)
        col.addView(n)
    }
    return col
}

/**
 * スライダー行。ラベルの右に現在値を出す。[show] は目盛り（0..[steps]）から表示文字列を作る。
 * [onCommit] は指を離したときに 1 回だけ呼ぶ
 */
internal fun Context.sliderRow(
    label: String,
    description: String?,
    steps: Int,
    progress: Int,
    show: (Int) -> String,
    onCommit: (Int) -> Unit,
): LinearLayout {
    val col = LinearLayout(this)
    col.orientation = LinearLayout.VERTICAL
    val p = dp(UiSize.ROW_PAD_DP)
    col.setPadding(p, dp(12), p, dp(8))

    val head = LinearLayout(this)
    head.orientation = LinearLayout.HORIZONTAL
    head.gravity = Gravity.CENTER_VERTICAL
    head.addView(textOf(label, UiSize.BODY_SP, cText), LinearLayout.LayoutParams(0, LP_WRAP, 1f))
    val valueView = textOf(show(progress), 15f, cAccent, bold = true)
    head.addView(valueView)
    col.addView(head)
    if (!description.isNullOrEmpty()) col.addView(textOf(description, UiSize.NOTE_SP, cTextSecondary))

    val bar = SeekBar(this)
    bar.max = steps
    bar.progress = progress.coerceIn(0, steps)
    bar.progressTintList = ColorStateList.valueOf(cAccent)
    bar.thumbTintList = ColorStateList.valueOf(cAccent)
    bar.progressBackgroundTintList = ColorStateList.valueOf(cDivider)
    bar.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
        override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
            valueView.text = show(progress)
        }

        override fun onStartTrackingTouch(seekBar: SeekBar?) {}

        override fun onStopTrackingTouch(seekBar: SeekBar?) {
            onCommit(bar.progress)
        }
    })
    val lp = LinearLayout.LayoutParams(LP_MATCH, dp(UiSize.MIN_TOUCH_DP))
    lp.topMargin = dp(4)
    col.addView(bar, lp)
    return col
}

// ---- バッジ・バナー・空状態 ----

/** 小さな丸薬型のラベル（「使用中」「取得済み」など） */
internal fun Context.badge(text: String, tone: Tone): TextView {
    val t = textOf(text, 12f, toneColor(tone), bold = true)
    t.setPadding(dp(10), dp(3), dp(10), dp(3))
    t.background = rounded(toneSoft(tone), 999f)
    return t
}

/** 情報バナー（情報・注意・エラー・成功）。[show] で文言と色を差し替える。任意で操作ボタンを付けられる */
internal class UiBanner(context: Context, tone: Tone = Tone.INFO) : LinearLayout(context) {
    val messageView: TextView = context.textOf("", UiSize.BANNER_SP, context.toneColor(tone))
    private var action: UiButton? = null

    init {
        orientation = VERTICAL
        setPadding(context.dp(16), context.dp(12), context.dp(16), context.dp(12))
        val lp = LayoutParams(LP_MATCH, LP_WRAP)
        lp.topMargin = context.dp(8)
        lp.bottomMargin = context.dp(8)
        layoutParams = lp
        addView(messageView)
        setTone(tone)
    }

    fun setTone(tone: Tone) {
        background = context.rounded(context.toneSoft(tone), UiSize.CARD_RADIUS_DP)
        messageView.setTextColor(context.toneColor(tone))
    }

    /** 文言と色を差し替えて表示する */
    fun show(text: String, tone: Tone) {
        messageView.text = text
        setTone(tone)
        visibility = VISIBLE
    }

    fun appendMessage(text: String) {
        messageView.append("\n" + text)
    }

    fun hide() {
        visibility = GONE
    }

    /** 文言の下に文字ボタンを付ける（もう一度呼ぶと差し替え） */
    fun setAction(label: String, onClick: () -> Unit) {
        action?.let { removeView(it) }
        val b = context.textButton(label, onClick)
        b.setPadding(0, context.dp(8), context.dp(16), context.dp(8))
        b.gravity = Gravity.START or Gravity.CENTER_VERTICAL
        addView(b, LayoutParams(LP_WRAP, LP_WRAP))
        action = b
    }
}

internal fun Context.banner(text: String, tone: Tone = Tone.INFO): UiBanner {
    val b = UiBanner(this, tone)
    b.messageView.text = text
    return b
}

/** 空状態の表示（一覧が空のときなど） */
internal fun Context.emptyState(title: String, message: String? = null): LinearLayout {
    val col = LinearLayout(this)
    col.orientation = LinearLayout.VERTICAL
    col.gravity = Gravity.CENTER_HORIZONTAL
    col.setPadding(dp(24), dp(32), dp(24), dp(32))
    val t = textOf(title, UiSize.BODY_SP, cText, bold = true)
    t.gravity = Gravity.CENTER
    col.addView(t)
    if (!message.isNullOrEmpty()) {
        val m = textOf(message, UiSize.NOTE_SP, cTextSecondary)
        m.gravity = Gravity.CENTER
        m.setPadding(0, dp(4), 0, 0)
        col.addView(m)
    }
    return col
}

// ---- 進捗 ----

/** 進捗のカード。[begin] / [report] / [end] で操作する。最初は非表示 */
internal class UiProgress(context: Context, hint: String?, onCancel: () -> Unit) : LinearLayout(context) {
    val textView: TextView = context.textOf("", UiSize.BODY_SP, context.cText)
    val bar: ProgressBar = ProgressBar(context, null, android.R.attr.progressBarStyleHorizontal)
    val cancelButton: UiButton = context.textButton("キャンセル", onCancel)

    init {
        orientation = VERTICAL
        background = context.rounded(context.cCard, UiSize.CARD_RADIUS_DP)
        val p = context.dp(UiSize.ROW_PAD_DP)
        setPadding(p, p, p, context.dp(8))
        val lp = LayoutParams(LP_MATCH, LP_WRAP)
        lp.topMargin = context.dp(8)
        lp.bottomMargin = context.dp(8)
        layoutParams = lp
        addView(textView)
        bar.max = 1000
        bar.progressTintList = ColorStateList.valueOf(context.cAccent)
        bar.indeterminateTintList = ColorStateList.valueOf(context.cAccent)
        bar.progressBackgroundTintList = ColorStateList.valueOf(context.cDivider)
        val blp = LayoutParams(LP_MATCH, context.dp(8))
        blp.topMargin = context.dp(12)
        addView(bar, blp)
        if (!hint.isNullOrEmpty()) {
            val h = context.textOf(hint, UiSize.NOTE_SP, context.cTextSecondary)
            h.setPadding(0, context.dp(8), 0, 0)
            addView(h)
        }
        addView(cancelButton, LayoutParams(LP_WRAP, LP_WRAP))
        visibility = GONE
    }

    /** 表示を始める（進捗が分からない状態から） */
    fun begin(text: String, cancellable: Boolean) {
        visibility = VISIBLE
        textView.text = text
        bar.isIndeterminate = true
        cancelButton.visibility = if (cancellable) VISIBLE else GONE
    }

    /** 文言と進捗を更新する。[total] が 0 以下なら不確定の進捗 */
    fun report(text: String, done: Long, total: Long) {
        val p = progressScaled(done, total, 1000)
        if (p == null) {
            bar.isIndeterminate = true
        } else {
            bar.isIndeterminate = false
            bar.progress = p
        }
        textView.text = text
    }

    fun end() {
        visibility = GONE
    }
}

// ---- ダイアログ ----

/** 確認ダイアログ。[destructive] なら実行ボタンを赤字にする */
internal fun Context.confirmDialog(
    title: String,
    message: String,
    positive: String,
    destructive: Boolean = false,
    onConfirm: () -> Unit,
) {
    val dialog = AlertDialog.Builder(this)
        .setTitle(title)
        .setMessage(message)
        .setPositiveButton(positive) { _, _ -> onConfirm() }
        .setNegativeButton("キャンセル", null)
        .create()
    dialog.show()
    if (destructive) dialog.getButton(DialogInterface.BUTTON_POSITIVE)?.setTextColor(cDanger)
}

/** 閉じるだけのメッセージ */
internal fun Context.messageDialog(title: String, message: String, close: String = "閉じる") {
    AlertDialog.Builder(this).setTitle(title).setMessage(message).setPositiveButton(close, null).show()
}

/** リスト選択ダイアログ（選ぶとすぐ閉じて [onSelect] に位置を渡す） */
internal fun Context.choiceDialog(title: String, labels: List<String>, selectedIndex: Int, onSelect: (Int) -> Unit) {
    AlertDialog.Builder(this)
        .setTitle(title)
        .setSingleChoiceItems(labels.toTypedArray(), selectedIndex) { d, which ->
            d.dismiss()
            onSelect(which)
        }
        .setNegativeButton("キャンセル", null)
        .show()
}

// ---- 画面の骨組み ----

internal class ScreenParts(val root: FrameLayout, val content: LinearLayout, val scroll: ScrollView)

/**
 * 画面の骨組み: 上に戻るボタン付きのバー、その下にスクロール領域（大きなタイトルから始まる）。
 * ステータスバー / ナビゲーションバー / キーボードの inset 分の余白を足す。[floating] は右下に浮かべるビュー（FAB 等）。
 * 呼び出し側は `setContentView(parts.root)` して、`parts.content` に項目を足す
 */
internal fun Context.screenParts(title: String, floating: View? = null): ScreenParts {
    val scroll = ScrollView(this)
    scroll.clipToPadding = false
    scroll.isVerticalScrollBarEnabled = false
    scroll.isFillViewport = false
    val content = LinearLayout(this)
    content.orientation = LinearLayout.VERTICAL
    val side = dp(UiSize.SCREEN_PAD_DP)
    content.setPadding(side, 0, side, dp(32) + if (floating != null) dp(88) else 0)
    scroll.addView(content)

    val titleView = textOf(title, UiSize.TITLE_SP, cText, bold = true)
    titleView.setPadding(dp(4), dp(4), dp(4), dp(12))
    content.addView(titleView)

    val frame = scaffold(title, floating, scroll, false) { left, right, bottom ->
        scroll.setPadding(left, 0, right, bottom)
    }
    val barTitle = frame.second
    scroll.setOnScrollChangeListener { _, _, y, _, _ ->
        val h = Math.max(1, titleView.height).toFloat()
        barTitle.alpha = ((y - h / 2f) / (h / 2f)).coerceIn(0f, 1f)
    }
    return ScreenParts(frame.first, content, scroll)
}

/** 旧来の呼び方。(画面のルート View, 項目を足す LinearLayout) を返す。`setContentView(first)` する */
internal fun Context.screen(title: String): Pair<View, LinearLayout> {
    val p = screenParts(title)
    return Pair(p.root, p.content)
}

/** [screenParts] の一覧版。[list] をスクロール領域にする。[pinned] はバーと一覧の間に固定で置くビュー（検索欄など）。大きなタイトルを一覧の先頭ヘッダーとして足す（アダプタを付ける前に呼ぶ）。ルートを返す */
internal fun Context.listScreen(title: String, list: ListView, floating: View? = null, pinned: View? = null): FrameLayout {
    val side = dp(UiSize.SCREEN_PAD_DP)
    list.clipToPadding = false
    list.divider = null
    list.dividerHeight = 0
    list.isVerticalScrollBarEnabled = false
    list.selector = ColorDrawable(Color.TRANSPARENT)
    val titleView = textOf(title, UiSize.TITLE_SP, cText, bold = true)
    titleView.setPadding(dp(4), dp(4), dp(4), dp(12))
    list.addHeaderView(titleView, null, false)
    val bottomExtra = if (floating != null) dp(88) else dp(32)
    val main: View = if (pinned != null) {
        val box = LinearLayout(this)
        box.orientation = LinearLayout.VERTICAL
        val plp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        plp.marginStart = side
        plp.marginEnd = side
        plp.bottomMargin = dp(8)
        box.addView(pinned, plp)
        box.addView(list, LinearLayout.LayoutParams(LP_MATCH, 0, 1f))
        box
    } else {
        list
    }
    val frame = scaffold(title, floating, main, false) { left, right, bottom ->
        list.setPadding(side + left, 0, side + right, bottom + bottomExtra)
    }
    val barTitle = frame.second
    list.setOnScrollListener(object : AbsListView.OnScrollListener {
        override fun onScrollStateChanged(view: AbsListView?, scrollState: Int) {}

        override fun onScroll(view: AbsListView?, firstVisibleItem: Int, visibleItemCount: Int, totalItemCount: Int) {
            val first = view?.getChildAt(0)
            barTitle.alpha = when {
                firstVisibleItem > 0 -> 1f
                first == null -> 0f
                else -> {
                    val h = Math.max(1, first.height).toFloat()
                    ((-first.top - h / 2f) / (h / 2f)).coerceIn(0f, 1f)
                }
            }
        }
    })
    return frame.first
}

/** バー + 本体 + (FAB) を重ねた骨組みと、バーのタイトル View を返す。inset が来るたびに [applyBody] を呼ぶ */
private fun Context.scaffold(
    title: String,
    floating: View?,
    main: View,
    barTitleAlwaysOn: Boolean,
    applyBody: (left: Int, right: Int, bottom: Int) -> Unit,
): Pair<FrameLayout, TextView> {
    val act = findActivity()
    if (act != null) {
        val w = act.window
        // 端から端まで描き、システムバー分の余白は自分で足す（targetSdk 35 では強制）
        WindowCompat.setDecorFitsSystemWindows(w, false)
        val ctl = WindowCompat.getInsetsController(w, w.decorView)
        ctl.isAppearanceLightStatusBars = !isNight
        ctl.isAppearanceLightNavigationBars = !isNight
        if (Build.VERSION.SDK_INT >= 29) w.isNavigationBarContrastEnforced = false
    }

    val root = FrameLayout(this)
    root.setBackgroundColor(cBg)
    val col = LinearLayout(this)
    col.orientation = LinearLayout.VERTICAL
    root.addView(col, FrameLayout.LayoutParams(LP_MATCH, LP_MATCH))

    val bar = LinearLayout(this)
    bar.orientation = LinearLayout.HORIZONTAL
    bar.gravity = Gravity.CENTER_VERTICAL
    bar.setPadding(dp(4), 0, dp(16), 0)
    val back = FrameLayout(this)
    back.isClickable = true
    back.isFocusable = true
    back.contentDescription = "戻る"
    back.background = rippleOver(null, 24f)
    back.addView(
        ChevronView(this, left = true, color = cText),
        FrameLayout.LayoutParams(dp(11), dp(20), Gravity.CENTER),
    )
    back.setOnClickListener { act?.finish() }
    bar.addView(back, LinearLayout.LayoutParams(dp(UiSize.MIN_TOUCH_DP), dp(UiSize.MIN_TOUCH_DP)))
    val barTitle = textOf(title, UiSize.BAR_TITLE_SP, cText, bold = true)
    barTitle.alpha = if (barTitleAlwaysOn) 1f else 0f
    barTitle.maxLines = 1
    bar.addView(barTitle, LinearLayout.LayoutParams(0, LP_WRAP, 1f))
    col.addView(bar, LinearLayout.LayoutParams(LP_MATCH, LP_WRAP))
    col.addView(main, LinearLayout.LayoutParams(LP_MATCH, 0, 1f))

    if (floating != null) {
        val lp = FrameLayout.LayoutParams(LP_WRAP, LP_WRAP, Gravity.BOTTOM or Gravity.END)
        lp.marginEnd = dp(20)
        lp.bottomMargin = dp(20)
        root.addView(floating, lp)
    }

    ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
        val bars: Insets = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
        val ime: Insets = insets.getInsets(WindowInsetsCompat.Type.ime())
        val bottom = Math.max(bars.bottom, ime.bottom)
        bar.setPadding(dp(4) + bars.left, bars.top, dp(16) + bars.right, 0)
        applyBody(bars.left, bars.right, bottom)
        if (floating != null) {
            val flp = floating.layoutParams as FrameLayout.LayoutParams
            flp.marginEnd = dp(20) + bars.right
            flp.bottomMargin = dp(20) + bottom
            floating.layoutParams = flp
        }
        insets
    }
    return Pair(root, barTitle)
}
