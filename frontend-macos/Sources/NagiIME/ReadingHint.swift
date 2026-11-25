import Foundation
import NagiClient

/// 変換された文節の読み（ふりがな）ヒントの純粋ロジック（表示するかどうか・文字列の組み立て・設定の読み取り）。
/// 描画は `ReadingHintPanel`、表示の出し分けは `NagiInputController` が行う
public enum ReadingHint {
    /// 設定キー（`config/default.toml` の `input.show_reading`）
    public static let settingKey = "input.show_reading"
    /// 文節どうしの区切り
    public static let separator = "｜"

    /// 表示する読みの行。かなのまま変換されなかった文節・英数字・部分確定の文節は含めず、文節順に並べる。
    /// 変換された文節が 1 つも無ければ nil（表示しない。未入力・Direct・全文節がかなのままのとき）
    public static func text(for preedit: PreeditView) -> String? {
        let readings = preedit.segments
            .filter { $0.kind == .japanese && $0.surface != $0.reading }
            .map(\.reading)
        guard !readings.isEmpty else { return nil }
        return readings.joined(separator: separator)
    }

    /// 設定スナップショットから `input.show_reading` を読む。項目が無い・値が取れなければ false（既定）
    public static func isEnabled(in snapshot: SettingsSnapshot) -> Bool {
        guard let item = snapshot.items.first(where: { $0.key == settingKey }),
            case .bool(let on)? = item.value
        else { return false }
        return on
    }
}
