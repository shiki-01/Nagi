package com.shiki01.nagi

import android.app.Activity
import android.content.Intent
import android.graphics.Typeface
import android.os.Bundle
import android.provider.Settings
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.shiki01.nagi.core.NagiCore
import org.json.JSONObject

/** 本体アプリ。有効化の案内・状態表示・入力テスト・学習データの削除を行う */
class SettingsActivity : Activity() {
    private lateinit var status: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pad = (16 * resources.displayMetrics.density).toInt()
        val root = LinearLayout(this)
        root.orientation = LinearLayout.VERTICAL
        root.setPadding(pad, pad * 2, pad, pad)

        val title = TextView(this)
        title.text = "Nagi"
        title.textSize = 28f
        title.setTypeface(null, Typeface.BOLD)
        root.addView(title)

        val guide = TextView(this)
        guide.text = "1. 「キーボードを有効にする」で Nagi をオンにします。\n" +
            "2. 「入力方法を切り替える」で Nagi を選びます。\n" +
            "3. 下の欄で入力を試せます。"
        guide.setPadding(0, pad, 0, pad)
        root.addView(guide)

        root.addView(button("キーボードを有効にする") {
            startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
        })
        root.addView(button("入力方法を切り替える") {
            (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).showInputMethodPicker()
        })

        val test = EditText(this)
        test.hint = "ここで入力を試す"
        test.minLines = 3
        root.addView(test)

        root.addView(button("学習データを削除") {
            NagiRuntime.get(this).call("clear_learning", JSONObject().put("session_id", "android")) {
                status.append("\n学習データを削除しました")
            }
        })

        status = TextView(this)
        status.setPadding(0, pad, 0, 0)
        status.text = "状態を確認しています…"
        root.addView(status)

        val scroll = ScrollView(this)
        scroll.addView(root)
        setContentView(scroll)
    }

    override fun onResume() {
        super.onResume()
        refreshStatus()
    }

    private fun refreshStatus() {
        val sb = StringBuilder()
        sb.append("アプリ ").append(versionName()).append('\n')
        sb.append("辞書: ").append(if (DataInstaller.dictFile(this).exists()) "あり" else "なし（読みのまま変換）").append('\n')
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        val enabled = imm.enabledInputMethodList.any { it.packageName == packageName }
        sb.append("キーボード: ").append(if (enabled) "有効" else "未有効").append('\n')
        status.text = sb.toString()
        NagiRuntime.get(this).call("get_status") { r ->
            if (r == null) {
                status.append("コア: 起動できませんでした")
            } else {
                status.append(
                    "コア ${NagiCore.nativeVersion()} / プロファイル ${r.optString("profile")} / " +
                        "学習 ${r.optInt("learned_entries")} 件",
                )
            }
        }
    }

    private fun versionName(): String = try {
        packageManager.getPackageInfo(packageName, 0).versionName ?: "?"
    } catch (e: Exception) {
        "?"
    }

    private fun button(label: String, onClick: () -> Unit): Button {
        val b = Button(this)
        b.text = label
        b.setOnClickListener { onClick() }
        return b
    }
}
