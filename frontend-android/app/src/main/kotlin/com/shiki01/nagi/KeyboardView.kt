package com.shiki01.nagi

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView

/** キーボードの操作を IME サービスに伝える */
internal interface KeyboardListener {
    fun onLetter(text: String)

    fun onSpace()

    fun onEnter()

    fun onBackspace()

    fun onToggleMode()

    fun onCandidate(index: Int)
}

/**
 * QWERTY ローマ字キーボード + 候補バー（プログラムで組み立てる View）。
 * ロジックは持たず、タップを [KeyboardListener] に渡すだけにする。
 */
internal class KeyboardView(context: Context, private val listener: KeyboardListener) : LinearLayout(context) {
    private val dark = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
        Configuration.UI_MODE_NIGHT_YES
    private val bg = if (dark) Color.rgb(32, 33, 36) else Color.rgb(232, 234, 237)
    private val keyBg = if (dark) Color.rgb(66, 68, 72) else Color.WHITE
    private val funcBg = if (dark) Color.rgb(48, 50, 54) else Color.rgb(207, 211, 216)
    private val pressedBg = if (dark) Color.rgb(100, 104, 110) else Color.rgb(176, 190, 205)
    private val fg = if (dark) Color.WHITE else Color.BLACK
    private val candBg = if (dark) Color.rgb(44, 46, 50) else Color.rgb(246, 247, 248)

    private val candBar = LinearLayout(context)
    private val candScroll = HorizontalScrollView(context)
    private val repeatHandler = Handler(Looper.getMainLooper())

    private var shift = false
    private var kanaMode = true
    private val letterKeys = ArrayList<TextView>()
    private lateinit var shiftKey: TextView
    private lateinit var modeKey: TextView
    private lateinit var enterKey: TextView
    private lateinit var spaceKey: TextView

    init {
        orientation = VERTICAL
        setBackgroundColor(bg)
        val pad = dp(4)
        setPadding(pad, 0, pad, pad)

        candScroll.isHorizontalScrollBarEnabled = false
        candScroll.setBackgroundColor(candBg)
        candBar.orientation = HORIZONTAL
        candBar.gravity = Gravity.CENTER_VERTICAL
        candScroll.addView(candBar, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        addView(candScroll, LayoutParams(LayoutParams.MATCH_PARENT, dp(44)))

        addRow("qwertyuiop".map { it.toString() })
        addRow("asdfghjkl".map { it.toString() }, sidePad = true)
        addThirdRow()
        addBottomRow()
    }

    private fun addRow(keys: List<String>, sidePad: Boolean = false) {
        val row = newRow()
        if (sidePad) row.addView(spacer(0.5f))
        for (k in keys) row.addView(letterKey(k), keyParams(1f))
        if (sidePad) row.addView(spacer(0.5f))
    }

    private fun addThirdRow() {
        val row = newRow()
        shiftKey = funcKey("⇧") { toggleShift() }
        row.addView(shiftKey, keyParams(1.5f))
        for (c in "zxcvbnm") row.addView(letterKey(c.toString()), keyParams(1f))
        val bs = funcKey("⌫", null)
        attachRepeat(bs) { listener.onBackspace() }
        row.addView(bs, keyParams(1.5f))
    }

    private fun addBottomRow() {
        val row = newRow()
        modeKey = funcKey("あ") { listener.onToggleMode() }
        row.addView(modeKey, keyParams(1.3f))
        row.addView(punctKey("ー", "-"), keyParams(1f))
        row.addView(punctKey("、", ","), keyParams(1f))
        spaceKey = baseKey("空白", keyBg)
        spaceKey.textSize = 16f
        attachTap(spaceKey) { listener.onSpace() }
        row.addView(spaceKey, keyParams(3.5f))
        row.addView(punctKey("。", "."), keyParams(1f))
        enterKey = funcKey("⏎") { listener.onEnter() }
        row.addView(enterKey, keyParams(1.3f))
    }

    private fun newRow(): LinearLayout {
        val row = LinearLayout(context)
        row.orientation = HORIZONTAL
        addView(row, LayoutParams(LayoutParams.MATCH_PARENT, dp(50)))
        return row
    }

    private fun spacer(weight: Float): View {
        val v = View(context)
        v.layoutParams = LayoutParams(0, LayoutParams.MATCH_PARENT, weight)
        return v
    }

    private fun keyParams(weight: Float): LayoutParams {
        val p = LayoutParams(0, LayoutParams.MATCH_PARENT, weight)
        p.setMargins(dp(2), dp(3), dp(2), dp(3))
        return p
    }

    private fun letterKey(c: String): TextView {
        val t = baseKey(c, keyBg)
        t.tag = c
        attachTap(t) {
            val s = if (shift) c.uppercase() else c
            listener.onLetter(s)
            if (shift) toggleShift()
        }
        letterKeys.add(t)
        return t
    }

    private fun punctKey(label: String, text: String): TextView {
        val t = baseKey(label, keyBg)
        attachTap(t) { listener.onLetter(text) }
        return t
    }

    private fun funcKey(label: String, onTap: (() -> Unit)?): TextView {
        val t = baseKey(label, funcBg)
        t.textSize = 16f
        if (onTap != null) attachTap(t, onTap)
        return t
    }

    private fun baseKey(label: String, color: Int): TextView {
        val t = TextView(context)
        t.text = label
        t.gravity = Gravity.CENTER
        t.textSize = 20f
        t.setTextColor(fg)
        t.setBackgroundDrawable(roundRect(color))
        t.tag = color
        return t
    }

    private fun roundRect(color: Int): GradientDrawable {
        val d = GradientDrawable()
        d.setColor(color)
        d.cornerRadius = dp(6).toFloat()
        return d
    }

    private fun attachTap(v: TextView, onTap: () -> Unit) {
        val normal = (v.tag as? Int) ?: keyBg
        v.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> view.background = roundRect(pressedBg)
                MotionEvent.ACTION_UP -> {
                    view.background = roundRect(normal)
                    onTap()
                }
                MotionEvent.ACTION_CANCEL -> view.background = roundRect(normal)
            }
            true
        }
    }

    /** 押している間、一定間隔で繰り返す（⌫ 用） */
    private fun attachRepeat(v: TextView, action: () -> Unit) {
        val normal = funcBg
        val repeat = object : Runnable {
            override fun run() {
                action()
                repeatHandler.postDelayed(this, 60)
            }
        }
        v.setOnTouchListener { view, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    view.background = roundRect(pressedBg)
                    action()
                    repeatHandler.postDelayed(repeat, 400)
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    view.background = roundRect(normal)
                    repeatHandler.removeCallbacks(repeat)
                }
            }
            true
        }
    }

    private fun toggleShift() {
        shift = !shift
        shiftKey.text = if (shift) "⇪" else "⇧"
        for (t in letterKeys) {
            val c = t.tag as? String ?: continue
            t.text = if (shift) c.uppercase() else c
        }
    }

    /** かな入力か英数直接入力かの表示を切り替える */
    fun setKanaMode(kana: Boolean) {
        kanaMode = kana
        modeKey.text = if (kana) "あ" else "A"
    }

    /** 入力中（変換できる状態）かどうかで Space と Enter の表示を変える */
    fun setComposing(composing: Boolean) {
        spaceKey.text = if (composing && kanaMode) "変換" else "空白"
        enterKey.text = if (composing) "確定" else "⏎"
    }

    /** 候補バーを更新する。空・非表示なら空にする（高さは保つ） */
    fun setCandidates(items: List<String>, selected: Int) {
        candBar.removeAllViews()
        items.forEachIndexed { i, s ->
            val t = TextView(context)
            t.text = s
            t.textSize = 18f
            t.setTextColor(fg)
            t.gravity = Gravity.CENTER
            t.setPadding(dp(14), 0, dp(14), 0)
            if (i == selected) t.setBackgroundDrawable(roundRect(pressedBg))
            t.setOnClickListener { listener.onCandidate(i) }
            candBar.addView(t, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        }
        candScroll.scrollTo(0, 0)
    }

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()
}
