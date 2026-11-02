package com.shiki01.nagi.core

/**
 * コア（Rust）の JNI 入口。`core/jni/src/lib.rs` の関数と 1 対 1 に対応する。
 * ハンドルは `nativeNew` が返す Long で、`nativeFree` で解放する。
 */
internal object NagiCore {
    init {
        System.loadLibrary("nagi_jni")
    }

    /** 失敗時は 0 */
    @JvmStatic external fun nativeNew(dataDir: String?, userDir: String?, noLlm: Boolean, memoryBudgetMb: Long): Long

    @JvmStatic external fun nativeHandleLine(handle: Long, line: String): String

    @JvmStatic external fun nativePollNotifications(handle: Long): String

    /** `waker.run()` は任意のスレッドから呼ばれる。中でコアを呼ばないこと。null で解除 */
    @JvmStatic external fun nativeSetWaker(handle: Long, waker: Runnable?)

    @JvmStatic external fun nativeFree(handle: Long)

    @JvmStatic external fun nativeNotes(handle: Long): String

    @JvmStatic external fun nativeVersion(): String

    @JvmStatic external fun nativeOptionalDataSources(): String

    /** 成功なら null、失敗なら理由 */
    @JvmStatic external fun nativeBuildOptionalIndex(kind: String, archivePath: String, outPath: String): String?
}
