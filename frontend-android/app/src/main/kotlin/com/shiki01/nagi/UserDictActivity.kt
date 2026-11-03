package com.shiki01.nagi

import android.app.Activity
import android.app.AlertDialog
import android.content.DialogInterface
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.util.Base64
import android.view.View
import android.view.ViewGroup
import android.widget.ArrayAdapter
import android.widget.BaseAdapter
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.concurrent.Executors

/**
 * ユーザー辞書。`<filesDir>/user/user_dict/` の `*.txt` / `*.tsv` を一覧し、編集できるのは `nagi-user.tsv` だけ
 * （ほかのファイル、たとえばインポートで追記される `imported.tsv` は読み取り専用で表示する）。
 * 保存後はコアに `reload_config` を送って反映する。
 */
class UserDictActivity : Activity() {
    private sealed class RowItem {
        data class Heading(val text: String) : RowItem()
        data class Note(val text: String) : RowItem()
        data class Own(val entry: UserDictEntry, val index: Int) : RowItem()
        data class ReadOnly(val entry: UserDictEntry) : RowItem()
    }

    private val main = Handler(Looper.getMainLooper())

    /** ファイルの読み書きは 1 本のスレッドで直列に行う */
    private val io = Executors.newSingleThreadExecutor { r -> Thread(r, "nagi-userdict").apply { isDaemon = true } }

    private var own: MutableList<UserDictEntry> = ArrayList()
    private var others: List<Pair<String, List<UserDictEntry>>> = emptyList()
    private var skippedFiles: List<String> = emptyList()
    private var ownIssues = 0
    private var loaded = false
    private var maxEntries: Int? = null
    private var coreCount: Int? = null
    private var query = ""
    private var items: List<RowItem> = emptyList()

    private lateinit var list: ListView
    private lateinit var adapter: ItemAdapter
    private lateinit var countView: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pad = dp(16)
        val root = LinearLayout(this)
        root.orientation = LinearLayout.VERTICAL
        root.setPadding(pad, pad, pad, 0)
        @Suppress("DEPRECATION")
        root.setOnApplyWindowInsetsListener { v, insets ->
            v.setPadding(pad, pad + insets.systemWindowInsetTop, pad, insets.systemWindowInsetBottom)
            insets
        }

        val title = TextView(this)
        title.text = "ユーザー辞書"
        title.textSize = 24f
        title.setTypeface(null, android.graphics.Typeface.BOLD)
        title.setPadding(0, pad / 2, 0, pad / 2)
        root.addView(title)

        val search = EditText(this)
        search.hint = "検索（読み・単語・コメント）"
        search.setSingleLine(true)
        search.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}

            override fun afterTextChanged(s: Editable?) {
                query = s?.toString() ?: ""
                rebuild()
            }
        })
        root.addView(search)

        list = ListView(this)
        list.addHeaderView(buildHeader(), null, false)
        adapter = ItemAdapter()
        list.adapter = adapter
        list.setOnItemClickListener { _, _, position, _ ->
            val row = items.getOrNull(position - list.headerViewsCount)
            if (row is RowItem.Own) showEditDialog(row.entry, row.index, false)
        }
        root.addView(list, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        setContentView(root)
    }

    override fun onResume() {
        super.onResume()
        loadMaxEntries()
        reload()
    }

    override fun onDestroy() {
        io.shutdown()
        super.onDestroy()
    }

    // ---- 画面の部品 ----

    private fun buildHeader(): View {
        val box = LinearLayout(this)
        box.orientation = LinearLayout.VERTICAL
        box.addView(
            body(
                "読みと単語を登録すると、変換候補に出ます。スニペットは、短い読みから定型文（複数行可）を出す登録です。" +
                    "スニペットを使うかどうかは「変換の設定」で切り替えられます（登録は消えません）。",
                secondary = true,
            ),
        )
        box.addView(
            body(
                "注意: ユーザー辞書は、システム辞書が読み込めているときだけ変換に反映されます。",
                secondary = true,
                color = COLOR_WARNING,
            ),
        )
        if (!DataInstaller.dictFile(this).exists()) {
            box.addView(
                body(
                    "いまシステム辞書が見つからないため、登録しても変換には反映されません。",
                    color = COLOR_ERROR,
                ),
            )
        }
        countView = body("")
        box.addView(countView)
        box.addView(
            row(
                button("追加") { showEditDialog(null, -1, false) }.also { it.layoutParams = weight1() },
                button("スニペット追加") { showEditDialog(null, -1, true) }.also { it.layoutParams = weight1() },
            ),
        )
        box.addView(
            row(
                button("インポート") { pickImport() }.also { it.layoutParams = weight1() },
                button("エクスポート") { startExport() }.also { it.layoutParams = weight1() },
            ),
        )
        return box
    }

    private inner class ItemAdapter : BaseAdapter() {
        override fun getCount(): Int = items.size

        override fun getItem(position: Int): Any = items[position]

        override fun getItemId(position: Int): Long = position.toLong()

        override fun isEnabled(position: Int): Boolean = items[position] is RowItem.Own

        override fun getView(position: Int, convertView: View?, parent: ViewGroup?): View =
            when (val item = items[position]) {
                is RowItem.Heading -> heading(item.text)
                is RowItem.Note -> body(item.text, secondary = true)
                is RowItem.Own -> entryView(item.entry)
                is RowItem.ReadOnly -> entryView(item.entry).also { it.alpha = 0.7f }
            }
    }

    private fun entryView(e: UserDictEntry): View {
        val col = LinearLayout(this)
        col.orientation = LinearLayout.VERTICAL
        col.setPadding(0, dp(8), 0, dp(8))
        val firstLine = e.surface.lineSequence().firstOrNull() ?: ""
        val multi = e.surface.contains('\n')
        val line = TextView(this)
        line.textSize = 16f
        line.text = "${e.reading}  →  $firstLine" + if (multi) " …" else ""
        col.addView(line)
        val sub = StringBuilder(if (e.isSnippet) "スニペット" else e.pos)
        if (e.comment.isNotEmpty()) sub.append(" / ").append(e.comment)
        col.addView(body(sub.toString(), secondary = true))
        return col
    }

    // ---- 一覧の組み立て ----

    private fun rebuild() {
        val out = ArrayList<RowItem>()
        val total = own.size + others.sumOf { it.second.size }
        val limit = maxEntries
        countView.text = buildString {
            append("登録 ").append(own.size).append(" 件（編集できる分）")
            if (others.isNotEmpty()) append(" + 読み取り専用 ").append(total - own.size).append(" 件")
            if (limit != null) append(" / 上限 ").append(limit).append(" 件")
            coreCount?.let { append("\nコアが読み込んだ件数: ").append(it).append(" 件") }
            if (ownIssues > 0) append("\n読み込めない行が ").append(ownIssues).append(" 行ありました（保存すると取り除かれます）")
        }
        if (limit != null && total > limit) {
            countView.append("\n上限を超えています。超えた分はコアが読み込みません。不要な登録を削除してください。")
            countView.setTextColor(COLOR_ERROR)
        } else {
            countView.setTextColor(0xFF000000.toInt())
        }
        if (!loaded) {
            out.add(RowItem.Note("読み込んでいます…"))
        } else {
            val mine = own.mapIndexedNotNull { i, e -> if (MozcTsv.matches(e, query)) RowItem.Own(e, i) else null }
            out.add(RowItem.Heading("自分の登録（${MozcTsv.OWN_FILE_NAME}）"))
            if (mine.isEmpty()) {
                out.add(RowItem.Note(if (query.isBlank()) "まだ登録がありません。「追加」から登録できます。" else "一致する登録がありません。"))
            }
            out.addAll(mine.reversed()) // 新しい登録を上に
            for ((name, entries) in others) {
                val matched = entries.filter { MozcTsv.matches(it, query) }
                if (matched.isEmpty()) continue
                out.add(RowItem.Heading("読み取り専用: $name"))
                matched.forEach { out.add(RowItem.ReadOnly(it)) }
            }
            for (name in skippedFiles) {
                out.add(RowItem.Note("$name は大きすぎるため表示していません（コアは読み込みます）"))
            }
        }
        items = out
        adapter.notifyDataSetChanged()
    }

    // ---- 読み込み / 保存 ----

    private fun dictDir(): File = File(DataInstaller.userDir(this), "user_dict")

    private fun loadMaxEntries() {
        NagiRuntime.get(this).call("get_settings") { r ->
            if (r != null) {
                val d = SettingsSchema.parseItems(r).firstOrNull { it.key == "userdict.max_entries" }
                maxEntries = (d?.value as? Number)?.toInt()
                if (loaded) rebuild()
            }
        }
        NagiRuntime.get(this).call("get_status") { r ->
            if (r != null && r.has("user_dict_entries")) {
                coreCount = r.optInt("user_dict_entries")
                if (loaded) rebuild()
            }
        }
    }

    /** ディスクから読み直して一覧を更新する */
    private fun reload() {
        val dir = dictDir()
        io.execute {
            val newOwn = ArrayList<UserDictEntry>()
            val newOthers = ArrayList<Pair<String, List<UserDictEntry>>>()
            val skipped = ArrayList<String>()
            var issues = 0
            val files = dir.listFiles()
                ?.filter { it.isFile && (it.name.endsWith(".txt", true) || it.name.endsWith(".tsv", true)) }
                ?.sortedBy { it.name }
                ?: emptyList()
            for (f in files) {
                if (f.length() > MozcTsv.MAX_IMPORT_BYTES) {
                    skipped.add(f.name)
                    continue
                }
                val parsed = try {
                    MozcTsv.parse(MozcTsv.decode(f.readBytes()))
                } catch (e: IOException) {
                    skipped.add(f.name)
                    continue
                }
                if (f.name == MozcTsv.OWN_FILE_NAME) {
                    newOwn.addAll(parsed.entries)
                    issues = parsed.issues.size
                } else {
                    newOthers.add(f.name to parsed.entries)
                }
            }
            main.post {
                if (isDestroyed) return@post
                own = newOwn
                others = newOthers
                skippedFiles = skipped
                ownIssues = issues
                loaded = true
                rebuild()
            }
        }
    }

    /** [own] を `nagi-user.tsv` に原子的に書き、コアに読み直させる */
    private fun saveOwn() {
        val snapshot = own.toList()
        val dir = dictDir()
        io.execute {
            var error: String? = null
            try {
                dir.mkdirs()
                val tmp = File(dir, MozcTsv.OWN_FILE_NAME + ".tmp") // 拡張子が .tmp なのでコアは読まない
                tmp.writeBytes(MozcTsv.serialize(snapshot).toByteArray(Charsets.UTF_8))
                val dest = File(dir, MozcTsv.OWN_FILE_NAME)
                try {
                    Files.move(tmp.toPath(), dest.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
                } catch (e: AtomicMoveNotSupportedException) {
                    Files.move(tmp.toPath(), dest.toPath(), StandardCopyOption.REPLACE_EXISTING)
                }
            } catch (e: Throwable) {
                error = e.message ?: e.javaClass.simpleName
            }
            main.post {
                if (isDestroyed) return@post
                if (error != null) {
                    toast("保存できませんでした: $error")
                } else {
                    reloadCore()
                }
                reload()
            }
        }
    }

    private fun reloadCore() {
        NagiRuntime.get(this).call("reload_config") { r ->
            if (r == null) {
                toast("コアに反映できませんでした（次にキーボードを開いたとき反映されます）")
                return@call
            }
            val errors = r.optJSONArray("errors")
            if (errors != null && errors.length() > 0) {
                val e = errors.optJSONObject(0)
                toast("設定の読み込みに問題があります: ${e?.optString("message")}")
            }
            NagiRuntime.get(this).call("get_status") { s ->
                if (s != null && s.has("user_dict_entries")) {
                    coreCount = s.optInt("user_dict_entries")
                    rebuild()
                }
            }
        }
    }

    // ---- 追加・編集・削除 ----

    private fun showEditDialog(existing: UserDictEntry?, ownIndex: Int, snippetMode: Boolean) {
        val snippet = existing?.isSnippet ?: snippetMode
        val pad = dp(16)
        val form = LinearLayout(this)
        form.orientation = LinearLayout.VERTICAL
        form.setPadding(pad, pad / 2, pad, 0)

        val reading = EditText(this)
        reading.hint = "読み（ひらがな）"
        reading.setSingleLine(true)
        reading.setText(existing?.reading ?: "")
        form.addView(reading)

        val surface = EditText(this)
        surface.hint = if (snippet) "本文（改行できます。${MozcTsv.MAX_SNIPPET_CHARS} 文字まで）" else "単語"
        if (snippet) {
            surface.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
            surface.minLines = 3
            surface.gravity = android.view.Gravity.TOP
        } else {
            surface.setSingleLine(true)
        }
        surface.setText(existing?.surface ?: "")
        form.addView(surface)

        var posSpinner: Spinner? = null
        if (!snippet) {
            val choices = ArrayList(MozcTsv.POS_CHOICES)
            val current = existing?.pos
            if (current != null && current !in choices) choices.add(0, current)
            val sp = Spinner(this)
            sp.adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, choices)
            sp.setSelection(if (current != null) choices.indexOf(current).coerceAtLeast(0) else 0)
            form.addView(body("品詞", secondary = true))
            form.addView(sp)
            posSpinner = sp
        }

        val comment = EditText(this)
        comment.hint = "コメント（任意）"
        comment.setSingleLine(true)
        comment.setText(existing?.comment ?: "")
        form.addView(comment)

        val errorView = body("", color = COLOR_ERROR)
        form.addView(errorView)

        val scroll = ScrollView(this)
        scroll.addView(form)

        val builder = AlertDialog.Builder(this)
            .setTitle(
                when {
                    existing == null && snippet -> "スニペットを追加"
                    existing == null -> "単語を追加"
                    snippet -> "スニペットを編集"
                    else -> "単語を編集"
                },
            )
            .setView(scroll)
            .setPositiveButton("保存", null)
            .setNegativeButton("キャンセル", null)
        if (existing != null) builder.setNeutralButton("削除", null)
        val dialog = builder.create()
        dialog.show()

        dialog.getButton(DialogInterface.BUTTON_POSITIVE).setOnClickListener {
            val entry = MozcTsv.normalize(
                UserDictEntry(
                    reading = reading.text.toString(),
                    surface = surface.text.toString(),
                    pos = if (snippet) MozcTsv.SNIPPET_POS else (posSpinner?.selectedItem as? String ?: "名詞"),
                    comment = comment.text.toString(),
                ),
            )
            val problem = MozcTsv.validate(entry) ?: checkLimits(entry, ownIndex)
            if (problem != null) {
                errorView.text = problem
                return@setOnClickListener
            }
            if (ownIndex >= 0 && ownIndex < own.size) own[ownIndex] = entry else own.add(entry)
            dialog.dismiss()
            rebuild()
            saveOwn()
        }
        if (existing != null) {
            dialog.getButton(DialogInterface.BUTTON_NEUTRAL).setOnClickListener {
                dialog.dismiss()
                confirmDelete(ownIndex, existing)
            }
        }
    }

    /** 重複・上限の検査。問題があればその説明 */
    private fun checkLimits(entry: UserDictEntry, ownIndex: Int): String? {
        val dup = own.withIndex().any { (i, e) -> i != ownIndex && e == entry }
        if (dup) return "同じ登録がすでにあります"
        if (ownIndex < 0) {
            if (own.size >= MozcTsv.MAX_OWN_ENTRIES) return "登録できる件数の上限（${MozcTsv.MAX_OWN_ENTRIES} 件）に達しています"
            val limit = maxEntries
            val total = own.size + others.sumOf { it.second.size }
            if (limit != null && total >= limit) {
                return "ユーザー辞書の上限（$limit 件）に達しています。不要な登録を削除するか、変換の設定で上限を上げてください"
            }
        }
        return null
    }

    private fun confirmDelete(index: Int, e: UserDictEntry) {
        AlertDialog.Builder(this)
            .setTitle("登録を削除")
            .setMessage("「${e.reading} → ${e.surface.lineSequence().firstOrNull() ?: ""}」を削除します。")
            .setPositiveButton("削除") { _, _ ->
                if (index in own.indices) {
                    own.removeAt(index)
                    rebuild()
                    saveOwn()
                }
            }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    // ---- インポート / エクスポート ----

    private fun pickImport() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
        intent.addCategory(Intent.CATEGORY_OPENABLE)
        intent.type = "*/*"
        @Suppress("DEPRECATION")
        startActivityForResult(intent, REQUEST_IMPORT)
    }

    private fun startExport() {
        if (!loaded) {
            toast("読み込み中です。少し待ってからお試しください")
            return
        }
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
        intent.addCategory(Intent.CATEGORY_OPENABLE)
        intent.type = "text/tab-separated-values"
        intent.putExtra(Intent.EXTRA_TITLE, "nagi-user-dict.tsv")
        @Suppress("DEPRECATION")
        startActivityForResult(intent, REQUEST_EXPORT)
    }

    @Deprecated("Activity Result API を使わない素の Activity のため")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        when (requestCode) {
            REQUEST_IMPORT -> doImport(uri)
            REQUEST_EXPORT -> doExport(uri)
        }
    }

    private fun doImport(uri: Uri) {
        val resolver = contentResolver
        toast("取り込んでいます…")
        io.execute {
            var base64: String? = null
            var error: String? = null
            try {
                val bytes = resolver.openInputStream(uri)?.use { s ->
                    val out = ByteArrayOutputStream()
                    val buf = ByteArray(64 * 1024)
                    var total = 0
                    while (true) {
                        val n = s.read(buf)
                        if (n < 0) break
                        total += n
                        if (total > MozcTsv.MAX_IMPORT_BYTES) throw IOException("ファイルが大きすぎます（16MiB まで）")
                        out.write(buf, 0, n)
                    }
                    out.toByteArray()
                } ?: throw IOException("ファイルを開けません")
                base64 = Base64.encodeToString(bytes, Base64.NO_WRAP)
            } catch (e: Throwable) {
                error = e.message ?: e.javaClass.simpleName
            }
            main.post {
                if (isDestroyed) return@post
                if (error != null || base64 == null) {
                    toast("読み込めませんでした: $error")
                    return@post
                }
                val params = org.json.JSONObject().put("data_base64", base64)
                NagiRuntime.get(this).call("import_user_dict", params) { r ->
                    if (r == null) {
                        showMessage(
                            "取り込めませんでした",
                            "コアが取り込みを断りました。形式が読めないか、登録の上限（${maxEntries ?: "不明"} 件）を超える可能性があります。" +
                                "上限を超える取り込みは、一部だけ入れずに全体を断ります。\n\n" +
                                "また、システム辞書が読み込めていないときは取り込めません。",
                        )
                    } else {
                        showMessage("取り込み結果", importSummary(r))
                    }
                    loadMaxEntries()
                    reload()
                }
            }
        }
    }

    private fun importSummary(r: org.json.JSONObject): String {
        val sb = StringBuilder()
        sb.append("形式: ").append(r.optString("format", "不明")).append('\n')
        sb.append("取り込み ").append(r.optInt("imported")).append(" 件 / スキップ ").append(r.optInt("skipped"))
            .append(" 件（全 ").append(r.optInt("total")).append(" 行）")
        val reasons = r.optJSONArray("skipped_reasons")
        if (reasons != null && reasons.length() > 0) {
            sb.append("\n\nスキップの理由:")
            for (i in 0 until reasons.length()) {
                val o = reasons.optJSONObject(i) ?: continue
                sb.append("\n・").append(o.optString("message")).append("（").append(o.optInt("count")).append(" 件、")
                    .append("最初は ").append(o.optInt("first_line")).append(" 行目）")
            }
        }
        sb.append("\n\n取り込んだ内容は読み取り専用の imported.tsv に追記されます。")
        return sb.toString()
    }

    private fun doExport(uri: Uri) {
        val all = ArrayList<UserDictEntry>()
        all.addAll(own)
        others.forEach { all.addAll(it.second) }
        val text = MozcTsv.serialize(all)
        val resolver = contentResolver
        io.execute {
            var error: String? = null
            try {
                (resolver.openOutputStream(uri, "wt") ?: throw IOException("書き出し先を開けません")).use { out ->
                    out.write(text.toByteArray(Charsets.UTF_8))
                }
            } catch (e: Throwable) {
                error = e.message ?: e.javaClass.simpleName
            }
            main.post {
                if (isDestroyed) return@post
                if (error != null) toast("書き出せませんでした: $error") else toast("${all.size} 件を書き出しました（Mozc TSV）")
            }
        }
    }

    private fun showMessage(title: String, message: String) {
        AlertDialog.Builder(this).setTitle(title).setMessage(message).setPositiveButton("閉じる", null).show()
    }

    companion object {
        private const val REQUEST_IMPORT = 2001
        private const val REQUEST_EXPORT = 2002
    }
}
