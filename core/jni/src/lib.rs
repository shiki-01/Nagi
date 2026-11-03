//! Nagi コアエンジンの JNI ラッパー。
//!
//! Kotlin 側は `com.shiki01.nagi.core.NagiCore`（`external fun`）から呼ぶ。実処理は `nagi-ffi`
//! （C ABI）に任せ、ここは JVM との文字列・コールバックの橋渡しだけを行う。
//! Kotlin が持つ `Long` は `Box<Handle>` のポインタで、`nativeFree` で解放する。

use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString};
use jni::sys::{jboolean, jlong, jstring};
use jni::{JNIEnv, JavaVM};

use nagi_ffi::NagiServerHandle;

/// 通知待ちコールバックの呼び出し先（Kotlin の `Runnable`）
struct WakerCtx {
    vm: JavaVM,
    target: GlobalRef,
}

/// Kotlin に渡すハンドル本体
struct Handle {
    ptr: NagiServerHandle,
    waker: Option<Box<WakerCtx>>,
}

/// `nagi_set_waker` に登録する C コールバック。任意のスレッドから呼ばれる
unsafe extern "C" fn wake_trampoline(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    // SAFETY: `user_data` は `Box<WakerCtx>` のポインタで、`nativeFree` / 差し替えの後まで有効
    let ctx = unsafe { &*(user_data as *const WakerCtx) };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(mut env) = ctx.vm.attach_current_thread_permanently() {
            let _ = env.call_method(&ctx.target, "run", "()V", &[]);
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
        }
    }));
}

fn opt_string(env: &mut JNIEnv, s: &JString) -> Option<CString> {
    if s.is_null() {
        return None;
    }
    let s: String = env.get_string(s).ok()?.into();
    CString::new(s).ok()
}

/// C 文字列（所有権を受け取る）を Java 文字列にする。NULL は空文字列
fn take_c_string(env: &mut JNIEnv, p: *mut std::os::raw::c_char) -> jstring {
    let text = if p.is_null() {
        String::new()
    } else {
        // SAFETY: nagi-ffi が `CString::into_raw` で返した NUL 終端の文字列
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        // SAFETY: 上記ポインタの所有権はここにあり、一度だけ解放する
        unsafe { nagi_ffi::nagi_string_free(p) };
        s
    };
    env.new_string(text)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn handle_ref<'a>(handle: jlong) -> Option<&'a mut Handle> {
    if handle == 0 {
        None
    } else {
        // SAFETY: Kotlin 側が `nativeNew` の戻り値をそのまま渡す契約（解放後は渡さない）
        Some(unsafe { &mut *(handle as *mut Handle) })
    }
}

/// サーバーを作る。失敗時は 0
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeNew(
    mut env: JNIEnv,
    _class: JClass,
    data_dir: JString,
    user_dir: JString,
    no_llm: jboolean,
    memory_budget_mb: jlong,
) -> jlong {
    let data = opt_string(&mut env, &data_dir);
    let user = opt_string(&mut env, &user_dir);
    let budget = u64::try_from(memory_budget_mb).unwrap_or(0);
    let ptr = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: 引数は NUL 終端の UTF-8 文字列または NULL
        unsafe {
            nagi_ffi::nagi_server_new_with_budget(
                data.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                user.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                i32::from(no_llm != 0),
                budget,
            )
        }
    }))
    .unwrap_or(std::ptr::null_mut());
    if ptr.is_null() {
        return 0;
    }
    Box::into_raw(Box::new(Handle { ptr, waker: None })) as jlong
}

/// JSON-RPC の 1 行を処理して応答行（改行区切り）を返す
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeHandleLine(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    line: JString,
) -> jstring {
    let Some(h) = handle_ref(handle) else {
        return take_c_string(&mut env, std::ptr::null_mut());
    };
    let Some(line) = opt_string(&mut env, &line) else {
        return take_c_string(&mut env, std::ptr::null_mut());
    };
    // SAFETY: ハンドルは有効で、line は NUL 終端
    let out = unsafe { nagi_ffi::nagi_handle_line(h.ptr, line.as_ptr()) };
    take_c_string(&mut env, out)
}

/// 溜まった非同期通知の行を取り出す（なければ空文字列）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativePollNotifications(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let Some(h) = handle_ref(handle) else {
        return take_c_string(&mut env, std::ptr::null_mut());
    };
    // SAFETY: ハンドルは有効
    let out = unsafe { nagi_ffi::nagi_poll_notifications(h.ptr) };
    take_c_string(&mut env, out)
}

/// 通知が取り出せるようになったときに呼ぶ `Runnable` を登録する（null で解除）。
/// `Runnable.run` は任意のスレッドから呼ばれるので、中でコアを呼ばずに自分のスレッドへ投げること
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeSetWaker(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    waker: JObject,
) {
    let Some(h) = handle_ref(handle) else {
        return;
    };
    let new_ctx = if waker.is_null() {
        None
    } else {
        match (env.get_java_vm(), env.new_global_ref(&waker)) {
            (Ok(vm), Ok(target)) => Some(Box::new(WakerCtx { vm, target })),
            _ => return,
        }
    };
    let user_data = new_ctx.as_ref().map_or(std::ptr::null_mut(), |b| {
        &**b as *const WakerCtx as *mut c_void
    });
    // SAFETY: ハンドルは有効。user_data は `h.waker` が保持している間は有効
    unsafe {
        nagi_ffi::nagi_set_waker(
            h.ptr,
            if new_ctx.is_some() {
                Some(wake_trampoline)
            } else {
                None
            },
            user_data,
        );
    }
    // 差し替え後に古い方を捨てる（nagi_set_waker はコールバック実行中なら終わるまで待つ）
    h.waker = new_ctx;
}

/// サーバーを解放する（学習データの保存を含む）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeFree(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    // SAFETY: `nativeNew` の Box を一度だけ戻す
    let h = unsafe { Box::from_raw(handle as *mut Handle) };
    // 先にコアを解放（waker の登録を外してから Server を捨てる）。その後で waker の文脈を捨てる
    // SAFETY: ハンドルは有効で、二重解放しない
    unsafe { nagi_ffi::nagi_server_free(h.ptr) };
    drop(h.waker);
}

/// 起動時の診断情報（notes + プロファイル）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeNotes(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let Some(h) = handle_ref(handle) else {
        return take_c_string(&mut env, std::ptr::null_mut());
    };
    // SAFETY: ハンドルは有効
    let out = unsafe { nagi_ffi::nagi_last_notes(h.ptr) };
    take_c_string(&mut env, out)
}

/// コアのバージョン文字列
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeVersion(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    // SAFETY: 静的な NUL 終端文字列（解放不要）
    let s = unsafe { CStr::from_ptr(nagi_ffi::nagi_version()) }
        .to_string_lossy()
        .into_owned();
    env.new_string(s)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// 任意データ（英語語彙・カタカナ語）の取得元一覧（JSON）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeOptionalDataSources(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    take_c_string(&mut env, nagi_ffi::nagi_optional_data_sources())
}

/// 取得済みアーカイブから索引を組み立てる。成功なら null、失敗なら理由
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeBuildOptionalIndex(
    mut env: JNIEnv,
    _class: JClass,
    kind: JString,
    archive_path: JString,
    out_path: JString,
) -> jstring {
    let (Some(k), Some(a), Some(o)) = (
        opt_string(&mut env, &kind),
        opt_string(&mut env, &archive_path),
        opt_string(&mut env, &out_path),
    ) else {
        return env
            .new_string("引数が不正です")
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut());
    };
    // SAFETY: 3 つとも NUL 終端の UTF-8 文字列
    let out = unsafe { nagi_ffi::nagi_build_optional_index(k.as_ptr(), a.as_ptr(), o.as_ptr()) };
    if out.is_null() {
        std::ptr::null_mut()
    } else {
        take_c_string(&mut env, out)
    }
}

/// 16 進数文字列（大文字小文字不問）をバイト列にする。不正なら None
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.is_ascii() || s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// ed25519 署名を検証する。公開鍵（32 バイト）・署名（64 バイト）が不正な形でも false を返す
fn verify_ed25519(public_key_hex: &str, data: &[u8], signature_hex: &str) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let Some(key) = decode_hex(public_key_hex).and_then(|b| <[u8; 32]>::try_from(b).ok()) else {
        return false;
    };
    let Some(sig) = decode_hex(signature_hex).and_then(|b| <[u8; 64]>::try_from(b).ok()) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key) else {
        return false;
    };
    key.verify(data, &Signature::from_bytes(&sig)).is_ok()
}

/// ed25519 署名の検証（署名対象は `data` 全体）。不正な入力は false（panic しない）
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeVerifyEd25519(
    mut env: JNIEnv,
    _class: JClass,
    public_key_hex: JString,
    data: JByteArray,
    signature_hex: JString,
) -> jboolean {
    let ok = catch_unwind(AssertUnwindSafe(|| {
        if public_key_hex.is_null() || data.is_null() || signature_hex.is_null() {
            return false;
        }
        let Ok(pk) = env.get_string(&public_key_hex).map(String::from) else {
            return false;
        };
        let Ok(sig) = env.get_string(&signature_hex).map(String::from) else {
            return false;
        };
        let Ok(bytes) = env.convert_byte_array(&data) else {
            return false;
        };
        verify_ed25519(&pk, &bytes, &sig)
    }))
    .unwrap_or(false);
    jboolean::from(ok)
}

/// ed25519 署名の検証（署名対象はファイルの内容全体）。大きな配布物を JVM ヒープに載せずに検証する。
/// 読めない・不正な入力は false
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_shiki01_nagi_core_NagiCore_nativeVerifyEd25519File(
    mut env: JNIEnv,
    _class: JClass,
    public_key_hex: JString,
    path: JString,
    signature_hex: JString,
) -> jboolean {
    let ok = catch_unwind(AssertUnwindSafe(|| {
        if public_key_hex.is_null() || path.is_null() || signature_hex.is_null() {
            return false;
        }
        let Ok(pk) = env.get_string(&public_key_hex).map(String::from) else {
            return false;
        };
        let Ok(sig) = env.get_string(&signature_hex).map(String::from) else {
            return false;
        };
        let Ok(path) = env.get_string(&path).map(String::from) else {
            return false;
        };
        match std::fs::read(&path) {
            Ok(bytes) => verify_ed25519(&pk, &bytes, &sig),
            Err(_) => false,
        }
    }))
    .unwrap_or(false);
    jboolean::from(ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn verify_accepts_valid_and_rejects_tampered() {
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let public = hex(key.verifying_key().as_bytes());
        let data = b"nagi-apk-bytes";
        let sig = hex(&key.sign(data).to_bytes());
        assert!(verify_ed25519(&public, data, &sig));
        assert!(verify_ed25519(&public.to_uppercase(), data, &sig));
        assert!(!verify_ed25519(&public, b"nagi-apk-byteS", &sig));
        let other = SigningKey::from_bytes(&[2u8; 32]);
        assert!(!verify_ed25519(
            &hex(other.verifying_key().as_bytes()),
            data,
            &sig
        ));
    }

    #[test]
    fn verify_rejects_malformed_input() {
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let public = hex(key.verifying_key().as_bytes());
        let sig = hex(&key.sign(b"x").to_bytes());
        assert!(!verify_ed25519("", b"x", &sig));
        assert!(!verify_ed25519("zz", b"x", &sig));
        assert!(!verify_ed25519(&public, b"x", ""));
        assert!(!verify_ed25519(&public, b"x", &sig[..126]));
        assert!(!verify_ed25519(&public, b"x", &sig[..127]));
        assert!(!verify_ed25519(&public, b"x", "あ"));
    }
}
