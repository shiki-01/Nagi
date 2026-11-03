package com.shiki01.nagi

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager

// 環境（メモリ逼迫・熱・電源）の変化をコアの `set_environment` に伝える（OT-01〜OT-03）。
// 純粋な写像・判定は [EnvLogic]（android.* に依存しない。単体テスト対象）、OS との接続は [EnvironmentMonitor]。

/** `set_environment` に載せる、最後に観測した値（コアを作り直したときも [NagiEngine] が同じ値を送り直す） */
internal object EnvironmentState {
    @Volatile
    var memoryPressure: String? = null

    @Volatile
    var thermal: String? = null

    @Volatile
    var onBattery: Boolean? = null
}

internal object EnvLogic {
    // ComponentCallbacks2 の定数と同じ値（android.* に依存せずにテストするため）
    const val TRIM_RUNNING_LOW = 10
    const val TRIM_RUNNING_CRITICAL = 15
    const val TRIM_UI_HIDDEN = 20
    const val TRIM_BACKGROUND = 40
    const val TRIM_MODERATE = 60

    /** メモリ逼迫を「通常」へ戻すまでの猶予（ミリ秒）。最後の通知からこの間は報告を維持する */
    const val PRESSURE_HOLD_MS = 10_000L

    /**
     * `onTrimMemory` の段階を `memory_pressure`（normal / warning / critical）に写す。
     * RUNNING_CRITICAL と、強制終了が近い MODERATE 以上は critical。RUNNING_LOW・UI_HIDDEN・BACKGROUND は warning。
     * それより軽い段階（RUNNING_MODERATE など）は null（状態を変えない）
     */
    fun memoryPressureForTrim(level: Int): String? = when {
        level >= TRIM_MODERATE -> "critical"
        level >= TRIM_BACKGROUND -> "warning"
        level >= TRIM_UI_HIDDEN -> "warning"
        level >= TRIM_RUNNING_CRITICAL -> "critical"
        level >= TRIM_RUNNING_LOW -> "warning"
        else -> null
    }

    private fun severity(p: String?): Int = when (p) {
        "critical" -> 2
        "warning" -> 1
        else -> 0
    }

    /** 保持中の逼迫 [current] に新しい [incoming] を重ねる。重い方を残す（保持中に軽い通知で下げない） */
    fun mergePressure(current: String?, incoming: String): String =
        if (severity(incoming) >= severity(current)) incoming else (current ?: "normal")

    /** `PowerManager.THERMAL_STATUS_*`（API 29）を `thermal`（nominal / fair / serious / critical）に写す */
    fun thermalForStatus(status: Int): String = when {
        status >= 4 -> "critical" // CRITICAL / EMERGENCY / SHUTDOWN
        status == 3 -> "serious" // SEVERE
        status == 2 -> "fair" // MODERATE
        else -> "nominal" // NONE / LIGHT
    }

    /** `BatteryManager.EXTRA_PLUGGED` が 0（どこにも接続されていない）ならバッテリー駆動 */
    fun onBatteryForPlugged(plugged: Int): Boolean = plugged == 0

    /**
     * 端末のメモリ状況から `memory_budget_mb` を決める。空きの半分を目安にし、システムが低メモリと
     * 報告しているとき（[lowMemory]）は空きの 1/4 まで絞る。64〜4096 に丸める（64 は LLM の必要量未満なので、
     * コアは LLM を使わない側へ寄せる）
     */
    fun memoryBudgetMb(availBytes: Long, lowMemory: Boolean): Long {
        val availMb = availBytes / (1024 * 1024)
        val budget = if (lowMemory) availMb / 4 else availMb / 2
        return budget.coerceIn(64L, 4096L)
    }

    /** 電源状態の変化を検出して不要な送信を避ける（ACTION_BATTERY_CHANGED は頻繁に届く） */
    data class Power(val lowPower: Boolean, val onBattery: Boolean)
}

/**
 * 環境の変化を購読してコアに伝える。サービスが生きている間だけ [start]〜[stop] する。
 * [engine] はコアを作り直したあとも最新のエンジンを返すこと。
 */
internal class EnvironmentMonitor(private val context: Context, private val engine: () -> NagiEngine) {
    private val app = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private var started = false
    private var releaseGeneration = 0
    private var thermalListener: Any? = null
    private var lastPower: EnvLogic.Power? = null

    private val powerReceiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context?, intent: Intent?) {
            if (intent == null) return
            handlePower(intent)
        }
    }

    fun start() {
        if (started) return
        started = true
        startThermal()
        try {
            val filter = IntentFilter().apply {
                addAction(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED)
                addAction(Intent.ACTION_BATTERY_CHANGED)
            }
            // ACTION_BATTERY_CHANGED は登録時に現在の値（sticky）が返る。起動時の送信は既存の setEnvironment が行うので、
            // ここでは基準の値として覚えるだけにする
            val sticky = app.registerReceiver(powerReceiver, filter)
            if (sticky != null) {
                val plugged = sticky.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0)
                val onBattery = EnvLogic.onBatteryForPlugged(plugged)
                EnvironmentState.onBattery = onBattery
                lastPower = EnvLogic.Power(isPowerSave(), onBattery)
            }
        } catch (e: Exception) {
            DiagLog.event(app, "env", "電源の購読に失敗: ${e.javaClass.simpleName}")
        }
    }

    fun stop() {
        if (!started) return
        started = false
        releaseGeneration++
        main.removeCallbacksAndMessages(null)
        try {
            app.unregisterReceiver(powerReceiver)
        } catch (e: Exception) {
            // 未登録なら何もしない
        }
        stopThermal()
    }

    // ---- メモリ（OT-01） ----

    /** `Service.onTrimMemory` から呼ぶ */
    fun onTrimMemory(level: Int) {
        val incoming = EnvLogic.memoryPressureForTrim(level) ?: return
        val merged = EnvLogic.mergePressure(EnvironmentState.memoryPressure, incoming)
        EnvironmentState.memoryPressure = merged
        DiagLog.event(app, "env", "memory_pressure=$merged trim=$level")
        engine().setEnvironment()
        // 一定時間、新しい通知が無ければ通常へ戻す。LLM の読み直しはコアの自動再読込に任せる
        val gen = ++releaseGeneration
        main.postDelayed({
            if (gen != releaseGeneration || !started) return@postDelayed
            EnvironmentState.memoryPressure = "normal"
            DiagLog.event(app, "env", "memory_pressure=normal (released)")
            engine().setEnvironment()
        }, EnvLogic.PRESSURE_HOLD_MS + 1000)
    }

    // ---- 熱（OT-02。API 29 以上） ----

    private fun startThermal() {
        if (Build.VERSION.SDK_INT < 29) return
        try {
            val pm = app.getSystemService(Context.POWER_SERVICE) as? PowerManager ?: return
            val listener = PowerManager.OnThermalStatusChangedListener { status -> onThermal(status) }
            pm.addThermalStatusListener(listener)
            thermalListener = listener
            // 現在値は起動時の setEnvironment に載せる
            EnvironmentState.thermal = EnvLogic.thermalForStatus(pm.currentThermalStatus)
        } catch (e: Exception) {
            DiagLog.event(app, "env", "熱状態の購読に失敗: ${e.javaClass.simpleName}")
        }
    }

    private fun stopThermal() {
        if (Build.VERSION.SDK_INT < 29) return
        val l = thermalListener as? PowerManager.OnThermalStatusChangedListener ?: return
        thermalListener = null
        try {
            (app.getSystemService(Context.POWER_SERVICE) as? PowerManager)?.removeThermalStatusListener(l)
        } catch (e: Exception) {
            // 解除の失敗は無視する
        }
    }

    private fun onThermal(status: Int) {
        val t = EnvLogic.thermalForStatus(status)
        if (t == EnvironmentState.thermal) return
        EnvironmentState.thermal = t
        DiagLog.event(app, "env", "thermal=$t status=$status")
        engine().setEnvironment()
    }

    // ---- 電源（OT-03） ----

    private fun isPowerSave(): Boolean =
        (app.getSystemService(Context.POWER_SERVICE) as? PowerManager)?.isPowerSaveMode ?: false

    private fun handlePower(intent: Intent) {
        val onBattery = if (intent.action == Intent.ACTION_BATTERY_CHANGED) {
            EnvLogic.onBatteryForPlugged(intent.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0))
        } else {
            EnvironmentState.onBattery ?: lastPower?.onBattery ?: false
        }
        val now = EnvLogic.Power(isPowerSave(), onBattery)
        if (now == lastPower) return
        lastPower = now
        EnvironmentState.onBattery = onBattery
        DiagLog.event(app, "env", "lowPower=${now.lowPower} onBattery=${now.onBattery}")
        engine().setEnvironment()
    }
}
