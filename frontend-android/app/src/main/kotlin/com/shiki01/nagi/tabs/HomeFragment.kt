package com.shiki01.nagi.tabs

import android.app.ActivityManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.widget.LinearLayout
import androidx.fragment.app.Fragment
import com.shiki01.nagi.*
import com.shiki01.nagi.core.NagiCore
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * ホームタブ（iOS のホームと同じ順）: 更新バナー・更新後の案内 → セットアップ → システム辞書 → 変換モデル →
 * [開発者モード] メモリ状況 → LLM が使える見込み → 追加データ → ユーザー辞書 → 取り込み。
 * Android のみの項目（コアの状態・データの使用量・アップデート）は、セットアップの下とホームの末尾に置く。
 * 状態は表示されるたびに（onResume）読み直す。
 */
class HomeFragment : Fragment() {
    private lateinit var noticeBanner: UiBanner
    private lateinit var updateBanner: UiBanner
    private lateinit var coreRow: UiRow
    private lateinit var setupBox: LinearLayout
    private lateinit var dictBox: LinearLayout
    private lateinit var modelBox: LinearLayout
    private lateinit var memoryBox: LinearLayout
    private lateinit var llmBox: LinearLayout
    private lateinit var importBox: LinearLayout

    private var noticeText: String? = null

    /** コアの設定 `conversion.model`（使用中のモデルの判定に使う）。コアから読めるまでは null */
    private var configuredModel: String? = null

    override fun onCreateView(inflater: LayoutInflater, container: ViewGroup?, savedInstanceState: Bundle?): View {
        val ctx = requireContext()
        val parts = ctx.pageParts("ホーム")
        val root = parts.content

        // 更新後の案内（版が変わった最初の 1 回だけ）
        noticeBanner = ctx.banner("", Tone.SUCCESS)
        noticeBanner.visibility = View.GONE
        root.addView(noticeBanner)
        showUpdateNotice(ctx, savedInstanceState)

        // 新しい版のお知らせ（自動確認で見つかったときだけ表示）
        updateBanner = ctx.banner("", Tone.INFO)
        updateBanner.setAction("アップデートを開く") { openScreen(UpdateActivity::class.java) }
        updateBanner.visibility = View.GONE
        root.addView(updateBanner)

        setupBox = verticalBox(ctx)
        root.addView(setupBox)

        // Android のみ: コアの状態（プロファイル・学習件数）
        coreRow = ctx.statusRow("コア", "確認中", Tone.NEUTRAL)
        root.addSection("コアの状態", ctx.card(coreRow))

        dictBox = verticalBox(ctx)
        modelBox = verticalBox(ctx)
        memoryBox = verticalBox(ctx)
        llmBox = verticalBox(ctx)
        importBox = verticalBox(ctx)
        root.addView(dictBox)
        root.addView(modelBox)
        root.addView(memoryBox)
        root.addView(llmBox)

        root.addSection(
            "追加データ（英語・カタカナ語の原語）",
            ctx.card(
                ctx.navRow("英語・カタカナ語データ", "英語語彙（SCOWL）・カタカナ語（JMdict）。任意の追加データ") {
                    openScreen(OptionalDataActivity::class.java)
                },
            ),
        )
        root.addSection(
            "ユーザー辞書",
            ctx.card(
                ctx.navRow("ユーザー辞書・スニペット", "単語とスニペットの登録") { openScreen(UserDictActivity::class.java) },
            ),
        )
        root.addView(importBox)

        // Android のみ
        root.addSection(
            "その他",
            ctx.card(
                ctx.navRow("データの使用量", "辞書・モデル・学習データの容量") { openScreen(DataUsageActivity::class.java) },
                ctx.navRow("アップデート", "新しい版の確認とインストール", UpdateChecker.currentVersion(ctx)) {
                    openScreen(UpdateActivity::class.java)
                },
            ),
        )
        return parts.root
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        noticeText?.let { outState.putString(STATE_NOTICE, it) }
    }

    override fun onResume() {
        super.onResume()
        refresh()
        checkUpdateIfDue()
    }

    private fun verticalBox(ctx: Context): LinearLayout {
        val b = LinearLayout(ctx)
        b.orientation = LinearLayout.VERTICAL
        return b
    }

    private fun alive(): Boolean = isAdded && view != null

    private fun openScreen(cls: Class<*>) {
        startActivity(Intent(requireContext(), cls))
    }

    // ---- 更新の案内 ----

    /** 版が変わっていれば成功バナーを出し、見た版を記録する。画面の作り直しでは同じ文言を出し直す */
    private fun showUpdateNotice(ctx: Context, saved: Bundle?) {
        val current = UpdateChecker.currentVersion(ctx)
        var text = saved?.getString(STATE_NOTICE)
        if (saved == null) {
            text = UpdateNotice.text(AppPrefs.lastSeenVersion(ctx), current)
            AppPrefs.setLastSeenVersion(ctx, current)
            AppPrefs.setLastSeenUpdateTime(ctx, installTime(ctx))
        }
        noticeText = text
        if (text != null) noticeBanner.show(text, Tone.SUCCESS)
    }

    private fun installTime(ctx: Context): Long = try {
        ctx.packageManager.getPackageInfo(ctx.packageName, 0).lastUpdateTime
    } catch (e: Exception) {
        0L
    }

    private fun checkUpdateIfDue() {
        val ctx = context ?: return
        if (!KeyboardPrefs.autoCheckUpdates(ctx)) return
        UpdateChecker.checkIfDue(ctx) { available ->
            if (!alive()) return@checkIfDue
            if (available != null) {
                updateBanner.show("新しい版 ${available.version} があります", Tone.INFO)
            }
        }
    }

    // ---- 状態の読み直し ----

    private fun refresh() {
        val ctx = context ?: return
        if (view == null) return
        renderAll(ctx)
        loadCoreStatus(ctx)
        loadConfiguredModel(ctx)
    }

    private fun loadCoreStatus(ctx: Context) {
        NagiRuntime.get(ctx).call("get_status") { r ->
            if (!alive()) return@call
            if (r == null) {
                coreRow.setDot(ctx.toneColor(Tone.ERROR))
                coreRow.setValue("起動できません", ctx.cDanger)
                val why = NagiRuntime.get(ctx).startError ?: NagiCore.loadError
                coreRow.setSubtitle(if (why != null) "理由: $why" else null)
            } else {
                coreRow.setDot(ctx.toneColor(Tone.SUCCESS))
                coreRow.setValue("動作中", ctx.cTextSecondary)
                coreRow.setSubtitle(
                    "バージョン ${NagiCore.nativeVersion()} / プロファイル ${r.optString("profile")} / " +
                        "学習 ${r.optInt("learned_entries")} 件",
                )
            }
        }
    }

    /** 使用中のモデルの判定に、コアの `conversion.model` を読む。変わっていたら描き直す */
    private fun loadConfiguredModel(ctx: Context) {
        NagiRuntime.get(ctx).call("get_settings") { r ->
            if (!alive() || r == null) return@call
            val items = SettingsSchema.parseItems(r)
            val configured = items.firstOrNull { it.key == "conversion.model" }?.value as? String
            if (configured != configuredModel) {
                configuredModel = configured
                renderAll(requireContext())
            }
        }
    }

    private fun renderAll(ctx: Context) {
        val dictExists = DataInstaller.dictFile(ctx).exists()
        val models = ModelStore.list(ctx)
        val budgetMb = budgetMb(ctx)
        renderSetup(ctx, dictExists, models.size)
        renderDictionary(ctx, dictExists)
        renderModels(ctx, models, budgetMb)
        renderMemory(ctx, budgetMb)
        renderLlm(ctx, dictExists, models.size, budgetMb)
        renderImport(ctx, models)
    }

    private fun availMem(ctx: Context): ActivityManager.MemoryInfo {
        val am = ctx.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val mi = ActivityManager.MemoryInfo()
        am.getMemoryInfo(mi)
        return mi
    }

    private fun budgetMb(ctx: Context): Long = ModelRecommendation.budgetMbFromAvail(availMem(ctx).availMem)

    private fun Context.statusLine(l: StatusLine): UiRow = statusRow(l.title, l.value, l.tone, l.subtitle)

    // ---- セットアップ ----

    private fun isKeyboardEnabled(ctx: Context): Boolean {
        val imm = ctx.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        return imm.enabledInputMethodList.any { it.packageName == ctx.packageName }
    }

    private fun isKeyboardSelected(ctx: Context): Boolean {
        val id = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.DEFAULT_INPUT_METHOD) ?: return false
        return id.contains(ctx.packageName)
    }

    private fun renderSetup(ctx: Context, dictExists: Boolean, modelCount: Int) {
        setupBox.removeAllViews()
        val c = ctx.card(
            ctx.statusLine(HomeStatus.keyboardAdded(isKeyboardEnabled(ctx))),
            ctx.statusLine(HomeStatus.keyboardSelected(isKeyboardSelected(ctx))),
            ctx.statusLine(HomeStatus.setupDictionary(dictExists)),
            ctx.statusLine(HomeStatus.setupModel(modelCount)),
            ctx.navRow("セットアップの案内を表示", "有効化から入力テストまで") { openScreen(OnboardingActivity::class.java) },
            ctx.actionRow("設定アプリを開く", "入力方法の設定で Nagi をオンにします") {
                try {
                    startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
                } catch (e: ActivityNotFoundException) {
                    ctx.toast("設定アプリを開けません")
                }
            },
            ctx.actionRow("入力方法を切り替える", "入力中のキーボードを Nagi にします") {
                val imm = ctx.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
                imm.showInputMethodPicker()
            },
        )
        setupBox.addSection("セットアップ", c)
    }

    // ---- システム辞書 ----

    private fun renderDictionary(ctx: Context, exists: Boolean) {
        dictBox.removeAllViews()
        val c = ctx.card(ctx.statusLine(HomeStatus.dictionaryState(exists)))
        if (exists) {
            val dict = DataInstaller.dictFile(ctx)
            val at = SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.JAPAN).format(Date(dict.lastModified()))
            c.add(ctx.infoRow("サイズ", formatBytes(dict.length())))
            c.add(ctx.infoRow("取得（展開）日時", at))
        }
        dictBox.addSection("システム辞書", c)
        if (!exists) dictBox.addView(ctx.note(HomeStatus.DICTIONARY_MISSING_NOTE, ctx.cWarning))
    }

    // ---- 変換モデル ----

    private fun renderModels(ctx: Context, models: List<InstalledModel>, budgetMb: Long) {
        modelBox.removeAllViews()
        val effective = ModelStore.effective(ctx, configuredModel)
        val c = ctx.card(ctx.statusLine(HomeStatus.modelState(models.size)))
        for (m in models) {
            val row = UiRow(ctx)
            row.titleView.text = m.name
            row.setSubtitle(HomeStatus.modelDetail(m.name, m.sizeBytes))
            if (effective?.file?.absolutePath == m.file.absolutePath) {
                row.setTrailing(ctx.badge("使用中", Tone.SUCCESS))
            }
            c.add(row)
        }
        c.add(
            ctx.navRow("変換モデルの詳細", "ダウンロード・取り込み・削除") { openScreen(ModelActivity::class.java) },
        )
        val catalog = ModelRecommendation.CATALOG.map { ModelSpec(it.fileName, it.sizeBytes) }
        val recommend = ModelRecommendation.summary(budgetMb, catalog)
        modelBox.addSection("変換モデル", c, recommend.ifEmpty { null })
    }

    // ---- メモリ状況（開発者モードのときだけ） ----

    private fun renderMemory(ctx: Context, budgetMb: Long) {
        memoryBox.removeAllViews()
        if (!AppPrefs.developerMode(ctx)) return
        val mi = availMem(ctx)
        val c = ctx.card(
            ctx.infoRow("空きメモリ", formatBytes(mi.availMem), if (mi.lowMemory) "端末は低メモリの状態です" else null),
            ctx.infoRow("メモリ予算", "$budgetMb MB", "空きの半分を目安にした、コアが使える量の見込みです"),
            ctx.actionRow("再計測", null) { refresh() },
        )
        memoryBox.addSection("メモリ状況（このアプリ）", c)
    }

    // ---- LLM が使える見込み ----

    private fun renderLlm(ctx: Context, dictExists: Boolean, modelCount: Int, budgetMb: Long) {
        llmBox.removeAllViews()
        val v = HomeStatus.llmVerdict(dictExists, modelCount, budgetMb)
        llmBox.addSection("LLM が使える見込み", ctx.card(ctx.statusRow("LLM", v.value, v.tone, v.detail)), HomeStatus.LLM_VERDICT_NOTE)
    }

    // ---- 取り込み ----

    private fun renderImport(ctx: Context, models: List<InstalledModel>) {
        importBox.removeAllViews()
        val c = ctx.card(
            ctx.navRow("モデルをインポート（.gguf を選択）", "変換モデルの画面でファイルを選びます") {
                openScreen(ModelActivity::class.java)
            },
        )
        if (models.size >= 2) {
            val inUse = ModelStore.effective(ctx, configuredModel)
            c.add(
                ctx.navRow(
                    "使うモデルの選択",
                    inUse?.let { "使用中: ${ModelRecommendation.label(it.name)}" },
                ) { openScreen(ModelActivity::class.java) },
            )
        }
        importBox.addSection("取り込み", c, "辞書はアプリに同梱されていて、アップデートで更新されます（取り込みは要りません）。")
    }

    companion object {
        private const val STATE_NOTICE = "update_notice"
    }
}
