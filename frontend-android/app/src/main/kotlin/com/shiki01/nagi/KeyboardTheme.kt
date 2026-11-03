package com.shiki01.nagi

// キーボードのアクセントカラー（`keyboard.accent`）の対応表。android.* に依存しない純粋な処理（単体テスト対象）。
// 色は ARGB の Int で持つ（白い文字を載せても読める濃さにそろえる）。

internal enum class AccentColor(val json: String, val label: String, private val rgbLight: Int, private val rgbDark: Int) {
    BLUE("blue", "ブルー", 0x3B82F6, 0x3B82F6),
    GREEN("green", "グリーン", 0x28A745, 0x2FB655),
    ORANGE("orange", "オレンジ", 0xD9731A, 0xE8832B),
    PINK("pink", "ピンク", 0xE0406F, 0xEC5782),
    GRAY("gray", "グレー", 0x6C757D, 0x7C858D),
    ;

    /** 不透明な ARGB */
    fun argb(dark: Boolean): Int = 0xFF000000.toInt() or (if (dark) rgbDark else rgbLight)

    companion object {
        /** 知らない名前は青 */
        fun fromJson(s: String?): AccentColor = values().firstOrNull { it.json == s } ?: BLUE

        /** [argb] の色のまま、不透明度だけ [alpha]（0..255）にする（選択中の候補・変換中の強調の背景用） */
        fun withAlpha(argb: Int, alpha: Int): Int = (alpha.coerceIn(0, 255) shl 24) or (argb and 0x00FFFFFF)
    }
}
