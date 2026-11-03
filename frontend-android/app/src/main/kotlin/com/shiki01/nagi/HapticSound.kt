package com.shiki01.nagi

import android.content.Context
import android.media.AudioManager
import android.provider.Settings
import android.view.HapticFeedbackConstants
import android.view.View

/** キー入力の触覚の強さ（`keyboard.haptic`）。VIBRATE 権限が要らない `performHapticFeedback` の種類の切り替えで表す */
internal enum class HapticLevel(val json: String, val label: String) {
    OFF("off", "オフ"),
    LIGHT("light", "弱"),
    MEDIUM("medium", "中"),
    HEAVY("heavy", "強"),
    ;

    companion object {
        fun fromJson(s: String?): HapticLevel? = values().firstOrNull { it.json == s }
    }
}

/** キーを押したときの触覚とクリック音 */
internal object KeyFeedback {
    /**
     * 触覚。弱 = KEYBOARD_TAP、中 = VIRTUAL_KEY、強 = LONG_PRESS（端末によって差が小さいことがある）。
     * 端末の「タッチ操作時の振動」がオフのときは OS が鳴らさない
     */
    fun haptic(view: View, level: HapticLevel) {
        val c = when (level) {
            HapticLevel.OFF -> return
            HapticLevel.LIGHT -> HapticFeedbackConstants.KEYBOARD_TAP
            HapticLevel.MEDIUM -> HapticFeedbackConstants.VIRTUAL_KEY
            HapticLevel.HEAVY -> HapticFeedbackConstants.LONG_PRESS
        }
        view.performHapticFeedback(c)
    }

    /**
     * クリック音。[fx] は AudioManager.FX_KEYPRESS_*。音量と鳴るかどうかは端末の
     * 「タッチ操作音」の設定に従う
     */
    fun click(audio: AudioManager?, fx: Int) {
        audio?.playSoundEffect(fx)
    }
}

/** 「アニメーションをオフ」（開発者向けオプション・アクセシビリティの設定）に従う */
internal object MotionPrefs {
    /** アニメーションを出してよいか。ANIMATOR_DURATION_SCALE が 0 のときは false */
    fun animationsEnabled(context: Context): Boolean = try {
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) > 0f
    } catch (e: Exception) {
        true
    }
}
