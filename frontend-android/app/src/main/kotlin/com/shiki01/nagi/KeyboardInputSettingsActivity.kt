package com.shiki01.nagi

import android.app.Activity
import android.os.Bundle
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.TextView
import java.util.Locale

/** 入力設定。Android フロント固有の設定（[KeyboardPrefs]）を編集する。キーボード側は次に表示されたときに読む */
class KeyboardInputSettingsActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val (scroll, root) = screen("入力設定")

        root.addView(body("キーボード（日本語入力）の見た目と操作の設定です。変更はキーボードを開き直すと反映されます。", secondary = true))

        root.addView(heading("日本語入力"))
        root.addView(
            spinnerRow(
                "レイアウト",
                "12 キー（フリック）か、ローマ字入力（QWERTY）かを選びます。",
                listOf("flick" to "12 キー（フリック）", "qwerty" to "QWERTY（ローマ字）"),
                KeyboardPrefs.layout(this),
            ) { KeyboardPrefs.setLayout(this, it) },
        )
        root.addView(
            switchRow(
                "トグル入力",
                "同じキーを続けて押すと「あ→い→う…」と文字が変わります。オフのときは押すと行の先頭の文字になります。",
                KeyboardPrefs.toggleInput(this),
            ) { KeyboardPrefs.setToggleInput(this, it) },
        )
        root.addView(
            seekRow(
                "フリックの感度（移動量）",
                "フリックとみなす指の移動量です。小さいほど敏感です。",
                min = 4f,
                max = 40f,
                step = 1f,
                value = KeyboardPrefs.flickThresholdDp(this),
                format = { String.format(Locale.ROOT, "%.0f dp", it) },
            ) { KeyboardPrefs.setFlickThresholdDp(this, it) },
        )
        root.addView(
            switchRow(
                "フリックのポップアップ",
                "キーを押したとき、中央と上下左右の 5 文字を表示します。",
                KeyboardPrefs.flickPopup(this),
            ) { KeyboardPrefs.setFlickPopup(this, it) },
        )

        root.addView(heading("見た目・操作感"))
        root.addView(
            switchRow("触覚フィードバック", "キーを押したときに振動します。", KeyboardPrefs.haptic(this)) {
                KeyboardPrefs.setHaptic(this, it)
            },
        )
        root.addView(
            seekRow(
                "キーボードの高さ",
                "標準の高さに対する倍率です。",
                min = 0.8f,
                max = 1.4f,
                step = 0.05f,
                value = KeyboardPrefs.heightScale(this),
                format = { String.format(Locale.ROOT, "×%.2f", it) },
            ) { KeyboardPrefs.setHeightScale(this, it) },
        )
        root.addView(
            spinnerRow(
                "テーマ",
                null,
                listOf("system" to "端末の設定に合わせる", "light" to "ライト", "dark" to "ダーク"),
                KeyboardPrefs.theme(this),
            ) { KeyboardPrefs.setTheme(this, it) },
        )

        root.addView(heading("アップデート"))
        root.addView(
            switchRow(
                "アップデートを自動で確認",
                "本体アプリを開いたとき、新しい版があるか確認します（24 時間に 1 回まで）。",
                KeyboardPrefs.autoCheckUpdates(this),
            ) { KeyboardPrefs.setAutoCheckUpdates(this, it) },
        )

        setContentView(scroll)
    }

    /** ラベル + 現在値 + シークバー。[onChange] は指を離したときに 1 回だけ呼ぶ */
    private fun seekRow(
        label: String,
        description: String,
        min: Float,
        max: Float,
        step: Float,
        value: Float,
        format: (Float) -> String,
        onChange: (Float) -> Unit,
    ): LinearLayout {
        val col = LinearLayout(this)
        col.orientation = LinearLayout.VERTICAL
        col.setPadding(0, dp(8), 0, dp(8))
        val steps = Math.round((max - min) / step)
        fun valueAt(progress: Int): Float = (min + progress * step).coerceIn(min, max)

        val head = TextView(this)
        head.textSize = 15f
        fun show(v: Float) {
            head.text = "$label: ${format(v)}"
        }
        show(value)
        col.addView(head)
        col.addView(body(description, secondary = true))

        val bar = SeekBar(this)
        bar.max = steps
        bar.progress = Math.round((value - min) / step).coerceIn(0, steps)
        bar.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                show(valueAt(progress))
            }

            override fun onStartTrackingTouch(seekBar: SeekBar?) {}

            override fun onStopTrackingTouch(seekBar: SeekBar?) {
                onChange(valueAt(bar.progress))
            }
        })
        col.addView(bar)
        return col
    }
}
