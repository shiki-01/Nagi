package com.shiki01.nagi

import android.app.Activity
import android.os.Bundle
import android.widget.LinearLayout

/** データの使用量: 辞書・モデル・学習データなどが端末内で使っている容量（`filesDir` の集計） */
class DataUsageActivity : Activity() {
    private lateinit var box: LinearLayout

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("データの使用量")
        box = LinearLayout(this)
        box.orientation = LinearLayout.VERTICAL
        parts.content.addView(box)
        setContentView(parts.root)
    }

    override fun onResume() {
        super.onResume()
        box.removeAllViews()
        box.addView(card(emptyState("集計しています…")))
        val app = applicationContext
        Thread {
            val result = DataUsage.collect(app)
            runOnUiThread {
                if (isDestroyed) return@runOnUiThread
                val (items, total) = result
                box.removeAllViews()
                val c = UiCard(this)
                for (i in items) c.add(infoRow(i.label, formatBytes(i.bytes)))
                c.add(infoRow("合計", formatBytes(total)))
                box.addSection(
                    "このアプリが端末内で使っている容量",
                    c,
                    "辞書は読み取り専用の展開先、変換モデルは「変換モデル」の画面で削除できます。" +
                        "学習データはホームの「学習データを削除」で消せます。",
                )
            }
        }.start()
    }
}
