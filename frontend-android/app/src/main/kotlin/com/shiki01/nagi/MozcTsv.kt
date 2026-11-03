package com.shiki01.nagi

/** ユーザー辞書の 1 行（読み・単語・品詞・コメント） */
internal data class UserDictEntry(
    val reading: String,
    val surface: String,
    val pos: String,
    val comment: String = "",
) {
    val isSnippet: Boolean get() = pos == MozcTsv.SNIPPET_POS
}

/** 読み込み時に無視した行 */
internal data class TsvIssue(val line: Int, val message: String)

internal data class TsvParseResult(val entries: List<UserDictEntry>, val issues: List<TsvIssue>)

/**
 * Mozc TSV（`読み<TAB>単語<TAB>品詞[<TAB>コメント]`）の読み書き。android.* に依存しない純粋関数。
 * コア（`core/dict/src/user.rs`）の `add_tsv` / `to_mozc_tsv` と同じ規則にする。
 */
internal object MozcTsv {
    /** スニペットを表す品詞名（コアの `SNIPPET_POS`） */
    const val SNIPPET_POS = "スニペット"

    /** スニペット本文の上限（文字数。iOS の `maxSnippetChars` と同じ） */
    const val MAX_SNIPPET_CHARS = 256

    /** 自分で編集する 1 ファイルの名前（他のファイルは読み取り専用で表示する） */
    const val OWN_FILE_NAME = "nagi-user.tsv"

    /** 編集できる件数の上限（iOS の `maxOwnEntries` と同じ） */
    const val MAX_OWN_ENTRIES = 10000

    /** 取り込むファイルの大きさの上限（バイト） */
    const val MAX_IMPORT_BYTES = 16 * 1024 * 1024

    /** 画面で選べる品詞（コアの `pos_patterns` が対応づける Mozc の品詞名） */
    val POS_CHOICES: List<String> = listOf(
        "名詞", "固有名詞", "人名", "姓", "名", "地名", "組織", "顔文字", "記号",
        "副詞", "接続詞", "感動詞", "連体詞", "形容詞", "名詞サ変",
    )

    /** カタカナをひらがなにする（長音などはそのまま） */
    fun toHiragana(s: String): String {
        val sb = StringBuilder(s.length)
        for (c in s) {
            if (c in '\u30a1'..'\u30f6') sb.append((c.code - 0x60).toChar()) else sb.append(c)
        }
        return sb.toString()
    }

    /** `\` `改行` `タブ` をエスケープする（スニペットの本文用） */
    fun escape(s: String): String =
        s.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t")

    /** [escape] の逆。未知のエスケープはそのまま残す（コアの `unescape` と同じ） */
    fun unescape(s: String): String {
        val sb = StringBuilder(s.length)
        var i = 0
        while (i < s.length) {
            val c = s[i]
            if (c != '\\') {
                sb.append(c)
                i++
                continue
            }
            if (i + 1 >= s.length) {
                sb.append('\\')
                break
            }
            when (val n = s[i + 1]) {
                'n' -> sb.append('\n')
                't' -> sb.append('\t')
                '\\' -> sb.append('\\')
                else -> sb.append('\\').append(n)
            }
            i += 2
        }
        return sb.toString()
    }

    /** バイト列を文字列にする。UTF-8（BOM 付き可）と UTF-16 LE/BE（BOM 付き。MS-IME のエクスポート）に対応 */
    fun decode(bytes: ByteArray): String {
        fun b(i: Int) = bytes[i].toInt() and 0xff
        if (bytes.size >= 2 && b(0) == 0xff && b(1) == 0xfe) {
            return String(bytes, 2, bytes.size - 2, Charsets.UTF_16LE)
        }
        if (bytes.size >= 2 && b(0) == 0xfe && b(1) == 0xff) {
            return String(bytes, 2, bytes.size - 2, Charsets.UTF_16BE)
        }
        if (bytes.size >= 3 && b(0) == 0xef && b(1) == 0xbb && b(2) == 0xbf) {
            return String(bytes, 3, bytes.size - 3, Charsets.UTF_8)
        }
        return String(bytes, Charsets.UTF_8)
    }

    /** テキストを解析する。不正な行は [TsvParseResult.issues] に記録して読み飛ばす（行番号は 1 始まり） */
    fun parse(text: String): TsvParseResult {
        val entries = ArrayList<UserDictEntry>()
        val issues = ArrayList<TsvIssue>()
        text.split('\n').forEachIndexed { i, raw ->
            val line = raw.trimEnd('\r')
            if (line.isBlank() || line.startsWith("#") || line.startsWith("!")) return@forEachIndexed
            val cols = line.split('\t')
            if (cols.size < 3) {
                issues.add(TsvIssue(i + 1, "列が 3 つ未満です（読み・単語・品詞が必要です）"))
                return@forEachIndexed
            }
            val reading = toHiragana(cols[0].trim())
            val pos = cols[2].trim()
            val surface = if (pos == SNIPPET_POS) unescape(cols[1]) else cols[1].trim()
            if (reading.isEmpty() || surface.isEmpty()) {
                issues.add(TsvIssue(i + 1, "読みまたは単語が空です"))
                return@forEachIndexed
            }
            entries.add(UserDictEntry(reading, surface, pos, cols.getOrNull(3)?.trim() ?: ""))
        }
        return TsvParseResult(entries, issues)
    }

    /** Mozc TSV にする（スニペットの本文はエスケープ）。1 行 1 エントリ、末尾は改行 */
    fun serialize(entries: List<UserDictEntry>): String {
        val sb = StringBuilder()
        for (e in entries) {
            val surface = if (e.isSnippet) escape(e.surface) else e.surface
            sb.append(e.reading).append('\t')
                .append(surface).append('\t')
                .append(e.pos).append('\t')
                .append(e.comment).append('\n')
        }
        return sb.toString()
    }

    /** 画面から入力されたエントリの検査。問題があればその説明（日本語）、なければ null */
    fun validate(e: UserDictEntry): String? {
        if (e.reading.isBlank()) return "読みを入力してください"
        if (e.surface.isBlank()) return if (e.isSnippet) "本文を入力してください" else "単語を入力してください"
        if (e.pos.isBlank()) return "品詞を選んでください"
        if (e.reading.any { it == '\t' || it == '\n' || it == '\r' }) return "読みに改行・タブは使えません"
        if (e.isSnippet) {
            if (e.surface.length > MAX_SNIPPET_CHARS) {
                return "本文は $MAX_SNIPPET_CHARS 文字以内にしてください（現在 ${e.surface.length} 文字）"
            }
        } else if (e.surface.any { it == '\t' || it == '\n' || it == '\r' }) {
            return "単語に改行・タブは使えません（改行を含む定型文はスニペットにしてください）"
        }
        return null
    }

    /** 保存前の正規化: 読みをひらがなにし、読み・単語（スニペット以外）・コメントの前後の空白とタブ・改行を整える */
    fun normalize(e: UserDictEntry): UserDictEntry {
        fun flat(s: String) = s.replace('\t', ' ').replace('\r', ' ').replace('\n', ' ').trim()
        return UserDictEntry(
            reading = toHiragana(flat(e.reading)),
            surface = if (e.isSnippet) e.surface.replace("\r\n", "\n").replace('\r', '\n') else flat(e.surface),
            pos = flat(e.pos),
            comment = flat(e.comment),
        )
    }

    /** 検索語に合うか（読み・単語・コメントの部分一致。読みはカタカナ入力もひらがなとして比較） */
    fun matches(e: UserDictEntry, query: String): Boolean {
        val q = query.trim()
        if (q.isEmpty()) return true
        val qh = toHiragana(q).lowercase()
        return e.reading.contains(qh) ||
            e.surface.lowercase().contains(q.lowercase()) ||
            e.comment.lowercase().contains(q.lowercase())
    }
}
