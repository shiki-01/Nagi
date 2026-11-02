package com.shiki01.nagi

import android.content.Context
import java.io.File
import java.io.IOException

/** 配布データ（辞書）の置き場。コアは辞書を mmap で読むので、assets から実ファイルに展開する */
internal object DataInstaller {
    private const val DICT_ASSET = "dict/nagi-dict.bin"

    fun dataDir(context: Context): File = File(context.filesDir, "data")

    fun userDir(context: Context): File = File(context.filesDir, "user")

    fun dictFile(context: Context): File = File(dataDir(context), "dict/nagi-dict.bin")

    /** 辞書が無い、またはアプリが更新されていれば assets から展開する。assets に無ければ何もしない */
    fun ensure(context: Context) {
        userDir(context).mkdirs()
        val dict = dictFile(context)
        val stampFile = File(dict.parentFile, "nagi-dict.stamp")
        val stamp = try {
            context.packageManager.getPackageInfo(context.packageName, 0).lastUpdateTime.toString()
        } catch (e: Exception) {
            "0"
        }
        if (dict.exists() && stampFile.exists() && stampFile.readText() == stamp) return
        dict.parentFile?.mkdirs()
        val tmp = File(dict.parentFile, "nagi-dict.bin.tmp")
        try {
            context.assets.open(DICT_ASSET).use { input ->
                tmp.outputStream().use { out -> input.copyTo(out, 1 shl 20) }
            }
        } catch (e: IOException) {
            // assets に辞書が無いビルド（開発用）。コアは読みそのままで動く
            tmp.delete()
            return
        }
        if (dict.exists()) dict.delete()
        if (tmp.renameTo(dict)) stampFile.writeText(stamp)
    }
}
