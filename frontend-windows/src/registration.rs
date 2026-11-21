//! COM サーバー登録（`HKCR\CLSID\{...}\InprocServer32`）に書く値。レジストリ操作は TSF 層、値の組み立てはここ（H-124）。
//!
//! `ThreadingModel` は `Both`。`Apartment` だと、MTA のスレッドから `CoCreateInstance` する呼び出し元
//! （設定アプリ SystemSettings が「言語のオプション」で `ITfFnConfigure` を取りに来る経路）は別アパートメントの
//! プロキシを受け取り、標準のプロキシ・スタブを持たない `ITfFnConfigure` の QueryInterface が失敗して
//! 「オプション」が出ない可能性がある。`Both` なら呼び出し元のアパートメントにそのまま読み込まれる。
//! STA のスレッド（TSF のアプリ側）では従来と同じ挙動になる。

/// `InprocServer32` の `ThreadingModel`
pub const THREADING_MODEL: &str = "Both";

/// `InprocServer32` キーに書く文字列値。`(値の名前, 値)`。名前 `None` は既定値
pub fn inproc_server_values(dll_path: &str) -> Vec<(Option<&'static str>, String)> {
    vec![
        (None, dll_path.to_owned()),
        (Some("ThreadingModel"), THREADING_MODEL.to_owned()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inproc_server_uses_both_threading_model() {
        let v = inproc_server_values(r"C:\Program Files\Nagi\nagi_windows_tsf.dll");
        assert_eq!(
            v[0],
            (
                None,
                r"C:\Program Files\Nagi\nagi_windows_tsf.dll".to_owned()
            )
        );
        assert_eq!(v[1], (Some("ThreadingModel"), "Both".to_owned()));
    }
}
