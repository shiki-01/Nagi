package com.shiki01.nagi

/** モデルファイル（名前とサイズ）。推奨の計算に使う純粋なデータ */
internal data class ModelSpec(val name: String, val sizeBytes: Long)

/** ダウンロードできる変換モデル（`core/datafetch/src/model.rs` と同じ。コミット固定の URL と SHA-256） */
internal data class ModelSource(
    val id: String,
    val label: String,
    val fileName: String,
    val url: String,
    val sizeBytes: Long,
    val sha256: String,
)

/**
 * 端末のメモリ予算から、載せて余裕のある最大のモデルを推奨する（iOS の `NagiModelRecommendation` と同じ式）。
 * 余裕の式: `予算 − (モデルのサイズ + 20MB) ≥ 40MB`。android.* に依存しない。
 */
internal object ModelRecommendation {
    /** 辞書（mmap）・推論の作業領域など、モデル以外の見込み（MB） */
    const val RUNTIME_OVERHEAD_MB = 20.0

    /** 予算に対して残しておく安全マージン（MB） */
    const val SAFETY_MARGIN_MB = 40.0

    /** コアが LLM を読み込む最低の予算（MB）。未満だと辞書のみで起動する（`MIN_MEMORY_BUDGET_FOR_LLM_MB`） */
    const val CORE_MIN_BUDGET_MB = 150L

    /** 既定のモデル（コアが `conversion.model` 未指定のときに `models/` から探す名前） */
    const val DEFAULT_MODEL_FILE = "zenz-v3.1-small-Q5_K_M.gguf"

    val CATALOG: List<ModelSource> = listOf(
        ModelSource(
            id = "small",
            label = "small（精度優先）",
            fileName = "zenz-v3.1-small-Q5_K_M.gguf",
            url = "https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf/resolve/" +
                "ddf6e44b2e05ab7ea9a3e31559c5e7948761365c/ggml-model-Q5_K_M.gguf",
            sizeBytes = 73_871_968L,
            sha256 = "4de930c06bef8c263aa1aa40684af206db4ce1b96375b3b8ed0ea508e0b14f6c",
        ),
        ModelSource(
            id = "xsmall",
            label = "xsmall（軽量）",
            fileName = "zenz-v3.1-xsmall-Q5_K_M.gguf",
            url = "https://huggingface.co/Miwa-Keita/zenz-v3.1-xsmall-gguf/resolve/" +
                "5c0c4db8c8cc66a6ef01520475bebd56b41f6236/ggml-model-Q5_K_M.gguf",
            sizeBytes = 20_970_304L,
            sha256 = "189638370c43292fd54ba5e83854b24887ddd57e8914e19095514e663f60c7f5",
        ),
    )

    /** 端末の空きメモリ（バイト）から予算（MB）を求める。空きの半分を目安にし、64〜4096 に丸める（NagiEngine と同じ） */
    fun budgetMbFromAvail(availBytes: Long): Long = (availBytes / 2 / (1024 * 1024)).coerceIn(64L, 4096L)

    private fun sizeMb(spec: ModelSpec): Double = spec.sizeBytes / 1024.0 / 1024.0

    /** このモデルを載せるのに必要な予算（MB） */
    fun requiredBudgetMb(spec: ModelSpec): Double = sizeMb(spec) + RUNTIME_OVERHEAD_MB + SAFETY_MARGIN_MB

    /** 余裕を持って載せられるか */
    fun fits(spec: ModelSpec, budgetMb: Long): Boolean =
        budgetMb - (sizeMb(spec) + RUNTIME_OVERHEAD_MB) >= SAFETY_MARGIN_MB

    /** 余裕を持って載せられるうち最大のもの。なければ null。同じサイズなら名前の昇順で先のもの */
    fun recommend(budgetMb: Long, available: List<ModelSpec>): ModelSpec? =
        available
            .filter { fits(it, budgetMb) }
            .sortedWith(compareByDescending<ModelSpec> { it.sizeBytes }.thenBy { it.name })
            .firstOrNull()

    /** 表示名（xsmall / small、それ以外はファイル名） */
    fun label(fileName: String): String {
        val lower = fileName.lowercase()
        return when {
            lower.contains("xsmall") -> "xsmall"
            lower.contains("small") -> "small"
            else -> fileName
        }
    }

    /** 推奨の説明 */
    fun summary(budgetMb: Long, available: List<ModelSpec>): String {
        if (available.isEmpty()) return ""
        val hit = recommend(budgetMb, available)
        return if (hit != null) {
            "推奨: ${label(hit.name)}（この端末の予算 ${budgetMb}MB に対して余裕あり）"
        } else {
            "推奨なし（この端末の予算 ${budgetMb}MB では、どのモデルも余裕を持って載せられない見込み）"
        }
    }

    /** 辞書のみで動作する理由（LLM を使えるなら null） */
    fun dictionaryOnlyReason(hasModel: Boolean, useLlm: Boolean, budgetMb: Long): String? = when {
        !hasModel -> "変換モデルが無いため、辞書のみで動作します。"
        !useLlm -> "「LLM を使う」がオフのため、辞書のみで動作します。"
        budgetMb < CORE_MIN_BUDGET_MB ->
            "空きメモリが少ない（予算 ${budgetMb}MB < ${CORE_MIN_BUDGET_MB}MB）ため、辞書のみで動作します。"
        else -> null
    }
}
