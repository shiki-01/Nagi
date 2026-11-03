package com.shiki01.nagi

// キーの表示文字・トグル入力・単語削除の判定（android.* に依存しない純粋関数。単体テスト対象）

internal object LayoutLabels {
    /** SF Symbols 名など、`icon` から出せる代替の文字（Android にはアイコン名の対応が無いので文字で代用する） */
    private val GLYPHS: Map<String, String> = mapOf(
        "shift" to "⇧",
        "shift.fill" to "⇧",
        "capslock" to "⇪",
        "capslock.fill" to "⇪",
        "delete.left" to "⌫",
        "delete.backward" to "⌫",
        "delete.left.fill" to "⌫",
        "globe" to "🌐",
        "return" to "⏎",
        "space" to "␣",
        "arrow.left" to "←",
        "arrow.right" to "→",
        "chevron.left" to "←",
        "chevron.right" to "→",
        "keyboard.chevron.compact.down" to "▾",
        "doc.on.clipboard" to "貼付",
        "textformat.abc" to "ABC",
        "mic" to "🎤",
        "face.smiling" to "😀",
    )

    /** `icon`（`sf:名前` / `iconify:..` / 名前のみ）に対応する代替文字。無ければ null */
    fun iconGlyph(icon: String?): String? {
        if (icon == null) return null
        val name = if (icon.startsWith("sf:")) icon.removePrefix("sf:") else icon
        return GLYPHS[name]
    }

    /** キーの静的な表示文字（フリックの kana / center は呼び出し側がコアの表から補う） */
    fun staticLabel(key: LayoutKey): String {
        key.label?.let { return it }
        iconGlyph(key.icon)?.let { return it }
        return when (val a = key.action) {
            is KeyAction.Text -> a.text
            is KeyAction.Flick -> a.center ?: a.kana ?: ""
            is KeyAction.Toggle -> a.cycle.firstOrNull() ?: ""
            is KeyAction.Act -> actionDefaultLabel(a.name)
            is KeyAction.Shortcut -> ""
            is KeyAction.LayoutSwitch -> a.target
            KeyAction.Spacer -> ""
        }
    }

    private fun actionDefaultLabel(name: String): String = when (name) {
        "space" -> "空白"
        "enter" -> "改行"
        "backspace" -> "⌫"
        "shift" -> "⇧"
        "capslock" -> "⇪"
        "convert" -> "変換"
        "moveLeft" -> "←"
        "moveRight" -> "→"
        "dakuten", "smallKana" -> "゛゜小"
        "modeSwitch" -> "あA"
        "globe" -> "🌐"
        "dismiss" -> "▾"
        "paste" -> "貼付"
        "deleteWord" -> "単語削除"
        else -> name
    }

    /** 空白キーの文字。入力中（かな）は「変換」 */
    fun spaceLabel(composing: Boolean, kanaMode: Boolean, keyLabel: String?): String =
        if (composing && kanaMode) "変換" else keyLabel ?: "空白"

    /**
     * 改行キーの文字。入力中は「確定」、そうでなければ入力欄の指定（検索・送信など。[imeLabel]）、
     * それも無ければレイアウトの label（無ければ「改行」）
     */
    fun enterLabel(composing: Boolean, imeLabel: String?, keyLabel: String?): String =
        if (composing) "確定" else imeLabel ?: keyLabel ?: "改行"

    /** 英字モードで Shift 中に送る・出す文字（`shift` 指定があればそれ、無ければ大文字化） */
    fun shifted(a: KeyAction.Text, shiftOn: Boolean): String =
        if (shiftOn) a.shift ?: a.text.uppercase() else a.text

    /** Shift 中の `text` キーの表示文字 */
    fun textKeyFace(key: LayoutKey, a: KeyAction.Text, shiftOn: Boolean): String {
        if (!shiftOn) return staticLabel(key)
        return a.shift ?: (key.label ?: a.text).uppercase()
    }
}

/** トグル入力（同じキーを続けて押すと文字を置き換えて循環する）の判定 */
internal object ToggleLogic {
    /** 既定の継続時間（ミリ秒）。iOS の `input.toggle_timeout_ms` の既定と同じ */
    const val TIMEOUT_MS = 1000L

    /** 前の入力の続きとみなすか。同じキー（[sameKey]）を [timeoutMs] 未満で押し、循環が 2 要素以上あるとき */
    fun continues(cycleSize: Int, sameKey: Boolean, elapsedMs: Long, timeoutMs: Long = TIMEOUT_MS): Boolean =
        sameKey && cycleSize >= 2 && elapsedMs >= 0 && elapsedMs < timeoutMs

    /** 次に出す位置。続きなら前の位置の次（末尾の次は先頭）、そうでなければ 0 */
    fun nextIndex(prevIndex: Int?, cycleSize: Int, sameKey: Boolean, elapsedMs: Long, timeoutMs: Long = TIMEOUT_MS): Int {
        if (prevIndex == null || !continues(cycleSize, sameKey, elapsedMs, timeoutMs)) return 0
        return (prevIndex + 1) % cycleSize
    }
}

internal object TextOps {
    /**
     * カーソル直前の文字列 [before] から「単語削除」で消す UTF-16 コード単位の数。
     * 末尾の空白を飛ばし、その手前の同じ種類の文字（英数字 / 空白以外の記号 / かな・漢字など）の並びを消す
     */
    fun wordDeleteLength(before: CharSequence): Int {
        var i = before.length
        while (i > 0 && before[i - 1].isWhitespace()) i--
        if (i == 0) return before.length
        val kind = charKind(before[i - 1])
        while (i > 0 && !before[i - 1].isWhitespace() && charKind(before[i - 1]) == kind) i--
        return before.length - i
    }

    private fun charKind(c: Char): Int = when {
        c.isLetterOrDigit() && c.code < 0x80 -> 0
        c in '぀'..'ゟ' -> 1
        c in '゠'..'ヿ' -> 2
        c.isLetterOrDigit() -> 3
        else -> 4
    }
}
