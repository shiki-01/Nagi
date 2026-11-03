package com.shiki01.nagi

import android.text.InputType
import android.view.inputmethod.EditorInfo

// 入力欄の種類によるレイアウトの自動選択と、改行キーのラベル決定（純粋関数。単体テスト対象）。
// 使っている android.* は定数だけ（コンパイル時に埋め込まれるので、JVM の単体テストでも動く）。
// iOS の `keyboard.layout_by_field`（InputTraits.swift）に相当し、UIKeyboardType を EditorInfo.inputType に置き換えた。

/** 入力欄の種類（レイアウトの自動選択の単位） */
internal enum class FieldKind(val key: String, val label: String) {
    /** 通常のテキスト（自動選択しない） */
    NORMAL("normal", "通常のテキスト"),
    EMAIL("email", "メールアドレス"),
    URL("url", "URL"),
    NUMBER("number", "数字"),
    PHONE("phone", "電話番号"),
    PASSWORD("password", "パスワード"),
    DATETIME("datetime", "日付・時刻"),
    ;

    companion object {
        fun fromKey(key: String?): FieldKind? = values().firstOrNull { it.key == key }

        /**
         * `EditorInfo.inputType` から入力欄の種類を決める。
         * 数字だけのパスワード（PIN）は [NUMBER] とする（英字配列に切り替えると入力できないため）
         */
        fun fromInputType(inputType: Int): FieldKind {
            val cls = inputType and InputType.TYPE_MASK_CLASS
            val variation = inputType and InputType.TYPE_MASK_VARIATION
            return when (cls) {
                InputType.TYPE_CLASS_NUMBER -> NUMBER
                InputType.TYPE_CLASS_PHONE -> PHONE
                InputType.TYPE_CLASS_DATETIME -> DATETIME
                InputType.TYPE_CLASS_TEXT -> when (variation) {
                    InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
                    InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS,
                    -> EMAIL
                    InputType.TYPE_TEXT_VARIATION_URI -> URL
                    InputType.TYPE_TEXT_VARIATION_PASSWORD,
                    InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
                    InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD,
                    -> PASSWORD
                    else -> NORMAL
                }
                else -> NORMAL
            }
        }
    }
}

/** かな / 英字のどちらに割り当てるか */
internal enum class AssignSlot { KANA, ALPHA }

internal object LayoutSelect {
    /** 入力欄別の割り当てで「自動選択しない」を表す値 */
    const val FIELD_NONE = "none"

    /** `kana` / `alpha`（割り当て済みのレイアウト）を表す切替先の名前 */
    const val TARGET_KANA = "kana"
    const val TARGET_ALPHA = "alpha"

    /** 入力欄の種類ごとの既定の切替先（null は自動選択しない）。iOS の対応表に合わせる */
    fun defaultTarget(kind: FieldKind): String? = when (kind) {
        FieldKind.NORMAL -> null
        FieldKind.EMAIL, FieldKind.URL, FieldKind.PASSWORD -> TARGET_ALPHA
        FieldKind.NUMBER, FieldKind.PHONE, FieldKind.DATETIME -> LayoutIds.NUMBERS
    }

    /**
     * 切替先を決める。[assignments] に利用者の割り当てがあればそれ（[FIELD_NONE] なら選択しない）、無ければ既定。
     * 戻り値は `kana` / `alpha` / レイアウト ID / null（選択しない）
     */
    fun resolveFieldTarget(kind: FieldKind, assignments: Map<FieldKind, String>): String? {
        val v = assignments[kind] ?: return defaultTarget(kind)
        return if (v == FIELD_NONE || v.isEmpty()) null else v
    }

    /** 切替先が指す入力モード。かな = true、英字 = false、それ以外（数字など）は null（モードは変えない） */
    fun kanaModeOfTarget(target: String): Boolean? = when (target) {
        TARGET_KANA, "flick-kana", "qwerty-kana" -> true
        TARGET_ALPHA, "flick-alpha", "qwerty-alpha", "toggle-alpha" -> false
        else -> null
    }

    /** レイアウトの用途に対する入力モード（かな = true、英字 = false、他は null） */
    fun kanaModeOf(mode: LayoutMode): Boolean? = when (mode) {
        LayoutMode.KANA -> true
        LayoutMode.ALPHA -> false
        else -> null
    }

    /**
     * 候補バーの ⇄ ボタンで切り替える先。かな: 12 キー ⇄ QWERTY、英字: QWERTY ⇄ 12 キー。
     * それ以外のレイアウトでは null（何もしない）
     */
    fun toggleTarget(mode: LayoutMode, currentId: String): String? = when (mode) {
        LayoutMode.KANA -> if (currentId == "qwerty-kana") "flick-kana" else "qwerty-kana"
        LayoutMode.ALPHA -> if (currentId == "qwerty-alpha") "flick-alpha" else "qwerty-alpha"
        else -> null
    }

    /**
     * 改行キーに出す文字を `EditorInfo.imeOptions` から決める。null ならレイアウトのラベルのまま。
     * 検索・送信・次へ・完了・移動など。`IME_FLAG_NO_ENTER_ACTION`（改行を入れる欄）では null
     */
    fun enterLabel(imeOptions: Int): String? {
        if ((imeOptions and EditorInfo.IME_FLAG_NO_ENTER_ACTION) != 0) return null
        return when (imeOptions and EditorInfo.IME_MASK_ACTION) {
            EditorInfo.IME_ACTION_SEARCH -> "検索"
            EditorInfo.IME_ACTION_SEND -> "送信"
            EditorInfo.IME_ACTION_NEXT -> "次へ"
            EditorInfo.IME_ACTION_DONE -> "完了"
            EditorInfo.IME_ACTION_GO -> "移動"
            EditorInfo.IME_ACTION_PREVIOUS -> "前へ"
            else -> null
        }
    }
}
