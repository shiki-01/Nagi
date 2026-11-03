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

    /** 日本語入力のレイアウト: "flick"（12 キー）/ "qwerty"（ローマ字） */
    fun layout(ctx: Context): String = sp(ctx).getString("layout", "flick") ?: "flick"

    fun setLayout(ctx: Context, v: String) = sp(ctx).edit().putString("layout", v).apply()

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
