package com.shiki01.nagi

import android.content.ClipboardManager
import android.content.Context

/**
 * クリップボードから履歴への取り込みと、「貼り付け」ボタンの出し分け用の確認（IME サービスが使う）。
 *
 * Android 10 以降、バックグラウンドのアプリはクリップボードを読めないが、既定の入力方法（IME）は自分が
 * 表示されている間は読める。そのため、キーボードを表示している間（onStartInputView〜onFinishInputView）だけ
 * 変化の通知（[ClipboardManager.OnPrimaryClipChangedListener]）を受け、表示した時点の現在値も取り込む。
 * 読めない場合（SecurityException など）は例外を握りつぶして何もしない。
 * 内容はログ（DiagLog）に出さない。
 */
internal class ClipboardCapture(context: Context) {
    private val app = context.applicationContext
    private val cm: ClipboardManager? = app.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
    private val store = ClipboardHistoryStore(java.io.File(app.filesDir, ClipboardHistoryStore.FILE_NAME))
    private var listener: ClipboardManager.OnPrimaryClipChangedListener? = null

    /** 変化の通知を受け始める（受け始めていれば何もしない）。[onChanged] はメインスレッドで呼ばれる */
    fun start(onChanged: () -> Unit) {
        if (listener != null) return
        val manager = cm ?: return
        val l = ClipboardManager.OnPrimaryClipChangedListener { onChanged() }
        try {
            manager.addPrimaryClipChangedListener(l)
            listener = l
        } catch (e: RuntimeException) {
            // 登録できなければ、表示時の取り込みだけで動く
        }
    }

    fun stop() {
        val l = listener ?: return
        listener = null
        try {
            cm?.removePrimaryClipChangedListener(l)
        } catch (e: RuntimeException) {
            // 無視
        }
    }

    /** クリップボードに文字があるか（中身は読まない。「貼り付け」ボタンの表示判定） */
    fun hasText(): Boolean = try {
        val manager = cm
        manager != null && manager.hasPrimaryClip() && manager.primaryClipDescription?.hasMimeType("text/*") == true
    } catch (e: RuntimeException) {
        false
    }

    /**
     * 履歴がオンで、パスワード欄などでなく、クリップボードが前回と変わっていれば読んで記録する。記録したら true。
     * [secure] はパスワード欄・学習しない指定の欄（[NagiImeService] の判定）
     */
    fun captureCurrent(secure: Boolean): Boolean {
        val config = KeyboardPrefs.clipboardConfig(app)
        if (!config.enabled || secure) return false
        return try {
            val manager = cm ?: return false
            if (!manager.hasPrimaryClip()) return false
            val description = manager.primaryClipDescription ?: return false
            if (!description.hasMimeType("text/*")) return false
            // 同じ内容を読み直さない（表示のたびに時刻が更新されて、期限が延びてしまうのを避ける）。
            // 時刻印が取れない（0）ときは内容の重複判定（ClipboardHistoryLogic.recorded）に任せる
            val stamp = description.timestamp
            if (stamp > 0L && stamp == KeyboardPrefs.clipboardLastStamp(app)) return false
            val clip = manager.primaryClip ?: return false
            if (clip.itemCount == 0) return false
            val text = clip.getItemAt(0).text?.toString() ?: return false
            if (stamp > 0L) KeyboardPrefs.setClipboardLastStamp(app, stamp)
            store.record(text, System.currentTimeMillis(), config)
        } catch (e: RuntimeException) {
            false
        }
    }
}
