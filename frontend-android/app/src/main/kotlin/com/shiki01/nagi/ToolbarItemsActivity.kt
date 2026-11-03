package com.shiki01.nagi

import android.app.Activity
import android.os.Bundle
import android.view.Gravity
import android.widget.LinearLayout
import android.widget.TextView

/**
 * ツールバーの項目の設定（`toolbar.items`）。項目ごとに有効/無効を切り替え、有効な項目は「上へ」「下へ」で並べ替える。
 * 変更はすぐ保存する（キーボードを次に開いたときに反映される）。純粋な部分は [ToolbarLogic]。
 */
class ToolbarItemsActivity : Activity() {
    private lateinit var box: LinearLayout

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("ツールバーの項目")
        box = LinearLayout(this)
        box.orientation = LinearLayout.VERTICAL
        parts.content.addView(box)
        setContentView(parts.root)
    }

    override fun onResume() {
        super.onResume()
        render()
    }

    private fun render() {
        box.removeAllViews()
        val enabled = KeyboardPrefs.toolbarItems(this)
        val card = UiCard(this)
        for (item in ToolbarLogic.displayOrder(enabled)) card.add(itemRow(item, enabled))
        box.addSection(
            "表示する項目（上から順に並びます）",
            card,
            "入力中でなく、候補が無いときに候補バーの場所に出す操作です。最後の 1 つは外せません。" +
                "「貼り付け」は、クリップボードに文字があり、設定の「ツールバーに「貼り付け」を出す」がオンのときだけ出ます。" +
                "「履歴」はクリップボード履歴を開きます（履歴は設定でオンにしたときだけ残ります）。",
        )
        box.addView(spacer(12))
        box.addView(
            tonalButton("既定に戻す") {
                KeyboardPrefs.reset(this, ResetKeys.TOOLBAR_ITEMS)
                render()
            },
        )
    }

    private fun itemRow(item: ToolbarItem, enabled: List<ToolbarItem>): UiRow {
        val row = UiRow(this)
        val on = item in enabled
        row.titleView.text = item.title
        row.setSubtitle(if (on) "${enabled.indexOf(item) + 1} 番目" else null)

        val trailing = LinearLayout(this)
        trailing.orientation = LinearLayout.HORIZONTAL
        trailing.gravity = Gravity.CENTER_VERTICAL
        if (on) {
            val index = enabled.indexOf(item)
            trailing.addView(moveButton("▲", "${item.title}を上へ", index > 0) { save(ToolbarLogic.move(enabled, item, -1)) })
            trailing.addView(
                moveButton("▼", "${item.title}を下へ", index < enabled.size - 1) { save(ToolbarLogic.move(enabled, item, 1)) },
            )
        }
        val sw = styledSwitch(on)
        sw.contentDescription = item.title
        sw.setOnCheckedChangeListener { _, _ ->
            val next = ToolbarLogic.toggle(enabled, item)
            if (next == enabled) toast("最後の 1 つは外せません")
            save(next)
        }
        val swLp = LinearLayout.LayoutParams(LP_WRAP, LP_WRAP)
        swLp.marginStart = dp(8)
        trailing.addView(sw, swLp)
        row.setTrailing(trailing)
        row.onTap { sw.toggle() }
        return row
    }

    private fun moveButton(label: String, description: String, active: Boolean, onClick: () -> Unit): TextView {
        val t = textOf(label, 16f, if (active) cAccent else cNeutral)
        t.gravity = Gravity.CENTER
        t.minWidth = dp(UiSize.MIN_TOUCH_DP)
        t.minHeight = dp(UiSize.MIN_TOUCH_DP)
        t.contentDescription = description
        if (active) {
            t.isClickable = true
            t.setOnClickListener { onClick() }
        }
        return t
    }

    private fun save(items: List<ToolbarItem>) {
        KeyboardPrefs.setToolbarItems(this, items)
        // スイッチの切り替えの処理が終わってから描き直す
        box.post { render() }
    }
}
