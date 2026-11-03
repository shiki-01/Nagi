package com.shiki01.nagi

import android.app.Activity
import android.app.AlertDialog
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.ScrollView
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.util.concurrent.Executors

/** 画面の表示に必要な情報（バックグラウンドで集めてメインスレッドで描く） */
internal data class LayoutScreenState(
    val presets: List<LayoutInfo>,
    val users: List<LayoutInfo>,
    val kana: String,
    val alpha: String,
    val byField: Boolean,
    val fieldAssigned: Map<FieldKind, String>,
)

/** 詳細ダイアログ用に読んだ JSON とその解釈 */
internal class LayoutDetailData(val raw: String?, val layout: LayoutModel?)

/** ファイルの読み取り結果 */
internal sealed class FileReadResult {
    class Text(val text: String) : FileReadResult()

    class Fail(val reason: String) : FileReadResult()
}

/** 複製の結果 */
internal sealed class DuplicateResult {
    class Ok(val id: String, val name: String) : DuplicateResult()

    class Fail(val reason: String) : DuplicateResult()
}

/**
 * キーボードレイアウトの管理画面の中身（レイアウトタブ）。「割り当て中」「入力欄ごとの割り当て」「取り込み」
 * 「自分のレイアウト」「同梱のレイアウト」「アイコンの出典」。
 * 取り込み（ファイル・貼り付け・リンク）、`nagi://layout` の受け取り（確認ダイアログ必須）、書き出し、削除ができる。
 * ファイルを読み書きする [LayoutStore] / [LayoutImporter] はバックグラウンドで呼び、UI はメインスレッドで更新する。
 *
 * [ContextWrapper] にしているのは、Ui.kt の `Context` 拡張（card / actionRow / dp / toast …）をそのまま使うため。
 * [base] は画面を持つ Activity（ダイアログの親になる）。Fragment が [reload] / [offerIntent] / [onFileResult] / [onPasteResult] /
 * [dispose] を呼ぶ。ファイル選択と貼り付け画面の起動は [launchFile] / [launchPaste]（結果は Fragment 側で受けて渡す）
 */
internal class LayoutManagerScreen(
    base: Context,
    parts: ScreenParts,
    private val launchFile: (Intent) -> Unit,
    private val launchPaste: (Intent) -> Unit,
) : ContextWrapper(base) {
    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor { r -> Thread(r, "nagi-layout").apply { isDaemon = true } }

    private val scroll: ScrollView = parts.scroll
    private val errorBanner: UiBanner
    private val importBox: LinearLayout
    private val assignBox: LinearLayout
    private val fieldBox: LinearLayout
    private val presetBox: LinearLayout
    private val userBox: LinearLayout

    private var state: LayoutScreenState? = null

    @Volatile
    private var disposed = false

    init {
        val root = parts.content
        errorBanner = banner("", Tone.ERROR)
        errorBanner.visibility = View.GONE
        root.addView(errorBanner)
        assignBox = verticalBox()
        fieldBox = verticalBox()
        importBox = verticalBox()
        userBox = verticalBox()
        presetBox = verticalBox()
        root.addView(assignBox)
        root.addView(fieldBox)
        root.addView(importBox)
        root.addView(userBox)
        root.addView(presetBox)
        val iconBox = verticalBox()
        iconBox.addSection(
            null,
            card(navRow("アイコンの出典・ライセンス", null) { showIconLicensesDialog() }),
            "Android 版は iconify: のアイコンを取得せず、代替の文字で表示します。",
        )
        root.addView(iconBox)
        renderImport()
    }

    /** 画面を閉じる。以後、バックグラウンドの結果は捨てる */
    fun dispose() {
        disposed = true
        io.shutdown()
    }

    /** 外部から開かれた Intent（`nagi://layout`）を受け取る。必ず確認ダイアログを挟む */
    fun offerIntent(i: Intent?) = handleIntent(i)

    private fun verticalBox(): LinearLayout {
        val b = LinearLayout(this)
        b.orientation = LinearLayout.VERTICAL
        return b
    }

    // ---- バックグラウンド実行 ----

    /** [work] をバックグラウンドで実行し、結果を [done] にメインスレッドで渡す */
    private fun <T> background(work: () -> T, done: (T) -> Unit) {
        if (disposed) return
        io.execute {
            val result = try {
                Result.success(work())
            } catch (e: Exception) {
                Result.failure(e)
            }
            main.post {
                if (disposed) return@post
                val value = result.getOrNull()
                if (result.isSuccess && value != null) {
                    done(value)
                } else if (result.isFailure) {
                    toast("処理に失敗しました: ${result.exceptionOrNull()?.message ?: "不明なエラー"}")
                }
            }
        }
    }

    private fun showError(message: String) {
        errorBanner.show(message, Tone.ERROR)
        scroll.smoothScrollTo(0, 0)
    }

    // ---- 一覧の読み込みと描画 ----

    /** 取り込みの入口（常設） */
    private fun renderImport() {
        importBox.removeAllViews()
        val c = card()
        c.add(actionRow("クリップボードから取り込む", "サイトで作った JSON やリンクをコピーしてからタップします") { importFromClipboard() })
        c.add(actionRow("ファイルから取り込む", "JSON ファイル（.json）を選びます") { pickFile() })
        c.add(actionRow("JSON を貼り付ける", "入力欄に貼り付けて取り込みます") { openPasteScreen() })
        c.add(actionRow("リンクから取り込む", "nagi://layout のリンクを貼り付けます") { showLinkInput() })
        importBox.addSection("取り込み", c, "サイト（${LayoutClipboard.SITE_URL}）でレイアウトを作れます")
    }

    /** 一覧と割り当てを読み直して描く（タブを開くたび・変更のたびに呼ぶ） */
    fun reload() {
        background({
            LayoutScreenState(
                presets = LayoutStore.listPresets(this),
                users = LayoutStore.listUser(this),
                kana = LayoutStore.assigned(this, AssignSlot.KANA),
                alpha = LayoutStore.assigned(this, AssignSlot.ALPHA),
                byField = KeyboardPrefs.layoutByField(this),
                fieldAssigned = KeyboardPrefs.fieldAssignments(this),
            )
        }) { s ->
            state = s
            renderAssign(s)
            renderFieldAssign(s)
            renderPresets(s)
            renderUsers(s)
        }
    }

    /** 割り当て中のレイアウト（表示のみ）。かな・英字の選択は設定タブ、またはレイアウトの詳細で行う */
    private fun renderAssign(s: LayoutScreenState) {
        assignBox.removeAllViews()
        val eff = LayoutChoices.effective(s.presets, s.users)
        val names = LayoutChoices.names(eff)
        val c = card()
        c.add(infoRow("かな", names[s.kana] ?: s.kana, s.kana))
        c.add(infoRow("英字", names[s.alpha] ?: s.alpha, s.alpha))
        assignBox.addSection(
            "割り当て中",
            c,
            "変更は、設定タブのキーボードの項目か、各レイアウトの詳細で行います。",
        )
    }

    /** 入力欄の種類ごとの割り当て（Android のみ。iOS は設定の `keyboard.layout_by_field` の 1 項目） */
    private fun renderFieldAssign(s: LayoutScreenState) {
        fieldBox.removeAllViews()
        val eff = LayoutChoices.effective(s.presets, s.users)
        val names = LayoutChoices.names(eff)
        val c = card()
        c.add(
            switchRow(
                "入力欄の種類で自動的に切り替える",
                "メール・URL・数字などの欄で、合うレイアウトに切り替えます",
                s.byField,
            ) { v ->
                KeyboardPrefs.setLayoutByField(this, v)
                KeyboardPrefs.bumpLayoutsRevision(this)
                reload()
            },
        )
        if (s.byField) {
            for (kind in FieldKind.values()) {
                c.add(
                    selectRow(
                        kind.label,
                        LayoutChoices.fieldSubtitle(kind, s.fieldAssigned, names),
                        LayoutChoices.fieldOptions(kind, eff),
                        LayoutChoices.fieldSelected(s.fieldAssigned[kind]),
                    ) { v -> assignField(kind, LayoutChoices.fieldValueToStore(v)) },
                )
            }
        }
        c.add(
            actionRow("すべて既定に戻す", "かな・英字・入力欄ごとの割り当てを初期状態にします", danger = true) { confirmResetAssignments() },
        )
        fieldBox.addSection("入力欄ごとの割り当て", c)
    }

    private fun renderPresets(s: LayoutScreenState) {
        presetBox.removeAllViews()
        val c = card()
        if (s.presets.isEmpty()) {
            c.add(cardBody(emptyState("同梱のレイアウトが見つかりません")))
        }
        for (info in s.presets) {
            c.add(navRow(info.name, LayoutTexts.summary(info), "${info.keyCount} キー") { showDetail(info) })
        }
        presetBox.addSection("同梱のレイアウト", c, "タップでプレビューと複製ができます")
    }

    private fun renderUsers(s: LayoutScreenState) {
        userBox.removeAllViews()
        val c = card()
        if (s.users.isEmpty()) {
            val button = tonalButton("web のエディタを開く") { openEditor() }
            c.add(
                cardBody(
                    emptyState(
                        "自分のレイアウトはまだありません",
                        "上の「クリップボードから取り込む」か「ファイルから取り込む」で、サイトで作ったレイアウトを入れられます",
                    ),
                    button,
                ),
            )
        } else {
            for (info in s.users) c.add(userEntry(info))
            c.add(actionRow("web のエディタを開く", "レイアウトを作る・編集する") { openEditor() })
        }
        userBox.addSection("自分のレイアウト", c)
    }

    /** 自分のレイアウト 1 件の行。不正なら理由を赤いバナーで下に付ける */
    private fun userEntry(info: LayoutInfo): View {
        val row = UiRow(this)
        row.titleView.text = info.name
        row.setSubtitle(LayoutTexts.summary(info))
        if (!info.valid) {
            row.setTrailing(badge("無効", Tone.ERROR))
        } else if (info.overridesPreset) {
            row.setTrailing(badge("同梱を上書き中", Tone.WARNING))
        }
        row.setChevron()
        row.onTap { showDetail(info) }
        val reason = info.error ?: return row
        val box = verticalBox()
        box.addView(row)
        val b = banner(reason, Tone.ERROR)
        val lp = b.layoutParams as LinearLayout.LayoutParams
        lp.marginStart = dp(UiSize.ROW_PAD_DP)
        lp.marginEnd = dp(UiSize.ROW_PAD_DP)
        lp.topMargin = 0
        b.layoutParams = lp
        box.addView(b)
        return box
    }

    // ---- 割り当て ----

    private fun assign(slot: AssignSlot, id: String) {
        background({ LayoutStore.assign(this, slot, id) }) { ok ->
            if (!ok) toast("このレイアウトは使えません")
            reload()
        }
    }

    private fun assignField(kind: FieldKind, value: String?) {
        background({ LayoutStore.assignField(this, kind, value) }) { ok ->
            if (!ok) toast("このレイアウトは使えません")
            reload()
        }
    }

    private fun confirmResetAssignments() {
        confirmDialog(
            "すべて既定に戻す",
            "かな・英字・入力欄ごとのレイアウトの割り当てを、初期状態に戻します。レイアウトのファイルは消えません。",
            "戻す",
            destructive = true,
        ) {
            background({ LayoutStore.resetAssignments(this) }) {
                toast("既定に戻しました")
                reload()
            }
        }
    }

    // ---- 詳細 ----

    private fun showDetail(info: LayoutInfo) {
        background({ readDetail(info) }) { d -> openDetailDialog(info, d) }
    }

    private fun readDetail(info: LayoutInfo): LayoutDetailData {
        // 同梱の行は、ユーザーが同じ id で上書いていても同梱のほうを見せる
        val raw = if (info.source == LayoutSource.PRESET) readPresetRaw(info.id) else LayoutStore.readRaw(this, info.id)
        val layout = raw?.let { LayoutParser.parseOrNull(it) }
        return LayoutDetailData(raw, layout)
    }

    private fun readPresetRaw(id: String): String? = try {
        assets.open("presets/$id.json").bufferedReader(Charsets.UTF_8).use { it.readText() }
    } catch (e: IOException) {
        null
    }

    private fun openDetailDialog(info: LayoutInfo, d: LayoutDetailData) {
        val user = info.source == LayoutSource.USER
        val col = LinearLayout(this)
        col.orientation = LinearLayout.VERTICAL
        col.setPadding(dp(20), dp(20), dp(20), dp(8))
        col.addView(textOf(info.name, 20f, cText, bold = true))
        col.addView(note(LayoutTexts.summary(info)))

        d.layout?.let { layout ->
            val preview = PreviewView(this)
            preview.setLayout(layout)
            val lp = LinearLayout.LayoutParams(LP_MATCH, LP_WRAP)
            lp.topMargin = dp(12)
            lp.bottomMargin = dp(8)
            col.addView(preview, lp)
        }
        info.error?.let { col.addView(banner(it, Tone.ERROR)) }

        var dialog: AlertDialog? = null
        fun act(block: () -> Unit): () -> Unit = {
            dialog?.dismiss()
            block()
        }

        val actions = card()
        val raw = d.raw
        val s = state
        if (!user) {
            if (raw != null) {
                actions.add(
                    actionRow("複製して自分のレイアウトに追加", "id を変えて保存します。編集用の取り出しに使えます", onClick = act { duplicate(info, raw) }),
                )
            }
        } else if (d.layout != null) {
            val slots = LayoutTexts.slotsFor(d.layout.mode)
            for (slot in slots) {
                val assignedId = if (slot == AssignSlot.KANA) s?.kana else s?.alpha
                val current = assignedId == info.id
                actions.add(
                    if (current) {
                        actionRow(LayoutTexts.slotLabel(slot) + "（使用中）", "既定に戻す", onClick = act { unassign(slot) })
                    } else {
                        actionRow(LayoutTexts.slotLabel(slot), "このレイアウトを割り当てる", onClick = act { assign(slot, info.id) })
                    },
                )
            }
            if (slots.isEmpty()) {
                actions.add(
                    infoRow(
                        "割り当てはありません",
                        null,
                        "数字・記号・絵文字のレイアウトは、他のレイアウトの layout キーや、入力欄の種類別の割り当てから使います",
                    ),
                )
            }
            if (raw != null) {
                actions.add(
                    actionRow("複製して自分のレイアウトに追加", "id を変えて保存します", onClick = act { duplicate(info, raw) }),
                )
            }
        }
        if (raw != null) {
            actions.add(actionRow("JSON をコピー", null, onClick = act { copyJson(raw) }))
            actions.add(actionRow("JSON を共有", null, onClick = act { shareJson(info, raw) }))
            if (user && info.valid) {
                actions.add(actionRow("nagi:// リンクをコピー", "他の端末で開くと取り込めます", onClick = act { copyLink(raw) }))
            }
        }
        if (user) {
            actions.add(actionRow("削除", null, danger = true, onClick = act { confirmDelete(info) }))
        }
        if (actions.childCount > 0) col.addView(actions)

        val sv = ScrollView(this)
        sv.addView(col)
        val dlg = AlertDialog.Builder(this).setView(sv).setNegativeButton("閉じる", null).create()
        dialog = dlg
        dlg.show()
    }

    private fun unassign(slot: AssignSlot) {
        background({ LayoutStore.assign(this, slot, null) }) {
            toast("既定に戻しました")
            reload()
        }
    }

    private fun confirmDelete(info: LayoutInfo) {
        confirmDialog(
            "レイアウトを削除",
            "「${info.name}」（${info.id}）を削除します。割り当てていた場合は既定に戻ります。元に戻せません。",
            "削除",
            destructive = true,
        ) {
            background({ LayoutStore.delete(this, info.id) }) { ok ->
                toast(if (ok) "削除しました" else "削除できませんでした")
                reload()
            }
        }
    }

    // ---- 複製・書き出し ----

    private fun duplicate(info: LayoutInfo, raw: String) {
        val baseName = info.name
        background({
            val taken = LayoutStore.presetIds(this).toSet() + LayoutStore.userIds(this)
            val newId = LayoutCopy.freeId(info.id, taken)
            val newName = LayoutCopy.copyName(baseName)
            duplicateInto(raw, newId, newName)
        }) { r ->
            when (r) {
                is DuplicateResult.Ok -> {
                    errorBanner.hide()
                    toast("「${r.name}」（${r.id}）として保存しました")
                    reload()
                }
                is DuplicateResult.Fail -> showError("複製できませんでした: ${r.reason}")
            }
        }
    }

    private fun duplicateInto(raw: String, newId: String, newName: String): DuplicateResult {
        val rewritten = LayoutCopy.rewrite(raw, newId, newName) ?: return DuplicateResult.Fail("JSON を読み取れません")
        return when (val p = LayoutParser.parse(rewritten, null)) {
            is LayoutParseResult.Err -> DuplicateResult.Fail(p.reason)
            is LayoutParseResult.Ok ->
                if (LayoutStore.saveUser(this, p.layout, rewritten)) DuplicateResult.Ok(newId, newName) else DuplicateResult.Fail("保存できませんでした")
        }
    }

    private fun copyToClipboard(label: String, text: String): Boolean = try {
        val cm = getSystemService(CLIPBOARD_SERVICE) as ClipboardManager
        cm.setPrimaryClip(ClipData.newPlainText(label, text))
        true
    } catch (e: RuntimeException) {
        false
    }

    private fun copyJson(raw: String) {
        toast(if (copyToClipboard("Nagi layout", raw)) "JSON をコピーしました" else "コピーできませんでした（大きすぎる可能性があります）")
    }

    private fun shareJson(info: LayoutInfo, raw: String) {
        val send = Intent(Intent.ACTION_SEND)
        send.type = "text/plain"
        send.putExtra(Intent.EXTRA_SUBJECT, "${info.id}.json")
        send.putExtra(Intent.EXTRA_TEXT, raw)
        try {
            startActivity(Intent.createChooser(send, "レイアウトを共有"))
        } catch (e: ActivityNotFoundException) {
            toast("共有できるアプリがありません")
        } catch (e: RuntimeException) {
            toast("共有できませんでした（大きすぎる可能性があります。コピーをお試しください）")
        }
    }

    private fun copyLink(raw: String) {
        background<Pair<String?, String?>>({
            val json = LayoutCopy.minify(raw)?.takeIf { it.length < raw.length } ?: raw
            val link = LayoutLink.encode(json)
            if (LayoutIntake.linkTooLong(link)) {
                Pair("大きすぎてリンクにできません。JSON のコピーまたは共有をお使いください", null)
            } else {
                // 受け取り側と同じ解釈で確かめる（画像つきなどはここで分かる）
                when (val r = LayoutLink.parse(link)) {
                    is DeepLinkResult.Err -> Pair("リンクにできません: ${r.reason}", null)
                    is DeepLinkResult.Ok -> Pair(null, link)
                }
            }
        }) { (problem, link) ->
            if (link != null) {
                toast(if (copyToClipboard("Nagi layout link", link)) "リンクをコピーしました" else "コピーできませんでした")
            } else {
                messageDialog("リンクにできません", problem ?: "不明なエラー")
            }
        }
    }

    // ---- 取り込み ----

    private fun showLinkInput() {
        showTextInput("リンクを貼り付け", "nagi://layout?data=...", 3) { text -> startPastedLink(text) }
    }

    private fun openPasteScreen() {
        launchPaste(Intent(this, LayoutPasteActivity::class.java))
    }

    /** ユーザーが「クリップボードから取り込む」をタップしたときだけクリップボードを読む */
    private fun importFromClipboard() {
        val text = LayoutClipboardReader.readText(this)
        when (LayoutClipboard.classify(text)) {
            ClipboardKind.EMPTY, ClipboardKind.OTHER -> {
                errorBanner.show(LayoutClipboard.NOT_FOUND, Tone.WARNING)
                scroll.smoothScrollTo(0, 0)
            }
            ClipboardKind.LINK -> if (text != null) startPastedLink(text)
            ClipboardKind.JSON -> if (text != null) startImport(text, viaLink = false)
        }
    }

    private fun showTextInput(title: String, hint: String, lines: Int, onOk: (String) -> Unit) {
        val field = inputField(hint)
        field.minLines = lines
        field.gravity = Gravity.TOP or Gravity.START
        field.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        val box = LinearLayout(this)
        box.setPadding(dp(20), dp(8), dp(20), 0)
        box.addView(field, LinearLayout.LayoutParams(LP_MATCH, LP_WRAP))
        AlertDialog.Builder(this)
            .setTitle(title)
            .setView(box)
            .setPositiveButton("確認") { _, _ -> onOk(field.text.toString()) }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    private fun startPastedLink(text: String) {
        when (val r = LayoutIntake.classifyUrl(text)) {
            is IntakeResult.Accept -> startLink(r.url)
            is IntakeResult.Reject -> showError(r.reason)
            IntakeResult.None -> showError("リンクが空です")
        }
    }

    private fun pickFile() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
        intent.addCategory(Intent.CATEGORY_OPENABLE)
        intent.type = "*/*"
        intent.putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("application/json", "text/plain", "application/octet-stream"))
        try {
            launchFile(intent)
        } catch (e: ActivityNotFoundException) {
            toast("ファイルを選ぶアプリが見つかりません")
        }
    }

    /** 貼り付け画面（[LayoutPasteActivity]）の結果。取り込み済みなので、割り当てを尋ねる */
    fun onPasteResult(resultCode: Int, data: Intent?) {
        val id = data?.getStringExtra(LayoutPasteActivity.EXTRA_LAYOUT_ID)
        if (resultCode == Activity.RESULT_OK && id != null) {
            background({ LayoutStore.load(this, id) }) { layout ->
                reload()
                if (layout != null) offerAssign(layout.id, layout.name, layout.mode)
            }
        }
    }

    /** ファイル選択の結果 */
    fun onFileResult(resultCode: Int, data: Intent?) {
        if (resultCode != Activity.RESULT_OK) return
        val uri = data?.data ?: return
        background({ readLimited(uri) }) { r ->
            when (r) {
                is FileReadResult.Text -> startImport(r.text, viaLink = false)
                is FileReadResult.Fail -> showError(r.reason)
            }
        }
    }

    /** 上限（[LayoutLimits.JSON_BYTES_MAX]）を超えるファイルは途中で読むのをやめて拒否する */
    private fun readLimited(uri: Uri): FileReadResult {
        return try {
            val max = LayoutLimits.JSON_BYTES_MAX
            val bytes = contentResolver.openInputStream(uri)?.use { s ->
                val out = ByteArrayOutputStream()
                val buf = ByteArray(32 * 1024)
                var total = 0
                while (true) {
                    val n = s.read(buf)
                    if (n < 0) break
                    total += n
                    if (total > max) throw IOException(LayoutTexts.tooLarge())
                    out.write(buf, 0, n)
                }
                out.toByteArray()
            } ?: throw IOException("ファイルを開けません")
            FileReadResult.Text(String(bytes, Charsets.UTF_8))
        } catch (e: IOException) {
            FileReadResult.Fail(e.message ?: "ファイルを読めません")
        } catch (e: SecurityException) {
            FileReadResult.Fail("ファイルを読む権限がありません")
        }
    }

    /** 外部から開かれた Intent（`nagi://layout`）を受け取る。必ず確認ダイアログを挟む */
    private fun handleIntent(i: Intent?) {
        when (val r = LayoutIntake.classify(i?.action == Intent.ACTION_VIEW, i?.dataString)) {
            IntakeResult.None -> Unit
            is IntakeResult.Reject -> showError(r.reason)
            is IntakeResult.Accept -> startLink(r.url)
        }
    }

    /** リンクを解釈して（保存はしない）、確認へ進む。同梱と同じ id は拒否する */
    private fun startLink(url: String) {
        background({ LayoutImporter.parseDeepLink(url) }) { r ->
            when (r) {
                is DeepLinkResult.Err -> showError(r.reason)
                is DeepLinkResult.Ok -> startImport(r.json, viaLink = true)
            }
        }
    }

    /** 検証（保存しない）→ 確認ダイアログ → 保存 */
    private fun startImport(json: String, viaLink: Boolean) {
        background({ LayoutImporter.inspect(this, json, allowPresetId = !viaLink) }) { r ->
            when (r) {
                is InspectResult.Err -> showError(r.reason)
                is InspectResult.Ok -> {
                    val c = r.candidate
                    errorBanner.hide()
                    confirmDialog(
                        LayoutImportText.confirmTitle(viaLink),
                        LayoutImportText.confirmMessage(
                            c.layout.name,
                            c.layout.id,
                            c.layout.mode,
                            c.keyCount,
                            c.replaces,
                            c.overridesPreset,
                            viaLink,
                            c.unsupported,
                        ),
                        "取り込む",
                    ) { runImport(c.json, viaLink) }
                }
            }
        }
    }

    private fun runImport(json: String, viaLink: Boolean) {
        background({ LayoutImporter.importJson(this, json, allowPresetId = !viaLink) }) { r ->
            when (r) {
                is ImportResult.Success -> {
                    errorBanner.hide()
                    toast(LayoutImportText.success(r.candidate.layout.name, r.candidate.replaces) + r.iconNote)
                    reload()
                    offerAssign(r.candidate.layout.id, r.candidate.layout.name, r.candidate.layout.mode)
                }
                is ImportResult.Failure -> showError(r.reason)
            }
        }
    }

    /** 取り込み後に「かな / 英字で使う」を尋ねる。割り当て対象外のモード（数字・記号・絵文字）では出さない */
    private fun offerAssign(id: String, name: String, mode: LayoutMode?) {
        val options = PostImportChoices.build(mode)
        if (options.isEmpty()) return
        choiceDialog(PostImportChoices.title(name), options.map { it.label }, -1) { i ->
            val slot = options.getOrNull(i)?.slot ?: return@choiceDialog
            background({ LayoutStore.assign(this, slot, id) }) { ok ->
                if (ok) toast(PostImportChoices.ASSIGNED) else toast("このレイアウトは使えません")
                reload()
            }
        }
    }

    private fun openEditor() {
        try {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(EDITOR_URL)))
        } catch (e: ActivityNotFoundException) {
            toast("ブラウザが見つかりません")
        }
    }

    companion object {
        /** web のレイアウトエディタ（`web/src/routes/custom-keyboard`） */
        private const val EDITOR_URL = "https://nagi.shiki-01.com/custom-keyboard"
    }
}
