package com.shiki01.nagi

import org.json.JSONException
import org.json.JSONObject

/**
 * 更新マニフェスト（latest.json）。コアの `update_check::Manifest` と同じ形に、Android 用の拡張
 * `data_assets` を任意で足したもの（コアは未知フィールドを無視する）。android.* 非依存。
 */
internal data class Manifest(
    val version: String,
    val notesUrl: String?,
    val downloadUrl: String?,
    /** 配布物（APK）の SHA-256（小文字 16 進 64 文字） */
    val sha256: String?,
    /** 配布物（APK）のバイト列全体への ed25519 署名（16 進 128 文字） */
    val signature: String?,
    val dataAssets: List<DataAsset>,
)

/** データ資産（辞書・モデルなど）。本体 APK とは独立に差し替える */
internal data class DataAsset(
    val name: String,
    val version: String,
    val url: String,
    val sha256: String,
    val signature: String,
    /** 対応する本体アプリの最小版（含む）。null なら下限なし */
    val minApp: String?,
    /** 対応する本体アプリの最大版（含む）。null なら上限なし */
    val maxApp: String?,
    /** `files/data/` からの相対パス（例: dict/nagi-dict.bin） */
    val dest: String,
) {
    /** 本体アプリの版 [appVersion] で使えるか */
    fun isCompatible(appVersion: String): Boolean {
        if (minApp != null && Version.compare(appVersion, minApp) < 0) return false
        if (maxApp != null && Version.compare(appVersion, maxApp) > 0) return false
        return true
    }
}

/** 更新確認の結果（新版の情報） */
internal data class UpdateInfo(val version: String, val notesUrl: String?, val manifest: Manifest)

internal object ManifestParser {
    /** `version` が無い・JSON として壊れているマニフェストは null。壊れた data_assets の要素は黙って捨てる */
    fun parse(text: String): Manifest? {
        val o = try {
            JSONObject(text)
        } catch (e: JSONException) {
            return null
        }
        val version = str(o, "version") ?: return null
        val assets = mutableListOf<DataAsset>()
        val arr = o.optJSONArray("data_assets")
        if (arr != null) {
            for (i in 0 until arr.length()) {
                val a = arr.optJSONObject(i) ?: continue
                parseAsset(a)?.let { assets.add(it) }
            }
        }
        return Manifest(
            version = version,
            notesUrl = str(o, "notes_url"),
            downloadUrl = str(o, "download_url"),
            sha256 = str(o, "sha256"),
            signature = str(o, "signature"),
            dataAssets = assets,
        )
    }

    private fun parseAsset(a: JSONObject): DataAsset? {
        val dest = str(a, "dest") ?: return null
        if (!isSafeDest(dest)) return null
        return DataAsset(
            name = str(a, "name") ?: return null,
            version = str(a, "version") ?: return null,
            url = str(a, "url") ?: return null,
            sha256 = str(a, "sha256") ?: return null,
            signature = str(a, "signature") ?: return null,
            minApp = str(a, "min_app"),
            maxApp = str(a, "max_app"),
            dest = dest,
        )
    }

    /** 無い・null・空白だけの値は null */
    private fun str(o: JSONObject, key: String): String? {
        if (!o.has(key) || o.isNull(key)) return null
        return o.optString(key).trim().ifEmpty { null }
    }

    /** データ配置先として安全な相対パスか（絶対パス・`..`・空要素・バックスラッシュを拒否） */
    fun isSafeDest(dest: String): Boolean {
        if (dest.isEmpty() || dest.contains('\\') || dest.contains('\u0000')) return false
        return dest.split('/').none { it.isEmpty() || it == "." || it == ".." }
    }
}
