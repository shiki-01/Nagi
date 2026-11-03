package com.shiki01.nagi

// ホームタブの状態表示の組み立て（文言と色味の決定）。android.* に依存しない純粋なロジック。単体テスト対象（HomeStatusTest）。
// iOS の `NagiSetupStatusSection` / `NagiDataStatusSections`（OnboardingView.swift / DataStatusView.swift）に揃える。

/** 状態行 1 つ分（丸 + タイトル + 値 + 補足） */
internal data class StatusLine(val title: String, val value: String, val tone: Tone, val subtitle: String? = null)

/** 「LLM が使える見込み」の判定 */
internal data class LlmVerdict(val value: String, val detail: String, val tone: Tone)

internal object HomeStatus {
    // ---- セットアップの 4 行 ----

    fun keyboardAdded(enabled: Boolean): StatusLine =
        StatusLine("キーボードの追加", if (enabled) "OK" else "未設定", if (enabled) Tone.SUCCESS else Tone.WARNING)

    fun keyboardSelected(selected: Boolean): StatusLine =
        StatusLine("選択中", if (selected) "OK" else "未選択", if (selected) Tone.SUCCESS else Tone.WARNING)

    fun setupDictionary(exists: Boolean): StatusLine =
        StatusLine("辞書", if (exists) "OK" else "未設定", if (exists) Tone.SUCCESS else Tone.WARNING)

    fun setupModel(count: Int): StatusLine =
        StatusLine("モデル", if (count > 0) "OK" else "未設定", if (count > 0) Tone.SUCCESS else Tone.NEUTRAL)

    // ---- システム辞書 ----

    fun dictionaryState(exists: Boolean): StatusLine =
        StatusLine("状態", if (exists) "あり" else "なし", if (exists) Tone.SUCCESS else Tone.ERROR)

    /** 辞書が無いときの注記 */
    const val DICTIONARY_MISSING_NOTE = "辞書がないと読みをそのまま返します（LLM も読み込まれません）"

    // ---- 変換モデル ----

    fun modelState(count: Int): StatusLine =
        if (count > 0) {
            StatusLine("状態", "あり（$count 個）", Tone.SUCCESS)
        } else {
            StatusLine("状態", "なし", Tone.ERROR, "辞書のみで変換します")
        }

    /** モデルの種類（ファイル名から。xsmall / small / 不明） */
    fun modelKind(fileName: String): String {
        val lower = fileName.lowercase()
        return when {
            lower.contains("xsmall") -> "xsmall"
            lower.contains("small") -> "small"
            else -> "不明"
        }
    }

    /** モデル 1 つの補足（「種類: small / 70.4 MB」） */
    fun modelDetail(fileName: String, sizeBytes: Long): String = "種類: ${modelKind(fileName)} / ${formatBytes(sizeBytes)}"

    // ---- LLM が使える見込み ----

    /**
     * 辞書が無い → モデルが無い → メモリ予算が足りない → 使える見込み、の順に判定する（iOS の `llmVerdict()` と同じ順）。
     * 予算の下限はコアが LLM を読み込む最低値（[ModelRecommendation.CORE_MIN_BUDGET_MB]）
     */
    fun llmVerdict(dictExists: Boolean, modelCount: Int, budgetMb: Long): LlmVerdict = when {
        !dictExists -> LlmVerdict(
            "使えません",
            "辞書がないため LLM は読み込まれません。ホームの「その他」のアップデートで辞書を取得してください",
            Tone.ERROR,
        )
        modelCount <= 0 -> LlmVerdict("使えません", "モデルがありません。辞書のみで変換します", Tone.WARNING)
        budgetMb < ModelRecommendation.CORE_MIN_BUDGET_MB -> LlmVerdict(
            "使えません",
            "メモリ予算の見込みが $budgetMb MB で、${ModelRecommendation.CORE_MIN_BUDGET_MB} MB に届きません。辞書のみで変換します",
            Tone.WARNING,
        )
        else -> LlmVerdict("使える見込み", "使える見込みです（メモリ予算 $budgetMb MB）", Tone.SUCCESS)
    }

    val LLM_VERDICT_NOTE: String =
        "判定の条件: 辞書があり、変換モデルがあり、メモリ予算が ${ModelRecommendation.CORE_MIN_BUDGET_MB} MB 以上のとき。" +
            "実際に使われるかは、設定の「LLM による変換」にも従います。"
}

/** 入力テストの文字数表示 */
internal object InputTestLogic {
    /** 「N 文字」。サロゲートペア（絵文字など）は 1 文字と数える */
    fun countText(text: CharSequence): String = "${text.toString().let { it.codePointCount(0, it.length) }} 文字"
}
