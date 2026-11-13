import XCTest

/// tests/e2e/scenarios の iOS 向けシナリオを、シミュレータ上の Nagi キーボードで再生する。
/// 結果は XCTest の失敗に加えて、`results.json` と `screenshots/` を出力する
/// （出力先は環境変数 NAGI_E2E_OUT。xcodebuild に `TEST_RUNNER_NAGI_E2E_OUT=<dir>` として渡す。未指定なら一時ディレクトリ）。
/// 同じ内容を XCTAttachment（keepAlways）にも付けるので、出力先が使えなくても結果バンドルから取り出せる
final class NagiE2ETests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = true
    }

    func testIOSScenarios() throws {
        let runner = E2ERunner(outputDir: Self.outputDirectory()) { [weak self] attachment in
            guard let self = self else { return }
            self.add(attachment)
        }
        defer { runner.writeResults() }

        var suites: [E2ESuite]
        do {
            suites = try E2EScenarioLoader.load(bundle: Bundle(for: Self.self), platform: "ios")
        } catch {
            runner.recordSetupFailure("\(error)")
            return
        }
        if suites.isEmpty {
            runner.recordSetupFailure("platforms に ios を含むシナリオがありません")
            return
        }
        // 環境変数 NAGI_E2E_SUITE（カンマ区切りのスイート名）があれば、そのスイートだけを実行する
        let filter = (ProcessInfo.processInfo.environment["NAGI_E2E_SUITE"] ?? "")
            .split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        if !filter.isEmpty {
            suites = suites.filter { filter.contains($0.suite) }
            if suites.isEmpty {
                runner.recordSetupFailure("NAGI_E2E_SUITE=\(filter) に一致するスイートがありません")
                return
            }
        }

        // キーボードを出せるか。出せなければ設定アプリで追加してもう一度試す
        if !runner.canShowNagi() {
            E2EKeyboardSetup.addKeyboardViaSettings()
            if !runner.canShowNagi() {
                runner.recordSetupFailure("Nagi キーボードを有効化・表示できませんでした")
                return
            }
        }

        for suite in suites { runner.run(suite) }
    }

    private static func outputDirectory() -> URL {
        if let path = ProcessInfo.processInfo.environment["NAGI_E2E_OUT"], !path.isEmpty {
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        return URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true).appendingPathComponent("nagi-e2e")
    }
}
