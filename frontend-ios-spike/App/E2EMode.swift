import Foundation

/// UI テスト（XCUITest。`UITests/`）用の起動モード。起動引数 `-nagi-e2e` があるときだけ有効で、
/// 引数が無い通常の起動（TestFlight / App Store を含む）の挙動は変わらない。
/// 有効なときは、初回案内を出さずに「入力テスト」タブを最初に開く（入力欄は `KeyboardTestView` の `e2e.input`）
enum NagiE2EMode {
    static let launchArgument = "-nagi-e2e"

    static var isEnabled: Bool {
        ProcessInfo.processInfo.arguments.contains(launchArgument)
    }
}
