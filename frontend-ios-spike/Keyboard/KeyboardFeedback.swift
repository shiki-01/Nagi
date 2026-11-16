import AudioToolbox
import UIKit

/// 画面の寸法と操作の判定に使う設定の現在値。`KeyboardModel` が focusIn のたびに `ios-settings.json` を読んで
/// 更新し、描画のたびにここを参照する（値を変えてから `@Published` を更新するので、再描画は新しい値を見る）
enum NagiKeyMetrics {
    /// メインスレッドからのみ読み書きする（Swift の並行性検査を抑制）
    nonisolated(unsafe) static var current = NagiIOSSettings()

    /// キー 1 行（高さ比 1）の高さ
    static var keyHeight: Double { 46 * current.heightScale }
    static var keySpacing: Double { current.keySpacingPt }

    /// キーボード全体の高さ。候補バー 44 + 余白 12 = 56 は固定で、残りをキーの高さの倍率で伸縮する
    /// （倍率 1.0 で従来の 270）
    static var totalHeight: Double { 56 + 214 * current.heightScale }
}

/// 互換モード（システムサウンド）の触覚・クリック音の再生。
/// `AudioServicesPlaySystemSound` をメインスレッドで呼ぶと、速く打ったときに鳴り終わるまで次の入力が遅れたので、
/// 専用の直列キューで鳴らす。種類（触覚・音）ごとに再生中なら次の要求は捨てる（重ねない・待たせない）。
/// 再生中のフラグはキューの上だけで読み書きする
private enum CompatFeedbackPlayer {
    enum Kind {
        case haptic, sound
    }

    private static let queue = DispatchQueue(label: "com.shiki.nagi.feedback", qos: .userInteractive)
    /// キューの上だけで読み書きする（Swift の並行性検査を抑制）
    nonisolated(unsafe) private static var hapticPlaying = false
    nonisolated(unsafe) private static var soundPlaying = false

    /// `id` を鳴らす（メインスレッドから呼ぶ。すぐ戻る）。
    /// `onDropped` は再生中で捨てたときにメインスレッドで呼ぶ（ログ用。ログはメインスレッドで出す）
    static func play(_ id: SystemSoundID, kind: Kind, onDropped: (() -> Void)? = nil) {
        queue.async {
            if isPlaying(kind) {
                if let onDropped = onDropped { DispatchQueue.main.async(execute: onDropped) }
                return
            }
            setPlaying(kind, true)
            AudioServicesPlaySystemSoundWithCompletion(id) {
                // 完了ハンドラは任意のスレッドで呼ばれるので、フラグはキューへ戻してから戻す
                queue.async { setPlaying(kind, false) }
            }
        }
    }

    private static func isPlaying(_ kind: Kind) -> Bool {
        switch kind {
        case .haptic: return hapticPlaying
        case .sound: return soundPlaying
        }
    }

    private static func setPlaying(_ kind: Kind, _ value: Bool) {
        switch kind {
        case .haptic: hapticPlaying = value
        case .sound: soundPlaying = value
        }
    }
}

/// キー入力のフィードバック（触覚・クリック音）。いずれも失敗しても何も起きない。
/// 触覚はシステムサウンド（1519/1520/1521）の振動で、フルアクセスの有無に関係なく鳴る（実機確認済み。H-91）。
/// `fullAccess` は診断ログ用（`KeyboardViewController` が画面表示のたびに更新する）。
/// 診断ログ（DiagLog）に、発火のたびに状況を残す（先頭の数十回のみ。focusIn のたびに数え直す）
enum NagiKeyFeedback {
    nonisolated(unsafe) private static var logCount = 0
    private static let logLimit = 30

    /// `hasFullAccess` の現在値（診断ログ用）
    nonisolated(unsafe) static var fullAccess = false

    /// 画面の表示時（focusIn）に呼ぶ。ログの回数を数え直す
    static func prepare() {
        logCount = 0
        let settings = NagiKeyMetrics.current
        DiagLog.log("feedback prepare haptic=\(settings.haptic) sound=\(settings.soundMode) fullAccess=\(fullAccess)")
    }

    /// キーを押したときに呼ぶ（メインスレッド）
    static func keyDown() {
        let settings = NagiKeyMetrics.current
        let haptic = settings.haptic
        let shouldLog = logCount < logLimit
        if shouldLog { logCount += 1 }
        // 鳴らすのは専用のキュー。ここでは要求するだけで、再生中で捨てたかは後からメインスレッドで分かる
        if let id = HapticMapping.systemSoundID(for: haptic) {
            CompatFeedbackPlayer.play(id, kind: .haptic, onDropped: shouldLog ? {
                DiagLog.log("haptic fire kind=\(haptic) dropped=true")
            } : nil)
            if shouldLog {
                DiagLog.log("haptic fire kind=\(haptic) fullAccess=\(fullAccess) requested=true")
            }
        }
        if settings.soundEnabled {
            if shouldLog {
                DiagLog.log("click play mode=\(settings.soundMode)")
            }
            // システムのキーボードのクリック音（1104）を直接鳴らす。端末の「キーボードのクリック」設定には
            // 従わず、消音スイッチに従う。鳴らすのは専用のキュー（再生中なら捨てる）。
            // 端末の設定に従う `UIDevice.playInputClick()` は実機で鳴らなかったので使わない
            CompatFeedbackPlayer.play(1104, kind: .sound, onDropped: shouldLog ? {
                DiagLog.log("click play mode=compat dropped=true")
            } : nil)
        }
    }
}
