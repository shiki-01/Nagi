package com.shiki01.nagi

import android.content.Context
import java.io.File
import java.io.IOException

/** レイアウトの出どころ */
internal enum class LayoutSource { PRESET, USER }

/**
 * 一覧用の情報。壊れたユーザーのレイアウトは [error] に理由が入り、[mode] / [name] は読めた範囲（読めなければ null / ID）
 */
internal data class LayoutInfo(
    val id: String,
    val name: String,
    val mode: LayoutMode?,
    val source: LayoutSource,
    val keyCount: Int,
    val error: String?,
    /** ユーザーのレイアウトが同梱と同じ ID で、そちらを上書きしているか */
    val overridesPreset: Boolean,
) {
    val valid: Boolean get() = error == null
}

/**
 * レイアウトの置き場。
 * - 同梱のプリセット: APK の assets の `presets/<id>.json`（`config/keyboard/presets` を build.gradle.kts で取り込む）
 * - ユーザーのレイアウト: `filesDir/layouts/<id>.json`（取り込んだ JSON の文字列をそのまま保存する）
 * - 割り当て: [KeyboardPrefs]（かな / 英字 / 入力欄の種類別）
 *
 * 読み込みの優先順位は「ユーザー → 同梱」。不正なものは捨てて次へ進む（入力を失わない）。
 * IME サービスと設定画面から呼ばれる。ファイル・assets を読むので、重い場合はメインスレッド以外で呼ぶこと
 * （キーボードの組み立てでは小さい JSON をキャッシュ付きで読む）。
 */
internal object LayoutStore {
    private const val PRESET_DIR = "presets"
    private const val USER_DIR = "layouts"

    private class CacheEntry(val revision: Long, val layout: LayoutModel?)

    private val cache = HashMap<String, CacheEntry>()

    private fun userDir(ctx: Context): File = File(ctx.filesDir, USER_DIR)

    // ---- 読み込み ----

    /** [id] のレイアウトを読む（ユーザー → 同梱の順）。無い・不正なら null。結果は保存・削除のたびに破棄するキャッシュに載せる */
    fun load(ctx: Context, id: String): LayoutModel? {
        if (!LayoutIds.isValid(id)) return null
        val rev = KeyboardPrefs.layoutsRevision(ctx)
        synchronized(cache) {
            val hit = cache[id]
            if (hit != null && hit.revision == rev) return hit.layout
        }
        val layout = loadUncached(ctx, id)
        synchronized(cache) { cache[id] = CacheEntry(rev, layout) }
        return layout
    }

    private fun loadUncached(ctx: Context, id: String): LayoutModel? {
        val user = readUserText(ctx, id)?.let { LayoutParser.parseOrNull(it) }
        // ファイル名と JSON 内の id が違うものは使わない（iOS は確認しないが、割り当て・切替の ID が曖昧になるため）
        if (user != null && user.id == id) return user
        val preset = readPresetText(ctx, id)?.let { LayoutParser.parseOrNull(it) }
        return if (preset != null && preset.id == id) preset else null
    }

    /**
     * かな（[kana] = true）/ 英字の割り当て済みのレイアウトを読む。
     * 割り当て → そのモードの既定（`flick-kana` / `qwerty-alpha`）の順。どちらも読めなければ null（組み込みの [BuiltinLayouts] へ）
     */
    fun loadAssigned(ctx: Context, kana: Boolean): LayoutModel? {
        val id = if (kana) KeyboardPrefs.kanaLayoutId(ctx) else KeyboardPrefs.alphaLayoutId(ctx)
        return load(ctx, id) ?: load(ctx, LayoutIds.defaultFor(kana))
    }

    /** 保存してある JSON の文字列（書き出し・プレビュー用）。ユーザー → 同梱の順。無ければ null */
    fun readRaw(ctx: Context, id: String): String? {
        if (!LayoutIds.isValid(id)) return null
        return readUserText(ctx, id) ?: readPresetText(ctx, id)
    }

    // ---- 一覧 ----

    /** 同梱のプリセットの ID（表示順） */
    fun presetIds(ctx: Context): List<String> {
        val names = try {
            ctx.assets.list(PRESET_DIR)?.toList() ?: emptyList()
        } catch (e: IOException) {
            emptyList()
        }
        val ids = names.filter { it.endsWith(".json") }.map { it.removeSuffix(".json") }.filter { LayoutIds.isValid(it) }
        return ids.sortedWith(
            compareBy<String>(
                { LayoutIds.PRESET_ORDER.indexOf(it).let { i -> if (i < 0) Int.MAX_VALUE else i } },
                { it },
            ),
        )
    }

    /** 同梱のレイアウトの一覧。読めないものは [LayoutInfo.error] 付きで含める */
    fun listPresets(ctx: Context): List<LayoutInfo> = presetIds(ctx).map { id ->
        infoOf(id, readPresetText(ctx, id), LayoutSource.PRESET, false)
    }

    /** ユーザーのレイアウトの一覧（ID 順）。壊れているものは [LayoutInfo.error] 付きで含める */
    fun listUser(ctx: Context): List<LayoutInfo> {
        val presets = presetIds(ctx).toSet()
        return userIds(ctx).map { id -> infoOf(id, readUserText(ctx, id), LayoutSource.USER, id in presets) }
    }

    /** ユーザーのレイアウトの ID（ファイル名から。ID の規則に合うものだけ）。ID 順 */
    fun userIds(ctx: Context): List<String> {
        val files = userDir(ctx).listFiles() ?: return emptyList()
        return files.filter { it.isFile && it.name.endsWith(".json") }
            .map { it.name.removeSuffix(".json") }
            .filter { LayoutIds.isValid(it) }
            .sorted()
    }

    /** 同梱と同じ ID のユーザーのレイアウトがあるか（取り込み前の衝突判定に使う） */
    fun hasUser(ctx: Context, id: String): Boolean = LayoutIds.isValid(id) && File(userDir(ctx), "$id.json").isFile

    private fun infoOf(id: String, text: String?, source: LayoutSource, overrides: Boolean): LayoutInfo {
        if (text == null) return LayoutInfo(id, id, null, source, 0, "ファイルを読めません", overrides)
        return when (val r = LayoutParser.parse(text, null)) {
            is LayoutParseResult.Ok -> {
                if (r.layout.id != id) {
                    LayoutInfo(id, r.layout.name, r.layout.mode, source, r.layout.keyCount, "ファイル名と id が一致しません（${r.layout.id}）", overrides)
                } else {
                    LayoutInfo(id, r.layout.name, r.layout.mode, source, r.layout.keyCount, null, overrides)
                }
            }
            is LayoutParseResult.Err -> LayoutInfo(id, id, null, source, 0, r.reason, overrides)
        }
    }

    // ---- 保存・削除 ----

    /**
     * 検証済みのレイアウトの JSON を `filesDir/layouts/<id>.json` に保存する（同じ ID は置き換える）。
     * 一時ファイルに書いてから置き換える。成功したら true
     */
    fun saveUser(ctx: Context, layout: LayoutModel, rawJson: String): Boolean {
        if (!LayoutIds.isValid(layout.id)) return false
        val dir = userDir(ctx)
        if (!dir.isDirectory && !dir.mkdirs()) return false
        val target = File(dir, "${layout.id}.json")
        val tmp = File(dir, "${layout.id}.json.tmp")
        try {
            tmp.writeText(rawJson, Charsets.UTF_8)
            if (!tmp.renameTo(target)) {
                target.delete()
                if (!tmp.renameTo(target)) return false
            }
        } catch (e: IOException) {
            tmp.delete()
            return false
        }
        KeyboardPrefs.bumpLayoutsRevision(ctx)
        return true
    }

    /**
     * ユーザーのレイアウトを削除する。そのレイアウトを割り当てていたモード・入力欄は既定に戻す。
     * 同梱と同じ ID なら、削除後は同梱のものが使われる。ファイルが無かった・消せなかったときは false
     */
    fun delete(ctx: Context, id: String): Boolean {
        if (!LayoutIds.isValid(id)) return false
        val f = File(userDir(ctx), "$id.json")
        if (!f.isFile) return false
        if (!f.delete()) return false
        // 同梱に同じ ID が無いときだけ、割り当てを既定に戻す（同梱があればそちらが使われるので割り当ては有効のまま）
        if (id !in presetIds(ctx)) {
            if (KeyboardPrefs.kanaLayoutId(ctx) == id) KeyboardPrefs.setKanaLayoutId(ctx, null)
            if (KeyboardPrefs.alphaLayoutId(ctx) == id) KeyboardPrefs.setAlphaLayoutId(ctx, null)
            for ((kind, assigned) in KeyboardPrefs.fieldAssignments(ctx)) {
                if (assigned == id) KeyboardPrefs.setFieldLayoutId(ctx, kind, null)
            }
        }
        KeyboardPrefs.bumpLayoutsRevision(ctx)
        return true
    }

    // ---- 割り当て ----

    /** かな / 英字に割り当てる。[id] が null なら既定に戻す。存在しない ID は false（割り当てない） */
    fun assign(ctx: Context, slot: AssignSlot, id: String?): Boolean {
        if (id != null && load(ctx, id) == null) return false
        when (slot) {
            AssignSlot.KANA -> KeyboardPrefs.setKanaLayoutId(ctx, id)
            AssignSlot.ALPHA -> KeyboardPrefs.setAlphaLayoutId(ctx, id)
        }
        KeyboardPrefs.bumpLayoutsRevision(ctx)
        return true
    }

    /** いま割り当てているレイアウトの ID */
    fun assigned(ctx: Context, slot: AssignSlot): String = when (slot) {
        AssignSlot.KANA -> KeyboardPrefs.kanaLayoutId(ctx)
        AssignSlot.ALPHA -> KeyboardPrefs.alphaLayoutId(ctx)
    }

    /**
     * 入力欄の種類ごとに割り当てる。[id] は `kana` / `alpha` / レイアウト ID / [LayoutSelect.FIELD_NONE]（選択しない）/
     * null（既定に戻す）。存在しないレイアウト ID は false
     */
    fun assignField(ctx: Context, kind: FieldKind, id: String?): Boolean {
        val special = id == null || id == LayoutSelect.FIELD_NONE || id == LayoutSelect.TARGET_KANA || id == LayoutSelect.TARGET_ALPHA
        if (!special && (id == null || load(ctx, id) == null)) return false
        KeyboardPrefs.setFieldLayoutId(ctx, kind, id)
        KeyboardPrefs.bumpLayoutsRevision(ctx)
        return true
    }

    /** 入力欄の種類に対して実際に使われる切替先（割り当て → 既定）。null は自動選択しない */
    fun fieldTarget(ctx: Context, kind: FieldKind): String? =
        LayoutSelect.resolveFieldTarget(kind, KeyboardPrefs.fieldAssignments(ctx))

    /** かな・英字・入力欄別の割り当てをすべて既定に戻す */
    fun resetAssignments(ctx: Context) {
        KeyboardPrefs.setKanaLayoutId(ctx, null)
        KeyboardPrefs.setAlphaLayoutId(ctx, null)
        for (k in FieldKind.values()) KeyboardPrefs.setFieldLayoutId(ctx, k, null)
        KeyboardPrefs.bumpLayoutsRevision(ctx)
    }

    // ---- ファイル・assets の読み取り ----

    private fun readUserText(ctx: Context, id: String): String? {
        val f = File(userDir(ctx), "$id.json")
        if (!f.isFile || f.length() > LayoutLimits.JSON_BYTES_MAX) return null
        return try {
            f.readText(Charsets.UTF_8)
        } catch (e: IOException) {
            null
        }
    }

    private fun readPresetText(ctx: Context, id: String): String? = try {
        ctx.assets.open("$PRESET_DIR/$id.json").bufferedReader(Charsets.UTF_8).use { it.readText() }
    } catch (e: IOException) {
        null
    }
}
