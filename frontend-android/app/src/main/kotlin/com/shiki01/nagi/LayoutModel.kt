package com.shiki01.nagi

// キーボードレイアウト（schema 2）のデータモデル。android.* に依存しない純粋な Kotlin（単体テスト対象）。
// 仕様: docs/ios-keyboard-layout.md / config/keyboard/layout.schema.json。iOS 実装（KeyboardLayout.swift）に揃える。
//
// 実装範囲: id / name / mode / rows（height）/ keys（kind 7 種・width・label・icon・role・longPress）。
// `look` / `roles` / `rules` / `states` / `metrics` / `background` / `assets` / `toolbar` / `visible` は
// 構文としては受け入れる（無視する）が、描画には使わない。

/** レイアウトの用途 */
internal enum class LayoutMode(val json: String) {
    KANA("kana"),
    ALPHA("alpha"),
    NUMBER("number"),
    SYMBOL("symbol"),
    EMOJI("emoji"),
    CUSTOM("custom"),
    ;

    companion object {
        fun fromJson(s: String?): LayoutMode? = values().firstOrNull { it.json == s }
    }
}

/** キーの色の役割（実際の色はテーマ側で決める） */
internal enum class KeyRole(val json: String) {
    NORMAL("normal"),
    FUNCTION("function"),
    ACCENT("accent"),
    ;

    companion object {
        fun fromJson(s: String?): KeyRole? = values().firstOrNull { it.json == s }
    }
}

/** `shortcut` の 1 手順。[action] か [text] のどちらか一方だけが入る */
internal data class ShortcutStep(val action: String?, val text: String?)

/** キーを押したときの動作（kind ごと） */
internal sealed class KeyAction {
    /** 文字を送る。[shift] は英字モードの Shift 中に送る文字（無ければ大文字化） */
    data class Text(val text: String, val shift: String?) : KeyAction()

    /**
     * フリック。[kana] は 12 キー名（`あ` `か`…。コアの `get_key_table` から文字表を引く）。
     * [kanaKey] はそのキー名に対応するコアのキー ID（`a` `ka`…）。kana が無いときは各方向の文字をそのまま送る
     */
    data class Flick(
        val kana: String?,
        val kanaKey: String?,
        val center: String?,
        val left: String?,
        val up: String?,
        val right: String?,
        val down: String?,
    ) : KeyAction()

    /** ガラケー式の循環入力 */
    data class Toggle(val cycle: List<String>) : KeyAction()

    /** 操作（`space` `enter` `backspace`…）。名前の一覧は [LayoutLimits.KNOWN_ACTIONS] */
    data class Act(val name: String) : KeyAction()

    /** 手順の連続実行 */
    data class Shortcut(val steps: List<ShortcutStep>) : KeyAction()

    /** 別レイアウトへ切り替える（`kana` / `alpha` は割り当て済みのレイアウト） */
    data class LayoutSwitch(val target: String) : KeyAction()

    /** 空白（幅だけ使う） */
    object Spacer : KeyAction()
}

/** 長押しポップアップの 1 項目（kind は text / action / shortcut のみ） */
internal data class PopupItem(val id: String?, val label: String, val icon: String?, val action: KeyAction)

internal data class LayoutKey(
    val id: String,
    /** 行内の幅の比 */
    val width: Double,
    val label: String?,
    val icon: String?,
    val role: KeyRole,
    val action: KeyAction,
    val longPress: List<PopupItem>,
)

internal data class LayoutRow(
    /** 行の高さの比（基準の高さに掛ける） */
    val height: Double,
    val keys: List<LayoutKey>,
)

internal data class LayoutModel(
    val id: String,
    val name: String,
    val mode: LayoutMode,
    val rows: List<LayoutRow>,
) {
    val keyCount: Int get() = rows.sumOf { it.keys.size }
}

/** 検証の上限値・固定の名前（iOS `LayoutValidation` / web `LIMITS` と同じ値。文字数はコードポイント数） */
internal object LayoutLimits {
    const val SCHEMA = 2
    const val ROWS_MAX = 8
    const val KEYS_PER_ROW_MAX = 24
    const val ROW_WIDTH_MIN = 1.0
    const val ROW_WIDTH_MAX = 20.0
    const val ROW_HEIGHT_MAX = 4.0

    /** 行の高さの比の合計の上限（schema の JSON 単体には無い。iOS と web にはある） */
    const val ROW_HEIGHT_SUM_MAX = 4.5
    const val WIDTH_MAX = 20.0
    const val TEXT_MAX = 16
    const val SHORTCUT_TEXT_MAX = 256
    const val CYCLE_MAX = 16
    const val STEPS_MAX = 32
    const val LONG_PRESS_MAX = 12
    const val NAME_MAX = 64
    const val LABEL_MAX = 16
    const val ICON_MAX = 64
    const val KANA_MAX = 4

    /** 取り込む JSON 全体の上限（バイト） */
    const val JSON_BYTES_MAX = 1536 * 1024

    val LAYOUT_ID = Regex("[a-z0-9][a-z0-9_-]{0,63}")
    val KEY_ID = Regex("[A-Za-z0-9][A-Za-z0-9_.-]{0,31}")
    val ACTION = Regex("[a-zA-Z][a-zA-Z0-9]{0,31}")

    /** `icon` の書式（`sf:` / `iconify:` / 接頭辞なし） */
    val ICON_SF = Regex("sf:[A-Za-z0-9._-]+")
    val ICON_ICONIFY = Regex("iconify:[a-z0-9]+(-[a-z0-9]+)*:[a-z0-9]+(-[a-z0-9]+)*")
    val ICON_PLAIN = Regex("[A-Za-z0-9._-]+")

    /** `layout.target` に使える組み込み名（`kana` / `alpha` は割り当て済みのレイアウト） */
    val BUILTIN_TARGETS: List<String> = listOf(
        "qwerty-kana", "qwerty-alpha", "flick-kana", "flick-alpha", "symbols", "numbers", "emoji", "kana", "alpha",
    )

    /** `shortcut` の手順などで実行できる操作の名前 */
    val KNOWN_ACTIONS: Set<String> = setOf(
        "space", "enter", "convert", "moveLeft", "moveRight", "backspace", "shift", "capslock", "dakuten",
        "smallKana", "modeSwitch", "globe", "dismiss", "paste", "deleteWord", "history",
    )

    /** `flick.kana` の先頭 1 文字 → コアの `get_key_table` のキー ID */
    val KANA_KEY_IDS: Map<Char, String> = mapOf(
        'あ' to "a", 'か' to "ka", 'さ' to "sa", 'た' to "ta", 'な' to "na",
        'は' to "ha", 'ま' to "ma", 'や' to "ya", 'ら' to "ra", 'わ' to "wa", '、' to "punct",
    )
}

/** レイアウト ID にまつわる固定値と判定 */
internal object LayoutIds {
    const val DEFAULT_KANA = "flick-kana"
    const val DEFAULT_ALPHA = "qwerty-alpha"
    const val NUMBERS = "numbers"
    const val SYMBOLS = "symbols"
    const val EMOJI = "emoji"

    /** プリセットの並び（一覧の表示順）。これに無いものは名前順で後ろ */
    val PRESET_ORDER: List<String> = listOf(
        "flick-kana", "qwerty-kana", "flick-alpha", "qwerty-alpha", "toggle-alpha", "numbers", "symbols", "emoji",
    )

    /** ファイル名に使える ID か（パス区切り等を含まない） */
    fun isValid(id: String): Boolean = LayoutLimits.LAYOUT_ID.matches(id)

    fun defaultFor(kana: Boolean): String = if (kana) DEFAULT_KANA else DEFAULT_ALPHA
}
