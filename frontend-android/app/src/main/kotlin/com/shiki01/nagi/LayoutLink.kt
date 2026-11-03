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
 */
internal object LayoutLink {
    /** `data`（base64url 文字列）の長さの上限。デコード前に検査する */
    const val MAX_DATA_CHARS = 1024 * 1024

    private const val URL_EXTRA = 4096

    fun parse(uri: String): DeepLinkResult {
        val trimmed = uri.trim()
        val schemeEnd = trimmed.indexOf("://")
        if (schemeEnd < 0 || !trimmed.substring(0, schemeEnd).equals("nagi", ignoreCase = true)) {
            return DeepLinkResult.Err("対応していない URL です")
        }
        // クエリ全体の長さも先に見る（巨大な URL を解析・コピーする前に弾く）
        if (trimmed.length > MAX_DATA_CHARS + URL_EXTRA) {
            return DeepLinkResult.Err("data が大きすぎます（${MAX_DATA_CHARS / 1024} KB まで）")
        }
        val rest = trimmed.substring(schemeEnd + 3).substringBefore('#')
        val host = rest.substringBefore('?').trimEnd('/')
        if (!host.equals("layout", ignoreCase = true)) return DeepLinkResult.Err("対応していない URL です")
        val query = if (rest.contains('?')) rest.substringAfter('?') else ""
        val encoded = queryValue(query, "data")
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

    /** JSON から `nagi://layout?data=...` を作る（パディングなしの base64url。サイト側・テスト用） */
    fun encode(json: String): String {
        val b64 = Base64.getUrlEncoder().withoutPadding().encodeToString(json.toByteArray(Charsets.UTF_8))
        return "nagi://layout?data=$b64"
    }
}
