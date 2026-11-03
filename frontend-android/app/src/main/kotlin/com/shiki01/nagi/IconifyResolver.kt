package com.shiki01.nagi

import android.content.Context
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.atomic.AtomicBoolean

/** [IconifyResolver.resolve] の結果 */
internal class IconResolution(
    /** 新しく取得するアイコンが上限（[IconifyLogic.MAX_FETCHES_PER_IMPORT]）を超えた */
    val tooMany: Boolean,
    /** 取得できなかったアイコン（`prefix:name`。出現順） */
    val failed: List<String>,
    /** 新しく保存したアイコンの数 */
    val saved: Int,
)

/**
 * レイアウトが使う Iconify アイコンを Iconify の API から取得して [IconStore] に保存する（iOS の `IconifyResolver` 相当）。
 * 通信するのは取り込み時と、同梱レイアウトの初回表示時だけ。キーボードの描画では通信しない。
 * ネットワークを使うので、メインスレッド以外で呼ぶこと。
 */
internal object IconifyResolver {
    private const val CONNECT_TIMEOUT_MS = 10_000
    private const val READ_TIMEOUT_MS = 10_000

    private val presetPrefetchStarted = AtomicBoolean(false)

    /**
     * [layout] のアイコンのうち、未取得のものを取得して保存する。取得済みは何もしない。
     * 失敗（オフライン・存在しない名前）は [IconResolution.failed] に載せるだけで、例外にしない
     */
    fun resolve(ctx: Context, layout: LayoutModel): IconResolution {
        val missing = IconifyLogic.collect(layout).filter { !IconStore.exists(ctx, it) }
        if (missing.isEmpty()) return IconResolution(false, emptyList(), 0)
        if (missing.size > IconifyLogic.MAX_FETCHES_PER_IMPORT) return IconResolution(true, emptyList(), 0)
        val failed = ArrayList<String>()
        var saved = 0
        var offline = false
        // アイコンセット（prefix）ごとに 1 回の要求でまとめて取得する
        for ((prefix, refs) in missing.groupBy { it.prefix }) {
            if (offline) {
                refs.forEach { failed.add(it.id) }
                continue
            }
            val names = refs.map { it.name }
            val text = try {
                get(IconifyLogic.apiUrl(prefix, names))
            } catch (e: IOException) {
                // 通信できない。残りのセットは待たずに失敗にする
                offline = true
                null
            } catch (e: SecurityException) {
                offline = true
                null
            }
            if (text == null) {
                refs.forEach { failed.add(it.id) }
                continue
            }
            val res = IconifyLogic.parseResponse(text, names)
            // 出典・ライセンスは、アイコンより先に保存する（失敗してもアイコンの取得は続ける。後で補う）
            if (res.shapes.isNotEmpty()) ensureLicense(ctx, prefix, text)
            for (ref in refs) {
                val shape = res.shapes[ref.name]
                if (shape != null && IconStore.save(ctx, ref, shape)) saved++ else failed.add(ref.id)
            }
        }
        return IconResolution(false, failed, saved)
    }

    /**
     * [prefix] のライセンス情報を保存する。保存済みなら何もしない。
     * 取得済みの応答 [iconsResponse]（アイコン API の本文。null なら無し）に `info` があればそれを使い、
     * 無ければ `/collection?prefix=…&info=true` を 1 回取得する（iOS と同じ）。通信できない・読めないときは false
     */
    private fun ensureLicense(ctx: Context, prefix: String, iconsResponse: String?): Boolean {
        val repo = IconStore.iconSets(ctx)
        if (repo.has(prefix)) return true
        var info = if (iconsResponse != null) IconSetLogic.parseInfo(prefix, iconsResponse) else null
        if (info == null) {
            val text = try {
                get(IconSetLogic.collectionUrl(prefix))
            } catch (e: IOException) {
                null
            } catch (e: SecurityException) {
                null
            }
            if (text != null) info = IconSetLogic.parseInfo(prefix, text)
        }
        return info != null && repo.save(info)
    }

    /**
     * 保存済みのアイコンのうち、ライセンス情報の無いアイコンセットの分を取得して補う（この版より前に取り込んだもの）。
     * ネットワークが使えない・失敗したときは何もしない（動作は変えない）。ネットワークを使うので、メインスレッド以外で呼ぶこと
     */
    fun backfillLicenses(ctx: Context) {
        val repo = IconStore.iconSets(ctx)
        val have = repo.load().map { it.prefix }.toSet()
        for (prefix in IconStore.storedPrefixes(ctx)) {
            if (prefix in have) continue
            // 通信できない状態で何度も待たないよう、1 件でも失敗したら打ち切る（次の機会に再試行）
            if (!ensureLicense(ctx, prefix, null)) return
        }
    }

    /** HTTP 200 の本文を文字列で返す。それ以外・大きすぎる応答は null（404 は存在しないセット）。通信できなければ IOException */
    private fun get(url: String): String? {
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.connectTimeout = CONNECT_TIMEOUT_MS
            conn.readTimeout = READ_TIMEOUT_MS
            conn.instanceFollowRedirects = true
            conn.setRequestProperty("User-Agent", "Nagi-Android")
            conn.setRequestProperty("Accept", "application/json")
            if (conn.responseCode != HttpURLConnection.HTTP_OK) return null
            return conn.inputStream.use { input ->
                val out = ByteArrayOutputStream()
                val buf = ByteArray(8192)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    if (out.size() > IconifyLogic.MAX_RESPONSE_BYTES) return@use null
                }
                out.toString("UTF-8")
            }
        } finally {
            conn.disconnect()
        }
    }

    /**
     * 同梱のレイアウトが Iconify のアイコンを使っているとき、初回のキーボード表示でバックグラウンドで取得して保存する。
     * プロセスにつき 1 回だけ。失敗しても動作は変えず、次にプロセスが起動したときに再試行する。
     * 保存できたらレイアウトの版を進めて、キーボードに組み立て直させる
     */
    fun prefetchPresetIcons(ctx: Context) {
        if (!presetPrefetchStarted.compareAndSet(false, true)) return
        val app = ctx.applicationContext
        val t = Thread({
            try {
                var saved = 0
                for (id in LayoutStore.presetIds(app)) {
                    val text = try {
                        app.assets.open("presets/$id.json").use { it.readBytes().toString(Charsets.UTF_8) }
                    } catch (e: IOException) {
                        continue
                    }
                    if (!text.contains("\"iconify:")) continue
                    val layout = LayoutParser.parseOrNull(text) ?: continue
                    saved += resolve(app, layout).saved
                }
                // 取り込み済みで出典の無いアイコンセット（以前の版で保存したもの）を補う。表示には影響しないので版は進めない
                backfillLicenses(app)
                if (saved > 0) KeyboardPrefs.bumpLayoutsRevision(app)
            } catch (e: Exception) {
                // 失敗しても動作は変えない
            }
        }, "nagi-icon-prefetch")
        t.isDaemon = true
        t.start()
    }
}
