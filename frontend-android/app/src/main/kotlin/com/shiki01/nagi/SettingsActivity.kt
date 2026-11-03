package com.shiki01.nagi

import android.app.Activity
import android.app.ActivityManager
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.LinearLayout
import com.shiki01.nagi.core.NagiCore
import org.json.JSONObject
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** 本体アプリのホーム。有効化の案内・状態表示・入力テストと、各設定画面への入口 */
class SettingsActivity : Activity() {
    private lateinit var updateBanner: UiBanner
    private lateinit var noticeBanner: UiBanner
    private lateinit var memoryRow: UiRow
    private lateinit var devSection: LinearLayout
    private var noticeText: String? = null
    private lateinit var coreRow: UiRow
    private lateinit var dictRow: UiRow
    private lateinit var modelRow: UiRow
    private lateinit var keyboardRow: UiRow
    private lateinit var layoutRow: UiRow

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("Nagi")
        val root = parts.content

        // 更新後の案内（版が変わった最初の 1 回だけ）
        noticeBanner = banner("", Tone.SUCCESS)
        noticeBanner.visibility = View.GONE
        root.addView(noticeBanner)
        showUpdateNotice(savedInstanceState)

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
        memoryRow = infoRow("メモリ予算", "")
        root.addSection("状態", card(coreRow, dictRow, modelRow, keyboardRow, memoryRow))

        // はじめる
        layoutRow = navRow("キーボードのレイアウト", "サイトで作ったレイアウトを取り込めます") {
            openScreen(LayoutManagerActivity::class.java)
        }
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
                navRow("使い始めの手順", "有効化から入力テストまで") { openScreen(OnboardingActivity::class.java) },
                layoutRow,
            ),
        )

        // 設定
        root.addSection(
            "設定",
            card(
                navRow("入力設定", "フリック・見た目・操作") { openScreen(KeyboardInputSettingsActivity::class.java) },
                navRow("変換の設定", "変換・予測・学習・表記") { openScreen(SettingsFormActivity::class.java) },
                navRow("ユーザー辞書", "単語とスニペットの登録") { openScreen(UserDictActivity::class.java) },
                navRow("変換モデル", "ダウンロード・取り込み・選択") { openScreen(ModelActivity::class.java) },
                navRow("英語・カタカナ語データ", "任意の追加データ") { openScreen(OptionalDataActivity::class.java) },
                navRow("データの使用量", "辞書・モデル・学習データの容量") { openScreen(DataUsageActivity::class.java) },
            ),
        )

        // アップデート
        root.addSection(
            "アップデート",
            card(navRow("アップデート", "新しい版の確認とインストール", versionName()) { openUpdate() }),
        )

        // 情報
        root.addSection(
            "情報",
            card(navRow("このアプリについて", "バージョン・プライバシー・ライセンス") { openScreen(AboutActivity::class.java) }),
        )
        // 開発者モードがオンのときだけ出す（onResume で表示を切り替える）
        devSection = LinearLayout(this)
        devSection.orientation = LinearLayout.VERTICAL
        devSection.addSection(
            "開発者",
            card(navRow("開発者", "診断ログ・応答時間・メモリ") { openScreen(DeveloperActivity::class.java) }),
        )
        root.addView(devSection)

        // データ管理
        root.addSection(
            "データ管理",
            card(actionRow("学習データを削除", "ユーザー辞書は消えません", danger = true) { confirmClearLearning() }),
        )

        setContentView(parts.root)

        // 初回だけ使い始めの手順を自動で開く（画面の作り直しでは開かない）
        if (savedInstanceState == null && !AppPrefs.onboardingDone(this)) {
            openScreen(OnboardingActivity::class.java)
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        noticeText?.let { outState.putString(STATE_NOTICE, it) }
    }

    /** 版が変わっていれば成功バナーを出し、見た版を記録する。画面の作り直しでは同じ文言を出し直す */
    private fun showUpdateNotice(saved: Bundle?) {
        val current = versionName()
        var text = saved?.getString(STATE_NOTICE)
        if (saved == null) {
            text = UpdateNotice.text(AppPrefs.lastSeenVersion(this), current)
            AppPrefs.setLastSeenVersion(this, current)
            AppPrefs.setLastSeenUpdateTime(this, installTime())
        }
        noticeText = text
        if (text != null) noticeBanner.show(text, Tone.SUCCESS)
    }

    private fun installTime(): Long = try {
        packageManager.getPackageInfo(packageName, 0).lastUpdateTime
    } catch (e: Exception) {
        0L
    }

    override fun onResume() {
        super.onResume()
        devSection.visibility = if (AppPrefs.developerMode(this)) View.VISIBLE else View.GONE
        refreshStatus()
        refreshLayoutRow()
        checkUpdateIfDue()
    }

    /** 現在の割り当て（かな: ○○ / 英字: ○○）を補足に出す */
    private fun refreshLayoutRow() {
        Thread {
            val text = loadAssignmentSummary() + "。サイトで作ったレイアウトを取り込めます"
            runOnUiThread {
                if (!isDestroyed) layoutRow.setSubtitle(text)
            }
        }.start()
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
        val dict = DataInstaller.dictFile(this)
        if (dict.exists()) {
            dictRow.setDot(toneColor(Tone.SUCCESS))
            dictRow.setValue("あり")
            val at = SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.JAPAN).format(Date(dict.lastModified()))
            dictRow.setSubtitle("${formatBytes(dict.length())} / 取得（展開）$at")
        } else {
            dictRow.setDot(toneColor(Tone.WARNING))
            dictRow.setValue("なし")
            dictRow.setSubtitle("読みのまま変換します")
        }
        val am = getSystemService(ACTIVITY_SERVICE) as ActivityManager
        val mi = ActivityManager.MemoryInfo()
        am.getMemoryInfo(mi)
        val budgetMb = ModelRecommendation.budgetMbFromAvail(mi.availMem)
        val catalog = ModelRecommendation.CATALOG.map { ModelSpec(it.fileName, it.sizeBytes) }
        val recommend = ModelRecommendation.summary(budgetMb, catalog)
        val models = ModelStore.list(this)
        if (models.isNotEmpty()) {
            modelRow.setDot(toneColor(Tone.SUCCESS))
            modelRow.setValue("あり")
            modelRow.setSubtitle(
                models.joinToString(" / ") { "${ModelRecommendation.label(it.name)} ${formatBytes(it.sizeBytes)}" },
            )
        } else {
            modelRow.setDot(toneColor(Tone.WARNING))
            modelRow.setValue("なし")
            modelRow.setSubtitle("辞書のみで動作します。$recommend")
        }
        memoryRow.setValue("$budgetMb MB")
        memoryRow.setSubtitle(recommend.ifEmpty { null })
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

    companion object {
        private const val STATE_NOTICE = "update_notice"
    }

    private fun versionName(): String = try {
        packageManager.getPackageInfo(packageName, 0).versionName ?: "?"
    } catch (e: Exception) {
        "?"
    }
}
