package com.shiki01.nagi

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * `nagi://layout`（ブラウザ等の外部アプリから）の受け口。画面は持たず、[MainActivity] のレイアウトタブを開いて
 * リンクを渡す（取り込みは必ずレイアウトタブの確認ダイアログを挟む）。レイアウトの画面そのものは [LayoutManagerScreen]。
 * 外部から呼ばれる Activity なので、ここでは受け取った内容を解釈せず、そのまま渡すだけにする。
 */
class LayoutManagerActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val to = Intent(this, MainActivity::class.java)
        to.putExtra(MainRouting.EXTRA_TAB, MainTab.LAYOUTS.id)
        val from: Intent? = intent
        if (from?.action == Intent.ACTION_VIEW) {
            to.action = Intent.ACTION_VIEW
            to.data = from.data
        }
        startActivity(to)
        finish()
    }
}
