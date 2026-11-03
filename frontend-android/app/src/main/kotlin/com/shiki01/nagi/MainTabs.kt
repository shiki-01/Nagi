package com.shiki01.nagi

// 本体アプリのタブ（ホーム / 入力テスト / 設定 / レイアウト / About）の並びと、起動 Intent の振り分け。
// android.* に依存しない純粋なロジック。単体テスト対象（MainTabsTest）。iOS の `TabView`（ContentView.swift）と順序を揃える。

/** 下部タブバーの 1 項目。[id] は Fragment のタグと保存状態、Intent の extra に使う。[glyph] はアイコン代わりの文字 */
internal enum class MainTab(val id: String, val label: String, val glyph: String) {
    HOME("home", "ホーム", "⌂"),

    // U+FE0E は「文字として描く」指定（カラー絵文字にならず、アクセント色で塗れる）
    INPUT_TEST("input_test", "入力テスト", "⌨︎"),
    SETTINGS("settings", "設定", "⚙︎"),
    LAYOUTS("layouts", "レイアウト", "▦"),
    ABOUT("about", "About", "ⓘ"),
    ;

    companion object {
        /** 起動直後に出すタブ */
        val DEFAULT: MainTab = HOME

        /** 表示順 */
        val ordered: List<MainTab> get() = values().toList()

        fun fromId(id: String?): MainTab? = values().firstOrNull { it.id == id }

        /** 表示順に (タブ, 選択中か)。選ばれているのは [selected] の 1 つだけ */
        fun withSelection(selected: MainTab): List<Pair<MainTab, Boolean>> = ordered.map { it to (it == selected) }
    }
}

/**
 * 起動 Intent の解釈結果。[tab] は開くタブ（null なら今のタブのまま）、
 * [layoutIntent] はレイアウトタブへ `nagi://layout` の取り込み（確認ダイアログ付き）を渡すか
 */
internal data class MainRoute(val tab: MainTab?, val layoutIntent: Boolean)

internal object MainRouting {
    /** 開くタブを指す extra（値は [MainTab.id]） */
    const val EXTRA_TAB = "com.shiki01.nagi.extra.TAB"

    /**
     * `nagi://layout` の VIEW（受け入れる・拒否するのどちらも。拒否の理由はレイアウトタブで見せる）はレイアウトタブ、
     * それ以外は extra のタブ。どちらも無ければ [MainRoute.tab] は null
     */
    fun resolve(isView: Boolean, data: String?, tabId: String?): MainRoute {
        if (LayoutIntake.classify(isView, data) !is IntakeResult.None) return MainRoute(MainTab.LAYOUTS, true)
        return MainRoute(MainTab.fromId(tabId), false)
    }
}
