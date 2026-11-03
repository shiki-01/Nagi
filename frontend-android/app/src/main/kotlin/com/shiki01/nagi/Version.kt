package com.shiki01.nagi

/** バージョン文字列（x.y.z）の比較。コアの `update_check::parse_version` と同じ規則（android.* 非依存） */
internal object Version {
    /** "x.y.z" を 3 整数にする。足りない・数値でない要素は 0 */
    fun parse(v: String): Triple<Long, Long, Long> {
        val parts = v.trim().split('.').map { it.toLongOrNull() ?: 0L }
        return Triple(parts.getOrElse(0) { 0L }, parts.getOrElse(1) { 0L }, parts.getOrElse(2) { 0L })
    }

    /** a < b なら負、等しければ 0、a > b なら正 */
    fun compare(a: String, b: String): Int {
        val (a1, a2, a3) = parse(a)
        val (b1, b2, b3) = parse(b)
        return compareValuesBy(Triple(a1, a2, a3), Triple(b1, b2, b3), { it.first }, { it.second }, { it.third })
    }

    /** latest が current より新しいか */
    fun isNewer(latest: String, current: String): Boolean = compare(latest, current) > 0
}
