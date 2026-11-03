package com.shiki01.nagi.tabs

import android.content.Intent
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import androidx.activity.result.contract.ActivityResultContracts
import androidx.fragment.app.Fragment
import com.shiki01.nagi.*

/**
 * レイアウトタブ。画面の中身（割り当て中・取り込み・自分のレイアウト・同梱のレイアウト・詳細ダイアログ）は
 * [LayoutManagerScreen]。ここはタブの器で、ファイル選択と貼り付け画面の結果を受けて渡す。
 * `nagi://layout` は [LayoutManagerActivity] が受け、[com.shiki01.nagi.MainActivity] 経由で [offerIntent] に届く。
 */
class LayoutsFragment : Fragment() {
    private var screen: LayoutManagerScreen? = null

    /** 届いたがまだ処理していない外部 Intent。表示中（RESUMED）になってから確認ダイアログを出す */
    private var pendingIntent: Intent? = null

    private val fileLauncher = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { r ->
        screen?.onFileResult(r.resultCode, r.data)
    }

    private val pasteLauncher = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { r ->
        screen?.onPasteResult(r.resultCode, r.data)
    }

    override fun onCreateView(inflater: LayoutInflater, container: ViewGroup?, savedInstanceState: Bundle?): View {
        val parts = requireContext().pageParts("レイアウト")
        screen = LayoutManagerScreen(
            requireContext(),
            parts,
            { fileLauncher.launch(it) },
            { pasteLauncher.launch(it) },
        )
        return parts.root
    }

    override fun onResume() {
        super.onResume()
        screen?.reload()
        consumePending()
    }

    override fun onDestroyView() {
        screen?.dispose()
        screen = null
        super.onDestroyView()
    }

    /** 外部から開かれた Intent（`nagi://layout`）を渡す。表示中ならすぐ、そうでなければ表示されたときに処理する */
    fun offerIntent(intent: Intent) {
        pendingIntent = intent
        if (isResumed) consumePending()
    }

    private fun consumePending() {
        val i = pendingIntent ?: return
        pendingIntent = null
        screen?.offerIntent(i)
    }
}
