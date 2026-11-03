package com.shiki01.nagi.tabs

import android.content.Context
import android.os.Bundle
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import androidx.fragment.app.Fragment
import com.shiki01.nagi.*

/**
 * 入力テストタブ（iOS の `KeyboardTestView` と同じ）: 使い方、複数行の入力欄、1 行の入力欄。
 * 各欄に文字数とクリアのボタンを付ける。診断ログ・入力欄の種類のテストは開発者の画面にある。
 */
class InputTestFragment : Fragment() {
    override fun onCreateView(inflater: LayoutInflater, container: ViewGroup?, savedInstanceState: Bundle?): View {
        val ctx = requireContext()
        val parts = ctx.pageParts("入力テスト")
        val root = parts.content

        root.addSection(
            "使い方",
            ctx.card(
                ctx.cardBody(
                    ctx.body(
                        "入力欄をタップしてキーボードを出し、キーボードの切り替え（ナビゲーションバーのキーボードのアイコン）で" +
                            " Nagi に切り替えてください。切り替えの一覧に Nagi が無いときは、ホームの「設定アプリを開く」から" +
                            " Nagi をオンにします。",
                        secondary = true,
                    ),
                ),
            ),
        )

        val multiline = ctx.inputField(null)
        multiline.minLines = 6
        multiline.gravity = Gravity.TOP or Gravity.START
        multiline.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        root.addSection("複数行", ctx.card(testBody(ctx, multiline)))

        val single = ctx.inputField("ここに入力")
        single.inputType = InputType.TYPE_CLASS_TEXT
        single.maxLines = 1
        root.addSection("1 行", ctx.card(testBody(ctx, single)))

        return parts.root
    }

    /** 入力欄 + 「N 文字」+ クリアのボタン */
    private fun testBody(ctx: Context, field: EditText): LinearLayout {
        val count = ctx.textOf(InputTestLogic.countText(field.text), UiSize.NOTE_SP, ctx.cTextSecondary)
        field.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, c: Int, after: Int) {}

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, c: Int) {}

            override fun afterTextChanged(s: Editable?) {
                count.text = InputTestLogic.countText(s ?: "")
            }
        })
        val clear = ctx.textButton("クリア") { field.setText("") }
        val bar = LinearLayout(ctx)
        bar.orientation = LinearLayout.HORIZONTAL
        bar.gravity = Gravity.CENTER_VERTICAL
        bar.addView(count, LinearLayout.LayoutParams(0, LP_WRAP, 1f))
        bar.addView(clear, LinearLayout.LayoutParams(LP_WRAP, LP_WRAP))

        val body = ctx.cardBody(field)
        val lp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        lp.topMargin = ctx.dp(8)
        body.addView(bar, lp)
        return body
    }
}
