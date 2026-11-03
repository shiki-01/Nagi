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
    private var rpcMethod = "" // 診断ログ用: 直近に送った要求のメソッド名

    /** 起動時の診断情報（プロファイル等）。起動後に読める */
    @Volatile
    var startInfo: String = ""
        private set

    /** 起動に失敗した理由（成功なら null） */
    @Volatile
    var startError: String? = null
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

    /** `perform_action`。方向を取る操作（`move_focus` 等）は [content] に `left` / `right` を渡す */
    fun action(type: String, content: String? = null, done: (EngineResult?) -> Unit) {
        callKey("perform_action", session().put("action", actionJson(type, content)), done)
    }

    /**
     * 同じ操作を [times] 回続けて送り、応答を 1 つにまとめて返す（途中で他の要求が割り込まない）。
     * preedit / candidates は変化したときだけ入るので、後のものを優先して重ねる。
     */
    fun actionRepeat(type: String, content: String?, times: Int, done: (EngineResult?) -> Unit) {
        exec.execute {
            if (!ensureStarted()) {
                main.post { done(null) }
                return@execute
            }
            var merged: EngineResult? = null
            val allNotes = ArrayList<EngineResult>()
            for (i in 0 until times) {
                val (res, notes) = rpc("perform_action", session().put("action", actionJson(type, content)))
                allNotes.addAll(notes)
                val r = Protocol.parseResult(res)
                if (r != null) {
                    val prev = merged
                    merged = if (prev == null) r else prev.merge(r)
                }
            }
            val out = merged
            main.post {
                done(out)
                allNotes.forEach { dispatch(it) }
            }
        }
    }

    /** 12 キーの 1 押下。[angleDeg] / [distance] はフリックのときだけ渡す（曖昧入力の補正用） */
    fun insertKey12(
        key: String,
        gesture: String,
        angleDeg: Double?,
        distance: Double?,
        done: (EngineResult?) -> Unit,
    ) {
        val content = JSONObject().put("key", key).put("gesture", gesture)
        val input = JSONObject().put("type", "key12").put("content", content)
        val p = session().put("input", input)
        if (angleDeg != null && distance != null) {
            p.put("flick", JSONObject().put("angle_deg", angleDeg).put("distance", distance))
        }
        callKey("insert_reading", p, done)
    }

    /** 濁点・小書きキー（゛゜小） */
    fun insertVariant(done: (EngineResult?) -> Unit) {
        callKey("insert_reading", session().put("input", JSONObject().put("type", "variant")), done)
    }

    /** 物理キーボードの 1 キー */
    fun processKey(event: PhysicalKeyEvent, done: (EngineResult?) -> Unit) {
        callKey("process_key", event.toParams(SESSION_ID), done)
    }

    /** 確定済みテキストの再変換。返った preedit を入力中の文字として反映する */
    fun reconvert(text: String, done: (EngineResult?) -> Unit) {
        callKey("reconvert", session().put("text", text), done)
    }

    /** 起動後に取得した 12 キーの表。まだ（または取得できなかった）なら null */
    @Volatile
    var keyTable: List<KanaKeyDef>? = null
        private set

    /** コアを起動して（まだなら）、12 キーの表を返す。取得できなければ null */
    fun withKeyTable(done: (List<KanaKeyDef>?) -> Unit) {
        exec.execute {
            ensureStarted()
            val t = keyTable
            main.post { done(t) }
        }
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

    /** [restart][NagiRuntime.restart] で置き換えられた古いエンジン。以後は二度と起動しない */
    @Volatile
    private var retired = false

    /** 古いエンジンとして畳む: ハンドルを解放し、[onFreed] を解放後に呼ぶ（nagi-core スレッド） */
    fun retire(onFreed: () -> Unit) {
        retired = true
        shutdown()
        exec.execute { onFreed() }
    }

    /** nagi-core スレッドで [block] を実行する（置き換え後の新エンジンが、古いエンジンの解放を待つのに使う） */
    fun runOnCoreThread(block: () -> Unit) {
        exec.execute { block() }
    }

    private fun session() = JSONObject().put("session_id", SESSION_ID)

    private fun actionJson(type: String, content: String?): JSONObject {
        val o = JSONObject().put("type", type)
        if (content != null) o.put("content", content)
        return o
    }

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
        if (failed || retired) return false
        val startedAt = System.nanoTime()
        try {
            NagiCore.loadError?.let { throw IllegalStateException("ネイティブライブラリを読み込めません: $it") }
            DataInstaller.ensure(app)
            val h = NagiCore.nativeNew(
                DataInstaller.dataDir(app).path,
                DataInstaller.userDir(app).path,
                !ModelStore.hasModel(app), // 変換モデルがあるときだけ LLM を有効にする（無ければ辞書 + Viterbi）
                0L,
            )
            if (h == 0L) {
                startError = "コアの初期化に失敗しました（nativeNew が 0 を返しました）"
                DiagLog.event(app, "core", "起動失敗: $startError")
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
                startError = "コアの initialize に失敗しました"
                DiagLog.event(app, "core", "起動失敗: $startError")
                NagiCore.nativeFree(handle)
                handle = 0L
                failed = true
                return false
            }
            startInfo = NagiCore.nativeNotes(handle)
            // 12 キーの表は initialize の後に 1 回だけ取る（取れなければフロントの複製表を使う）
            keyTable = KeyTables.parse(rpc("get_key_table", JSONObject()).first)
            // initialize の後に登録する（前だと通知が積まれたままになる）
            NagiCore.nativeSetWaker(handle, Runnable { exec.execute { drain() } })
            DiagLog.event(app, "core", "起動成功 ${(System.nanoTime() - startedAt) / 1_000_000} ms / $startInfo")
            return true
        } catch (e: Throwable) {
            Log.e(TAG, "コアの起動に失敗", e)
            startError = e.message ?: e.toString()
            DiagLog.event(app, "core", "起動失敗: $startError")
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
        rpcMethod = method
        val t0 = System.nanoTime()
        val out = NagiCore.nativeHandleLine(handle, line)
        if (method != "initialize") LatencyLog.record((System.nanoTime() - t0) / 1_000_000.0)
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
                    if (result == null) {
                        Log.w(TAG, "エラー応答: ${o.optJSONObject("error")?.optString("message")}")
                        DiagLog.event(app, "rpc-error", "method=$rpcMethod code=${o.optJSONObject("error")?.optInt("code")}")
                    }
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
        private const val SESSION_ID = "android"
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

    /**
     * エンジンを作り直す（モデルの追加・選択、再起動が必要な設定の反映に使う）。
     * 古いエンジンのハンドルを解放してから、新しいエンジンが（遅延して）起動する。
     * 以後 [get] は新しいエンジンを返す。`onNotification` は引き継ぐが、既に [get] の戻り値を保持している側
     * （IME サービス等）は、古い参照を捨てて [get] し直すこと（古い参照からの呼び出しは失敗として扱われる）
     */
    fun restart(context: Context): NagiEngine = synchronized(this) {
        val old = engine
        DiagLog.event(context, "core", "restart")
        val fresh = NagiEngine(context)
        if (old != null) {
            fresh.onNotification = old.onNotification
            val freed = java.util.concurrent.CountDownLatch(1)
            old.retire { freed.countDown() }
            // 新エンジンの最初の仕事として、古いハンドルの解放（学習の保存を含む）を待つ
            fresh.runOnCoreThread { freed.await(10, java.util.concurrent.TimeUnit.SECONDS) }
        }
        engine = fresh
        fresh
    }
}
