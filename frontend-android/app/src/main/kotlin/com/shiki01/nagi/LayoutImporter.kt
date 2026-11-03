package com.shiki01.nagi

import android.content.Context

/** 取り込み前の確認結果（保存はしていない）。確認ダイアログの表示に使う */
internal class LayoutCandidate(
    val layout: LayoutModel,
    val json: String,
    /** 同じ ID のユーザーのレイアウトがあり、取り込むと置き換わる */
    val replaces: Boolean,
    /** 同梱と同じ ID（ユーザーのものが優先して使われる） */
    val overridesPreset: Boolean,
    /** この端末がまだ反映しない指定（JSON に実際に含まれるものだけ。[LayoutCompat.UNSUPPORTED] の順）。確認ダイアログで知らせる */
    val unsupported: List<String> = emptyList(),
) {
    val keyCount: Int get() = layout.keyCount
}

/** [LayoutImporter.inspect] の結果 */
internal sealed class InspectResult {
    class Ok(val candidate: LayoutCandidate) : InspectResult()

    class Err(val reason: String) : InspectResult()
}

/** [LayoutImporter.importJson] の結果 */
internal sealed class ImportResult {
    class Success(
        val candidate: LayoutCandidate,
        /** 取得できなかった Iconify アイコンの案内（無ければ空文字。取り込みの通知に足す。iOS の `iconFallbackNote` 相当） */
        val iconNote: String = "",
    ) : ImportResult()

    class Failure(val reason: String) : ImportResult()
}

/** ID の衝突の規則（純粋関数。単体テスト対象） */
internal object LayoutConflict {
    /**
     * 取り込み可否の理由。null なら取り込める。
     * - 同じ ID のユーザーのレイアウトは置き換える（確認は呼び出し側）。
     * - 同梱と同じ ID: ファイル・貼り付けは許可（ユーザーのものが優先して読まれる）。
     *   URL 経由（[allowPresetId] = false）は拒否する（iOS と同じ）。
     */
    fun rejection(id: String, presetIds: Set<String>, allowPresetId: Boolean): String? =
        if (!allowPresetId && id in presetIds) "同梱のレイアウトと同じ id（$id）は、URL からは取り込めません" else null

    fun replaces(id: String, userIds: Set<String>): Boolean = id in userIds

    fun overridesPreset(id: String, presetIds: Set<String>): Boolean = id in presetIds
}

/**
 * レイアウト JSON の取り込み。画面（ファイル選択・貼り付け・`nagi://layout` の受け取りと確認ダイアログ）は呼び出し側で作る。
 * 流れは「[inspect]（検証。保存しない）→ 確認 → [importJson]（検証 → ID 衝突処理 → 保存）」。
 * ファイル・assets を読み書きするので、メインスレッド以外で呼ぶのが望ましい。
 */
internal object LayoutImporter {
    /**
     * [jsonText] を検証する（保存しない）。`layout.target` は存在するレイアウト（ユーザー・同梱）・組み込み名・自身の id に限る。
     * @param allowPresetId false なら同梱と同じ id を拒否する（URL 経由）
     */
    fun inspect(ctx: Context, jsonText: String, allowPresetId: Boolean = true): InspectResult {
        val presets = LayoutStore.presetIds(ctx).toSet()
        val users = LayoutStore.userIds(ctx).toSet()
        return when (val r = LayoutParser.parse(jsonText, presets + users)) {
            is LayoutParseResult.Err -> InspectResult.Err(r.reason)
            is LayoutParseResult.Ok -> {
                val id = r.layout.id
                val reject = LayoutConflict.rejection(id, presets, allowPresetId)
                if (reject != null) {
                    InspectResult.Err(reject)
                } else {
                    InspectResult.Ok(
                        LayoutCandidate(
                            r.layout,
                            jsonText,
                            LayoutConflict.replaces(id, users),
                            LayoutConflict.overridesPreset(id, presets),
                            LayoutCompat.unsupported(jsonText),
                        ),
                    )
                }
            }
        }
    }

    /**
     * 検証して ID 衝突を処理し、`filesDir/layouts/<id>.json` に保存する。同じ ID のユーザーのレイアウトは置き換える
     * （上書きの確認が要るなら先に [inspect] の [LayoutCandidate.replaces] で確認すること）。
     * レイアウトが使う Iconify アイコンは、保存の前に Iconify の API から取得して端末に保存する（ネットワークを使う）。
     * 取得できなくても取り込みは成功させ、[ImportResult.Success.iconNote] に案内を入れる（キーは label などで表示する）。
     * 新しく取得するアイコンが [IconifyLogic.MAX_FETCHES_PER_IMPORT] 種類を超えると失敗にする（iOS と同じ）。
     * 失敗は日本語の理由つき
     */
    fun importJson(ctx: Context, jsonText: String, allowPresetId: Boolean = true): ImportResult {
        val candidate = when (val r = inspect(ctx, jsonText, allowPresetId)) {
            is InspectResult.Err -> return ImportResult.Failure(r.reason)
            is InspectResult.Ok -> r.candidate
        }
        val icons = IconifyResolver.resolve(ctx, candidate.layout)
        if (icons.tooMany) {
            return ImportResult.Failure("新しく取得するアイコンが多すぎます（${IconifyLogic.MAX_FETCHES_PER_IMPORT} 種類まで）")
        }
        if (!LayoutStore.saveUser(ctx, candidate.layout, candidate.json)) {
            return ImportResult.Failure("保存できませんでした")
        }
        return ImportResult.Success(candidate, IconifyLogic.failureNote(icons.failed))
    }

    /**
     * `nagi://layout?data=<base64url の JSON>` を解釈する（保存も存在確認もしない）。
     * 取り込むなら、結果の JSON を [inspect]（`allowPresetId = false`）→ 確認ダイアログ → [importJson]（同）へ渡す
     */
    fun parseDeepLink(uri: String): DeepLinkResult = LayoutLink.parse(uri)
}
