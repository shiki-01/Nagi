package com.shiki01.nagi.tabs

import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import androidx.fragment.app.Fragment
import com.shiki01.nagi.AppPrefs
import com.shiki01.nagi.ClipboardHistoryStore
import com.shiki01.nagi.DeveloperActivity
import com.shiki01.nagi.FrontKeys
import com.shiki01.nagi.FrontKind
import com.shiki01.nagi.FrontSettingDef
import com.shiki01.nagi.FrontSettings
import com.shiki01.nagi.KeyboardPrefs
import com.shiki01.nagi.LP_MATCH
import com.shiki01.nagi.LP_WRAP
import com.shiki01.nagi.NagiRuntime
import com.shiki01.nagi.ResetKeys
import com.shiki01.nagi.ScreenParts
import com.shiki01.nagi.SettingDef
import com.shiki01.nagi.SettingsEntry
import com.shiki01.nagi.SettingsPlan
import com.shiki01.nagi.SettingsSchema
import com.shiki01.nagi.SettingsUiMode
import com.shiki01.nagi.Tone
import com.shiki01.nagi.UiBanner
import com.shiki01.nagi.UiButton
import com.shiki01.nagi.UiCard
import com.shiki01.nagi.UpdateActivity
import com.shiki01.nagi.actionRow
import com.shiki01.nagi.addSection
import com.shiki01.nagi.banner
import com.shiki01.nagi.card
import com.shiki01.nagi.confirmDialog
import com.shiki01.nagi.destructiveButton
import com.shiki01.nagi.dp
import com.shiki01.nagi.emptyState
import com.shiki01.nagi.filledButton
import com.shiki01.nagi.navRow
import com.shiki01.nagi.note
import com.shiki01.nagi.pageParts
import com.shiki01.nagi.selectRow
import com.shiki01.nagi.spacer
import com.shiki01.nagi.toast
import org.json.JSONObject

/**
 * 設定タブ。「入力」「変換」などのグループを 1 本のリストにまとめる。
 * - コアの記述子（`get_settings`）の項目と、フロント固有の項目（[FrontSettings]。[KeyboardPrefs] / [AppPrefs]）を、
 *   [SettingsPlan] の固定順（iOS と同じ）で並べる。
 * - 上部の「表示: シンプル / 詳細」で出し分ける（[SettingsUiMode]。値は変わらない）。
 * - コアの項目の変更は `update_settings` で即時反映（検証と `config.toml` への書き込みはコア）。
 *   フロント固有の項目はキーボードが次に表示されたときに読む。
 */
class SettingsFragment : Fragment() {
    private val handler = Handler(Looper.getMainLooper())

    /** 画面が生きているあいだだけ non-null（非同期の結果を捨てる判定に使う） */
    private var parts: ScreenParts? = null
    private var frontRows: FrontSettingRows? = null
    private var coreRows: CoreSettingRows? = null

    private lateinit var errorBanner: UiBanner
    private lateinit var restartButton: UiButton
    private lateinit var modeBox: LinearLayout
    private lateinit var content: LinearLayout

    private var coreItems: List<SettingDef> = emptyList()
    private var coreFailed = false
    private var loaded = false
    private var layouts: LayoutSlots? = null

    /** コアの項目の行（1 項目だけ差し替えるため） */
    private val coreViews = HashMap<String, View>()

    private val host = object : FrontSettingRows.Host {
        override fun valueChanged() {
            frontRows?.refreshResets()
        }

        override fun rebuild() {
            handler.post { render() }
        }

        override fun layoutsChanged() {
            reloadLayouts()
        }

        override fun <T> background(work: () -> T, done: (T) -> Unit) {
            this@SettingsFragment.background(work, done)
        }
    }

    override fun onCreateView(inflater: LayoutInflater, container: ViewGroup?, savedInstanceState: Bundle?): View {
        val ctx = requireContext()
        val p = ctx.pageParts("設定")
        parts = p
        frontRows = FrontSettingRows(ctx, host)
        coreRows = CoreSettingRows(ctx) { def, value -> send(def, value) }
        val root = p.content

        errorBanner = ctx.banner("", Tone.ERROR)
        errorBanner.visibility = View.GONE
        root.addView(errorBanner)

        modeBox = LinearLayout(ctx)
        modeBox.orientation = LinearLayout.VERTICAL
        root.addView(modeBox)
        renderMode()

        restartButton = ctx.filledButton("変更を反映する（エンジンを再起動）") {
            NagiRuntime.restart(ctx)
            restartButton.visibility = View.GONE
            ctx.toast("エンジンを再起動しました")
        }
        restartButton.visibility = View.GONE
        val rlp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
        rlp.topMargin = ctx.dp(16)
        root.addView(restartButton, rlp)

        content = LinearLayout(ctx)
        content.orientation = LinearLayout.VERTICAL
        root.addView(content)
        content.addView(ctx.emptyState("読み込んでいます…"))
        return p.root
    }

    override fun onResume() {
        super.onResume()
        load()
    }

    override fun onDestroyView() {
        handler.removeCallbacksAndMessages(null)
        parts = null
        frontRows = null
        coreRows = null
        coreViews.clear()
        super.onDestroyView()
    }

    // ---- 読み込み ----

    /** コアの設定とレイアウトの一覧を読み、そろったら 1 回だけ描く（他の画面で変えた値も拾う） */
    private fun load() {
        val app = requireContext().applicationContext
        var pending = 2
        fun oneDone() {
            pending--
            if (pending == 0) {
                loaded = true
                render()
            }
        }
        background({ runCatching { LayoutSlots.load(app) }.getOrNull() }) { slots ->
            if (slots != null) layouts = slots
            oneDone()
        }
        NagiRuntime.get(app).call("get_settings") { r ->
            if (parts == null) return@call
            if (r == null) {
                coreFailed = true
                coreItems = emptyList()
            } else {
                coreFailed = false
                applySnapshot(r)
            }
            oneDone()
        }
    }

    private fun reloadLayouts() {
        val app = requireContext().applicationContext
        background({ LayoutSlots.load(app) }) { slots ->
            layouts = slots
            render()
        }
    }

    private fun applySnapshot(snapshot: JSONObject) {
        coreItems = SettingsSchema.parseItems(snapshot)
        val err = snapshot.optString("error", "")
        if (err.isNotEmpty()) {
            errorBanner.show("設定ファイルを読めませんでした（既定値を表示しています。ここでの変更は保存されません）\n$err", Tone.ERROR)
        } else {
            errorBanner.hide()
        }
    }

    /** [work] をバックグラウンドで実行し、結果を [done] にメインスレッドで渡す（画面が無くなっていたら捨てる） */
    private fun <T> background(work: () -> T, done: (T) -> Unit) {
        val app = requireContext().applicationContext
        Thread {
            val result = try {
                Result.success(work())
            } catch (e: Exception) {
                Result.failure(e)
            }
            handler.post {
                if (parts == null) return@post
                result.onSuccess { done(it) }
                result.onFailure { app.toast("処理に失敗しました: ${it.message ?: "不明なエラー"}") }
            }
        }.start()
    }

    // ---- 描画 ----

    /** 上部の「表示: シンプル / 詳細」 */
    private fun renderMode() {
        val ctx = requireContext()
        modeBox.removeAllViews()
        val mode = AppPrefs.uiMode(ctx)
        modeBox.addSection(
            null,
            ctx.card(
                ctx.selectRow("表示", null, SettingsUiMode.values().map { it.json to it.label }, mode.json) { id ->
                    AppPrefs.setUiMode(ctx, SettingsUiMode.fromJson(id))
                    render()
                },
            ),
            FrontSettings.MODE_FOOTNOTE,
        )
    }

    /** 一覧を組み立て直す。スクロール位置は保つ */
    private fun render() {
        val p = parts ?: return
        val ctx = requireContext()
        val front = frontRows ?: return
        val core = coreRows ?: return
        val scrollY = p.scroll.scrollY

        content.removeAllViews()
        coreViews.clear()
        front.clear()
        if (!loaded) {
            content.addView(ctx.emptyState("読み込んでいます…"))
            return
        }
        if (coreFailed) {
            content.addView(ctx.banner("コアを起動できないため、変換の設定を読み込めませんでした。", Tone.ERROR))
        }

        val sections = SettingsPlan.build(coreItems, AppPrefs.uiMode(ctx), AppPrefs.developerMode(ctx))
        for (s in sections) {
            val groupCard = UiCard(ctx)
            for (e in s.entries) {
                val v = when (e) {
                    is SettingsEntry.Core -> core.build(e.def)
                    is SettingsEntry.Front -> frontEntry(e.def, front)
                }
                if (e is SettingsEntry.Core) coreViews[e.def.key] = v
                groupCard.add(v)
            }
            content.addSection(s.title, groupCard, s.footnote)
        }

        content.addView(ctx.note(FrontSettings.PAGE_FOOTNOTE))
        content.addView(ctx.spacer(20))
        content.addView(
            ctx.destructiveButton("すべて既定に戻す") {
                ctx.confirmDialog(
                    "すべて既定に戻す",
                    "キーボードの設定（入力・キーの見た目と操作・片手モード・高さ・レイアウトの割り当て・ツールバー・" +
                        "クリップボード履歴の設定・アップデートの自動確認）を" +
                        "すべて既定の値に戻します。変換の設定（コア）は戻りません。",
                    "戻す",
                    destructive = true,
                ) {
                    KeyboardPrefs.reset(ctx, ResetKeys.ALL)
                    KeyboardPrefs.setKanaLayoutId(ctx, null)
                    KeyboardPrefs.setAlphaLayoutId(ctx, null)
                    KeyboardPrefs.bumpLayoutsRevision(ctx)
                    reloadLayouts()
                }
            },
        )
        front.refreshResets()
        p.scroll.post { p.scroll.scrollTo(0, scrollY) }
    }

    private fun frontEntry(def: FrontSettingDef, front: FrontSettingRows): View {
        val ctx = requireContext()
        if (def.kind != FrontKind.ACTION) return front.build(def, layouts)
        return when (def.key) {
            FrontKeys.LEARNING_CLEAR -> ctx.actionRow(def.label, def.description, danger = true) { confirmClearLearning() }
            FrontKeys.CLIPBOARD_CLEAR -> ctx.actionRow(def.label, def.description, danger = true) { confirmClearClipboard() }
            FrontKeys.UPDATE_OPEN -> ctx.navRow(def.label, def.description) {
                startActivity(Intent(ctx, UpdateActivity::class.java))
            }
            FrontKeys.DEVELOPER_OPEN -> ctx.navRow(def.label, def.description) {
                startActivity(Intent(ctx, DeveloperActivity::class.java))
            }
            else -> View(ctx)
        }
    }

    // ---- 学習データの削除 ----

    private fun confirmClearLearning() {
        val ctx = requireContext()
        val app = ctx.applicationContext
        ctx.confirmDialog(
            "学習データをすべて削除",
            "これまでの変換の学習をすべて消します。元に戻せません。ユーザー辞書は消えません。",
            "削除",
            destructive = true,
        ) {
            NagiRuntime.get(app).call("clear_learning", JSONObject().put("session_id", "android")) { r ->
                app.toast(if (r != null) "学習データを削除しました" else "削除できませんでした（コアが応答しません）")
            }
        }
    }

    // ---- クリップボード履歴の削除 ----

    private fun confirmClearClipboard() {
        val ctx = requireContext()
        val app = ctx.applicationContext
        ctx.confirmDialog(
            "履歴をすべて削除",
            "クリップボード履歴を、ピン留めした項目も含めてすべて消します。元に戻せません。",
            "削除",
            destructive = true,
        ) {
            val store = ClipboardHistoryStore(java.io.File(app.filesDir, ClipboardHistoryStore.FILE_NAME))
            background({ store.clear() }) { app.toast("クリップボード履歴を削除しました") }
        }
    }

    // ---- コアの設定の変更 ----

    /** コアの項目 1 件を `update_settings` で送る。[value] が null なら既定に戻す */
    private fun send(def: SettingDef, value: Any?) {
        val app = requireContext().applicationContext
        val params = SettingsSchema.updateParams(listOf(Triple(def.key, def.type, value)))
        NagiRuntime.get(app).call("update_settings", params) { r ->
            if (parts == null) return@call
            if (r == null) {
                app.toast("設定を保存できませんでした（コアが応答しません）")
                refreshCoreRow(def.key)
                return@call
            }
            val errors = r.optJSONArray("errors")
            if (errors != null && errors.length() > 0) {
                val msgs = (0 until errors.length()).mapNotNull { errors.optJSONObject(it)?.optString("message") }
                app.toast(msgs.joinToString("\n").ifEmpty { "設定を変更できませんでした" })
            }
            r.optJSONObject("snapshot")?.let { applySnapshot(it) }
            refreshCoreRow(def.key)
            if (def.requiresRestart && r.optJSONArray("applied")?.length().let { it != null && it > 0 }) {
                restartButton.visibility = View.VISIBLE
            }
        }
    }

    /** コアの 1 項目を差し替える（スクロール位置を保つため全体は作り直さない） */
    private fun refreshCoreRow(key: String) {
        val old = coreViews[key] ?: return
        val def = coreItems.firstOrNull { it.key == key } ?: return
        val core = coreRows ?: return
        val parent = old.parent as? LinearLayout ?: return
        val index = parent.indexOfChild(old)
        val v = core.build(def)
        parent.removeViewAt(index)
        parent.addView(v, index)
        coreViews[key] = v
    }
}
