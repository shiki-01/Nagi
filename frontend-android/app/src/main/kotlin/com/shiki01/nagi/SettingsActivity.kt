package com.shiki01.nagi

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import com.shiki01.nagi.core.NagiCore
import org.json.JSONObject

/** 本体アプリのホーム。有効化の案内・状態表示・入力テストと、各設定画面への入口 */
class SettingsActivity : Activity() {
    private lateinit var updateBanner: UiBanner
    private lateinit var coreRow: UiRow
    private lateinit var dictRow: UiRow
    private lateinit var modelRow: UiRow
    private lateinit var keyboardRow: UiRow

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("Nagi")
        val root = parts.content

        // 新しい版のお知らせ（自動確認で見つかったときだけ表示）
        updateBanner = banner("", Tone.INFO)
        updateBanner.setAction("アップデートを開く") { openUpdate() }
        updateBanner.visibility = View.GONE
        root.addView(updateBanner)

        // 状態
        coreRow = statusRow("コア", "確認中", Tone.NEUTRAL)
        dictRow = statusRow("辞書", "確認中", Tone.NEUTRAL)
        modelRow = statusRow("変換モデル", "確認中", Tone.NEUTRAL)
        keyboardRow = statusRow("キーボード", "確認中", Tone.NEUTRAL)
        root.addSection("状態", card(coreRow, dictRow, modelRow, keyboardRow))

        // はじめる
        val test: EditText = inputField("ここで入力を試す")
        test.minLines = 3
        test.gravity = android.view.Gravity.TOP
        root.addSection(
            "はじめる",
            card(
                actionRow("キーボードを有効にする", "設定で Nagi をオンにします") {
                    startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
                },
                actionRow("入力方法を切り替える", "入力中のキーボードを Nagi にします") {
                    (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).showInputMethodPicker()
                },
                cardBody(test),
            ),
        )

        // 設定
        root.addSection(
            "設定",
            card(
                navRow("入力設定", "レイアウト・フリック・見た目") { openScreen(KeyboardInputSettingsActivity::class.java) },
                navRow("変換の設定", "変換・予測・学習・表記") { openScreen(SettingsFormActivity::class.java) },
                navRow("ユーザー辞書", "単語とスニペットの登録") { openScreen(UserDictActivity::class.java) },
                navRow("変換モデル", "ダウンロード・取り込み・選択") { openScreen(ModelActivity::class.java) },
                navRow("英語・カタカナ語データ", "任意の追加データ") { openScreen(OptionalDataActivity::class.java) },
            ),
        )

        // アップデート
        root.addSection(
            "アップデート",
            card(navRow("アップデート", "新しい版の確認とインストール", versionName()) { openUpdate() }),
        )

        // データ管理
        root.addSection(
            "データ管理",
            card(actionRow("学習データを削除", "ユーザー辞書は消えません", danger = true) { confirmClearLearning() }),
        )

        setContentView(parts.root)
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

    private fun checkUpdateIfDue() {
        if (!KeyboardPrefs.autoCheckUpdates(this)) return
        UpdateChecker.checkIfDue(this) { available ->
            if (isDestroyed) return@checkIfDue
            if (available != null) {
                updateBanner.show("新しい版 ${available.version} があります", Tone.INFO)
            }
        }
    }

    private fun confirmClearLearning() {
        confirmDialog(
            "学習データを削除",
            "これまでの変換の学習をすべて消します。元に戻せません。ユーザー辞書は消えません。",
            "削除",
            destructive = true,
        ) {
            NagiRuntime.get(this).call("clear_learning", JSONObject().put("session_id", "android")) { r ->
                toast(if (r != null) "学習データを削除しました" else "削除できませんでした（コアが応答しません）")
                refreshStatus()
            }
        }
    }

    private fun refreshStatus() {
        if (DataInstaller.dictFile(this).exists()) {
            dictRow.setDot(toneColor(Tone.SUCCESS))
            dictRow.setValue("あり")
            dictRow.setSubtitle(null)
        } else {
            dictRow.setDot(toneColor(Tone.WARNING))
            dictRow.setValue("なし")
            dictRow.setSubtitle("読みのまま変換します")
        }
        if (ModelStore.hasModel(this)) {
            modelRow.setDot(toneColor(Tone.SUCCESS))
            modelRow.setValue("あり")
            modelRow.setSubtitle(null)
        } else {
            modelRow.setDot(toneColor(Tone.WARNING))
            modelRow.setValue("なし")
            modelRow.setSubtitle("辞書のみで動作します")
        }
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        val enabled = imm.enabledInputMethodList.any { it.packageName == packageName }
        keyboardRow.setDot(toneColor(if (enabled) Tone.SUCCESS else Tone.WARNING))
        keyboardRow.setValue(if (enabled) "有効" else "未有効")

        NagiRuntime.get(this).call("get_status") { r ->
            if (isDestroyed) return@call
            if (r == null) {
                coreRow.setDot(toneColor(Tone.ERROR))
                coreRow.setValue("起動できません", cDanger)
                val why = NagiRuntime.get(this).startError ?: NagiCore.loadError
                coreRow.setSubtitle(if (why != null) "理由: $why" else null)
            } else {
                coreRow.setDot(toneColor(Tone.SUCCESS))
                coreRow.setValue("動作中", cTextSecondary)
                coreRow.setSubtitle(
                    "バージョン ${NagiCore.nativeVersion()} / プロファイル ${r.optString("profile")} / " +
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
