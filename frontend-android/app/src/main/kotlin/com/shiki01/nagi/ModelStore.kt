package com.shiki01.nagi

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.io.IOException

/** インストール済みのモデルファイル */
internal data class InstalledModel(val name: String, val file: File, val sizeBytes: Long) {
    val spec: ModelSpec get() = ModelSpec(name, sizeBytes)
}

/** 変換モデル（GGUF）の置き場 `<filesDir>/data/models/` の操作 */
internal object ModelStore {
    fun modelsDir(ctx: Context): File = File(DataInstaller.dataDir(ctx), "models")

    /** 完成している `.gguf`（`.partial` は含まない）を名前順に返す */
    fun list(ctx: Context): List<InstalledModel> {
        val files = modelsDir(ctx).listFiles() ?: return emptyList()
        return files
            .filter { it.isFile && it.name.endsWith(".gguf", ignoreCase = true) }
            .sortedBy { it.name }
            .map { InstalledModel(it.name, it, it.length()) }
    }

    /** 1 つでもモデルがあるか（エンジンの起動時に LLM を有効にするかの判定に使う） */
    fun hasModel(ctx: Context): Boolean = list(ctx).isNotEmpty()

    /**
     * いま使われるモデル。`conversion.model`（[configured]）が実在する絶対パスならそれ、
     * そうでなければ（コアと同じく）既定名のファイル。どちらも無ければ null
     */
    fun effective(ctx: Context, configured: String?): InstalledModel? {
        val all = list(ctx)
        if (!configured.isNullOrEmpty()) {
            val f = File(configured)
            if (f.isAbsolute) return all.firstOrNull { it.file.absolutePath == f.absolutePath }
            if (configured.endsWith(".gguf")) {
                // 相対名はコアではカレントディレクトリ基準で読まれ、見つからない。使用中とはみなさない
                return null
            }
        }
        return all.firstOrNull { it.name == ModelRecommendation.DEFAULT_MODEL_FILE }
    }

    /** ファイル名として安全な `.gguf` 名か（パス区切りや隠しファイルを拒否） */
    fun isValidModelName(name: String): Boolean =
        name.endsWith(".gguf", ignoreCase = true) &&
            name.length > ".gguf".length &&
            !name.contains('/') && !name.contains('\\') && !name.startsWith(".")

    fun delete(ctx: Context, name: String): Boolean {
        if (!isValidModelName(name)) return false
        return File(modelsDir(ctx), name).delete()
    }

    /** SAF で選んだファイルの表示名 */
    fun displayName(ctx: Context, uri: Uri): String? {
        ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) {
                val i = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (i >= 0) return c.getString(i)
            }
        }
        return uri.lastPathSegment?.substringAfterLast('/')
    }

    /**
     * SAF で選んだ `.gguf` を `models/` へコピーする（バックグラウンドスレッドで呼ぶ）。
     * `.partial` に書いて GGUF のマジックを確認してから置き換える。失敗しても既存のファイルは残る。
     * 戻り値はインストールしたファイル
     */
    fun importFrom(ctx: Context, uri: Uri, onProgress: (Long) -> Unit): File {
        val name = displayName(ctx, uri)?.trim().orEmpty()
        if (!isValidModelName(name)) throw IOException("拡張子が .gguf のファイルを選んでください（選択: ${name.ifEmpty { "不明" }}）")
        val dir = modelsDir(ctx)
        dir.mkdirs()
        val dest = File(dir, name)
        val partial = File(dir, "$name.partial")
        partial.delete()
        try {
            val input = ctx.contentResolver.openInputStream(uri) ?: throw IOException("ファイルを開けません")
            var copied = 0L
            var lastReport = 0L
            input.use { src ->
                partial.outputStream().use { out ->
                    val buf = ByteArray(1 shl 20)
                    while (true) {
                        val n = src.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        copied += n
                        val now = System.currentTimeMillis()
                        if (now - lastReport >= 200) {
                            lastReport = now
                            onProgress(copied)
                        }
                    }
                }
            }
            onProgress(copied)
            if (!hasGgufMagic(partial)) throw IOException("GGUF 形式のファイルではありません")
            DataDownloader.replace(partial, dest)
            return dest
        } catch (e: Throwable) {
            partial.delete()
            throw e
        }
    }

    private fun hasGgufMagic(f: File): Boolean = try {
        f.inputStream().use { s ->
            val b = ByteArray(4)
            s.read(b) == 4 && String(b, Charsets.US_ASCII) == "GGUF"
        }
    } catch (e: IOException) {
        false
    }
}
