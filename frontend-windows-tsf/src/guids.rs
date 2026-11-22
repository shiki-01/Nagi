//! この IME を識別する GUID。値は固定（変えると登録済みの入力方式が別物になる）。

use windows::core::GUID;

/// TextService の CLSID
pub const CLSID_NAGI_TEXT_SERVICE: GUID = GUID::from_u128(0xB0D6897E_C414_4BB1_B621_46EEE659E0CD);

/// 入力方式（言語プロファイル）の GUID
pub const GUID_NAGI_PROFILE: GUID = GUID::from_u128(0x5B02A01F_6B4F_430F_A6A8_D9B4D83C1918);

/// UILess モードで `BeginUIElement` に渡す候補リスト要素の GUID（`ITfUIElement::GetGUID`）
pub const GUID_NAGI_CANDIDATE_LIST_ELEMENT: GUID =
    GUID::from_u128(0x10ED1F08_8044_4596_883C_5026DE54CB87);

/// `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`: UILess（`ITfUIElementMgr` で候補を通知する）に対応する宣言。
/// 値は Windows SDK の msctf.h（`windows` クレート 0.62 の `GUID_TFCAT_TIPCAP_UIELEMENTENABLED` も同じ。
/// win32metadata 由来）の `{49D2F9CF-1F5E-11D7-A6D3-00065B84435C}`。隣の `…CE` は SECUREMODE で別物（H-129）
pub const CATEGORY_UIELEMENTENABLED: GUID = GUID::from_u128(0x49D2F9CF_1F5E_11D7_A6D3_00065B84435C);

/// `GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT`: 入力モードのコンパートメント
/// （`GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION`。`mode_comp`）に対応する宣言。
/// 値は msctf.h の `{CCF05DD7-4A87-11D7-A6E2-00065B84435C}`（Microsoft IME・Mozc も登録）
pub const CATEGORY_INPUTMODECOMPARTMENT: GUID =
    GUID::from_u128(0xCCF05DD7_4A87_11D7_A6E2_00065B84435C);

/// `GUID_TFCAT_TIPCAP_SECUREMODE`: セキュアデスクトップ（ログオン画面等）対応の宣言。Nagi は宣言しない。
/// 値は msctf.h の `{49D2F9CE-…}`。登録はせず、過去に誤って登録された場合の掃除（`UnregisterCategory`）にだけ使う
pub const CATEGORY_SECUREMODE: GUID = GUID::from_u128(0x49D2F9CE_1F5E_11D7_A6D3_00065B84435C);

/// 日本語（ja-JP）の LANGID
pub const LANGID_JAPANESE: u16 = 0x0411;

/// 入力方式の表示名
pub const PROFILE_DESCRIPTION: &str = "Nagi";

/// GUID をレジストリのキー名の形式 `{XXXXXXXX-XXXX-...}` にする
pub fn braced(guid: &GUID) -> String {
    format!("{{{guid:?}}}")
}
