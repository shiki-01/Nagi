package com.shiki01.nagi

import org.json.JSONObject

// レイアウト管理画面（LayoutManagerActivity）の純粋なロジック。android.* に依存しない（org.json のみ）。単体テスト対象。
// プレビューの寸法計算、割り当ての選択肢、取り込みの確認文、nagi://layout の受け入れ判定、複製用の JSON の書き換え。

/** プレビュー上の区間（開始位置と大きさ。単位は呼び出し側が決める） */
internal data class PreviewSpan(val start: Float, val size: Float)

/** プレビューの寸法計算。キーボード本体と同じく、行は高さ比、キーは幅比で按分する（iOS `LayoutGeometry` に倣う） */
internal object PreviewGeometry {
    /** 各行の縦の区間。[baseHeight] は高さ比 1 の行の高さ、[gap] は行の間隔 */
    fun rowSpans(heights: List<Double>, baseHeight: Float, gap: Float): List<PreviewSpan> {
        var y = 0f
        return heights.map { h ->
            val span = PreviewSpan(y, (baseHeight * h).toFloat())
            y += span.size + gap
            span
        }
    }

    /** 行の高さの合計（行間を含む） */
    fun totalHeight(heights: List<Double>, baseHeight: Float, gap: Float): Float {
        if (heights.isEmpty()) return 0f
        var sum = 0f
        for (h in heights) sum += (baseHeight * h).toFloat()
        return sum + gap * (heights.size - 1)
    }

    /** 1 行のキーの横の区間。幅 = (行の幅 − 間隔 × (キー数 − 1)) × 幅比 ÷ 幅比の合計 */
    fun keySpans(widths: List<Double>, rowWidth: Float, gap: Float): List<PreviewSpan> {
        if (widths.isEmpty()) return emptyList()
        val total = widths.sum()
        if (total <= 0.0) return emptyList()
        val avail = Math.max(0f, rowWidth - gap * (widths.size - 1))
        var x = 0f
        return widths.map { w ->
            val span = PreviewSpan(x, (avail * (w / total)).toFloat())
            x += span.size + gap
            span
        }
    }
}

/** 画面に出す文言の部品 */
internal object LayoutTexts {
    fun modeLabel(mode: LayoutMode?): String = when (mode) {
        LayoutMode.KANA -> "かな"
        LayoutMode.ALPHA -> "英字"
        LayoutMode.NUMBER -> "数字"
        LayoutMode.SYMBOL -> "記号"
        LayoutMode.EMOJI -> "絵文字"
        LayoutMode.CUSTOM -> "カスタム"
        null -> "不明"
    }

    /** 「id ・ モード ・ N キー」の補足行 */
    fun summary(info: LayoutInfo): String =
        if (info.valid) "${info.id} ・ ${modeLabel(info.mode)} ・ ${info.keyCount} キー" else "${info.id} ・ 使えません"

    /** レイアウトのモードで決まる、割り当てられる先。number / symbol / emoji は割り当てずに layout キーから使う */
    fun slotsFor(mode: LayoutMode?): List<AssignSlot> = when (mode) {
        LayoutMode.KANA -> listOf(AssignSlot.KANA)
        LayoutMode.ALPHA -> listOf(AssignSlot.ALPHA)
        LayoutMode.CUSTOM -> listOf(AssignSlot.KANA, AssignSlot.ALPHA)
        else -> emptyList()
    }

    fun slotLabel(slot: AssignSlot): String = when (slot) {
        AssignSlot.KANA -> "かな入力で使う"
        AssignSlot.ALPHA -> "英字入力で使う"
    }

    fun tooLarge(): String = "ファイルが大きすぎます（${LayoutLimits.JSON_BYTES_MAX / 1024} KB まで）"
}

/** 割り当て画面の選択肢の組み立て */
internal object LayoutChoices {
    /** 入力欄別の割り当てで「自動（既定）」を表す選択肢の値（保存時は null に戻す） */
    const val AUTO = ""

    private val RESERVED_IDS = setOf(LayoutSelect.TARGET_KANA, LayoutSelect.TARGET_ALPHA, LayoutSelect.FIELD_NONE)

    /**
     * 実際に読み込まれる有効なレイアウト（id → 情報）。ユーザーのものが同梱の同じ id を上書きする。
     * ユーザーのものが不正なら同梱へ戻る（[LayoutStore.load] と同じ）。無効なものは含めない
     */
    fun effective(presets: List<LayoutInfo>, users: List<LayoutInfo>): Map<String, LayoutInfo> {
        val m = LinkedHashMap<String, LayoutInfo>()
        for (p in presets) if (p.valid) m[p.id] = p
        for (u in users) if (u.valid) m[u.id] = u
        return m
    }

    private fun label(i: LayoutInfo): String = if (i.source == LayoutSource.USER) "${i.name}（自分）" else i.name

    /** id → 表示名 */
    fun names(effective: Map<String, LayoutInfo>): Map<String, String> = effective.mapValues { it.value.name }

    /**
     * かな / 英字に割り当てられるレイアウトの選択肢 (id, 表示名)。そのモードに合うもの（custom は両方）だけ。
     * [current] が有効なレイアウトならモードが合わなくても含める（現在の割り当てを選択肢から消さない）
     */
    fun slotOptions(slot: AssignSlot, effective: Map<String, LayoutInfo>, current: String?): List<Pair<String, String>> {
        val want: Set<LayoutMode> = when (slot) {
            AssignSlot.KANA -> setOf(LayoutMode.KANA, LayoutMode.CUSTOM)
            AssignSlot.ALPHA -> setOf(LayoutMode.ALPHA, LayoutMode.CUSTOM)
        }
        val list = ArrayList<Pair<String, String>>()
        for (i in effective.values) if (i.mode != null && i.mode in want) list.add(i.id to label(i))
        if (current != null && list.none { it.first == current }) {
            effective[current]?.let { list.add(current to label(it)) }
        }
        return list
    }

    /** 切替先の表示名。null は「切り替えない」 */
    fun targetLabel(target: String?, names: Map<String, String>): String = when (target) {
        null -> "切り替えない"
        LayoutSelect.TARGET_KANA -> "かな"
        LayoutSelect.TARGET_ALPHA -> "英字"
        else -> names[target] ?: target
    }

    /** 入力欄の種類ごとの選択肢 (値, 表示名)。「自動」「かな」「英字」「切り替えない」の後に各レイアウト */
    fun fieldOptions(kind: FieldKind, effective: Map<String, LayoutInfo>): List<Pair<String, String>> {
        val names = names(effective)
        val list = ArrayList<Pair<String, String>>()
        list.add(AUTO to "自動（${targetLabel(LayoutSelect.defaultTarget(kind), names)}）")
        list.add(LayoutSelect.TARGET_KANA to "かな")
        list.add(LayoutSelect.TARGET_ALPHA to "英字")
        list.add(LayoutSelect.FIELD_NONE to "切り替えない")
        // kana / alpha / none という id のレイアウトは上の特別な値と区別できないので出さない
        for (i in effective.values) if (i.id !in RESERVED_IDS) list.add(i.id to label(i))
        return list
    }

    /** 選択肢の「いま選ばれている値」。未設定（既定）は [AUTO] */
    fun fieldSelected(assigned: String?): String = assigned ?: AUTO

    /** 選択肢の値を保存する値に戻す（[AUTO] は null = 既定に戻す） */
    fun fieldValueToStore(value: String): String? = if (value == AUTO) null else value

    /** 行の補足: 現在実際に使われる切替先 */
    fun fieldSubtitle(kind: FieldKind, assignments: Map<FieldKind, String>, names: Map<String, String>): String =
        "いまの切替先: " + targetLabel(LayoutSelect.resolveFieldTarget(kind, assignments), names)
}

/** 取り込み前の確認文・結果文 */
internal object LayoutImportText {
    fun confirmTitle(viaLink: Boolean): String = if (viaLink) "リンクのレイアウトを取り込む" else "レイアウトを取り込む"

    fun confirmMessage(
        name: String,
        id: String,
        mode: LayoutMode?,
        keyCount: Int,
        replaces: Boolean,
        overridesPreset: Boolean,
        viaLink: Boolean,
    ): String {
        val sb = StringBuilder()
        sb.append("名前: ").append(name).append('\n')
        sb.append("id: ").append(id).append('\n')
        sb.append("モード: ").append(LayoutTexts.modeLabel(mode)).append('\n')
        sb.append("キー数: ").append(keyCount)
        if (replaces) sb.append("\n\n同じ id の自分のレイアウトを置き換えます。")
        if (overridesPreset) sb.append("\n\n同梱のレイアウト（").append(id).append("）と同じ id です。取り込むと、同梱のものの代わりにこちらが使われます。")
        if (viaLink) sb.append("\n\n外部のアプリやページから開かれたリンクです。心当たりがなければ取り込まないでください。")
        return sb.toString()
    }

    fun success(name: String, replaced: Boolean): String = if (replaced) "「$name」を置き換えました" else "「$name」を取り込みました"
}

/** [LayoutIntake.classify] の結果 */
internal sealed class IntakeResult {
    /** 取り込む対象ではない（VIEW 以外・データなし） */
    object None : IntakeResult()

    /** 受け入れる。[url] は `nagi://layout?...`（解釈・検証はこの後） */
    class Accept(val url: String) : IntakeResult()

    class Reject(val reason: String) : IntakeResult()
}

/** `nagi://layout` の受け入れ判定（スキームとホスト、長さだけ。中身の検証と確認ダイアログは呼び出し側） */
internal object LayoutIntake {
    private const val LINK_PREFIX = "nagi://layout?data="

    fun classify(isView: Boolean, data: String?): IntakeResult {
        if (!isView || data.isNullOrBlank()) return IntakeResult.None
        return classifyUrl(data)
    }

    /** 貼り付けたリンクなど、VIEW の Intent を介さない文字列の判定 */
    fun classifyUrl(raw: String): IntakeResult {
        val url = raw.trim()
        val schemeEnd = url.indexOf("://")
        if (schemeEnd < 0 || !url.substring(0, schemeEnd).equals("nagi", ignoreCase = true)) {
            return IntakeResult.Reject("対応していない URL です")
        }
        val host = url.substring(schemeEnd + 3).substringBefore('#').substringBefore('?').trimEnd('/')
        if (!host.equals("layout", ignoreCase = true)) return IntakeResult.Reject("対応していない URL です")
        if (url.length > LayoutLink.MAX_DATA_CHARS + URL_EXTRA) {
            return IntakeResult.Reject("リンクが大きすぎます（${LayoutLink.MAX_DATA_CHARS / 1024} KB まで）")
        }
        return IntakeResult.Accept(url)
    }

    /** 貼り付けた文字列が、JSON ではなく nagi:// のリンクに見えるか */
    fun looksLikeLink(text: String): Boolean = text.trimStart().startsWith("nagi://", ignoreCase = true)

    /** 生成したリンクが、受け取り側の上限（data の文字数）を超えるか */
    fun linkTooLong(link: String): Boolean = link.length - LINK_PREFIX.length > LayoutLink.MAX_DATA_CHARS

    private const val URL_EXTRA = 4096
}

/** レイアウト JSON の複製・整形（org.json で読み書きするので、キーの順序は保たれない場合がある） */
internal object LayoutCopy {
    /** 複製の id。`<id>-copy`、`<id>-copy2`… のうち [taken] に無いもの。64 文字に収める */
    fun freeId(baseId: String, taken: Set<String>): String {
        var n = 1
        while (true) {
            val suffix = if (n == 1) "-copy" else "-copy$n"
            val id = baseId.take(64 - suffix.length) + suffix
            if (id !in taken) return id
            n++
        }
    }

    /** 複製の名前。「<name> のコピー」を 64 文字（コードポイント）に収める */
    fun copyName(name: String): String = truncateCodePoints("$name のコピー", LayoutLimits.NAME_MAX)

    fun truncateCodePoints(s: String, max: Int): String {
        if (s.codePointCount(0, s.length) <= max) return s
        return s.substring(0, s.offsetByCodePoints(0, max))
    }

    /** JSON の id と name を差し替える。読めなければ null */
    fun rewrite(raw: String, newId: String, newName: String): String? = try {
        val o = JSONObject(raw.removePrefix("﻿"))
        o.put("id", newId)
        o.put("name", newName)
        o.toString(2) + "\n"
    } catch (e: Exception) {
        null
    }

    /** 空白を詰めた JSON（リンクを短くする用）。読めなければ null */
    fun minify(raw: String): String? = try {
        JSONObject(raw.removePrefix("﻿")).toString()
    } catch (e: Exception) {
        null
    }
}
