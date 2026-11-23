//! 入力モードの OS 側との同期（OS 非依存）。macOS 版の `requestMode` / `syncMode` に相当する。
//!
//! Windows のモード表示（タスクトレイの「あ / A」）は TSF のコンパートメント
//! `GUID_COMPARTMENT_KEYBOARD_OPENCLOSE`（開閉）と `GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION`
//! （変換モード）が正。コアのモードが真実で、コアの変化は OS へ書き、OS 側の操作（トレイのクリック・
//! 他の IME との往復）は `perform_action(SetMode)` でコアへ送る。**同じモードなら送らない・書かない**
//! （`SetMode` は入力中の文字列を確定するため。自分で書いた値の通知で往復しないようにする）。

use nagi_protocol::InputMode;

/// `TF_CONVERSIONMODE_ALPHANUMERIC`（英数。ビットではなく 0）
pub const CONVERSION_ALPHANUMERIC: u32 = 0x0000;
/// `TF_CONVERSIONMODE_NATIVE`（日本語入力）
pub const CONVERSION_NATIVE: u32 = 0x0001;
/// `TF_CONVERSIONMODE_KATAKANA`（カタカナ。`NATIVE` と組み合わせる）
pub const CONVERSION_KATAKANA: u32 = 0x0002;
/// `TF_CONVERSIONMODE_FULLSHAPE`（全角）
pub const CONVERSION_FULLSHAPE: u32 = 0x0008;
/// `TF_CONVERSIONMODE_ROMAN`（ローマ字入力。モードとは直交するので常に立てる）
pub const CONVERSION_ROMAN: u32 = 0x0010;

/// コンパートメントに書く値
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompartmentValues {
    /// `KEYBOARD_OPENCLOSE`（IME が開いているか）
    pub open: bool,
    /// `KEYBOARD_INPUTMODE_CONVERSION`（`TF_CONVERSIONMODE_*` の組み合わせ）
    pub conversion: u32,
}

/// コアのモード 1 つ分の対応（表示・コンパートメント。decisions.md H-140）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeRow {
    /// コアのモード
    pub mode: InputMode,
    /// 言語バー・ポップアップに出す 1 文字
    pub indicator: &'static str,
    /// ツールチップ・メニューの名前
    pub name: &'static str,
    /// `KEYBOARD_OPENCLOSE`
    pub open: bool,
    /// `TF_CONVERSIONMODE_*` のうちモードを表すビット（`ROMAN` は別に足す）
    pub bits: u32,
}

/// 全モードの対応表（メニューの並び順）。コア ↔ 表示 ↔ コンパートメントの正。
///
/// | モード | 表示 | 開閉 | 変換モード |
/// | --- | --- | --- | --- |
/// | ひらがな | あ | 開 | NATIVE \| FULLSHAPE |
/// | 全角カタカナ | カ | 開 | NATIVE \| KATAKANA \| FULLSHAPE |
/// | 半角カタカナ | ｶ | 開 | NATIVE \| KATAKANA |
/// | 全角英数 | Ａ | 開 | ALPHANUMERIC \| FULLSHAPE |
/// | 半角英数（`HankakuAlnum`） | A | 開 | ALPHANUMERIC |
/// | 英数（`Alphanumeric`。IME オフ相当） | A | 閉 | ALPHANUMERIC |
///
/// `Alphanumeric` と `HankakuAlnum` は表示が同じで、コアでは同じ動作（素通し）。OS 側で区別するために
/// 前者は閉、後者は開で表す（MS-IME の半角英数は「開・英数」）。
pub const MODE_TABLE: [ModeRow; 6] = [
    ModeRow {
        mode: InputMode::Hiragana,
        indicator: "あ",
        name: "ひらがな",
        open: true,
        bits: CONVERSION_NATIVE | CONVERSION_FULLSHAPE,
    },
    ModeRow {
        mode: InputMode::Katakana,
        indicator: "カ",
        name: "全角カタカナ",
        open: true,
        bits: CONVERSION_NATIVE | CONVERSION_KATAKANA | CONVERSION_FULLSHAPE,
    },
    ModeRow {
        mode: InputMode::HankakuKatakana,
        indicator: "ｶ",
        name: "半角カタカナ",
        open: true,
        bits: CONVERSION_NATIVE | CONVERSION_KATAKANA,
    },
    ModeRow {
        mode: InputMode::ZenkakuAlnum,
        indicator: "Ａ",
        name: "全角英数",
        open: true,
        bits: CONVERSION_ALPHANUMERIC | CONVERSION_FULLSHAPE,
    },
    ModeRow {
        mode: InputMode::HankakuAlnum,
        indicator: "A",
        name: "半角英数",
        open: true,
        bits: CONVERSION_ALPHANUMERIC,
    },
    ModeRow {
        mode: InputMode::Alphanumeric,
        indicator: "A",
        name: "半角英数",
        open: false,
        bits: CONVERSION_ALPHANUMERIC,
    },
];

/// モードの対応表の行
pub fn row(mode: InputMode) -> &'static ModeRow {
    MODE_TABLE
        .iter()
        .find(|r| r.mode == mode)
        .unwrap_or(&MODE_TABLE[0])
}

/// モード → コンパートメントの値（[`MODE_TABLE`]。ローマ字入力のビットは常に足す）
pub fn compartment_values(mode: InputMode) -> CompartmentValues {
    let r = row(mode);
    CompartmentValues {
        open: r.open,
        conversion: r.bits | CONVERSION_ROMAN,
    }
}

/// コンパートメントの値 → モード。閉じていれば英数。開いていて日本語入力（`NATIVE` または `KATAKANA`）なら
/// カタカナ（全角なら全角カタカナ、半角なら半角カタカナ）かひらがな。日本語入力でなければ全角英数（全角）か
/// 半角英数。対応表にない組み合わせは最も近いモード（半角のひらがな = ひらがな、`KATAKANA` だけ = カタカナ扱い）
pub fn mode_from_compartments(open: bool, conversion: u32) -> InputMode {
    if !open {
        return InputMode::Alphanumeric;
    }
    let full = conversion & CONVERSION_FULLSHAPE != 0;
    let katakana = conversion & CONVERSION_KATAKANA != 0;
    let native = conversion & CONVERSION_NATIVE != 0;
    match (native || katakana, katakana, full) {
        (true, true, true) => InputMode::Katakana,
        (true, true, false) => InputMode::HankakuKatakana,
        (true, false, _) => InputMode::Hiragana,
        (false, _, true) => InputMode::ZenkakuAlnum,
        (false, _, false) => InputMode::HankakuAlnum,
    }
}

/// モードが変わった原因（`tsf.log` の `[mode] … (source=…)`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeSource {
    /// キー操作（コアの応答の `mode`。無変換・カタカナ・F6〜F10 など）
    Key,
    /// 言語バーのメニュー・ボタン
    Menu,
    /// OS のコンパートメントの変更（アプリ・言語バー以外のモード切替 UI・他の IME との往復）
    Compartment,
}

/// モードのログ用の名前（入力内容は含まない）
pub fn log_name(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Hiragana => "hiragana",
        InputMode::Katakana => "katakana",
        InputMode::HankakuKatakana => "hankaku_katakana",
        InputMode::ZenkakuAlnum => "zenkaku_alnum",
        InputMode::HankakuAlnum => "hankaku_alnum",
        InputMode::Alphanumeric => "alphanumeric",
    }
}

/// `[mode]` の 1 行（`<from> -> <to> (source=key|menu|compartment)`。`from` が未確定なら `none`）
pub fn describe_change(from: Option<InputMode>, to: InputMode, source: ModeSource) -> String {
    let source = match source {
        ModeSource::Key => "key",
        ModeSource::Menu => "menu",
        ModeSource::Compartment => "compartment",
    };
    format!(
        "{} -> {} (source={source})",
        from.map_or("none", log_name),
        log_name(to)
    )
}

/// OS とコアが合意している現在のモード
#[derive(Debug, Clone, Copy, Default)]
pub struct ModeSync {
    known: Option<InputMode>,
}

impl ModeSync {
    /// 未確定の状態で作る（最初の同期は必ず通る）
    pub fn new() -> Self {
        Self::default()
    }

    /// 合意しているモード
    pub fn known(&self) -> Option<InputMode> {
        self.known
    }

    /// コアでモードが変わった（応答の `mode`）。OS へ書くべき値があれば返す
    pub fn core_changed(&mut self, mode: InputMode) -> Option<CompartmentValues> {
        if self.known == Some(mode) {
            return None;
        }
        self.known = Some(mode);
        Some(compartment_values(mode))
    }

    /// OS のコンパートメントが変わった（または activate 時に読んだ）。コアへ `SetMode` を送るべきなら返す
    pub fn os_changed(&mut self, open: bool, conversion: u32) -> Option<InputMode> {
        let mode = mode_from_compartments(open, conversion);
        if self.known == Some(mode) {
            return None;
        }
        self.known = Some(mode);
        Some(mode)
    }

    /// エンジンの起動・再起動後に呼ぶ。新しいエンジンは既定のモード（ひらがな）で始まるため、
    /// 合意を既定に戻し、OS 側の値を再度コアへ送れるようにする
    pub fn engine_restarted(&mut self) {
        self.known = Some(InputMode::Hiragana);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [InputMode; 6] = [
        InputMode::Hiragana,
        InputMode::Katakana,
        InputMode::HankakuKatakana,
        InputMode::ZenkakuAlnum,
        InputMode::HankakuAlnum,
        InputMode::Alphanumeric,
    ];

    #[test]
    fn values_round_trip_for_every_mode() {
        for mode in ALL {
            let v = compartment_values(mode);
            assert_eq!(
                mode_from_compartments(v.open, v.conversion),
                mode,
                "{mode:?}"
            );
        }
        // 表は全モードを 1 回ずつ持つ
        for mode in ALL {
            assert_eq!(MODE_TABLE.iter().filter(|r| r.mode == mode).count(), 1);
        }
    }

    #[test]
    fn table_bits_match_the_spec() {
        let bits = |m| compartment_values(m).conversion & !CONVERSION_ROMAN;
        assert_eq!(bits(InputMode::Hiragana), 0x1 | 0x8);
        assert_eq!(bits(InputMode::Katakana), 0x1 | 0x2 | 0x8);
        assert_eq!(bits(InputMode::HankakuKatakana), 0x1 | 0x2);
        assert_eq!(bits(InputMode::ZenkakuAlnum), 0x8);
        assert_eq!(bits(InputMode::HankakuAlnum), 0);
        assert_eq!(bits(InputMode::Alphanumeric), 0);
        assert!(!compartment_values(InputMode::Alphanumeric).open);
        for mode in ALL.iter().filter(|m| **m != InputMode::Alphanumeric) {
            assert!(compartment_values(*mode).open, "{mode:?}");
        }
    }

    #[test]
    fn every_bit_combination_maps_to_a_mode() {
        // NATIVE / KATAKANA / FULLSHAPE / ROMAN の全 16 通り × 開閉。逆写像は常に決まり、
        // 開いている日本語入力なら必ずかな系、そうでなければ英数系
        for open in [false, true] {
            for conv in 0..16u32 {
                let mode = mode_from_compartments(open, conv);
                let japanese = conv & (CONVERSION_NATIVE | CONVERSION_KATAKANA) != 0;
                assert_eq!(
                    mode.is_kana(),
                    open && japanese,
                    "open={open} conv={conv:#x}"
                );
                // 書き戻すと同じモードに戻る（往復で揺れない）
                let v = compartment_values(mode);
                assert_eq!(mode_from_compartments(v.open, v.conversion), mode);
            }
        }
    }

    #[test]
    fn nearest_mode_for_odd_combinations() {
        // 半角のひらがな（NATIVE のみ）はひらがな
        assert_eq!(
            mode_from_compartments(true, CONVERSION_NATIVE | CONVERSION_ROMAN),
            InputMode::Hiragana
        );
        // KATAKANA だけ・全角なし NATIVE|KATAKANA は半角カタカナ、全角あり KATAKANA は全角カタカナ
        assert_eq!(
            mode_from_compartments(true, CONVERSION_KATAKANA),
            InputMode::HankakuKatakana
        );
        assert_eq!(
            mode_from_compartments(true, CONVERSION_KATAKANA | CONVERSION_FULLSHAPE),
            InputMode::Katakana
        );
        // 閉じていれば、他のビットが何でも英数
        assert_eq!(
            mode_from_compartments(false, CONVERSION_NATIVE | CONVERSION_KATAKANA),
            InputMode::Alphanumeric
        );
        // 開いて NATIVE 無しは英数系（全角ビットで分かれる）
        assert_eq!(
            mode_from_compartments(true, CONVERSION_ROMAN),
            InputMode::HankakuAlnum
        );
        assert_eq!(
            mode_from_compartments(true, CONVERSION_FULLSHAPE | CONVERSION_ROMAN),
            InputMode::ZenkakuAlnum
        );
    }

    #[test]
    fn change_log_line_has_no_input_text() {
        assert_eq!(
            describe_change(
                Some(InputMode::Hiragana),
                InputMode::HankakuKatakana,
                ModeSource::Key
            ),
            "hiragana -> hankaku_katakana (source=key)"
        );
        assert_eq!(
            describe_change(None, InputMode::ZenkakuAlnum, ModeSource::Compartment),
            "none -> zenkaku_alnum (source=compartment)"
        );
        assert_eq!(
            describe_change(
                Some(InputMode::Katakana),
                InputMode::Alphanumeric,
                ModeSource::Menu
            ),
            "katakana -> alphanumeric (source=menu)"
        );
    }

    #[test]
    fn indicator_and_name_come_from_the_table() {
        let got: Vec<_> = ALL
            .iter()
            .map(|m| (row(*m).indicator, row(*m).name))
            .collect();
        assert_eq!(
            got,
            [
                ("あ", "ひらがな"),
                ("カ", "全角カタカナ"),
                ("ｶ", "半角カタカナ"),
                ("Ａ", "全角英数"),
                ("A", "半角英数"),
                ("A", "半角英数"),
            ]
        );
    }

    #[test]
    fn same_mode_is_not_sent_either_way() {
        let mut s = ModeSync::new();
        // コアが英数になった → OS へ書く
        assert!(s.core_changed(InputMode::Alphanumeric).is_some());
        // その書き込みの通知が戻ってきてもコアへ送らない（往復しない）
        let v = compartment_values(InputMode::Alphanumeric);
        assert_eq!(s.os_changed(v.open, v.conversion), None);
        // コアの応答が同じモードを重ねても書かない
        assert_eq!(s.core_changed(InputMode::Alphanumeric), None);
        // OS 側で日本語入力に切り替えた → コアへ送る
        let h = compartment_values(InputMode::Hiragana);
        assert_eq!(
            s.os_changed(h.open, h.conversion),
            Some(InputMode::Hiragana)
        );
        assert_eq!(s.known(), Some(InputMode::Hiragana));
    }

    #[test]
    fn extended_modes_do_not_echo_and_switch_between_each_other() {
        let mut s = ModeSync::new();
        for mode in ALL {
            assert!(s.core_changed(mode).is_some() || s.known() == Some(mode));
            let v = compartment_values(mode);
            // 自分で書いた値の通知は送り返さない
            assert_eq!(s.os_changed(v.open, v.conversion), None, "{mode:?}");
        }
        // OS（言語バーなど）が全角カタカナへ変えたらコアへ送る
        let v = compartment_values(InputMode::Katakana);
        assert_eq!(
            s.os_changed(v.open, v.conversion),
            Some(InputMode::Katakana)
        );
    }

    #[test]
    fn first_sync_always_passes() {
        let mut s = ModeSync::new();
        assert_eq!(s.os_changed(true, 0x19), Some(InputMode::Hiragana));
        let mut s = ModeSync::new();
        assert!(s.core_changed(InputMode::Hiragana).is_some());
    }

    #[test]
    fn restart_resets_agreement_to_default() {
        let mut s = ModeSync::new();
        s.core_changed(InputMode::Alphanumeric);
        s.engine_restarted();
        // 新エンジンはひらがな。OS が英数のままなら SetMode を送って揃える
        assert_eq!(
            s.os_changed(false, CONVERSION_ROMAN),
            Some(InputMode::Alphanumeric)
        );
    }
}
