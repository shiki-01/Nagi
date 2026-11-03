package com.shiki01.nagi

// TalkBack などが読み上げる、キー・候補・文節列の名前（contentDescription）。android.* に依存しない純粋な処理（単体テスト対象）。

internal object KeyDescriptions {
    /** `action` キーの読み上げ名。知らない操作は null */
    fun actionName(name: String): String? = when (name) {
        "space" -> "空白"
        "enter" -> "改行"
        "backspace" -> "削除"
        "shift" -> "シフト"
        "capslock" -> "キャプスロック"
        "convert" -> "変換"
        "moveLeft" -> "カーソルを左へ"
        "moveRight" -> "カーソルを右へ"
        "dakuten", "smallKana" -> "濁点・小書き"
        "modeSwitch" -> "かなと英字の切り替え"
        "globe" -> "次の入力方法"
        "dismiss" -> "キーボードを閉じる"
        "paste" -> "貼り付け"
        "deleteWord" -> "単語を削除"
        "history" -> "履歴"
        else -> null
    }

    /** 記号などの読み上げ名（TalkBack が読み飛ばしやすいもの） */
    private val SYMBOLS: Map<String, String> = mapOf(
        "、" to "読点", "。" to "句点", "ー" to "長音", "-" to "ハイフン", "," to "カンマ", "." to "ピリオド",
        "?" to "疑問符", "？" to "疑問符", "!" to "感嘆符", "！" to "感嘆符", "…" to "三点リーダー", "~" to "チルダ",
        "〜" to "波ダッシュ", "/" to "スラッシュ", ":" to "コロン", ";" to "セミコロン", "(" to "左かっこ", ")" to "右かっこ",
        "（" to "左かっこ", "）" to "右かっこ", "¥" to "円記号", "&" to "アンド", "@" to "アットマーク", "\"" to "ダブルクォーテーション",
        "'" to "シングルクォーテーション", "「" to "かぎかっこ開く", "」" to "かぎかっこ閉じる", "_" to "アンダースコア",
        "+" to "プラス", "=" to "イコール", "*" to "アスタリスク", "#" to "シャープ", "%" to "パーセント", "<" to "小なり",
        ">" to "大なり", "\\" to "バックスラッシュ", "|" to "縦線", "^" to "キャレット", "`" to "バッククォート",
        "[" to "左角かっこ", "]" to "右角かっこ", "{" to "左波かっこ", "}" to "右波かっこ", "$" to "ドル", "€" to "ユーロ",
        "£" to "ポンド", "・" to "中点", "☆" to "星", "★" to "黒星", "〒" to "郵便記号", "々" to "踊り字",
    )

    /** 12 キーの行の名前（コアのキー ID から） */
    private val ROWS: Map<String, String> = mapOf(
        "a" to "あ行", "ka" to "か行", "sa" to "さ行", "ta" to "た行", "na" to "な行", "ha" to "は行",
        "ma" to "ま行", "ya" to "や行", "ra" to "ら行", "wa" to "わ行", "punct" to "句読点",
    )

    fun symbolName(text: String): String = SYMBOLS[text] ?: text

    /** フリックキー。例: 「か行（タップで か、左フリックで き、上で く、右で け、下で こ）」 */
    fun flick(def: KanaKeyDef): String {
        val row = ROWS[def.key] ?: def.center
        val parts = ArrayList<String>()
        if (def.center.isNotEmpty()) parts.add("タップで ${symbolName(def.center)}")
        if (def.left.isNotEmpty()) parts.add("左フリックで ${symbolName(def.left)}")
        if (def.up.isNotEmpty()) parts.add("上で ${symbolName(def.up)}")
        if (def.right.isNotEmpty()) parts.add("右で ${symbolName(def.right)}")
        if (def.down.isNotEmpty()) parts.add("下で ${symbolName(def.down)}")
        return if (parts.isEmpty()) row else "$row（${parts.joinToString("、")}）"
    }

    /** 組み込み（従来のコード組み立て）のキーの読み上げ名。表示と同じ文字で足りるものは表示のまま */
    fun legacyFace(label: String): String = when (label) {
        "←" -> "カーソルを左へ"
        "→" -> "カーソルを右へ"
        "⌫" -> "削除"
        "⇧" -> "シフト"
        "⇪" -> "キャプスロック"
        "🌐" -> "次の入力方法"
        "゛゜小" -> "濁点・小書き"
        "☆123", "123" -> "数字と記号"
        "あ" -> "かな入力中。英字に切り替え"
        "A" -> "英字入力中。かなに切り替え"
        "あいう" -> "かな入力に戻る"
        "ABC" -> "英字入力に戻る"
        "⏎" -> "改行"
        else -> symbolName(label)
    }

    /** 空白キー: 入力中（かな）は「変換」 */
    fun space(composing: Boolean, kanaMode: Boolean): String = if (composing && kanaMode) "変換" else "空白"

    /** 改行キー: 入力中は「確定」、そうでなければ入力欄の指定（検索・送信など）、無ければ「改行」 */
    fun enter(composing: Boolean, imeLabel: String?): String = if (composing) "確定" else imeLabel ?: "改行"

    /** 切替先レイアウトの読み上げ名 */
    fun layoutTarget(target: String): String? = when (target) {
        LayoutIds.NUMBERS -> "数字"
        LayoutIds.SYMBOLS -> "記号"
        LayoutIds.EMOJI -> "絵文字"
        "kana" -> "かな"
        "alpha" -> "英字"
        "flick-kana" -> "12キーのかな"
        "qwerty-kana" -> "ローマ字のかな"
        "flick-alpha" -> "12キーの英字"
        "qwerty-alpha", "toggle-alpha" -> "英字"
        else -> null
    }

    /**
     * JSON レイアウトの 1 キーの読み上げ名。[flickDef] はフリックキーの文字表、[composing] / [kanaMode] / [enterLabel] は
     * 空白・改行キーの今の名前に使う。[shift] は Shift の状態（シフト / キャプスロックの案内用）
     */
    fun forKey(
        key: LayoutKey,
        flickDef: KanaKeyDef?,
        composing: Boolean,
        kanaMode: Boolean,
        enterLabel: String?,
        shift: ShiftState,
    ): String {
        val base: String = when (val a = key.action) {
            is KeyAction.Flick -> if (flickDef != null) flick(flickDef) else symbolName(a.center ?: key.label ?: "")
            is KeyAction.Text -> symbolName(if (shift != ShiftState.OFF) (a.shift ?: a.text.uppercase()) else a.text)
            is KeyAction.Toggle -> {
                val first = a.cycle.firstOrNull() ?: ""
                if (a.cycle.size >= 2) "$first（続けて押すと ${a.cycle.joinToString(" ") { symbolName(it) }}）" else symbolName(first)
            }
            is KeyAction.Act -> when (a.name) {
                "space" -> space(composing, kanaMode)
                "enter" -> enter(composing, enterLabel)
                "shift" -> when (shift) {
                    ShiftState.OFF -> "シフト"
                    ShiftState.ONCE -> "シフト（次の1文字が大文字）"
                    ShiftState.CAPS -> "シフト（キャプスロック中）"
                }
                "capslock" -> if (shift == ShiftState.CAPS) "キャプスロック（オン）" else "キャプスロック"
                else -> actionName(a.name) ?: key.label ?: a.name
            }
            is KeyAction.Shortcut -> key.label ?: "ショートカット"
            is KeyAction.LayoutSwitch -> {
                val n = layoutTarget(a.target)
                if (n != null) "${n}に切り替え" else (key.label ?: a.target)
            }
            KeyAction.Spacer -> ""
        }
        if (key.longPress.isEmpty() || base.isEmpty()) return base
        return "$base。長押しで ${key.longPress.joinToString(" ") { symbolName(it.label) }}"
    }

    /** 候補バーの 1 候補。[selected] なら「選択中」を添える */
    fun candidate(text: String, index: Int, total: Int, selected: Boolean): String {
        val pos = "${index + 1}/$total"
        return if (selected) "$text、$pos、選択中" else "$text、$pos"
    }

    /** 文節列の 1 文節 */
    fun segment(text: String, index: Int, total: Int, focused: Boolean): String {
        val pos = "文節 ${index + 1}/$total"
        return if (focused) "$text、$pos、注目中" else "$text、$pos"
    }
}
