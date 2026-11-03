package com.shiki01.nagi

import org.json.JSONArray
import org.json.JSONObject

/**
 * レイアウト JSON のうち、この端末（Android）がまだ反映しない指定の検出（取り込み時の確認ダイアログ用）。
 * 構文は [LayoutParser] が受け付けて無視するので、取り込みは止めず、利用者に「反映されない」ことだけ伝える。
 * android.* に依存しない（org.json のみ）。サイトの `web/src/lib/keyboard/compat.ts` と同じ一覧にそろえる。
 */
internal object LayoutCompat {
    /**
     * 未対応の指定名（JSON のプロパティ名）と日本語の説明。表示順もこの並び。
     * Android が対応したものは、ここから外す（描画の対応範囲は `docs/android-layout-from-web.md` §4-3）
     */
    val UNSUPPORTED: List<Pair<String, String>> = listOf(
        "look" to "見た目",
        "roles" to "役割ごとの見た目",
        "rules" to "状態の見た目",
        "states" to "キーの状態ごとの見た目",
        "metrics" to "余白",
        "background" to "背景",
        "assets" to "画像",
        "toolbar" to "ツールバー",
        "visible" to "表示条件",
    )

    private val UNSUPPORTED_NAMES: Set<String> = UNSUPPORTED.map { it.first }.toSet()

    /**
     * [jsonText] が実際に含む未対応の指定名を、[UNSUPPORTED] の順で返す（無ければ空）。
     * JSON として読めなければ空（検証の失敗は [LayoutParser] の役目）
     */
    fun unsupported(jsonText: String): List<String> {
        val root = try {
            JSONObject(jsonText.removePrefix("﻿"))
        } catch (e: Exception) {
            return emptyList()
        }
        val found = HashSet<String>()
        collect(root, found)
        val rows: JSONArray? = root.optJSONArray("rows")
        if (rows != null) {
            for (i in 0 until rows.length()) {
                val row = rows.optJSONObject(i) ?: continue
                collect(row, found)
                val keys: JSONArray = row.optJSONArray("keys") ?: continue
                for (j in 0 until keys.length()) {
                    val key = keys.optJSONObject(j) ?: continue
                    collect(key, found)
                }
            }
        }
        return UNSUPPORTED.map { it.first }.filter { it in found }
    }

    /** [o] の直下に、値が null でない未対応のプロパティがあれば [found] に足す。`assets` は空のオブジェクトなら数えない */
    private fun collect(o: JSONObject, found: MutableSet<String>) {
        for (name in UNSUPPORTED_NAMES) {
            if (!o.has(name) || o.isNull(name)) continue
            if (name == "assets") {
                val a = o.optJSONObject("assets")
                if (a != null && a.length() == 0) continue
            }
            found.add(name)
        }
    }

    /** 指定名 → 表示用（例 `look（見た目）`）。一覧外の名前はそのまま */
    fun label(name: String): String {
        val ja = UNSUPPORTED.firstOrNull { it.first == name }?.second ?: return name
        return "$name（$ja）"
    }

    /** 確認ダイアログに足す一文。[unsupported] が空なら null */
    fun notice(unsupported: List<String>): String? {
        if (unsupported.isEmpty()) return null
        return "この端末では次の指定は反映されません: " + unsupported.joinToString("、") { label(it) }
    }
}
