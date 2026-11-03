package com.shiki01.nagi

import org.json.JSONArray
import org.json.JSONObject

/** [LayoutParser.parse] の結果 */
internal sealed class LayoutParseResult {
    class Ok(val layout: LayoutModel) : LayoutParseResult()

    /** [reason] は利用者に見せられる日本語の理由 */
    class Err(val reason: String) : LayoutParseResult()
}

private class LayoutException(message: String) : Exception(message)

private fun fail(message: String): Nothing = throw LayoutException(message)

/**
 * レイアウト JSON（schema 2）の読み取りと検証。android.* に依存しない（org.json のみ）。
 *
 * 検証の規則は iOS（`KeyboardLayout.swift` の `LayoutValidation`）に揃える。web（`web/src/lib/keyboard/validate.ts`）との差異:
 * - 未知のプロパティは **無視** する（iOS と同じ。web は拒否する）。`look` 系の不正値もレイアウト全体は無効にしない。
 * - `flick.kana` は 12 キー名（`あ か さ た な は ま や ら わ 、`）か検査する（iOS と同じ。web は長さしか見ない）。
 * - 行の高さの比の合計は 4.5 以下（iOS と同じ。schema の JSON 単体には無い）。
 * - キーの `id` は全キーで必須（spacer も。iOS の注釈と実装の食い違いは、実装＝必須に合わせた）。
 * - `flick` / ⌫ の `longPress` は検証では許し、描画で無視する（iOS と同じ）。
 * - 長押し項目の kind は text / action / shortcut のみ（toggle / layout は拒否。iOS・schema と同じ）。
 * - 参照先（`layout.target`）の存在確認は取り込み時だけ（[parse] の `knownLayoutIds` を渡したときだけ）。
 */
internal object LayoutParser {
    private val KINDS = setOf("text", "flick", "toggle", "action", "shortcut", "layout", "spacer")
    private val POPUP_KINDS = setOf("text", "action", "shortcut")

    /**
     * [text] を読んで検証する。
     * @param knownLayoutIds 取り込み時に渡す。`layout.target` がこの集合・組み込み名・自身の id のどれでもなければ拒否する。
     *   null（実行時の読み込み）なら存在確認をしない
     */
    fun parse(text: String, knownLayoutIds: Set<String>? = null): LayoutParseResult {
        val body = text.removePrefix("\uFEFF")
        if (body.isBlank()) return LayoutParseResult.Err("JSON が空です")
        if (body.toByteArray(Charsets.UTF_8).size > LayoutLimits.JSON_BYTES_MAX) {
            return LayoutParseResult.Err("JSON が大きすぎます（${LayoutLimits.JSON_BYTES_MAX / 1024} KB まで）")
        }
        val root = try {
            JSONObject(body)
        } catch (e: Exception) {
            return LayoutParseResult.Err("JSON として読み取れません: ${e.message ?: "形式が不正です"}")
        }
        return try {
            LayoutParseResult.Ok(build(root, knownLayoutIds))
        } catch (e: LayoutException) {
            LayoutParseResult.Err(e.message ?: "レイアウトが不正です")
        }
    }

    /** 実行時の読み込み用。不正なら null（呼び出し側が既定にフォールバックする） */
    fun parseOrNull(text: String): LayoutModel? = (parse(text, null) as? LayoutParseResult.Ok)?.layout

    // ---- レイアウト ----

    private fun build(root: JSONObject, known: Set<String>?): LayoutModel {
        val schema = number(root, "schema", "レイアウト") ?: fail("必須の項目「schema」がありません")
        if (schema != LayoutLimits.SCHEMA.toDouble()) {
            fail("未対応のスキーマ ${trimNumber(schema)}（schema ${LayoutLimits.SCHEMA} に移行してください）")
        }
        val id = string(root, "id", "レイアウト") ?: fail("必須の項目「id」がありません")
        if (!LayoutLimits.LAYOUT_ID.matches(id)) {
            fail("id は英小文字・数字・- _ で始まりが英小文字か数字、64 文字以内にしてください")
        }
        val name = string(root, "name", "レイアウト") ?: fail("必須の項目「name」がありません")
        if (cp(name) !in 1..LayoutLimits.NAME_MAX) fail("name は 1〜${LayoutLimits.NAME_MAX} 文字にしてください")
        val modeText = string(root, "mode", "レイアウト") ?: fail("必須の項目「mode」がありません")
        val mode = LayoutMode.fromJson(modeText)
            ?: fail("mode は kana / alpha / number / symbol / emoji / custom のいずれかにしてください（$modeText）")
        val rowsJson = array(root, "rows", "レイアウト") ?: fail("必須の項目「rows」がありません")
        if (rowsJson.length() < 1 || rowsJson.length() > LayoutLimits.ROWS_MAX) {
            fail("rows は 1〜${LayoutLimits.ROWS_MAX} 行（${rowsJson.length()}）")
        }
        val targets: Set<String>? = known?.let { LayoutLimits.BUILTIN_TARGETS.toSet() + it + id }
        val keyIds = HashSet<String>()
        var heightSum = 0.0
        val rows = ArrayList<LayoutRow>()
        for (i in 0 until rowsJson.length()) {
            val label = "行 ${i + 1}"
            val rowJson = rowsJson.optJSONObject(i) ?: fail("$label はオブジェクトで指定してください")
            val height = number(rowJson, "height", label) ?: 1.0
            if (!(height > 0.0 && height <= LayoutLimits.ROW_HEIGHT_MAX)) fail("$label の高さが範囲外（${trimNumber(height)}）")
            heightSum += height
            val keysJson = array(rowJson, "keys", label) ?: fail("$label には 1 つ以上のキーが必要です")
            if (keysJson.length() == 0) fail("$label には 1 つ以上のキーが必要です")
            if (keysJson.length() > LayoutLimits.KEYS_PER_ROW_MAX) {
                fail("$label のキーは ${LayoutLimits.KEYS_PER_ROW_MAX} 個以内にしてください")
            }
            val keys = ArrayList<LayoutKey>()
            var total = 0.0
            for (j in 0 until keysJson.length()) {
                val keyJson = keysJson.optJSONObject(j) ?: fail("$label のキー ${j + 1} はオブジェクトで指定してください")
                val key = parseKey(keyJson, targets)
                if (!keyIds.add(key.id)) fail("キー id の重複: ${key.id}")
                total += key.width
                keys.add(key)
            }
            if (total < LayoutLimits.ROW_WIDTH_MIN || total > LayoutLimits.ROW_WIDTH_MAX) {
                fail("$label の幅の合計が範囲外（${trimNumber(total)}）")
            }
            rows.add(LayoutRow(height, keys))
        }
        if (heightSum > LayoutLimits.ROW_HEIGHT_SUM_MAX) {
            fail("行の高さの比の合計は ${LayoutLimits.ROW_HEIGHT_SUM_MAX} 以下にしてください（現在 ${trimNumber(heightSum)}）")
        }
        return LayoutModel(id, name, mode, rows)
    }

    // ---- キー ----

    private fun parseKey(o: JSONObject, targets: Set<String>?): LayoutKey {
        val id = string(o, "id", "キー") ?: fail("キー「(id なし)」の id は英数字・_ . - で 32 文字以内にしてください")
        if (!LayoutLimits.KEY_ID.matches(id)) fail("キー「$id」の id は英数字・_ . - で 32 文字以内にしてください")
        val name = "キー $id"
        val width = number(o, "width", name) ?: 1.0
        if (!(width > 0.0 && width <= LayoutLimits.WIDTH_MAX)) {
            fail("$name の幅は 0 より大きく ${LayoutLimits.WIDTH_MAX.toInt()} 以下にしてください")
        }
        val label = string(o, "label", name)
        if (label != null && cp(label) > LayoutLimits.LABEL_MAX) fail("$name の label は ${LayoutLimits.LABEL_MAX} 文字以内にしてください")
        val icon = string(o, "icon", name)
        if (icon != null) iconError(icon)?.let { fail("$name の icon: $it") }
        val roleText = string(o, "role", name)
        val role = if (roleText == null) KeyRole.NORMAL else {
            KeyRole.fromJson(roleText) ?: fail("$name の role は normal / function / accent のいずれかにしてください（$roleText）")
        }
        val kind = string(o, "kind", name) ?: fail("$name に kind がありません")
        if (kind !in KINDS) fail("$name の kind「$kind」は未対応です（text / flick / toggle / action / shortcut / layout / spacer）")
        val action = parseAction(o, kind, name, targets)

        val longPress = ArrayList<PopupItem>()
        val popupJson = array(o, "longPress", name)
        if (popupJson != null) {
            if (popupJson.length() < 1 || popupJson.length() > LayoutLimits.LONG_PRESS_MAX) {
                fail("$name の longPress は 1〜${LayoutLimits.LONG_PRESS_MAX} 個（${popupJson.length()}）")
            }
            for (i in 0 until popupJson.length()) {
                val item = popupJson.optJSONObject(i) ?: fail("$name の longPress の項目 ${i + 1} はオブジェクトで指定してください")
                longPress.add(parsePopupItem(item, name))
            }
        }
        return LayoutKey(id, width, label, icon, role, action, longPress)
    }

    private fun parseAction(o: JSONObject, kind: String, name: String, targets: Set<String>?): KeyAction = when (kind) {
        "text" -> {
            val text = string(o, "text", name)
            if (text == null || cp(text) !in 1..LayoutLimits.TEXT_MAX) fail("$name の text は 1〜${LayoutLimits.TEXT_MAX} 文字で指定してください")
            val shift = string(o, "shift", name)
            if (shift != null && cp(shift) !in 1..LayoutLimits.TEXT_MAX) {
                fail("$name の shift は 1〜${LayoutLimits.TEXT_MAX} 文字にしてください")
            }
            KeyAction.Text(text, shift)
        }
        "flick" -> parseFlick(o, name)
        "toggle" -> {
            val cycleJson = array(o, "cycle", name)
            if (cycleJson == null || cycleJson.length() < 1 || cycleJson.length() > LayoutLimits.CYCLE_MAX) {
                fail("$name の cycle は 1〜${LayoutLimits.CYCLE_MAX} 要素（${cycleJson?.length() ?: 0}）")
            }
            val cycle = ArrayList<String>()
            for (i in 0 until cycleJson.length()) {
                val s = cycleJson.opt(i) as? String
                if (s == null || cp(s) !in 1..LayoutLimits.TEXT_MAX) {
                    fail("$name の cycle の要素は 1〜${LayoutLimits.TEXT_MAX} 文字にしてください")
                }
                cycle.add(s)
            }
            KeyAction.Toggle(cycle)
        }
        "action" -> {
            val actionName = string(o, "action", name)
            if (actionName == null || !LayoutLimits.ACTION.matches(actionName)) {
                fail("$name の action は英字で始まる英数字（32 文字以内）にしてください")
            }
            KeyAction.Act(actionName)
        }
        "shortcut" -> KeyAction.Shortcut(parseSteps(o, name))
        "layout" -> {
            val target = string(o, "target", name) ?: ""
            if (!LayoutLimits.LAYOUT_ID.matches(target)) fail("$name の target は切り替え先のレイアウト id にしてください")
            if (targets != null && target !in targets) {
                fail("$name の切り替え先「$target」は存在するレイアウトでも組み込み名でもありません")
            }
            KeyAction.LayoutSwitch(target)
        }
        else -> KeyAction.Spacer
    }

    private fun parseFlick(o: JSONObject, name: String): KeyAction.Flick {
        val cells = HashMap<String, String?>()
        for (dir in listOf("center", "up", "down", "left", "right")) {
            val cell = obj(o, dir, name)
            if (cell == null) {
                cells[dir] = null
                continue
            }
            val text = string(cell, "text", "$name の $dir")
            if (text == null || cp(text) !in 1..LayoutLimits.TEXT_MAX) {
                fail("$name の $dir の text は 1〜${LayoutLimits.TEXT_MAX} 文字にしてください")
            }
            cells[dir] = text
        }
        val kana = string(o, "kana", name)
        if (cells["center"] == null && kana == null) fail("$name: フリックには center（中央の文字）か kana が必要です")
        var kanaKey: String? = null
        if (kana != null) {
            if (cp(kana) !in 1..LayoutLimits.KANA_MAX) fail("$name の kana は 1〜${LayoutLimits.KANA_MAX} 文字にしてください")
            kanaKey = LayoutLimits.KANA_KEY_IDS[kana[0]] ?: fail("$name の kana「$kana」は未知のキー名です")
        }
        return KeyAction.Flick(kana, kanaKey, cells["center"], cells["left"], cells["up"], cells["right"], cells["down"])
    }

    private fun parseSteps(o: JSONObject, owner: String): List<ShortcutStep> {
        val stepsJson = array(o, "steps", owner)
        if (stepsJson == null || stepsJson.length() < 1 || stepsJson.length() > LayoutLimits.STEPS_MAX) {
            fail("$owner の steps は 1〜${LayoutLimits.STEPS_MAX} 個で指定してください")
        }
        val steps = ArrayList<ShortcutStep>()
        for (i in 0 until stepsJson.length()) {
            val stepJson = stepsJson.optJSONObject(i) ?: fail("$owner の手順 ${i + 1} はオブジェクトで指定してください")
            val action = string(stepJson, "action", "$owner の手順 ${i + 1}")
            val text = string(stepJson, "text", "$owner の手順 ${i + 1}")
            if ((action != null) == (text != null)) {
                fail("$owner の手順 ${i + 1} は action か text のどちらか一方だけを指定してください")
            }
            if (action != null && !LayoutLimits.ACTION.matches(action)) fail("$owner の手順 ${i + 1} の action が不正です")
            if (text != null && cp(text) !in 1..LayoutLimits.SHORTCUT_TEXT_MAX) {
                fail("$owner の手順 ${i + 1} の text は 1〜${LayoutLimits.SHORTCUT_TEXT_MAX} 文字にしてください")
            }
            steps.add(ShortcutStep(action, text))
        }
        return steps
    }

    /** 長押しの 1 項目（kind は text / action / shortcut のみ。label 必須。入れ子の longPress は不可） */
    private fun parsePopupItem(o: JSONObject, owner: String): PopupItem {
        val where = "$owner の longPress"
        val id = string(o, "id", where)
        if (id != null && !LayoutLimits.KEY_ID.matches(id)) fail("$where の id が不正です")
        val label = string(o, "label", where)
        if (label == null || cp(label) !in 1..LayoutLimits.LABEL_MAX) {
            fail("$where の label は 1〜${LayoutLimits.LABEL_MAX} 文字で指定してください")
        }
        if (o.has("longPress") && !o.isNull("longPress")) fail("$where の中に longPress は置けません")
        val icon = string(o, "icon", where)
        if (icon != null) {
            if (cp(icon) > LayoutLimits.ICON_MAX) fail("$where の icon は ${LayoutLimits.ICON_MAX} 文字以内にしてください")
            iconError(icon)?.let { fail("$where の icon: $it") }
        }
        val kind = string(o, "kind", where)
        if (kind == null || kind !in POPUP_KINDS) fail("$where の kind は text / action / shortcut のいずれかにしてください")
        val action: KeyAction = when (kind) {
            "text" -> {
                val text = string(o, "text", where)
                if (text == null || cp(text) !in 1..LayoutLimits.TEXT_MAX) {
                    fail("$where の text は 1〜${LayoutLimits.TEXT_MAX} 文字にしてください")
                }
                KeyAction.Text(text, null)
            }
            "action" -> {
                val actionName = string(o, "action", where)
                if (actionName == null || !LayoutLimits.ACTION.matches(actionName)) fail("$where の action が不正です")
                KeyAction.Act(actionName)
            }
            else -> KeyAction.Shortcut(parseSteps(o, where))
        }
        return PopupItem(id, label, icon, action)
    }

    /** `icon` の書式（`sf:<名前>` / `iconify:<prefix>:<name>` / 接頭辞なし）。問題があれば理由 */
    fun iconError(icon: String): String? {
        if (cp(icon) > LayoutLimits.ICON_MAX) return "${LayoutLimits.ICON_MAX} 文字以内にしてください"
        val ok = when {
            icon.startsWith("sf:") -> LayoutLimits.ICON_SF.matches(icon)
            icon.startsWith("iconify:") -> LayoutLimits.ICON_ICONIFY.matches(icon)
            else -> LayoutLimits.ICON_PLAIN.matches(icon)
        }
        return if (ok) null else "書式が不正です（sf:<名前> / iconify:<prefix>:<name>）"
    }

    // ---- 型を確かめて読む（型違いは理由つきで失敗。無い・null は null） ----

    private fun string(o: JSONObject, key: String, owner: String): String? {
        if (!o.has(key) || o.isNull(key)) return null
        return o.get(key) as? String ?: fail("$owner の $key は文字列で指定してください")
    }

    private fun number(o: JSONObject, key: String, owner: String): Double? {
        if (!o.has(key) || o.isNull(key)) return null
        val v = o.get(key)
        return (v as? Number)?.toDouble() ?: fail("$owner の $key は数で指定してください")
    }

    private fun array(o: JSONObject, key: String, owner: String): JSONArray? {
        if (!o.has(key) || o.isNull(key)) return null
        return o.get(key) as? JSONArray ?: fail("$owner の $key は配列で指定してください")
    }

    private fun obj(o: JSONObject, key: String, owner: String): JSONObject? {
        if (!o.has(key) || o.isNull(key)) return null
        return o.get(key) as? JSONObject ?: fail("$owner の $key はオブジェクトで指定してください")
    }

    /** コードポイント数 */
    private fun cp(s: String): Int = s.codePointCount(0, s.length)

    private fun trimNumber(d: Double): String = if (d == Math.floor(d) && !d.isInfinite()) d.toLong().toString() else d.toString()
}
