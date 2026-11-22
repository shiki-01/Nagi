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

/// DLL の文字列リソース ID: 入力方式の表示名（`build.rs` の STRINGTABLE と一致させる）
pub const STRING_ID_NAME: u32 = 101;

/// DLL の文字列リソース ID: 入力方式の説明（同上）
pub const STRING_ID_DESCRIPTION: u32 = 102;

/// 言語プロファイルのレジストリキー（`HKLM` 以下）。`RegisterProfile` が書く場所と同じ。
/// `clsid` / `profile` は `{XXXXXXXX-...}` 形式、`langid` は LANGID（日本語は `0x0411`）
pub fn language_profile_key(clsid: &str, langid: u16, profile: &str) -> String {
    format!(r"SOFTWARE\Microsoft\CTF\TIP\{clsid}\LanguageProfile\0x{langid:08x}\{profile}")
}

/// 言語プロファイルの `Display Description`（REG_EXPAND_SZ）の値。設定アプリが
/// `@<モジュール>,-<文字列 ID>` を MUI 形式で解決して表示名にする（Microsoft IME は
/// `@%SystemRoot%\system32\input.dll,-5183`）。`dll_path` は DLL の絶対パス
pub fn display_description_value(dll_path: &str) -> String {
    format!("@{dll_path},-{STRING_ID_NAME}")
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

    #[test]
    fn language_profile_key_uses_eight_digit_hex_langid() {
        assert_eq!(
            language_profile_key("{AAAA}", 0x0411, "{BBBB}"),
            r"SOFTWARE\Microsoft\CTF\TIP\{AAAA}\LanguageProfile\0x00000411\{BBBB}"
        );
    }

    #[test]
    fn display_description_points_at_string_resource() {
        assert_eq!(
            display_description_value(r"C:\Program Files\Nagi\nagi_windows_tsf.dll"),
            r"@C:\Program Files\Nagi\nagi_windows_tsf.dll,-101"
        );
    }
}
