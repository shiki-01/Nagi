import XCTest

/// シミュレータで Nagi キーボードを使える状態にする。
///
/// 1. 理想は CI が事前にシミュレータの GlobalPreferences（AppleKeyboards）へ拡張を足しておくこと
///    （.github/workflows/ios-uitest.yml）。この場合、ここでは地球儀キーで切り替えるだけ
/// 2. 足されていなければ、設定アプリの UI（一般 > キーボード > キーボード > 新しいキーボードを追加）で追加する
///
/// 設定アプリの文言は、英語と日本語のシミュレータのどちらでも動くよう両方を候補にする
enum E2EKeyboardSetup {
    /// Nagi のキーが出ているときの識別子の接頭辞（`NagiKeyboardView`）
    static let keyPrefix = "key."
    static let keyboardDisplayName = "Nagi"

    // MARK: 要素の検索

    static func element(_ app: XCUIApplication, identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    static func nagiKeysVisible(_ app: XCUIApplication) -> Bool {
        let predicate = NSPredicate(format: "identifier BEGINSWITH %@", keyPrefix)
        return app.descendants(matching: .any).matching(predicate).firstMatch.exists
    }

    /// システムのキーボードの地球儀キー（次のキーボード）。ラベルは OS の言語による
    static func systemGlobe(_ app: XCUIApplication) -> XCUIElement {
        let predicate = NSPredicate(
            format: "label CONTAINS[c] 'next keyboard' OR label CONTAINS %@ OR identifier == 'Next keyboard'", "次のキーボード")
        return app.keyboards.buttons.matching(predicate).firstMatch
    }

    // MARK: キーボードを出して Nagi に切り替える

    /// 入力欄をタップしてキーボードを出し、Nagi が出るまで地球儀キーで切り替える。
    /// Nagi が見つからなければ false（キーボードが追加されていない可能性）
    static func showNagi(in app: XCUIApplication, input: XCUIElement) -> Bool {
        if !input.waitForExistence(timeout: 10) { return false }
        input.tap()
        dismissKeyboardTutorial(app)
        if wait(timeout: 5, { nagiKeysVisible(app) }) { return true }

        // 地球儀で順に切り替える（キーボードが 2〜3 個なら数回で一巡する）
        for _ in 0..<6 {
            let globe = systemGlobe(app)
            guard globe.waitForExistence(timeout: 3) else { break }
            globe.tap()
            dismissKeyboardTutorial(app)
            if wait(timeout: 3, { nagiKeysVisible(app) }) { return true }
        }
        return false
    }

    /// 初めてシステムのキーボードを出したときのガイド（"Continue" など）を閉じる
    static func dismissKeyboardTutorial(_ app: XCUIApplication) {
        for title in ["Continue", "続ける"] {
            let button = app.buttons[title]
            if button.exists && button.isHittable { button.tap() }
        }
    }

    // MARK: 設定アプリでの追加（フォールバック）

    /// 設定アプリで Nagi を追加する。成功したかは分からないので、呼び出し側が `showNagi` で確かめる
    static func addKeyboardViaSettings() {
        let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
        settings.terminate()
        settings.launch()

        // 一般 > キーボード > キーボード > 新しいキーボードを追加
        guard tapRow(settings, ["General", "一般"]),
              tapRow(settings, ["Keyboard", "キーボード"]),
              tapRow(settings, ["Keyboards", "キーボード"])
        else { return }
        guard tapRow(settings, ["Add New Keyboard…", "Add New Keyboard...", "新しいキーボードを追加…", "新しいキーボードを追加..."])
        else { return }
        // サードパーティ製は一覧の下の方にあるので、見つかるまでスクロールする
        _ = tapRow(settings, [keyboardDisplayName])
        settings.terminate()
    }

    /// 一覧の行（セルのラベルが `titles` のどれか）をタップする。無ければ下へスクロールして探す。
    private static func tapRow(_ app: XCUIApplication, _ titles: [String]) -> Bool {
        for _ in 0..<8 {
            for title in titles {
                let cell = app.cells.containing(.staticText, identifier: title).firstMatch
                if cell.waitForExistence(timeout: 3) {
                    cell.tap()
                    return true
                }
                let button = app.buttons[title]
                if button.exists {
                    button.tap()
                    return true
                }
            }
            app.swipeUp()
        }
        return false
    }

    // MARK: 待機

    /// 条件が真になるまで（最大 timeout 秒）待つ
    @discardableResult
    static func wait(timeout: TimeInterval, interval: TimeInterval = 0.25, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(interval))
        } while Date() < deadline
        return condition()
    }
}
