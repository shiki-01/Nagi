package com.shiki01.nagi

import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Locale

// クリップボード履歴（iOS の `Shared/ClipboardHistory.swift`、docs/decisions.md H-69 に揃える）。
// 種類の判定・刈り込み・表示用の整形・取り込みの判定・保存形式は android.* に依存しない（単体テスト対象）。
// ファイルへの保存（ClipboardHistoryStore）も java.io のみ。クリップボードの読み取りは ClipboardCapture.kt。
// プライバシー: 既定はオフ。内容は端末内の filesDir にだけ置き、ログ（DiagLog）には出さない。

/** 履歴に記録する文字列の種類。[json] が保存される名前 */
internal enum class ClipboardKind(val json: String, val title: String, val glyph: String) {
    /** その他の文字列 */
    TEXT("text", "文字列", "Aa"),
    URL("url", "URL", "🔗"),

    /** 数字・記号のみ（電話番号・コードなど） */
    NUMBER("number", "数字・記号", "#"),
    EMAIL("email", "メールアドレス", "✉"),
    ;

    companion object {
        /** 知らない種類（新しい版が書いた値）は text として読む */
        fun fromJson(s: String?): ClipboardKind = values().firstOrNull { it.json == s } ?: TEXT

        /** 設定の保存値から。名前が分からないものは捨てる */
        fun fromJsonOrNull(s: String): ClipboardKind? = values().firstOrNull { it.json == s }

        private val EMAIL_PATTERN = Regex("[A-Za-z0-9._%+\\-]+@[A-Za-z0-9\\-]+(\\.[A-Za-z0-9\\-]+)+")

        /** 数字と並べてよい記号（電話番号・郵便番号・金額・コードなど） */
        private const val NUMBER_SYMBOLS = "+-−－ー()（）.,．，/／:#*%％\$¥￥ 　"

        /**
         * 前後の空白・改行を除いた全体で判定する。
         * URL は http / https で始まり空白を含まない、email は簡易パターン、number は数字と電話番号などの記号のみで数字を含む
         */
        fun classify(text: String): ClipboardKind {
            val trimmed = text.trim()
            if (trimmed.isEmpty()) return TEXT
            val hasWhitespace = trimmed.any { it.isWhitespace() }
            if (!hasWhitespace) {
                val lower = trimmed.lowercase(Locale.ROOT)
                for (scheme in listOf("http://", "https://")) {
                    if (lower.startsWith(scheme) && lower.length > scheme.length) return URL
                }
                if (EMAIL_PATTERN.matches(trimmed)) return EMAIL
            }
            if (isNumberLike(trimmed)) return NUMBER
            return TEXT
        }

        private fun isNumberLike(text: String): Boolean {
            var hasDigit = false
            var i = 0
            while (i < text.length) {
                val cp = text.codePointAt(i)
                i += Character.charCount(cp)
                if (Character.isDigit(cp)) {
                    hasDigit = true
                } else if (cp > 0xFFFF || NUMBER_SYMBOLS.indexOf(cp.toChar()) < 0) {
                    return false
                }
            }
            return hasDigit
        }
    }
}

/** 履歴の 1 件。[id] は UUID 文字列、[copiedAt] は取り込んだ時刻（epoch ミリ秒） */
internal data class ClipboardEntry(
    val id: String,
    val text: String,
    val kind: ClipboardKind,
    val copiedAt: Long,
    val pinned: Boolean = false,
)

/** 履歴の設定を、ミリ秒・件数・種類の集合にしたもの */
internal data class ClipboardConfig(
    val enabled: Boolean,
    /** 保存期間（ミリ秒）。null は無期限 */
    val retentionMs: Long?,
    val maxItems: Int,
    val kinds: Set<ClipboardKind>,
    /** カードに出す最大文字数 */
    val previewChars: Int,
)

/** 既定値と選択肢（iOS の `IOSSettings.swift` と同じ。設定画面・保存の読み出しが共通で使う） */
internal object ClipboardDefaults {
    const val ENABLED = false
    const val RETENTION = "24h"
    const val MAX_ITEMS = 50
    const val PREVIEW_CHARS = 40

    val RETENTION_CHOICES: List<Pair<String, String>> = listOf(
        "1h" to "1 時間", "24h" to "24 時間", "7d" to "7 日", "30d" to "30 日", "forever" to "無期限",
    )
    val MAX_ITEMS_CHOICES: List<Pair<String, String>> =
        listOf("10" to "10 件", "30" to "30 件", "50" to "50 件", "100" to "100 件", "200" to "200 件")
    val PREVIEW_CHOICES: List<Pair<String, String>> =
        listOf("20" to "20 文字", "40" to "40 文字", "80" to "80 文字", "120" to "120 文字")

    const val MAX_ITEMS_MIN = 10
    const val MAX_ITEMS_MAX = 200
}

internal object ClipboardHistoryLogic {
    /** 1 件の最大文字数（超えたものは記録しない） */
    const val MAX_TEXT_LENGTH = 10_000

    /** ピン留めの上限 */
    const val MAX_PINNED = 20

    private const val HOUR = 3600L * 1000L

    /** 保存期間の選択肢の値 → ミリ秒。"forever" は null（無期限）。知らない値は 24 時間 */
    fun retentionMs(value: String?): Long? = when (value) {
        "1h" -> HOUR
        "24h" -> 24 * HOUR
        "7d" -> 7 * 24 * HOUR
        "30d" -> 30 * 24 * HOUR
        "forever" -> null
        else -> 24 * HOUR
    }

    /** 保存件数の選択肢の値 → 件数（10〜200 に丸める。読めなければ既定） */
    fun maxItems(value: String?): Int =
        (value?.toIntOrNull() ?: ClipboardDefaults.MAX_ITEMS).coerceIn(ClipboardDefaults.MAX_ITEMS_MIN, ClipboardDefaults.MAX_ITEMS_MAX)

    /** カードの文字数の選択肢の値 → 文字数（1 以上。読めなければ既定） */
    fun previewChars(value: String?): Int = (value?.toIntOrNull() ?: ClipboardDefaults.PREVIEW_CHARS).coerceAtLeast(1)

    /** 保存する種類の保存値（"text,url"）→ 集合。未設定（null）は全種類。名前が分からないものは捨てる */
    fun parseKinds(stored: String?): Set<ClipboardKind> {
        if (stored == null) return ClipboardKind.values().toSet()
        return stored.split(",").mapNotNull { ClipboardKind.fromJsonOrNull(it.trim()) }.toSet()
    }

    /** 保存する種類の保存値。宣言順で並べる（空集合は空文字 = 何も保存しない） */
    fun encodeKinds(kinds: Set<ClipboardKind>): String =
        ClipboardKind.values().filter { it in kinds }.joinToString(",") { it.json }

    /** 設定画面の表示: 全部なら「すべて」、無ければ「なし」 */
    fun kindsSummary(kinds: Set<ClipboardKind>): String = when {
        kinds.containsAll(ClipboardKind.values().toList()) -> "すべて"
        kinds.isEmpty() -> "なし"
        else -> ClipboardKind.values().filter { it in kinds }.joinToString("、") { it.title }
    }

    /** 期限切れ（ピン留め以外）と、保存件数を超えた分（ピン留め以外の古い順）を除く。残りの並びは保つ */
    fun prune(items: List<ClipboardEntry>, now: Long, config: ClipboardConfig): List<ClipboardEntry> {
        val retention = config.retentionMs
        val alive = items.filter { it.pinned || retention == null || now - it.copiedAt <= retention }
        val limit = config.maxItems.coerceAtLeast(0)
        val unpinned = alive.filter { !it.pinned }
        if (unpinned.size <= limit) return alive
        val keep = unpinned.sortedByDescending { it.copiedAt }.take(limit).map { it.id }.toSet()
        return alive.filter { it.pinned || it.id in keep }
    }

    /** 記録できる文字列か（空白のみ・長すぎるもの・許可されていない種類は不可）。可能なら種類を返す */
    fun acceptedKind(text: String, config: ClipboardConfig): ClipboardKind? {
        if (!config.enabled || text.length > MAX_TEXT_LENGTH || text.isBlank()) return null
        val kind = ClipboardKind.classify(text)
        return if (kind in config.kinds) kind else null
    }

    /**
     * 1 件記録した結果。同一文字列は先頭へ移して日時を更新する（ID・ピン留めは保つ）。そのあと刈り込む。
     * 記録できない文字列は [items] のまま。[newId] は新しい項目の ID
     */
    fun recorded(
        items: List<ClipboardEntry>,
        text: String,
        now: Long,
        config: ClipboardConfig,
        newId: () -> String,
    ): List<ClipboardEntry> {
        val kind = acceptedKind(text, config) ?: return items
        val existing = items.firstOrNull { it.text == text }
        val entry = ClipboardEntry(
            id = existing?.id ?: newId(),
            text = text,
            kind = kind,
            copiedAt = now,
            pinned = existing?.pinned ?: false,
        )
        val result = ArrayList<ClipboardEntry>(items.size + 1)
        result.add(entry)
        for (e in items) if (e.text != text) result.add(e)
        return prune(result, now, config)
    }

    /** 表示順: ピン留めが先頭、続けて新しい順（同時刻は元の並びを保つ） */
    fun ordered(items: List<ClipboardEntry>): List<ClipboardEntry> =
        items.sortedWith(compareByDescending<ClipboardEntry> { it.pinned }.thenByDescending { it.copiedAt })

    /** ピン留めを切り替える。ピン留めは [MAX_PINNED] 件まで（超えるときは変えない） */
    fun togglePin(items: List<ClipboardEntry>, id: String): List<ClipboardEntry> {
        val target = items.firstOrNull { it.id == id } ?: return items
        if (!target.pinned && items.count { it.pinned } >= MAX_PINNED) return items
        return items.map { if (it.id == id) it.copy(pinned = !it.pinned) else it }
    }

    /** カードの本文。[limit] 文字（コードポイント）を超えた分は「…」で省略する */
    fun preview(text: String, limit: Int): String {
        val n = limit.coerceAtLeast(1)
        if (text.codePointCount(0, text.length) <= n) return text
        return text.substring(0, text.offsetByCodePoints(0, n)) + "…"
    }

    /** 相対時刻（「3 分前」など）。未来の時刻（時計のずれ）は「たった今」 */
    fun relativeTime(from: Long, now: Long): String {
        val seconds = (now - from) / 1000L
        if (seconds < 60) return "たった今"
        val minutes = seconds / 60
        if (minutes < 60) return "$minutes 分前"
        val hours = minutes / 60
        if (hours < 24) return "$hours 時間前"
        return "${hours / 24} 日前"
    }

    /** 履歴ページの表示内容の種別 */
    enum class PageState(val message: String?) {
        DISABLED("設定でクリップボード履歴をオンにしてください"),
        SECURE("パスワード欄では履歴は使えません"),
        EMPTY("履歴はありません"),
        ITEMS(null),
    }

    fun pageState(enabled: Boolean, secure: Boolean, itemCount: Int): PageState = when {
        !enabled -> PageState.DISABLED
        secure -> PageState.SECURE
        itemCount == 0 -> PageState.EMPTY
        else -> PageState.ITEMS
    }
}

/** `clipboard-history.json` の保存形式（スキーマ版つき） */
internal object ClipboardHistoryCodec {
    const val SCHEMA = 1

    fun encode(items: List<ClipboardEntry>): String {
        val arr = JSONArray()
        for (e in items) {
            arr.put(
                JSONObject()
                    .put("id", e.id)
                    .put("text", e.text)
                    .put("kind", e.kind.json)
                    .put("copiedAt", e.copiedAt)
                    .put("pinned", e.pinned),
            )
        }
        return JSONObject().put("schema", SCHEMA).put("items", arr).toString()
    }

    /** 壊れた内容・読めない項目は空 / 飛ばす（データを落とさない範囲で読む） */
    fun decode(json: String?): List<ClipboardEntry> {
        if (json.isNullOrBlank()) return emptyList()
        val root = try {
            JSONObject(json)
        } catch (e: org.json.JSONException) {
            return emptyList()
        }
        val arr = root.optJSONArray("items") ?: return emptyList()
        val out = ArrayList<ClipboardEntry>(arr.length())
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            val id = o.optString("id", "")
            if (id.isEmpty() || !o.has("text")) continue
            out.add(
                ClipboardEntry(
                    id = id,
                    text = o.optString("text", ""),
                    kind = ClipboardKind.fromJson(o.optString("kind", "")),
                    copiedAt = o.optLong("copiedAt", 0L),
                    pinned = o.optBoolean("pinned", false),
                ),
            )
        }
        return out
    }
}

/**
 * `filesDir/clipboard-history.json` への保存。すべての更新は「ロックを取る → 読み直す → 変更 → 原子的に書く」。
 * 設定画面（本体アプリ）とキーボード（IME サービス）は同じプロセスなので、プロセス内の共有ロックで足りる
 */
internal class ClipboardHistoryStore(private val file: File) {
    companion object {
        const val FILE_NAME = "clipboard-history.json"
        private val LOCK = Any()
    }

    private fun load(): List<ClipboardEntry> =
        try {
            if (file.exists()) ClipboardHistoryCodec.decode(file.readText(Charsets.UTF_8)) else emptyList()
        } catch (e: java.io.IOException) {
            emptyList()
        }

    /** 一時ファイルに書いてから置き換える（書き込み中に落ちても元のファイルは壊れない） */
    private fun write(items: List<ClipboardEntry>): Boolean {
        return try {
            file.parentFile?.mkdirs()
            val tmp = File(file.path + ".tmp")
            tmp.writeText(ClipboardHistoryCodec.encode(items), Charsets.UTF_8)
            Files.move(tmp.toPath(), file.toPath(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE)
            true
        } catch (e: java.io.IOException) {
            false
        } catch (e: UnsupportedOperationException) {
            false
        }
    }

    /** 読み直して [change] を適用し、変わっていれば書く。書いたかを返す */
    private fun mutate(change: (List<ClipboardEntry>) -> List<ClipboardEntry>): Boolean = synchronized(LOCK) {
        val before = load()
        val after = change(before)
        if (after == before) false else write(after)
    }

    /** 記録する。記録できない文字列（空白のみ・許可されていない種類・長すぎる）は無視して false */
    fun record(text: String, now: Long, config: ClipboardConfig): Boolean {
        if (ClipboardHistoryLogic.acceptedKind(text, config) == null) return false
        return mutate { ClipboardHistoryLogic.recorded(it, text, now, config) { java.util.UUID.randomUUID().toString() } }
    }

    /** 期限切れと保存件数の超過を消す（変わったときだけ書く） */
    fun prune(now: Long, config: ClipboardConfig) {
        mutate { ClipboardHistoryLogic.prune(it, now, config) }
    }

    fun togglePin(id: String): Boolean = mutate { ClipboardHistoryLogic.togglePin(it, id) }

    fun delete(id: String) {
        mutate { items -> items.filter { it.id != id } }
    }

    /** 履歴をすべて消す（ピン留めも） */
    fun clear() {
        mutate { emptyList() }
    }

    /** 表示順の項目（ピン留めが先頭、続けて新しい順）。[config] を渡すと期限切れを除く（ファイルは書き換えない） */
    fun items(now: Long, config: ClipboardConfig? = null): List<ClipboardEntry> = synchronized(LOCK) {
        var items = load()
        if (config != null) items = ClipboardHistoryLogic.prune(items, now, config)
        ClipboardHistoryLogic.ordered(items)
    }
}
