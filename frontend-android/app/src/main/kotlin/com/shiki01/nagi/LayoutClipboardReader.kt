package com.shiki01.nagi

import android.content.ClipboardManager
import android.content.Context

/** クリップボードの読み取り。ユーザーが明示的にタップしたときだけ呼ぶこと（画面表示時に自動では読まない） */
internal object LayoutClipboardReader {
    /** 現在のクリップのテキスト。空・テキストでない・読めないときは null */
    fun readText(ctx: Context): String? = try {
        val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = cm.primaryClip
        if (clip == null || clip.itemCount == 0) {
            null
        } else {
            clip.getItemAt(0)?.coerceToText(ctx)?.toString()
        }
    } catch (e: RuntimeException) {
        null
    }
}

/** 現在のかな・英字の割り当てを「かな: ○○ / 英字: ○○」にする。ファイルを読むので、メインスレッド以外で呼ぶこと */
internal fun Context.loadAssignmentSummary(): String {
    fun nameOf(slot: AssignSlot): String {
        val id = LayoutStore.assigned(this, slot)
        return LayoutAssignSummary.nameOrId(id, LayoutStore.load(this, id)?.name)
    }
    return LayoutAssignSummary.text(nameOf(AssignSlot.KANA), nameOf(AssignSlot.ALPHA))
}
