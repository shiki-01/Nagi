package com.shiki01.nagi

import java.io.File

/** レイアウトの単体テスト用の入力。Gradle のテストの作業ディレクトリ（frontend-android/app）からの相対パスで config/keyboard を読む */
internal object LayoutFixtures {
    private val root = File("../../config/keyboard")

    val presetFiles: List<File> get() = (File(root, "presets").listFiles() ?: emptyArray()).filter { it.name.endsWith(".json") }.sortedBy { it.name }

    fun testLayout(name: String): String = File(File(root, "test-layouts"), name).readText(Charsets.UTF_8)

    val presetIds: Set<String> get() = presetFiles.map { it.name.removeSuffix(".json") }.toSet()

    /** 1 行 1 キーの最小のレイアウト。[key] はキーの JSON（`{...}`） */
    fun minimal(key: String = """{"id": "a", "kind": "text", "text": "a"}""", id: String = "t1", extra: String = ""): String =
        """{"schema": 2, "id": "$id", "name": "テスト", "mode": "kana", "rows": [{"keys": [$key]}]$extra}"""

    fun parse(json: String, known: Set<String>? = null): LayoutParseResult = LayoutParser.parse(json, known)

    fun reason(json: String, known: Set<String>? = null): String? = (parse(json, known) as? LayoutParseResult.Err)?.reason
}
