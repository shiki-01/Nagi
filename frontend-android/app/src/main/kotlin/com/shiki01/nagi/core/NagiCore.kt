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

    /** ed25519 署名の検証（署名対象は data 全体）。公開鍵・署名は 16 進文字列。不正な入力は false */
    @JvmStatic external fun nativeVerifyEd25519(publicKeyHex: String, data: ByteArray, signatureHex: String): Boolean

    /** ed25519 署名の検証（署名対象はファイルの内容全体）。大きな配布物を JVM ヒープに載せずに検証する。読めなければ false */
    @JvmStatic external fun nativeVerifyEd25519File(publicKeyHex: String, path: String, signatureHex: String): Boolean
}
