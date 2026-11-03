package com.shiki01.nagi

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import com.shiki01.nagi.core.NagiCore
import org.json.JSONObject

/** 本体アプリのホーム。有効化の案内・状態表示・入力テストと、各設定画面への入口 */
class SettingsActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var updateBanner: LinearLayout
    private lateinit var updateBannerText: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val (scroll, root) = screen("Nagi")

        // 新しい版のお知らせ（自動確認で見つかったときだけ表示）
        updateBanner = LinearLayout(this)
        updateBanner.orientation = LinearLayout.VERTICAL
        updateBanner.setBackgroundColor(0xFFFFF3CD.toInt())
        updateBanner.setPadding(dp(12), dp(8), dp(12), dp(8))
        updateBanner.visibility = View.GONE
        updateBannerText = body("")
        updateBanner.addView(updateBannerText)
        updateBanner.addView(button("アップデートを開く") { openUpdate() })
        root.addView(updateBanner)

        root.addView(
            body(
                "1. 「キーボードを有効にする」で Nagi をオンにします。\n" +
                    "2. 「入力方法を切り替える」で Nagi を選びます。\n" +
                    "3. 下の欄で入力を試せます。",
            ),
        )

        root.addView(
            button("キーボードを有効にする") {
                startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
            },
        )
        root.addView(
            button("入力方法を切り替える") {
                (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).showInputMethodPicker()
            },
        )

        val test = EditText(this)
        test.hint = "ここで入力を試す"
        test.minLines = 3
        root.addView(test)

        status = TextView(this)
        status.setPadding(0, dp(12), 0, dp(4))
        status.text = "状態を確認しています…"
        root.addView(status)

        root.addView(heading("設定"))
        root.addView(menuButton("入力設定", "キーボードのレイアウト・フリック・見た目") { openScreen(KeyboardInputSettingsActivity::class.java) })
        root.addView(menuButton("変換の設定", "変換・予測・学習・表記などコアの設定") { openScreen(SettingsFormActivity::class.java) })
        root.addView(menuButton("ユーザー辞書", "単語とスニペットの登録・インポート・エクスポート") { openScreen(UserDictActivity::class.java) })
        root.addView(menuButton("変換モデル（LLM）", "モデルのダウンロード・取り込み・選択") { openScreen(ModelActivity::class.java) })
        root.addView(menuButton("英語・カタカナ語データ", "任意の追加データの取得") { openScreen(OptionalDataActivity::class.java) })
        root.addView(menuButton("アップデート", "新しい版の確認とインストール") { openUpdate() })

        root.addView(heading("データ"))
        root.addView(
            button("学習データを削除") { confirmClearLearning() },
        )

        setContentView(scroll)
    }

    override fun onResume() {
        super.onResume()
        refreshStatus()
        checkUpdateIfDue()
    }

    private fun openScreen(cls: Class<*>) {
        startActivity(Intent(this, cls))
    }

    private fun openUpdate() {
        startActivity(Intent(this, UpdateActivity::class.java))
    }

    private fun menuButton(title: String, subtitle: String, onClick: () -> Unit): View {
        val col = LinearLayout(this)
        col.orientation = LinearLayout.VERTICAL
        val b = button(title, onClick)
        col.addView(b)
        col.addView(body(subtitle, secondary = true))
        return col
    }

    private fun checkUpdateIfDue() {
        if (!KeyboardPrefs.autoCheckUpdates(this)) return
        UpdateChecker.checkIfDue(this) { available ->
            if (isDestroyed) return@checkIfDue
            if (available != null) {
                updateBannerText.text = "新しい版 ${available.version} があります。"
                updateBanner.visibility = View.VISIBLE
            }
        }
    }

    private fun confirmClearLearning() {
        AlertDialog.Builder(this)
            .setTitle("学習データを削除")
            .setMessage("これまでの変換の学習をすべて消します。元に戻せません。ユーザー辞書は消えません。")
            .setPositiveButton("削除") { _, _ ->
                NagiRuntime.get(this).call("clear_learning", JSONObject().put("session_id", "android")) { r ->
                    toast(if (r != null) "学習データを削除しました" else "削除できませんでした（コアが応答しません）")
                    refreshStatus()
                }
            }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    private fun refreshStatus() {
        val sb = StringBuilder()
        sb.append("アプリ ").append(versionName()).append('\n')
        sb.append("辞書: ").append(if (DataInstaller.dictFile(this).exists()) "あり" else "なし（読みのまま変換）").append('\n')
        sb.append("変換モデル: ").append(if (ModelStore.hasModel(this)) "あり" else "なし（辞書のみで動作）").append('\n')
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        val enabled = imm.enabledInputMethodList.any { it.packageName == packageName }
        sb.append("キーボード: ").append(if (enabled) "有効" else "未有効").append('\n')
        status.text = sb.toString()
        NagiRuntime.get(this).call("get_status") { r ->
            if (r == null) {
                status.append("コア: 起動できませんでした")
                val why = NagiRuntime.get(this).startError ?: NagiCore.loadError
                if (why != null) status.append("\n理由: $why")
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
}
