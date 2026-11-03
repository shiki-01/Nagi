package com.shiki01.nagi

import java.util.Locale

// 設定タブ（コアの記述子 + フロント固有の項目を 1 本のリストにしたもの）の、android.* に依存しない部分。
// 項目の定義（キー・グループ・ラベル・範囲・既定・シンプル/詳細）と、表示する項目・並びの組み立て。単体テスト対象。
// 値の読み書きは KeyboardPrefs / AppPrefs（tabs/FrontSettingRows.kt が結び付ける）。
// ラベル・並び・シンプル/詳細は iOS の `IOSSettings.swift`（`NagiIOSSettings.definitions`）に揃える

/** 設定タブの表示モード。シンプルは詳細フラグの無い項目だけ。値は変わらない（出し分けだけ） */
internal enum class SettingsUiMode(val json: String, val label: String) {
    SIMPLE("simple", "シンプル"),
    DETAIL("detail", "詳細"),
    ;

    val showDetail: Boolean get() = this == DETAIL

    companion object {
        /** 未設定・読めない値はシンプル */
        fun fromJson(s: String?): SettingsUiMode = values().firstOrNull { it.json == s } ?: SIMPLE
    }
}

/** フロント固有の項目の操作の種類 */
internal enum class FrontKind {
    /** スイッチ */
    TOGGLE,

    /** スライダー（値は Float。整数の項目は刻みも整数にして丸める） */
    SLIDER,

    /** 選択式 */
    CHOICE,

    /** キーボードの高さ（低め / 標準 / 高め。倍率と一致しないときは「カスタム」を足す） */
    HEIGHT_PRESET,

    /** かな・英字のレイアウトの割り当て（選択肢は実行時にレイアウトの一覧から作る） */
    LAYOUT,

    /** 押すと何かが起きる行（削除・画面を開く） */
    ACTION,

    /** 複数選択（クリップボード履歴の保存する種類。選択肢は [FrontSettingDef.choices]） */
    MULTI,

    /** 順序つきの項目の編集（ツールバーの項目。専用の画面を開く） */
    ORDERED_LIST,
}

/** スライダーの値の表示 */
internal enum class FrontUnit { INT, DEC1, TIMES1, TIMES2 }

/** フロント固有の項目 1 件の定義 */
internal data class FrontSettingDef(
    val key: String,
    val group: String,
    val label: String,
    val description: String?,
    /** true なら「詳細」のときだけ出す（iOS の `isSimple == false`） */
    val detail: Boolean,
    val kind: FrontKind,
    val min: Float = 0f,
    val max: Float = 0f,
    val step: Float = 1f,
    val unit: FrontUnit = FrontUnit.INT,
    val defaultFloat: Float = 0f,
    val defaultBool: Boolean = false,
    val choices: List<Pair<String, String>> = emptyList(),
    val defaultChoice: String = "",
)

/** フロント固有の項目のキー（iOS の `docs/ios-settings.md` の ID と同じ。Android 固有のものは末尾） */
internal object FrontKeys {
    const val TOGGLE_INPUT = "input.toggle_input"
    const val COMPOSE_NUMBERS = "input.compose_numbers"
    const val KEEP_PREEDIT_ON_SYMBOLS = "input.keep_preedit_on_symbols"
    const val KEEP_PREEDIT_ON_ENGLISH = "input.keep_preedit_on_english"
    const val FLICK_SENSITIVITY = "flick.sensitivity"
    const val FLICK_THRESHOLD = "flick.threshold_pt"
    const val FLICK_POPUP = "flick.popup"
    const val FLICK_HINTS = "flick.hints"
    const val FLICK_LONG_PRESS = "flick.long_press_ms"
    const val KEY_REPEAT_DELAY = "touch.key_repeat_delay_ms"
    const val KEY_REPEAT = "touch.key_repeat_ms"
    const val SPACE_SWIPE = "touch.space_swipe"
    const val SPACE_SWIPE_STEP = "touch.space_swipe_step_pt"
    const val SPACE_SWIPE_ACCEL = "touch.space_swipe_accel"
    const val SPACE_SWIPE_ACCEL_STRENGTH = "touch.space_swipe_accel_strength"
    const val BACKSPACE_SWIPE = "touch.backspace_swipe"

    const val HEIGHT = "keyboard.height"
    const val HEIGHT_SCALE = "keyboard.height_scale"
    const val ONE_HANDED = "keyboard.one_handed"
    const val ONE_HANDED_WIDTH = "keyboard.one_handed_width"
    const val KEY_SPACING = "keyboard.key_spacing_pt"
    const val KEY_CORNER = "keyboard.key_corner_pt"
    const val LAYOUT_BY_FIELD = "keyboard.layout_by_field"
    const val HAPTIC = "keyboard.haptic"
    const val SOUND = "keyboard.sound"
    const val THEME = "keyboard.theme"
    const val ACCENT = "keyboard.accent"
    const val LAYOUT_KANA = "keyboard.layout_kana"
    const val LAYOUT_ALPHA = "keyboard.layout_alpha"

    /** Android のみ（iOS に無い） */
    const val HEIGHT_SCALE_LANDSCAPE = "keyboard.height_scale_landscape"

    /** フロントの更新確認。コアの `update.auto_check` は [SettingsSchema] が非表示にする */
    const val UPDATE_AUTO_CHECK = "update.auto_check"
    const val UPDATE_OPEN = "update.open"

    const val CANDBAR_PASTE_BUTTON = "candbar.paste_button"
    const val TOOLBAR_ITEMS = "toolbar.items"

    const val CLIPBOARD_ENABLED = "clipboard.history.enabled"
    const val CLIPBOARD_RETENTION = "clipboard.history.retention"
    const val CLIPBOARD_MAX_ITEMS = "clipboard.history.max_items"
    const val CLIPBOARD_TYPES = "clipboard.history.types"
    const val CLIPBOARD_PREVIEW_CHARS = "clipboard.history.preview_chars"
    const val CLIPBOARD_CLEAR = "clipboard.history.clear"

    const val LEARNING_CLEAR = "learning.clear"

    const val DEVELOPER_MODE = "developer.mode"
    const val DEVELOPER_OPEN = "developer.open"
}

/** 設定タブの 1 行（コアの記述子かフロント固有の項目） */
internal sealed class SettingsEntry {
    class Core(val def: SettingDef) : SettingsEntry()

    class Front(val def: FrontSettingDef) : SettingsEntry()

    val key: String get() = when (this) {
        is Core -> def.key
        is Front -> def.key
    }
}

/** 設定タブの 1 グループ */
internal data class SettingsSection(val title: String, val entries: List<SettingsEntry>, val footnote: String?)

internal object FrontSettings {
    const val RELOAD_NOTE = "キーボードを開き直すと反映されます。"

    /** 画面の末尾の注記 */
    const val PAGE_FOOTNOTE =
        "変更はキーボードを次に開いたときに反映されます。「次回起動で反映」と出ている項目は、エンジンを再起動してください。"

    /** 「クリップボード履歴」グループの注記 */
    const val CLIPBOARD_NOTE =
        "履歴はキーボードが表示されている間にコピーしたものだけが残ります。パスワード欄と、学習しない指定の入力欄では" +
            "取り込みも表示もしません。1 件が 1 万文字を超えるものは記録しません。履歴はこの端末内にだけ保存し、" +
            "内容は診断ログにも出しません。オフにしても保存済みの履歴は消えません（「履歴をすべて削除」で消せます）。"

    const val MODE_FOOTNOTE = "詳細にすると項目が増えます。切り替えても設定値は変わりません。"

    /** 高さのプリセットの選択肢の値のうち「カスタム」（倍率が 3 択のどれとも一致しないとき） */
    const val HEIGHT_CUSTOM = "custom"

    private fun toggle(key: String, group: String, label: String, desc: String?, detail: Boolean, default: Boolean) =
        FrontSettingDef(key, group, label, desc, detail, FrontKind.TOGGLE, defaultBool = default)

    private fun slider(
        key: String,
        group: String,
        label: String,
        desc: String?,
        detail: Boolean,
        min: Float,
        max: Float,
        step: Float,
        unit: FrontUnit,
        default: Float,
    ) = FrontSettingDef(
        key, group, label, desc, detail, FrontKind.SLIDER,
        min = min, max = max, step = step, unit = unit, defaultFloat = default,
    )

    private fun choice(
        key: String,
        group: String,
        label: String,
        desc: String?,
        detail: Boolean,
        choices: List<Pair<String, String>>,
        default: String,
    ) = FrontSettingDef(key, group, label, desc, detail, FrontKind.CHOICE, choices = choices, defaultChoice = default)

    private fun action(key: String, group: String, label: String, desc: String?) =
        FrontSettingDef(key, group, label, desc, detail = false, kind = FrontKind.ACTION)

    private val K = KeyboardDefaults

    /** 「キー入力の音」の選択肢の値。保存は従来どおり boolean（"compat" = true） */
    const val SOUND_OFF = "off"
    const val SOUND_ON = "compat"

    /** 項目の定義（設定画面の並び。iOS の定義順から、Android に機能が無いものを除いたもの） */
    val definitions: List<FrontSettingDef> = listOf(
        // ---- 入力 ----
        toggle(
            FrontKeys.TOGGLE_INPUT, "入力", "トグル入力",
            "同じキーを続けて押すと次の文字（あ→い→う）になります。オフのときはフリックで入力し、タップは常に先頭の文字です。",
            detail = false, default = K.TOGGLE_INPUT,
        ),
        toggle(
            FrontKeys.COMPOSE_NUMBERS, "入力", "数字・記号も入力中の文字列にする",
            "かなから数字・記号のキーボードに切り替えて入力した文字を、入力中の文字列に入れます。" +
                "スペースで電卓の答え・全角・記号の候補が出ます。何も選ばずに確定すれば半角のままです。" +
                "オフにすると直接入力します。パスワード欄では常に直接入力です。",
            detail = true, default = K.COMPOSE_NUMBERS,
        ),
        toggle(
            FrontKeys.KEEP_PREEDIT_ON_SYMBOLS, "入力", "数字・記号への切替で入力中の文字列を保つ",
            "かなと数字・記号のキーボードを切り替えても、入力中の文字列を確定せずに続けます（1+2 のような式を続けて打てます）。" +
                "オフにすると切り替えるときに確定します。「数字・記号も入力中の文字列にする」がオフのときは確定します。" +
                "パスワード欄では常に確定します。",
            detail = true, default = K.KEEP_PREEDIT_ON_SYMBOLS,
        ),
        toggle(
            FrontKeys.KEEP_PREEDIT_ON_ENGLISH, "入力", "英字への切替で入力中の文字列を保つ",
            "かなと英字のキーボードを切り替えても、入力中の文字列を確定せずに続けます（英字は入力中の文字列に入ります）。" +
                "オフにすると切り替えるときに確定します。パスワード欄では常に確定します。",
            detail = true, default = K.KEEP_PREEDIT_ON_ENGLISH,
        ),
        slider(
            FrontKeys.FLICK_SENSITIVITY, "入力", "フリック感度",
            "値が大きいほど、短い移動でフリックと判定します。判定距離をこの倍率で割った距離が実際の判定に使われます。",
            false, K.FLICK_SENSITIVITY_MIN, K.FLICK_SENSITIVITY_MAX, 0.1f, FrontUnit.TIMES1, K.FLICK_SENSITIVITY,
        ),
        slider(
            FrontKeys.FLICK_THRESHOLD, "入力", "フリックの判定距離（pt）",
            "指をこの距離より動かすとフリックと判定します。感度の倍率で割った値が実際の距離です。",
            true, K.FLICK_THRESHOLD_MIN, K.FLICK_THRESHOLD_MAX, 1f, FrontUnit.INT, K.FLICK_THRESHOLD_DP,
        ),
        toggle(
            FrontKeys.FLICK_POPUP, "入力", "フリックの案内を表示",
            "キーを押している間、中央と上下左右の文字をポップアップで見せます。オフにすると描画が軽くなります。",
            detail = false, default = K.FLICK_POPUP,
        ),
        toggle(
            FrontKeys.FLICK_HINTS, "入力", "フリック先の文字をキーに表示",
            "かなのキーの上下左右に、フリックで入る文字を小さく表示します。",
            detail = false, default = K.FLICK_HINTS,
        ),
        slider(
            FrontKeys.FLICK_LONG_PRESS, "入力", "長押しの判定時間（ミリ秒）",
            "長押しと判定するまでの時間です。レイアウトの longPress を持つキーを、この時間押し続けると候補のポップアップが出ます。",
            true, K.FLICK_LONG_PRESS_MIN.toFloat(), K.FLICK_LONG_PRESS_MAX.toFloat(), 50f, FrontUnit.INT,
            K.FLICK_LONG_PRESS_MS.toFloat(),
        ),
        slider(
            FrontKeys.KEY_REPEAT_DELAY, "入力", "⌫の連打を始めるまで（ミリ秒）",
            "⌫を押し続けてから、連続して消し始めるまでの時間です。",
            true, K.KEY_REPEAT_DELAY_MIN.toFloat(), K.KEY_REPEAT_DELAY_MAX.toFloat(), 50f, FrontUnit.INT,
            K.KEY_REPEAT_DELAY_MS.toFloat(),
        ),
        slider(
            FrontKeys.KEY_REPEAT, "入力", "⌫の連打の間隔（ミリ秒）",
            "連打の最初の間隔です。押し続けると少しずつ短くなります。",
            true, K.KEY_REPEAT_MIN.toFloat(), K.KEY_REPEAT_MAX.toFloat(), 10f, FrontUnit.INT, K.KEY_REPEAT_MS.toFloat(),
        ),
        toggle(
            FrontKeys.SPACE_SWIPE, "入力", "スペースキーのスワイプでカーソル移動",
            "スペースキーを押したまま左右に動かすと、カーソルを動かします（離してもスペースは入りません）。" +
                "入力中は入力中の文字列の中を動きます。タップは通常のスペースです。",
            detail = true, default = K.SPACE_SWIPE,
        ),
        slider(
            FrontKeys.SPACE_SWIPE_STEP, "入力", "カーソル移動 1 文字あたりの距離（pt）",
            "スペースキーをスワイプしたとき、この距離ごとにカーソルが 1 つ動きます。小さいほど速く動きます。",
            true, K.SPACE_SWIPE_STEP_MIN, K.SPACE_SWIPE_STEP_MAX, 1f, FrontUnit.INT, K.SPACE_SWIPE_STEP_DP,
        ),
        toggle(
            FrontKeys.SPACE_SWIPE_ACCEL, "入力", "大きくスライドするほど速く動かす",
            "スペースキーのスワイプで、押し始めから大きく動かすほどカーソルを速く動かします。" +
                "近い距離では上の「1 文字あたりの距離」のままなので、1 文字だけずらすのも正確です。指を戻せば元の位置に戻ります。",
            detail = true, default = K.SPACE_SWIPE_ACCEL,
        ),
        slider(
            FrontKeys.SPACE_SWIPE_ACCEL_STRENGTH, "入力", "加速の強さ",
            "「大きくスライドするほど速く動かす」がオンのときの、速くなる度合いです。値が大きいほど急に速くなります。",
            true, K.SPACE_SWIPE_ACCEL_STRENGTH_MIN, K.SPACE_SWIPE_ACCEL_STRENGTH_MAX, 0.1f, FrontUnit.DEC1,
            K.SPACE_SWIPE_ACCEL_STRENGTH,
        ),
        toggle(
            FrontKeys.BACKSPACE_SWIPE, "入力", "⌫の左スワイプで単語を削除",
            "⌫を押したまま左へ大きく動かすと、カーソル手前の単語（同じ種類の文字の連なり）をまとめて消します。入力中は働きません。",
            detail = true, default = K.BACKSPACE_SWIPE,
        ),

        // ---- キーボード ----
        FrontSettingDef(
            FrontKeys.HEIGHT, "キーボード", "キーボードの高さ",
            "キー全体の高さです。細かく調整するときは「高さの倍率」を使います。",
            detail = false, kind = FrontKind.HEIGHT_PRESET,
            choices = HeightPreset.values().map { it.json to it.label },
            defaultChoice = HeightPreset.STANDARD.json,
        ),
        slider(
            FrontKeys.HEIGHT_SCALE, "キーボード", "高さの倍率",
            "「キーボードの高さ」にさらに掛ける倍率です。縦画面のときの値です。",
            true, K.HEIGHT_SCALE_MIN, K.HEIGHT_SCALE_MAX, 0.05f, FrontUnit.TIMES2, K.HEIGHT_SCALE,
        ),
        slider(
            FrontKeys.HEIGHT_SCALE_LANDSCAPE, "キーボード", "高さの倍率（横画面）",
            "横向きのときの高さの倍率です。",
            true, K.HEIGHT_SCALE_LANDSCAPE_MIN, K.HEIGHT_SCALE_LANDSCAPE_MAX, 0.05f, FrontUnit.TIMES2,
            K.HEIGHT_SCALE_LANDSCAPE,
        ),
        choice(
            FrontKeys.ONE_HANDED, "キーボード", "片手モード",
            "キー部分を画面の左または右に寄せます。空いた側の「⇄」で反対側へ、「↔」で通常に戻せます。",
            false, OneHanded.values().map { it.json to it.label }, K.ONE_HANDED,
        ),
        slider(
            FrontKeys.ONE_HANDED_WIDTH, "キーボード", "片手モードの幅",
            "片手モードのとき、キー部分が画面の幅に占める割合です。",
            true, OneHandedLogic.WIDTH_MIN, OneHandedLogic.WIDTH_MAX, 0.05f, FrontUnit.TIMES2, K.ONE_HANDED_WIDTH,
        ),
        slider(
            FrontKeys.KEY_SPACING, "キーボード", "キーの間隔（pt）",
            "キーとキーのあいだの余白です。",
            true, K.KEY_SPACING_MIN, K.KEY_SPACING_MAX, 0.5f, FrontUnit.DEC1, K.KEY_SPACING_DP,
        ),
        slider(
            FrontKeys.KEY_CORNER, "キーボード", "キーの角丸（pt）",
            "キーの角の丸さです。",
            true, K.KEY_CORNER_MIN, K.KEY_CORNER_MAX, 1f, FrontUnit.INT, K.KEY_CORNER_DP,
        ),
        toggle(
            FrontKeys.LAYOUT_BY_FIELD, "キーボード", "入力欄の種類でレイアウトを切り替える",
            "メール・URL の欄では英字、数字の欄では数字の配列で開きます。改行キーの表示も入力欄に合わせます。" +
                "欄ごとの割り当ては「キーボードのレイアウト」の画面で変えられます。",
            detail = true, default = true,
        ),
        choice(
            FrontKeys.HAPTIC, "キーボード", "キー入力の触覚",
            "キーを押したときの振動です。端末の「タッチ操作時の振動」がオフだと振動しません。",
            false, HapticLevel.values().map { it.json to it.label }, K.HAPTIC_LEVEL,
        ),
        choice(
            FrontKeys.SOUND, "キーボード", "キー入力の音",
            "キーを押したときにクリック音を鳴らします。音量と鳴るかどうかは端末の「タッチ操作音」の設定に従います。",
            false, listOf(SOUND_OFF to "オフ", SOUND_ON to "クリック音"), soundValue(K.KEY_SOUND),
        ),
        choice(
            FrontKeys.THEME, "キーボード", "テーマ",
            "キーボードの明るさです。「システム」は端末の外観（ライト/ダーク）に従います。",
            false, listOf("system" to "システムに合わせる", "light" to "ライト", "dark" to "ダーク"), K.THEME,
        ),
        choice(
            FrontKeys.ACCENT, "キーボード", "アクセントカラー",
            "確定キーや選択中の候補などの色です。",
            false, AccentColor.values().map { it.json to it.label }, K.ACCENT,
        ),
        toggle(
            FrontKeys.CANDBAR_PASTE_BUTTON, "キーボード", "ツールバーに「貼り付け」を出す",
            "ツールバーの項目に「貼り付け」を入れていても、これがオフなら出しません。クリップボードに文字があるときだけ出します。",
            detail = true, default = true,
        ),
        FrontSettingDef(
            FrontKeys.TOOLBAR_ITEMS, "キーボード", "ツールバーの項目",
            "入力中でなく、候補が無いときに候補バーの場所に出す操作です。表示する項目と並び順を選べます" +
                "（入力中の候補が出ると候補バーに切り替わります）。",
            detail = true, kind = FrontKind.ORDERED_LIST,
        ),
        FrontSettingDef(
            FrontKeys.LAYOUT_KANA, "キーボード", "かなモードのレイアウト",
            "使うレイアウトです。ローマ字入力にするには、かなに qwerty-kana を選びます。",
            detail = false, kind = FrontKind.LAYOUT, defaultChoice = LayoutIds.DEFAULT_KANA,
        ),
        FrontSettingDef(
            FrontKeys.LAYOUT_ALPHA, "キーボード", "英字モードのレイアウト",
            "使うレイアウトです。",
            detail = false, kind = FrontKind.LAYOUT, defaultChoice = LayoutIds.DEFAULT_ALPHA,
        ),

        // ---- クリップボード履歴 ----
        toggle(
            FrontKeys.CLIPBOARD_ENABLED, "クリップボード履歴", "クリップボード履歴を使う",
            "コピーした文字列をキーボードの履歴ページ（ツールバーの「履歴」）に残し、タップで入力できます。" +
                "オフの間はクリップボードを一切読まず、何も保存しません。",
            detail = false, default = ClipboardDefaults.ENABLED,
        ),
        choice(
            FrontKeys.CLIPBOARD_RETENTION, "クリップボード履歴", "保存期間",
            "この期間を過ぎた履歴は消えます。ピン留めした項目は消えません。",
            false, ClipboardDefaults.RETENTION_CHOICES, ClipboardDefaults.RETENTION,
        ),
        choice(
            FrontKeys.CLIPBOARD_MAX_ITEMS, "クリップボード履歴", "保存する最大数",
            "超えたぶんは古い順に消えます。ピン留めは数に含めず、消えません（ピン留めは 20 件まで）。",
            true, ClipboardDefaults.MAX_ITEMS_CHOICES, ClipboardDefaults.MAX_ITEMS.toString(),
        ),
        FrontSettingDef(
            FrontKeys.CLIPBOARD_TYPES, "クリップボード履歴", "保存する種類",
            "チェックを外した種類は保存しません。数字・記号は電話番号やコードなど、数字と記号だけの文字列です。",
            detail = true, kind = FrontKind.MULTI,
            choices = ClipboardKind.values().map { it.json to it.title },
        ),
        choice(
            FrontKeys.CLIPBOARD_PREVIEW_CHARS, "クリップボード履歴", "カードに表示する文字数",
            "履歴ページのカードに出す文字数の上限です。超えたぶんは「…」で省略します（入力するときは全文です）。",
            true, ClipboardDefaults.PREVIEW_CHOICES, ClipboardDefaults.PREVIEW_CHARS.toString(),
        ),
        action(FrontKeys.CLIPBOARD_CLEAR, "クリップボード履歴", "履歴をすべて削除", "ピン留めした項目も消えます"),

        // ---- 学習 ----
        action(FrontKeys.LEARNING_CLEAR, "学習", "学習データをすべて削除", "ユーザー辞書は消えません"),

        // ---- アップデート ----
        toggle(
            FrontKeys.UPDATE_AUTO_CHECK, "アップデート", "アップデートを自動で確認",
            "本体アプリを開いたとき、新しい版があるか確認します（24 時間に 1 回まで）。",
            detail = false, default = K.AUTO_CHECK_UPDATES,
        ),
        action(FrontKeys.UPDATE_OPEN, "アップデート", "アップデート", "新しい版の確認とインストール・データの更新"),

        // ---- 開発者 ----
        toggle(
            FrontKeys.DEVELOPER_MODE, "開発者", "開発者モード",
            "診断ログ・応答時間・メモリを見る画面など、開発者向けの機能を表示します。通常の使い方には要りません。",
            detail = true, default = false,
        ),
        action(FrontKeys.DEVELOPER_OPEN, "開発者", "開発者ツール", "診断ログ・応答時間・メモリ"),
    )

    /** 「キー入力の音」の選択肢の値 ⇄ 保存している boolean */
    fun soundValue(on: Boolean): String = if (on) SOUND_ON else SOUND_OFF

    fun soundEnabled(value: String): Boolean = value == SOUND_ON

    /** この項目を出すか。詳細の項目は詳細モードのときだけ。「開発者ツール」は開発者モードがオンのときだけ（シンプルでも出す） */
    fun isVisible(def: FrontSettingDef, mode: SettingsUiMode, developerMode: Boolean): Boolean = when (def.key) {
        FrontKeys.DEVELOPER_OPEN -> developerMode
        else -> mode.showDetail || !def.detail
    }

    /** スライダーの値の表示（単位はラベル側に書くので付けない） */
    fun format(unit: FrontUnit, v: Float): String = when (unit) {
        FrontUnit.INT -> String.format(Locale.ROOT, "%.0f", v)
        FrontUnit.DEC1 -> String.format(Locale.ROOT, "%.1f", v)
        FrontUnit.TIMES1 -> String.format(Locale.ROOT, "×%.1f", v)
        FrontUnit.TIMES2 -> String.format(Locale.ROOT, "×%.2f", v)
    }

    /** 「既定に戻す」の確認に出す、既定値の表示 */
    fun defaultText(def: FrontSettingDef): String = when (def.kind) {
        FrontKind.TOGGLE -> if (def.defaultBool) "オン" else "オフ"
        FrontKind.SLIDER -> format(def.unit, def.defaultFloat)
        FrontKind.CHOICE, FrontKind.HEIGHT_PRESET -> def.choices.firstOrNull { it.first == def.defaultChoice }?.second ?: def.defaultChoice
        FrontKind.LAYOUT -> def.defaultChoice
        FrontKind.ACTION -> ""
        FrontKind.MULTI -> ClipboardHistoryLogic.kindsSummary(ClipboardKind.values().toSet())
        FrontKind.ORDERED_LIST -> ToolbarLogic.summary(ToolbarLogic.DEFAULT)
    }

    /** グループの下に出す、反映のタイミングの注記（フロント固有の項目があるグループ） */
    fun footnote(group: String): String? = when (group) {
        "入力", "キーボード" -> RELOAD_NOTE
        "クリップボード履歴" -> CLIPBOARD_NOTE
        else -> null
    }

    /** スライダーの目盛りの数（0..steps） */
    fun steps(def: FrontSettingDef): Int = Math.round((def.max - def.min) / def.step)

    /** 目盛り [progress] の値 */
    fun valueAt(def: FrontSettingDef, progress: Int): Float = (def.min + progress * def.step).coerceIn(def.min, def.max)

    /** 値 [v] に近い目盛り */
    fun progressOf(def: FrontSettingDef, v: Float): Int = Math.round((v - def.min) / def.step).coerceIn(0, steps(def))
}

/** 設定タブの行・グループの組み立て */
internal object SettingsPlan {
    /**
     * コアの記述子 [core] とフロント固有の項目から、表示するグループと行を作る。
     * グループは [SettingsSchema.GROUP_ORDER] の固定順（未知のグループは最後に出現順）、グループ内はコア → フロント固有の順。
     * 「学習」「アップデート」は項目が無くても（シンプルでも）出る（削除ボタン・アップデートの行があるため）
     */
    fun build(core: List<SettingDef>, mode: SettingsUiMode, developerMode: Boolean): List<SettingsSection> {
        val map = LinkedHashMap<String, MutableList<SettingsEntry>>()
        for (d in SettingsSchema.visible(core, mode.showDetail)) {
            map.getOrPut(d.group) { ArrayList() }.add(SettingsEntry.Core(d))
        }
        for (f in FrontSettings.definitions) {
            if (FrontSettings.isVisible(f, mode, developerMode)) {
                map.getOrPut(f.group) { ArrayList() }.add(SettingsEntry.Front(f))
            }
        }
        return SettingsSchema.orderedGroupNames(map.keys).map {
            SettingsSection(it, map.getValue(it), FrontSettings.footnote(it))
        }
    }
}
