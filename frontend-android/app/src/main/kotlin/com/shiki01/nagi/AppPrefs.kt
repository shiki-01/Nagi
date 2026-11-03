package com.shiki01.nagi

import android.content.Context
import android.content.SharedPreferences

/**
 * 本体アプリ（画面まわり）の設定。キーボード側の設定は [KeyboardPrefs]。
 * 初回案内・開発者モード・設定の表示モード・診断ログ・更新後の案内の記憶をここに置く。
 */
internal object AppPrefs {
    private const val NAME = "nagi_app"

    private fun sp(ctx: Context): SharedPreferences =
        ctx.applicationContext.getSharedPreferences(NAME, Context.MODE_PRIVATE)

    /** 初回案内を一度出したか（既定 false） */
    fun onboardingDone(ctx: Context): Boolean = sp(ctx).getBoolean("onboarding_done", false)

    fun setOnboardingDone(ctx: Context, v: Boolean) {
        sp(ctx).edit().putBoolean("onboarding_done", v).apply()
    }

    /** 開発者モード（既定 false）。オンのときだけホームに「開発者」の行を出す */
    fun developerMode(ctx: Context): Boolean = sp(ctx).getBoolean("developer_mode", false)

    fun setDeveloperMode(ctx: Context, v: Boolean) {
        sp(ctx).edit().putBoolean("developer_mode", v).apply()
    }

    /** 設定タブの表示モード（既定 シンプル）。iOS の `ui.mode` に相当 */
    fun uiMode(ctx: Context): SettingsUiMode = SettingsUiMode.fromJson(sp(ctx).getString("ui_mode", null))

    fun setUiMode(ctx: Context, v: SettingsUiMode) {
        sp(ctx).edit().putString("ui_mode", v.json).apply()
    }

    /** 診断ログを記録するか（既定 false） */
    fun diagEnabled(ctx: Context): Boolean = sp(ctx).getBoolean("diag_enabled", false)

    fun setDiagEnabled(ctx: Context, v: Boolean) {
        sp(ctx).edit().putBoolean("diag_enabled", v).apply()
    }

    /**
     * 診断ログに入力内容を含めるか。保存値が true でも、開発者モードがオフなら false を返す
     * （開発者モードをオフにした時点で、内容の記録は止まる）
     */
    fun diagIncludeContent(ctx: Context): Boolean =
        developerMode(ctx) && sp(ctx).getBoolean("diag_include_content", false)

    /** 開発者モードがオフのときは変更できない（何もしない） */
    fun setDiagIncludeContent(ctx: Context, v: Boolean) {
        if (!developerMode(ctx)) return
        sp(ctx).edit().putBoolean("diag_include_content", v).apply()
    }

    /** 前回ホームを開いたときの versionName（未記録は空文字） */
    fun lastSeenVersion(ctx: Context): String = sp(ctx).getString("last_seen_version", "") ?: ""

    fun setLastSeenVersion(ctx: Context, v: String) {
        sp(ctx).edit().putString("last_seen_version", v).apply()
    }

    /** 前回ホームを開いたときの `PackageInfo.lastUpdateTime`（未記録は 0） */
    fun lastSeenUpdateTime(ctx: Context): Long = sp(ctx).getLong("last_seen_update_time", 0L)

    fun setLastSeenUpdateTime(ctx: Context, v: Long) {
        sp(ctx).edit().putLong("last_seen_update_time", v).apply()
    }
}

/** 更新後の案内の判定（android.* に依存しない） */
internal object UpdateNotice {
    /**
     * 「0.1.1 から 0.1.2 に更新しました」。初回起動（[lastSeen] が空）と、版が変わっていないときは null
     */
    fun text(lastSeen: String, current: String): String? {
        val before = lastSeen.trim()
        val now = current.trim()
        if (before.isEmpty() || now.isEmpty() || before == now) return null
        return "$before から $now に更新しました"
    }
}
