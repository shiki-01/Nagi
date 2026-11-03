package com.shiki01.nagi

import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.util.Base64

/** [LayoutLink.parse] の結果 */
internal sealed class DeepLinkResult {
    /** [json] は URL から取り出したレイアウト JSON、[layout] は検証済みのモデル（参照先の存在確認は取り込み時） */
    class Ok(val json: String, val layout: LayoutModel) : DeepLinkResult()

    class Err(val reason: String) : DeepLinkResult()
}

/**
 * `nagi://layout?data=<base64url の JSON>` の解釈（iOS `LayoutURL.swift` と同じ仕様。android.* に依存しない）。
 * スキーム `nagi`・ホスト `layout`・クエリ `data` だけを見る。`assets` を含むレイアウトは URL では渡せない。
 * App Links 用に、`https://nagi.shiki-01.com/layout#data=…`（フラグメント。クエリの `?data=…` も可）も同じ扱いで受ける。
 */
internal object LayoutLink {
    /** `data`（base64url 文字列）の長さの上限。デコード前に検査する */
    const val MAX_DATA_CHARS = 1024 * 1024

    private const val URL_EXTRA = 4096

    /** App Links の https リンクのホスト・パス（サイトの共有ページ `/layout`。`AndroidManifest.xml` の intent-filter と揃える） */
    const val WEB_HOST = "nagi.shiki-01.com"
    const val WEB_PATH = "/layout"

    /** [uri] が `https://nagi.shiki-01.com/layout` の形か（`data` の有無や中身は見ない） */
    fun isWebLink(uri: String): Boolean = splitWebLink(uri.trim()) != null

    /** https リンクを (クエリ, フラグメント) に分ける。ホスト・パスが違う、または https でなければ null */
    private fun splitWebLink(trimmed: String): Pair<String, String>? {
        val schemeEnd = trimmed.indexOf("://")
        if (schemeEnd < 0 || !trimmed.substring(0, schemeEnd).equals("https", ignoreCase = true)) return null
        val afterScheme = trimmed.substring(schemeEnd + 3)
        val fragment = if (afterScheme.contains('#')) afterScheme.substringAfter('#') else ""
        val beforeFragment = afterScheme.substringBefore('#')
        val query = if (beforeFragment.contains('?')) beforeFragment.substringAfter('?') else ""
        val hostAndPath = beforeFragment.substringBefore('?')
        val slash = hostAndPath.indexOf('/')
        val host = if (slash >= 0) hostAndPath.substring(0, slash) else hostAndPath
        val path = if (slash >= 0) hostAndPath.substring(slash) else ""
        if (!host.equals(WEB_HOST, ignoreCase = true)) return null
        if (path.trimEnd('/') != WEB_PATH) return null
        return Pair(query, fragment)
    }

    fun parse(uri: String): DeepLinkResult {
        val trimmed = uri.trim()
        val web = splitWebLink(trimmed)
        val schemeEnd = trimmed.indexOf("://")
        if (web == null && (schemeEnd < 0 || !trimmed.substring(0, schemeEnd).equals("nagi", ignoreCase = true))) {
            return DeepLinkResult.Err("対応していない URL です")
        }
        // クエリ全体の長さも先に見る（巨大な URL を解析・コピーする前に弾く）
        if (trimmed.length > MAX_DATA_CHARS + URL_EXTRA) {
            return DeepLinkResult.Err("data が大きすぎます（${MAX_DATA_CHARS / 1024} KB まで）")
        }
        val encoded: String?
        if (web != null) {
            // フラグメント（サーバーに送られない）を優先し、無ければクエリを見る
            encoded = queryValue(web.second, "data") ?: queryValue(web.first, "data")
        } else {
            val rest = trimmed.substring(schemeEnd + 3).substringBefore('#')
            val host = rest.substringBefore('?').trimEnd('/')
            if (!host.equals("layout", ignoreCase = true)) return DeepLinkResult.Err("対応していない URL です")
            val query = if (rest.contains('?')) rest.substringAfter('?') else ""
            encoded = queryValue(query, "data")
        }
        if (encoded.isNullOrEmpty()) return DeepLinkResult.Err("URL に data がありません")
        if (encoded.length > MAX_DATA_CHARS) return DeepLinkResult.Err("data が大きすぎます（${MAX_DATA_CHARS / 1024} KB まで）")
        val json = when (val d = decodeData(encoded)) {
            is Decoded.Fail -> return DeepLinkResult.Err(d.reason)
            is Decoded.Text -> d.text
        }
        return when (val r = LayoutParser.parse(json, null)) {
            is LayoutParseResult.Err -> DeepLinkResult.Err(r.reason)
            is LayoutParseResult.Ok -> {
                // 画像つきのレイアウトは URL では渡せない（ファイルか貼り付けで取り込む）
                if (hasAssets(json)) {
                    DeepLinkResult.Err("画像（assets）を含むレイアウトは URL では取り込めません")
                } else {
                    DeepLinkResult.Ok(json, r.layout)
                }
            }
        }
    }

    private fun hasAssets(json: String): Boolean = try {
        val o = JSONObject(json.removePrefix("\uFEFF"))
        o.has("assets") && !o.isNull("assets") && (o.optJSONObject("assets")?.length() ?: 1) > 0
    } catch (e: Exception) {
        false
    }

    /** `a=1&b=2` 形式のクエリから [name] の値を取り出す（パーセントエンコードを戻す） */
    private fun queryValue(query: String, name: String): String? {
        for (part in query.split('&')) {
            val eq = part.indexOf('=')
            val k = if (eq >= 0) part.substring(0, eq) else part
            if (k != name) continue
            val raw = if (eq >= 0) part.substring(eq + 1) else ""
            return try {
                // base64 の `+` はクエリでは空白に読まれるので戻す
                java.net.URLDecoder.decode(raw, "UTF-8").replace(' ', '+')
            } catch (e: IllegalArgumentException) {
                null
            }
        }
        return null
    }

    private sealed class Decoded {
        class Text(val text: String) : Decoded()

        class Fail(val reason: String) : Decoded()
    }

    /** base64url（パディングは無くてもよい）を UTF-8 の文字列に戻す */
    private fun decodeData(encoded: String): Decoded {
        var b64 = encoded.replace('-', '+').replace('_', '/')
        while (b64.length % 4 != 0) b64 += "="
        val bytes = try {
            Base64.getDecoder().decode(b64)
        } catch (e: IllegalArgumentException) {
            return Decoded.Fail("data を base64url として読み取れません")
        }
        return try {
            val decoder = Charsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
            Decoded.Text(decoder.decode(ByteBuffer.wrap(bytes)).toString())
        } catch (e: CharacterCodingException) {
            Decoded.Fail("data が UTF-8 の JSON ではありません")
        }
    }

    /** JSON から `https://nagi.shiki-01.com/layout#data=...`（App Links 用。サイトの共有ページと同じ形）を作る。テスト用 */
    fun encodeWeb(json: String): String {
        val b64 = Base64.getUrlEncoder().withoutPadding().encodeToString(json.toByteArray(Charsets.UTF_8))
        return "https://$WEB_HOST$WEB_PATH#data=$b64"
    }

    /** JSON から `nagi://layout?data=...` を作る（パディングなしの base64url。サイト側・テスト用） */
    fun encode(json: String): String {
        val b64 = Base64.getUrlEncoder().withoutPadding().encodeToString(json.toByteArray(Charsets.UTF_8))
        return "nagi://layout?data=$b64"
    }
}
