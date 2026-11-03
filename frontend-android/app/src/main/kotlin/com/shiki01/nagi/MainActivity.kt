package com.shiki01.nagi

import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.fragment.app.Fragment
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.Lifecycle
import com.shiki01.nagi.tabs.AboutFragment
import com.shiki01.nagi.tabs.HomeFragment
import com.shiki01.nagi.tabs.InputTestFragment
import com.shiki01.nagi.tabs.LayoutsFragment
import com.shiki01.nagi.tabs.SettingsFragment

/**
 * 本体アプリの入口。下部のタブバー（ホーム / 入力テスト / 設定 / レイアウト / About）と、タブごとの Fragment を持つ。
 * Fragment はタブを初めて開いたときに作り、以後は show / hide で切り替える（入力欄の中身などの状態を保つ）。
 * サブ画面（辞書・モデル・追加データ・アップデート・データの使用量・開発者・貼り付け・初回案内）は別 Activity のまま開く。
 * 初回起動では初回案内（[OnboardingActivity]）を自動で出す。`nagi://layout` は [LayoutManagerActivity] が受けて、ここへ渡す。
 */
class MainActivity : FragmentActivity() {
    private lateinit var tabBar: TabBarView
    private var current: MainTab = MainTab.DEFAULT

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setUpWindow()
        current = MainTab.fromId(savedInstanceState?.getString(STATE_TAB)) ?: MainTab.DEFAULT

        val root = LinearLayout(this)
        root.orientation = LinearLayout.VERTICAL
        root.setBackgroundColor(cBg)
        val container = FrameLayout(this)
        container.id = R.id.main_container
        root.addView(container, LinearLayout.LayoutParams(LP_MATCH, 0, 1f))
        tabBar = TabBarView(this) { selectTab(it) }
        root.addView(tabBar, LinearLayout.LayoutParams(LP_MATCH, LP_WRAP))
        setContentView(root)

        // 左右（切り欠き・横向きのバー）はここで受け取る。上は各ページ、下はタブバーが受け取る。
        // キーボードが出ている間はタブバーを隠し、キーボードの上までを本体にする（入力テストで広く使える）
        ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            val imeUp = ime.bottom > 0
            root.setPadding(bars.left, 0, bars.right, if (imeUp) ime.bottom else 0)
            tabBar.visibility = if (imeUp) View.GONE else View.VISIBLE
            tabBar.setBottomInset(bars.bottom)
            insets
        }

        selectTab(current)
        if (savedInstanceState == null) {
            val route = handleIntent(intent)
            // 初回だけ使い始めの手順を自動で開く（画面の作り直しでは開かない）。リンクの取り込み確認を隠さない
            if (!route.layoutIntent && !AppPrefs.onboardingDone(this)) {
                startActivity(Intent(this, OnboardingActivity::class.java))
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIntent(intent)
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putString(STATE_TAB, current.id)
    }

    /** 端から端まで描き、システムバー分の余白は自分で足す（targetSdk 35 では強制） */
    private fun setUpWindow() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val ctl = WindowCompat.getInsetsController(window, window.decorView)
        ctl.isAppearanceLightStatusBars = !isNight
        ctl.isAppearanceLightNavigationBars = !isNight
        if (Build.VERSION.SDK_INT >= 29) window.isNavigationBarContrastEnforced = false
    }

    /** 起動 Intent の宛先タブを開き、レイアウトの取り込みリンクがあればレイアウトタブへ渡す */
    private fun handleIntent(i: Intent?): MainRoute {
        val route = MainRouting.resolve(
            isView = i?.action == Intent.ACTION_VIEW,
            data = i?.dataString,
            tabId = i?.getStringExtra(MainRouting.EXTRA_TAB),
        )
        route.tab?.let { selectTab(it) }
        if (route.layoutIntent && i != null) {
            (supportFragmentManager.findFragmentByTag(MainTab.LAYOUTS.id) as? LayoutsFragment)?.offerIntent(i)
        }
        return route
    }

    /** [tab] を表示し、ほかのタブは隠す。隠したタブは STARTED で止め、表示したタブだけ RESUMED にする（onResume で最新に更新する） */
    internal fun selectTab(tab: MainTab) {
        current = tab
        val fm = supportFragmentManager
        val tx = fm.beginTransaction()
        for (t in MainTab.ordered) {
            val existing = fm.findFragmentByTag(t.id)
            if (t == tab) {
                val f = existing ?: createFragment(t).also { tx.add(R.id.main_container, it, t.id) }
                tx.show(f)
                tx.setMaxLifecycle(f, Lifecycle.State.RESUMED)
            } else if (existing != null) {
                tx.hide(existing)
                tx.setMaxLifecycle(existing, Lifecycle.State.STARTED)
            }
        }
        // onNewIntent は onSaveInstanceState の後（停止中）に来ることがある。タブの表示は保存しないので状態の損失は許す
        tx.commitNowAllowingStateLoss()
        tabBar.select(tab)
    }

    private fun createFragment(tab: MainTab): Fragment = when (tab) {
        MainTab.HOME -> HomeFragment()
        MainTab.INPUT_TEST -> InputTestFragment()
        MainTab.SETTINGS -> SettingsFragment()
        MainTab.LAYOUTS -> LayoutsFragment()
        MainTab.ABOUT -> AboutFragment()
    }

    companion object {
        private const val STATE_TAB = "main_tab"

        /** [tab] を開く Intent（初回案内などの別 Activity から使う） */
        internal fun intentFor(context: Context, tab: MainTab): Intent =
            Intent(context, MainActivity::class.java)
                .putExtra(MainRouting.EXTRA_TAB, tab.id)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    }
}
