package com.shiki01.nagi

// 候補バーのツールバー（入力中でなく、候補も無いときに候補バーの場所に出す操作の列）の、android.* に依存しない部分。
// 項目の定義・名前・既定は iOS の `Shared/ToolbarAction.swift` に揃える（名前 = 設定に保存する値。変えない）。
// 保存形式のパース・検証・並べ替え・出す判定をここに置く（単体テスト対象）。描画は KeyboardView、設定画面は ToolbarItemsActivity。

/** ツールバーの項目。[json] が保存される名前（iOS の `ToolbarAction.rawValue` と同じ）。宣言順が設定画面の選択肢の順 */
internal enum class ToolbarItem(val json: String, val title: String, val glyph: String) {
    /** キーボードを閉じる */
    DISMISS("dismiss", "キーボードを閉じる", "閉じる"),

    /** クリップボードの文字列を貼り付ける（クリップボードに文字があるときだけ出す） */
    PASTE("paste", "貼り付け", "貼付"),

    /** カーソルを 1 文字左へ */
    CURSOR_LEFT("cursorLeft", "カーソルを左へ", "◀"),

    /** カーソルを 1 文字右へ */
    CURSOR_RIGHT("cursorRight", "カーソルを右へ", "▶"),

    /** 改行 */
    NEWLINE("newline", "改行", "⏎"),

    /** 半角スペース */
    SPACE("space", "半角スペース", "空白"),

    /** カーソル手前の単語を削除 */
    DELETE_WORD("deleteWord", "単語を削除", "単語⌫"),

    /** 絵文字のキーボードへ */
    EMOJI("emoji", "絵文字", "😀"),

    /** 次のキーボード（他の入力方法）へ */
    NEXT_KEYBOARD("nextKeyboard", "次のキーボード", "🌐"),

    /** クリップボード履歴を開く */
    HISTORY("history", "履歴", "履歴"),
    ;

    companion object {
        fun fromJson(s: String): ToolbarItem? = values().firstOrNull { it.json == s }
    }
}

internal object ToolbarLogic {
    /** 初期に出す項目と並び順（iOS の `ToolbarAction.defaultItems`） */
    val DEFAULT: List<ToolbarItem> =
        listOf(ToolbarItem.DISMISS, ToolbarItem.PASTE, ToolbarItem.CURSOR_LEFT, ToolbarItem.CURSOR_RIGHT)

    /** 保存形式の区切り */
    private const val SEPARATOR = ","

    /**
     * 保存された文字列（"dismiss,paste,…"）から項目を作る。知らない名前・重複は捨てる（並びは保つ）。
     * 未設定・空・有効な名前が 1 つも無いときは [DEFAULT]（iOS は空配列を「何も出さない」とするが、Android は既定に戻す）
     */
    fun parse(stored: String?): List<ToolbarItem> {
        if (stored.isNullOrBlank()) return DEFAULT
        val seen = LinkedHashSet<ToolbarItem>()
        for (part in stored.split(SEPARATOR)) {
            val item = ToolbarItem.fromJson(part.trim()) ?: continue
            seen.add(item)
        }
        return if (seen.isEmpty()) DEFAULT else seen.toList()
    }

    /** 保存用の文字列。重複は捨てる。空なら空文字（読むときは既定になる） */
    fun encode(items: List<ToolbarItem>): String = items.distinct().joinToString(SEPARATOR) { it.json }

    /** 設定画面に並べる順: 有効な項目（保存された並び）→ 無効な項目（宣言順） */
    fun displayOrder(enabled: List<ToolbarItem>): List<ToolbarItem> {
        val on = enabled.distinct()
        return on + ToolbarItem.values().filter { it !in on }
    }

    /**
     * [item] の有効/無効を切り替える。無効 → 有効は末尾に足す。有効 → 無効は外すが、最後の 1 つは外さない
     * （空は既定に戻る扱いなので、利用者の意図と食い違わないようにする）
     */
    fun toggle(enabled: List<ToolbarItem>, item: ToolbarItem): List<ToolbarItem> {
        val on = enabled.distinct()
        if (item !in on) return on + item
        if (on.size <= 1) return on
        return on - item
    }

    /** 有効な項目 [item] を [delta]（-1 で上、+1 で下）だけ動かす。範囲外・無効な項目は変えない */
    fun move(enabled: List<ToolbarItem>, item: ToolbarItem, delta: Int): List<ToolbarItem> {
        val on = enabled.distinct().toMutableList()
        val from = on.indexOf(item)
        if (from < 0) return on
        val to = from + delta
        if (to < 0 || to >= on.size) return on
        on.removeAt(from)
        on.add(to, item)
        return on
    }

    /** ツールバーを候補バーの場所に出すか。入力中でなく、候補が無く、メッセージも無いとき */
    fun shouldShow(hasPreedit: Boolean, candidatesEmpty: Boolean, hasMessage: Boolean): Boolean =
        !hasPreedit && candidatesEmpty && !hasMessage

    /**
     * 実際に出す項目。「貼り付け」は、設定 `candbar.paste_button` がオンで、クリップボードに文字があるときだけ出す
     * （iOS の `ToolbarKeys.visibleKeys` と同じ）
     */
    fun visible(items: List<ToolbarItem>, pasteButton: Boolean, clipboardHasText: Boolean): List<ToolbarItem> =
        items.filter { it != ToolbarItem.PASTE || (pasteButton && clipboardHasText) }

    /** 設定画面の補足に出す、現在の並び（「キーボードを閉じる → 貼り付け → …」） */
    fun summary(items: List<ToolbarItem>): String = items.joinToString(" → ") { it.title }
}
