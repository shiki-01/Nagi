package com.shiki01.nagi

import android.content.Context
import android.content.SharedPreferences

/**
 * Android フロント固有の設定（コアの `config.toml` に無いもの。iOS の `ios-settings.json` に相当）。
 * キーボード（入力側）が読み、設定画面が書く。値の範囲は読むときに丸める。
 */
internal object KeyboardPrefs {
    private const val NAME = "nagi_android"

    private fun sp(ctx: Context): SharedPreferences = ctx.getSharedPreferences(NAME, Context.MODE_PRIVATE)

    /**
     * 日本語入力のレイアウト（旧来の値）: "flick"（12 キー）/ "qwerty"（ローマ字）。
     * かな用のレイアウト ID（[kanaLayoutId]）から読み替える。ID が QWERTY 系なら "qwerty"、それ以外は "flick"
     */
    fun layout(ctx: Context): String = if (kanaLayoutId(ctx).startsWith("qwerty")) "qwerty" else "flick"

    /** 旧来の値で設定する。かな用のレイアウト ID も `flick-kana` / `qwerty-kana` に合わせる */
    fun setLayout(ctx: Context, v: String) {
        sp(ctx).edit()
            .putString("layout", v)
            .putString("layout_kana", if (v == "qwerty") "qwerty-kana" else LayoutIds.DEFAULT_KANA)
            .apply()
    }

    /** かな入力で使うレイアウトの ID。未設定なら旧来の "layout" の値から（flick-kana / qwerty-kana） */
    fun kanaLayoutId(ctx: Context): String {
        val v = sp(ctx).getString("layout_kana", null)
        if (v != null && LayoutIds.isValid(v)) return v
        return if (sp(ctx).getString("layout", "flick") == "qwerty") "qwerty-kana" else LayoutIds.DEFAULT_KANA
    }

    /** 英字入力で使うレイアウトの ID。未設定なら `qwerty-alpha` */
    fun alphaLayoutId(ctx: Context): String {
        val v = sp(ctx).getString("layout_alpha", null)
        return if (v != null && LayoutIds.isValid(v)) v else LayoutIds.DEFAULT_ALPHA
    }

    /** かな用のレイアウト ID を設定する（null で既定に戻す） */
    fun setKanaLayoutId(ctx: Context, id: String?) {
        val e = sp(ctx).edit()
        if (id == null) e.remove("layout_kana").remove("layout") else e.putString("layout_kana", id)
        e.apply()
    }

    /** 英字用のレイアウト ID を設定する（null で既定に戻す） */
    fun setAlphaLayoutId(ctx: Context, id: String?) {
        val e = sp(ctx).edit()
        if (id == null) e.remove("layout_alpha") else e.putString("layout_alpha", id)
        e.apply()
    }

    /** 入力欄の種類ごとに利用者が割り当てたレイアウト（`kana` / `alpha` / ID / "none"）。未設定は null（既定） */
    fun fieldLayoutId(ctx: Context, kind: FieldKind): String? {
        val v = sp(ctx).getString("layout_field_${kind.key}", null) ?: return null
        return if (v == LayoutSelect.FIELD_NONE || v == LayoutSelect.TARGET_KANA || v == LayoutSelect.TARGET_ALPHA ||
            LayoutIds.isValid(v)
        ) {
            v
        } else {
            null
        }
    }

    /** 入力欄の種類ごとのレイアウトを設定する（null で既定に戻す） */
    fun setFieldLayoutId(ctx: Context, kind: FieldKind, id: String?) {
        val e = sp(ctx).edit()
        if (id == null) e.remove("layout_field_${kind.key}") else e.putString("layout_field_${kind.key}", id)
        e.apply()
    }

    /** 利用者が割り当てた入力欄別のレイアウト（未設定の種類は含まない） */
    fun fieldAssignments(ctx: Context): Map<FieldKind, String> {
        val out = HashMap<FieldKind, String>()
        for (k in FieldKind.values()) fieldLayoutId(ctx, k)?.let { out[k] = it }
        return out
    }

    /** 入力欄の種類（メール・URL・数字など）に合わせてレイアウトを自動で選び、改行キーの文字を変える */
    fun layoutByField(ctx: Context): Boolean = sp(ctx).getBoolean("layout_by_field", true)

    fun setLayoutByField(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean("layout_by_field", v).apply()

    /** 長押しと判定する時間（ミリ秒、150〜1000）。iOS の `flick.long_press_ms` */
    fun flickLongPressMs(ctx: Context): Int = sp(ctx).getInt("flick_long_press_ms", 350).coerceIn(150, 1000)

    fun setFlickLongPressMs(ctx: Context, v: Int) = sp(ctx).edit().putInt("flick_long_press_ms", v).apply()

    /** フリック先の文字を、キーの角に小さく表示する。iOS の `flick.hints` */
    fun flickHints(ctx: Context): Boolean = sp(ctx).getBoolean("flick_hints", true)

    fun setFlickHints(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean("flick_hints", v).apply()

    /** レイアウトの保存・削除・割り当てのたびに増える番号（キーボードが組み立て直すかの判定とキャッシュ破棄に使う） */
    fun layoutsRevision(ctx: Context): Long = sp(ctx).getLong("layouts_rev", 0L)

    fun bumpLayoutsRevision(ctx: Context) {
        val p = sp(ctx)
        p.edit().putLong("layouts_rev", p.getLong("layouts_rev", 0L) + 1L).apply()
    }

    /** 12 キーでキーを押したときトグル入力（あ→い→う…）にする（false なら tap = 行の先頭） */
    fun toggleInput(ctx: Context): Boolean = sp(ctx).getBoolean("toggle_input", false)

    fun setToggleInput(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean("toggle_input", v).apply()

    /** フリックとみなす移動量（dp）。小さいほど敏感 */
    fun flickThresholdDp(ctx: Context): Float = sp(ctx).getFloat("flick_threshold_dp", 14f).coerceIn(4f, 40f)

    fun setFlickThresholdDp(ctx: Context, v: Float) = sp(ctx).edit().putFloat("flick_threshold_dp", v).apply()

    /** フリックの押下ポップアップ（中央と上下左右の 5 文字）を出す */
    fun flickPopup(ctx: Context): Boolean = sp(ctx).getBoolean("flick_popup", true)

    fun setFlickPopup(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean("flick_popup", v).apply()

    /** キー押下時の触覚フィードバック */
    fun haptic(ctx: Context): Boolean = sp(ctx).getBoolean("haptic", true)

    fun setHaptic(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean("haptic", v).apply()

    /** キーボードの高さの倍率（0.8〜1.4） */
    fun heightScale(ctx: Context): Float = sp(ctx).getFloat("height_scale", 1.0f).coerceIn(0.8f, 1.4f)

    fun setHeightScale(ctx: Context, v: Float) = sp(ctx).edit().putFloat("height_scale", v).apply()

    /** テーマ: "system" / "light" / "dark" */
    fun theme(ctx: Context): String = sp(ctx).getString("theme", "system") ?: "system"

    fun setTheme(ctx: Context, v: String) = sp(ctx).edit().putString("theme", v).apply()

    /** アップデートの自動確認（本体アプリを開いたとき。24 時間に 1 回まで） */
    fun autoCheckUpdates(ctx: Context): Boolean = sp(ctx).getBoolean("auto_check_updates", true)

    fun setAutoCheckUpdates(ctx: Context, v: Boolean) = sp(ctx).edit().putBoolean("auto_check_updates", v).apply()

    /** 最後にアップデートを確認した時刻（epoch ミリ秒） */
    fun lastUpdateCheck(ctx: Context): Long = sp(ctx).getLong("last_update_check", 0L)

    fun setLastUpdateCheck(ctx: Context, v: Long) = sp(ctx).edit().putLong("last_update_check", v).apply()
}
