package com.shiki01.nagi

import android.app.ActivityManager
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.util.Log
import com.shiki01.nagi.core.NagiCore
import org.json.JSONObject
import java.util.TimeZone
import java.util.concurrent.Executors

/**
 * コアとの JSON-RPC 接続。コアの呼び出しはすべて専用の 1 スレッドで直列に行い（`nagi_handle_line` と
 * `nagi_poll_notifications` を同時に呼べないため）、結果はメインスレッドで返す。
 */
internal class NagiEngine(context: Context) {
    private val app = context.applicationContext
    private val exec = Executors.newSingleThreadExecutor { r ->
        Thread(r, "nagi-core").apply { isDaemon = true }
    }
    private val main = Handler(Looper.getMainLooper())

    // 以下は nagi-core スレッドからのみ触る
    private var handle = 0L
    private var failed = false
    private var nextId = 1

    /** 起動時の診断情報（プロファイル等）。起動後に読める */
    @Volatile
    var startInfo: String = ""
        private set

    /** 非同期通知（ライブ変換・LLM の遅延結果）の受け口。メインスレッドで呼ばれる */
    @Volatile
    var onNotification: ((EngineResult) -> Unit)? = null

    /** 結果（成功した `result`、失敗・未起動は null）をメインスレッドで返す */
    fun call(method: String, params: JSONObject = JSONObject(), done: ((JSONObject?) -> Unit)? = null) {
        exec.execute {
            if (!ensureStarted()) {
                if (done != null) main.post { done(null) }
                return@execute
            }
            val (result, notes) = rpc(method, params)
            main.post {
                done?.invoke(result)
                notes.forEach { dispatch(it) }
            }
        }
    }

    /** 描画差分を返す要求（`insert_reading` / `perform_action` / `select_candidate` 等） */
    fun callKey(method: String, params: JSONObject, done: (EngineResult?) -> Unit) {
        call(method, params) { done(Protocol.parseResult(it)) }
    }

    fun insertText(text: String, done: (EngineResult?) -> Unit) {
        val input = JSONObject().put("type", "text").put("content", text)
        callKey("insert_reading", session().put("input", input), done)
    }

    fun action(type: String, done: (EngineResult?) -> Unit) {
        callKey("perform_action", session().put("action", JSONObject().put("type", type)), done)
    }

    fun selectCandidate(index: Int, done: (EngineResult?) -> Unit) {
        callKey("select_candidate", session().put("index", index), done)
    }

    fun reset(done: ((JSONObject?) -> Unit)? = null) = call("reset", session(), done)

    fun focusIn() = call("focus_in", session())

    fun focusOut() = call("focus_out", session())

    fun setContext(secure: Boolean, textBefore: String?) {
        val p = session().put("secure_input", secure)
        if (!secure && !textBefore.isNullOrEmpty()) {
            p.put(
                "surrounding_text",
                JSONObject().put("text", textBefore).put("cursor", textBefore.length),
            )
        }
        // 通知（id なし）として送る
        exec.execute {
            if (ensureStarted()) notify("set_context", p)
        }
    }

    fun setEnvironment() {
        exec.execute {
            if (ensureStarted()) notify("set_environment", environment())
        }
    }

    fun shutdown() {
        exec.execute {
            if (handle != 0L) {
                NagiCore.nativeSetWaker(handle, null)
                NagiCore.nativeFree(handle)
                handle = 0L
            }
        }
    }

    private fun session() = JSONObject().put("session_id", "android")

    private fun environment(): JSONObject {
        val env = JSONObject()
        try {
            val pm = app.getSystemService(Context.POWER_SERVICE) as PowerManager
            env.put("low_power_mode", pm.isPowerSaveMode)
            val am = app.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            val info = ActivityManager.MemoryInfo()
            am.getMemoryInfo(info)
            // 端末全体の空きの半分を目安にする（コアは LLM の可否とプロファイル選択に使う）
            env.put("memory_budget_mb", (info.availMem / 2 / (1024 * 1024)).coerceIn(64L, 4096L))
        } catch (e: Exception) {
            Log.w(TAG, "環境情報の取得に失敗", e)
        }
        env.put("utc_offset_minutes", TimeZone.getDefault().rawOffset / 60000)
        return env
    }

    // ---- 以下は nagi-core スレッドで実行する ----

    private fun ensureStarted(): Boolean {
        if (handle != 0L) return true
        if (failed) return false
        try {
            DataInstaller.ensure(app)
            val h = NagiCore.nativeNew(
                DataInstaller.dataDir(app).path,
                DataInstaller.userDir(app).path,
                true, // LLM は未対応（M2）。辞書 + Viterbi の minimal で動く
                0L,
            )
            if (h == 0L) {
                failed = true
                return false
            }
            handle = h
            val caps = JSONObject()
                .put("input_method", "touch")
                .put("environment", environment())
                .put("partial_suggestions", true)
            val init = JSONObject()
                .put("protocol_version", 1)
                .put("client_name", "nagi-android")
                .put("client_version", appVersion())
                .put("os", "android")
                .put("capabilities", caps)
            val (res, _) = rpc("initialize", init)
            if (res == null) {
                NagiCore.nativeFree(handle)
                handle = 0L
                failed = true
                return false
            }
            startInfo = NagiCore.nativeNotes(handle)
            // initialize の後に登録する（前だと通知が積まれたままになる）
            NagiCore.nativeSetWaker(handle, Runnable { exec.execute { drain() } })
            return true
        } catch (e: Throwable) {
            Log.e(TAG, "コアの起動に失敗", e)
            failed = true
            return false
        }
    }

    private fun appVersion(): String = try {
        app.packageManager.getPackageInfo(app.packageName, 0).versionName ?: "0"
    } catch (e: Exception) {
        "0"
    }

    /** 要求を送り、`result`（エラーなら null）と、同じ戻り値に続いた通知を返す */
    private fun rpc(method: String, params: JSONObject): Pair<JSONObject?, List<EngineResult>> {
        val id = nextId++
        val line = JSONObject()
            .put("jsonrpc", "2.0")
            .put("id", id)
            .put("method", method)
            .put("params", params)
            .toString()
        val out = NagiCore.nativeHandleLine(handle, line)
        return parseLines(out, id)
    }

    private fun notify(method: String, params: JSONObject) {
        val line = JSONObject()
            .put("jsonrpc", "2.0")
            .put("method", method)
            .put("params", params)
            .toString()
        val (_, notes) = parseLines(NagiCore.nativeHandleLine(handle, line), -1)
        if (notes.isNotEmpty()) main.post { notes.forEach { dispatch(it) } }
    }

    private fun drain() {
        if (handle == 0L) return
        val (_, notes) = parseLines(NagiCore.nativePollNotifications(handle), -1)
        if (notes.isNotEmpty()) main.post { notes.forEach { dispatch(it) } }
    }

    private fun parseLines(text: String, expectedId: Int): Pair<JSONObject?, List<EngineResult>> {
        var result: JSONObject? = null
        val notes = ArrayList<EngineResult>()
        for (raw in text.split('\n')) {
            val line = raw.trim()
            if (line.isEmpty()) continue
            val o = try {
                JSONObject(line)
            } catch (e: Exception) {
                Log.w(TAG, "JSON として読めない行を捨てた")
                continue
            }
            if (o.has("id") && !o.isNull("id")) {
                if (o.optInt("id", -2) == expectedId) {
                    result = o.optJSONObject("result")
                    if (result == null) Log.w(TAG, "エラー応答: ${o.optJSONObject("error")?.optString("message")}")
                }
            } else {
                val method = o.optString("method", "")
                Protocol.parseNotification(method, o.optJSONObject("params"))?.let { notes.add(it) }
            }
        }
        return Pair(result, notes)
    }

    private fun dispatch(r: EngineResult) {
        onNotification?.invoke(r)
    }

    companion object {
        private const val TAG = "NagiEngine"
    }
}

/** IME サービスと設定画面で 1 つのエンジンを共有する（同じプロセス） */
internal object NagiRuntime {
    @Volatile
    private var engine: NagiEngine? = null

    fun get(context: Context): NagiEngine =
        engine ?: synchronized(this) {
            engine ?: NagiEngine(context).also { engine = it }
        }
}
