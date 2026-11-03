package com.shiki01.nagi

import android.app.Activity
import android.os.Bundle
import android.widget.LinearLayout
import java.util.Locale

/** 入力設定。Android フロント固有の設定（[KeyboardPrefs]）を編集する。キーボード側は次に表示されたときに読む */
class KeyboardInputSettingsActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val parts = screenParts("入力設定")
        val root = parts.content

        root.addView(body("キーボードの見た目と操作の設定です。変更はキーボードを開き直すと反映されます。", secondary = true))

        root.addSection(
            "日本語入力",
            card(
                selectRow(
                    "レイアウト",
                    "12 キー（フリック）か、ローマ字入力（QWERTY）かを選びます。",
                    listOf("flick" to "12 キー（フリック）", "qwerty" to "QWERTY（ローマ字）"),
                    KeyboardPrefs.layout(this),
                ) { KeyboardPrefs.setLayout(this, it) },
                switchRow(
                    "トグル入力",
                    "同じキーを続けて押すと「あ→い→う…」と文字が変わります。オフのときは押すと行の先頭の文字になります。",
                    KeyboardPrefs.toggleInput(this),
                ) { KeyboardPrefs.setToggleInput(this, it) },
                seekRow(
                    "フリックの感度",
                    "フリックとみなす指の移動量です。小さいほど敏感です。",
                    min = 4f,
                    max = 40f,
                    step = 1f,
                    value = KeyboardPrefs.flickThresholdDp(this),
                    format = { String.format(Locale.ROOT, "%.0f dp", it) },
                ) { KeyboardPrefs.setFlickThresholdDp(this, it) },
                switchRow(
                    "フリックのポップアップ",
                    "キーを押したとき、中央と上下左右の 5 文字を表示します。",
                    KeyboardPrefs.flickPopup(this),
                ) { KeyboardPrefs.setFlickPopup(this, it) },
            ),
        )

        root.addSection(
            "見た目・操作感",
            card(
                switchRow("触覚フィードバック", "キーを押したときに振動します。", KeyboardPrefs.haptic(this)) {
                    KeyboardPrefs.setHaptic(this, it)
                },
                seekRow(
                    "キーボードの高さ",
                    "標準の高さに対する倍率です。",
                    min = 0.8f,
                    max = 1.4f,
                    step = 0.05f,
                    value = KeyboardPrefs.heightScale(this),
                    format = { String.format(Locale.ROOT, "×%.2f", it) },
                ) { KeyboardPrefs.setHeightScale(this, it) },
                selectRow(
                    "テーマ",
                    null,
                    listOf("system" to "端末の設定に合わせる", "light" to "ライト", "dark" to "ダーク"),
                    KeyboardPrefs.theme(this),
                ) { KeyboardPrefs.setTheme(this, it) },
            ),
        )

        root.addSection(
            "アップデート",
            card(
                switchRow(
                    "アップデートを自動で確認",
                    "本体アプリを開いたとき、新しい版があるか確認します（24 時間に 1 回まで）。",
                    KeyboardPrefs.autoCheckUpdates(this),
                ) { KeyboardPrefs.setAutoCheckUpdates(this, it) },
            ),
        )

        setContentView(parts.root)
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
        val steps = Math.round((max - min) / step)
        fun valueAt(progress: Int): Float = (min + progress * step).coerceIn(min, max)
        return sliderRow(
            label,
            description,
            steps,
            Math.round((value - min) / step).coerceIn(0, steps),
            show = { format(valueAt(it)) },
            onCommit = { onChange(valueAt(it)) },
        )
    }
}
