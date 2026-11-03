package com.shiki01.nagi

import java.util.Locale
import org.json.JSONException
import org.json.JSONObject

// Iconify アイコンの純粋ロジック（android.* に依存しない。org.json のみ。単体テスト対象）。
// 取り込み時に Iconify の API から SVG の body を取得し、path 群（viewBox 付き）に直して端末に保存する。
// 描画は [IconGlyph]、取得・保存は [IconifyResolver] / [IconStore]。

/** Iconify のアイコン名（`prefix:name`） */
internal data class IconRef(val prefix: String, val name: String) {
    /** `prefix:name`（表示・通知用） */
    val id: String get() = "$prefix:$name"

    /** 保存ファイル名（prefix・name は `[a-z0-9-]` だけなので `__` で曖昧にならない） */
    val fileName: String get() = "${prefix}__$name.json"
}

/** アイコンの 1 本の path。色は描くときに文字色に置き換える（単色で描く） */
internal data class IconPath(
    /** SVG の path data */
    val d: String,
    val fill: Boolean,
    val stroke: Boolean,
    /** viewBox の単位での線の太さ */
    val strokeWidth: Double,
    /** `butt` / `round` / `square` */
    val cap: String,
    /** `miter` / `round` / `bevel` */
    val join: String,
    val evenOdd: Boolean,
)

/** アイコンの描画データ。viewBox は (left, top, width, height) */
internal data class IconShape(
    val left: Double,
    val top: Double,
    val width: Double,
    val height: Double,
    val paths: List<IconPath>,
)

/** [IconifyLogic.parseResponse] の結果 */
internal class IconifyResponse(
    /** 取り出せたアイコン（名前 → 描画データ） */
    val shapes: Map<String, IconShape>,
    /** 取り出せなかった名前（API が not_found と返した・描けない・応答に無い） */
    val missing: List<String>,
)

internal object IconifyLogic {
    /** 1 回の取り込みで新しく取得するアイコンの上限（iOS と同じ） */
    const val MAX_FETCHES_PER_IMPORT = 64

    /** 応答の大きさの上限（バイト） */
    const val MAX_RESPONSE_BYTES = 512 * 1024

    private const val HOST = "api.iconify.design"
    private val PART = Regex("[a-z0-9]+(-[a-z0-9]+)*")

    /** `iconify:<prefix>:<name>` を読む。書式が違えば null（`sf:` や接頭辞なしは対象外） */
    fun parse(icon: String?): IconRef? {
        if (icon == null || !icon.startsWith("iconify:")) return null
        val parts = icon.removePrefix("iconify:").split(':')
        if (parts.size != 2) return null
        return ref(parts[0], parts[1])
    }

    /** prefix・name の検証つきで [IconRef] を作る */
    fun ref(prefix: String, name: String): IconRef? =
        if (PART.matches(prefix) && PART.matches(name)) IconRef(prefix, name) else null

    /** レイアウトが使う Iconify アイコン（キーと長押し項目の `icon`）。重複なし・出現順 */
    fun collect(layout: LayoutModel): List<IconRef> {
        val out = LinkedHashSet<IconRef>()
        for (row in layout.rows) {
            for (key in row.keys) {
                parse(key.icon)?.let { out.add(it) }
                for (item in key.longPress) parse(item.icon)?.let { out.add(it) }
            }
        }
        return out.toList()
    }

    /** `https://api.iconify.design/<prefix>.json?icons=a,b`（[names] は検証済みの名前） */
    fun apiUrl(prefix: String, names: List<String>): String =
        "https://$HOST/$prefix.json?icons=${names.joinToString(",")}"

    /** 取り込み結果に添える通知（取得できなかったアイコンが無ければ空文字） */
    fun failureNote(failed: List<String>): String =
        if (failed.isEmpty()) "" else "\n取得できなかったアイコン: ${failed.joinToString("、")}（label などで代替します）"

    // ---- API 応答 ----

    /**
     * `GET /<prefix>.json?icons=...` の応答から、[wanted] の各アイコンの描画データを取り出す。
     * 形式: `{"prefix":"mdi","width":24,"height":24,"icons":{"home":{"body":"<path .../>","width":..}},"aliases":{..},"not_found":[..]}`。
     * JSON として読めない応答は全部 missing にする
     */
    fun parseResponse(text: String, wanted: List<String>): IconifyResponse {
        val root = try {
            JSONObject(text)
        } catch (e: JSONException) {
            return IconifyResponse(emptyMap(), wanted)
        }
        val defW = root.optDouble("width", 16.0)
        val defH = root.optDouble("height", 16.0)
        val defL = root.optDouble("left", 0.0)
        val defT = root.optDouble("top", 0.0)
        val icons = root.optJSONObject("icons")
        val aliases = root.optJSONObject("aliases")
        val shapes = LinkedHashMap<String, IconShape>()
        val missing = ArrayList<String>()
        for (name in wanted) {
            var node = icons?.optJSONObject(name)
            // 別名は親のアイコンを指す（回転・反転などの上書きには対応しない）
            var hops = 0
            var alias = aliases?.optJSONObject(name)
            while (node == null && alias != null && hops < 4) {
                val parent = alias.optString("parent", "")
                node = icons?.optJSONObject(parent)
                alias = aliases?.optJSONObject(parent)
                hops++
            }
            val body = node?.optString("body", "") ?: ""
            if (node == null || body.isEmpty()) {
                missing.add(name)
                continue
            }
            val shape = shapeFromBody(
                body,
                node.optDouble("left", defL),
                node.optDouble("top", defT),
                node.optDouble("width", defW),
                node.optDouble("height", defH),
            )
            if (shape == null) missing.add(name) else shapes[name] = shape
        }
        return IconifyResponse(shapes, missing)
    }

    // ---- SVG の body → path 群 ----

    private val TAG = Regex("<(/?)([a-zA-Z][a-zA-Z0-9]*)((?:\"[^\"]*\"|'[^']*'|[^>\"'])*?)(/?)>")
    private val ATTR = Regex("([a-zA-Z_:][a-zA-Z0-9_:.-]*)\\s*=\\s*(?:\"([^\"]*)\"|'([^']*)')")
    private val SKIP_TAGS = setOf(
        "defs", "mask", "clippath", "style", "symbol", "lineargradient", "radialgradient", "pattern", "filter",
        "marker", "metadata", "title", "desc", "script", "foreignobject", "animate", "animatetransform", "set",
    )
    private val INHERITED = listOf(
        "fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "fill-rule",
    )

    private class Frame(val name: String, val attrs: Map<String, String>, val skip: Boolean)

    private fun attrsOf(raw: String): Map<String, String> {
        val m = HashMap<String, String>()
        for (r in ATTR.findAll(raw)) {
            val v = r.groups[2]?.value ?: r.groups[3]?.value ?: continue
            m[r.groupValues[1].lowercase(Locale.ROOT)] = v.trim()
        }
        return m
    }

    private fun num(s: String?): Double? {
        if (s == null) return null
        val t = s.trim().removeSuffix("px")
        val v = t.toDoubleOrNull() ?: return null
        return if (v.isNaN() || v.isInfinite()) null else v
    }

    private fun fmt(v: Double): String {
        if (v == Math.rint(v) && Math.abs(v) < 1e9) return v.toLong().toString()
        val s = String.format(Locale.ROOT, "%.4f", v).trimEnd('0').trimEnd('.')
        return if (s == "-0" || s.isEmpty()) "0" else s
    }

    /**
     * SVG の body（`<path d="..."/>` などの並び）から path 群を取り出す。
     * 対応: path / circle / ellipse / rect / line / polyline / polygon と、`<g>` から継承する
     * fill / stroke / stroke-width / stroke-linecap / stroke-linejoin / fill-rule。
     * 非対応（無視）: transform・グラデーション・mask・clipPath・defs の中身・不透明度。描ける path が無ければ null
     */
    fun shapeFromBody(body: String, left: Double, top: Double, width: Double, height: Double): IconShape? {
        if (!(width > 0.0) || !(height > 0.0) || width.isInfinite() || height.isInfinite()) return null
        if (left.isNaN() || top.isNaN()) return null
        val stack = ArrayList<Frame>()
        val paths = ArrayList<IconPath>()
        for (m in TAG.findAll(body)) {
            val closing = m.groupValues[1] == "/"
            val name = m.groupValues[2].lowercase(Locale.ROOT)
            val selfClosing = m.groupValues[4] == "/"
            if (closing) {
                val i = stack.indexOfLast { it.name == name }
                if (i >= 0) while (stack.size > i) stack.removeAt(stack.size - 1)
                continue
            }
            val own = attrsOf(m.groupValues[3])
            if (name !in SHAPES) {
                // <g> も未知の要素も、属性は継承する。SKIP_TAGS の中身は描かない
                if (!selfClosing) stack.add(Frame(name, own, name in SKIP_TAGS))
                continue
            }
            if (stack.any { it.skip }) {
                if (!selfClosing) stack.add(Frame(name, emptyMap(), true))
                continue
            }
            val style = HashMap<String, String>()
            for (f in stack) for (k in INHERITED) f.attrs[k]?.let { style[k] = it }
            for (k in INHERITED) own[k]?.let { style[k] = it }
            val d = shapeToPathData(name, own) ?: continue
            val fillAttr = style["fill"] ?: "currentColor"
            val strokeAttr = style["stroke"] ?: "none"
            val fill = !fillAttr.equals("none", ignoreCase = true) && !fillAttr.equals("transparent", ignoreCase = true)
            val stroke = !strokeAttr.equals("none", ignoreCase = true) && !strokeAttr.equals("transparent", ignoreCase = true)
            if (!fill && !stroke) continue
            val cap = style["stroke-linecap"]?.takeIf { it == "round" || it == "square" } ?: "butt"
            val join = style["stroke-linejoin"]?.takeIf { it == "round" || it == "bevel" } ?: "miter"
            paths.add(
                IconPath(
                    d = d,
                    fill = fill,
                    stroke = stroke,
                    strokeWidth = num(style["stroke-width"])?.takeIf { it >= 0.0 } ?: 1.0,
                    cap = cap,
                    join = join,
                    evenOdd = style["fill-rule"] == "evenodd",
                ),
            )
            if (!selfClosing) stack.add(Frame(name, emptyMap(), true))
        }
        if (paths.isEmpty()) return null
        return IconShape(left, top, width, height, paths)
    }

    private val SHAPES = setOf("path", "circle", "ellipse", "rect", "line", "polyline", "polygon")

    /** 1 つの図形要素を path data にする。値が足りない・不正なら null */
    private fun shapeToPathData(tag: String, a: Map<String, String>): String? = when (tag) {
        "path" -> a["d"]?.takeIf { it.isNotBlank() }
        "circle" -> {
            val r = num(a["r"])
            if (r == null || r <= 0.0) null else ellipse(num(a["cx"]) ?: 0.0, num(a["cy"]) ?: 0.0, r, r)
        }
        "ellipse" -> {
            val rx = num(a["rx"])
            val ry = num(a["ry"])
            if (rx == null || ry == null || rx <= 0.0 || ry <= 0.0) null else ellipse(num(a["cx"]) ?: 0.0, num(a["cy"]) ?: 0.0, rx, ry)
        }
        "rect" -> {
            val w = num(a["width"])
            val h = num(a["height"])
            if (w == null || h == null || w <= 0.0 || h <= 0.0) null else rect(num(a["x"]) ?: 0.0, num(a["y"]) ?: 0.0, w, h, num(a["rx"]), num(a["ry"]))
        }
        "line" -> {
            val x1 = num(a["x1"]) ?: 0.0
            val y1 = num(a["y1"]) ?: 0.0
            val x2 = num(a["x2"]) ?: 0.0
            val y2 = num(a["y2"]) ?: 0.0
            "M${fmt(x1)} ${fmt(y1)}L${fmt(x2)} ${fmt(y2)}"
        }
        "polyline" -> points(a["points"], false)
        "polygon" -> points(a["points"], true)
        else -> null
    }

    private fun ellipse(cx: Double, cy: Double, rx: Double, ry: Double): String =
        "M${fmt(cx - rx)} ${fmt(cy)}" +
            "a${fmt(rx)} ${fmt(ry)} 0 1 0 ${fmt(2 * rx)} 0" +
            "a${fmt(rx)} ${fmt(ry)} 0 1 0 ${fmt(-2 * rx)} 0Z"

    private fun rect(x: Double, y: Double, w: Double, h: Double, rxAttr: Double?, ryAttr: Double?): String {
        var rx = rxAttr ?: ryAttr ?: 0.0
        var ry = ryAttr ?: rxAttr ?: 0.0
        rx = rx.coerceIn(0.0, w / 2)
        ry = ry.coerceIn(0.0, h / 2)
        if (rx == 0.0 || ry == 0.0) {
            return "M${fmt(x)} ${fmt(y)}h${fmt(w)}v${fmt(h)}h${fmt(-w)}Z"
        }
        return "M${fmt(x + rx)} ${fmt(y)}" +
            "h${fmt(w - 2 * rx)}a${fmt(rx)} ${fmt(ry)} 0 0 1 ${fmt(rx)} ${fmt(ry)}" +
            "v${fmt(h - 2 * ry)}a${fmt(rx)} ${fmt(ry)} 0 0 1 ${fmt(-rx)} ${fmt(ry)}" +
            "h${fmt(-(w - 2 * rx))}a${fmt(rx)} ${fmt(ry)} 0 0 1 ${fmt(-rx)} ${fmt(-ry)}" +
            "v${fmt(-(h - 2 * ry))}a${fmt(rx)} ${fmt(ry)} 0 0 1 ${fmt(rx)} ${fmt(-ry)}Z"
    }

    private fun points(raw: String?, close: Boolean): String? {
        if (raw == null) return null
        val nums = raw.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }.map { it.toDoubleOrNull() ?: return null }
        if (nums.size < 4 || nums.size % 2 != 0) return null
        val sb = StringBuilder()
        var i = 0
        while (i < nums.size) {
            sb.append(if (i == 0) "M" else "L").append(fmt(nums[i])).append(' ').append(fmt(nums[i + 1]))
            i += 2
        }
        if (close) sb.append('Z')
        return sb.toString()
    }

    // ---- 保存形式 ----

    private const val FORMAT_VERSION = 1

    /** 保存用の JSON 文字列 */
    fun encode(shape: IconShape): String {
        val o = JSONObject()
        o.put("v", FORMAT_VERSION)
        o.put("l", shape.left)
        o.put("t", shape.top)
        o.put("w", shape.width)
        o.put("h", shape.height)
        val arr = org.json.JSONArray()
        for (p in shape.paths) {
            val po = JSONObject()
            po.put("d", p.d)
            po.put("f", p.fill)
            po.put("s", p.stroke)
            po.put("sw", p.strokeWidth)
            po.put("c", p.cap)
            po.put("j", p.join)
            po.put("e", p.evenOdd)
            arr.put(po)
        }
        o.put("p", arr)
        return o.toString()
    }

    /** [encode] の逆。壊れている・版が違う・描けるものが無いときは null */
    fun decode(text: String): IconShape? {
        return try {
            val o = JSONObject(text)
            if (o.optInt("v", 0) != FORMAT_VERSION) return null
            val w = o.getDouble("w")
            val h = o.getDouble("h")
            if (!(w > 0.0) || !(h > 0.0)) return null
            val arr = o.getJSONArray("p")
            val paths = ArrayList<IconPath>()
            for (i in 0 until arr.length()) {
                val po = arr.getJSONObject(i)
                val d = po.getString("d")
                if (d.isBlank()) continue
                paths.add(
                    IconPath(
                        d = d,
                        fill = po.optBoolean("f", true),
                        stroke = po.optBoolean("s", false),
                        strokeWidth = po.optDouble("sw", 1.0),
                        cap = po.optString("c", "butt"),
                        join = po.optString("j", "miter"),
                        evenOdd = po.optBoolean("e", false),
                    ),
                )
            }
            if (paths.isEmpty()) null else IconShape(o.optDouble("l", 0.0), o.optDouble("t", 0.0), w, h, paths)
        } catch (e: JSONException) {
            null
        }
    }
}
