import XCTest

/// 共通シナリオ（tap / flick / expect / screenshot ほか）を iOS のソフトキーボード上で再生する。
///
/// - キー: キーボード拡張が付ける accessibilityIdentifier `key.<表示ラベル>` で探す
/// - 候補: `candidate.<index>`（ラベルが候補の文字列）。ツールバー: `toolbar.<名前>`
/// - テキスト欄: ホストアプリの `e2e.input`（起動引数 `-nagi-e2e` で入力テスト画面を開く）
/// - iOS の XCUITest では未確定部分を分離して取れないので、`expect` は `text`（と `candidates_contain`）だけを比較する
final class E2ERunner {
    struct ScenarioResult {
        let suite: String
        let name: String
        let ok: Bool
        let message: String
    }

    static let inputIdentifier = "e2e.input"
    static let defaultTimeoutMs = 2000

    private let app = XCUIApplication()
    private let outputDir: URL
    private let attach: (XCTAttachment) -> Void
    private(set) var results: [ScenarioResult] = []
    private var currentSuite = ""
    private var currentScenario = ""

    init(outputDir: URL, attach: @escaping (XCTAttachment) -> Void) {
        self.outputDir = outputDir
        self.attach = attach
        try? FileManager.default.createDirectory(
            at: outputDir.appendingPathComponent("screenshots"), withIntermediateDirectories: true)
    }

    // MARK: シナリオの実行

    func run(_ suite: E2ESuite) {
        for scenario in suite.scenarios {
            currentSuite = suite.suite
            currentScenario = scenario.name
            var failure: String?
            // 入力系の操作ごとに読んだテキスト欄の表示（失敗メッセージに添える経過）
            var history: [String] = []
            do {
                try launchFresh()
                for (index, step) in scenario.steps.enumerated() {
                    do {
                        try perform(step)
                        if step.tap != nil || step.flick != nil || step.key != nil {
                            let line = "step\(index + 1) \(step.summary) -> \"\(currentText())\" cand=\(currentCandidates().count)"
                            history.append(line)
                            print("[trace] \(suite.suite)/\(scenario.name) \(line)")
                        }
                    } catch {
                        throw E2EError.step("step \(index + 1) (\(step.summary)): \(error)")
                    }
                }
            } catch {
                var message = "\(error)"
                if !history.isEmpty { message += " | 経過: " + history.joined(separator: "; ") }
                // FAIL 行を grep で取れるよう 1 行にする
                // ホストのフィクスチャ配置の結果（辞書が拡張に届かない問題の診断用）
                message += " | fixture: " + fixtureStatus()
                failure = message.replacingOccurrences(of: "\n", with: " ")
            }
            if let failure = failure {
                saveScreenshot(name: "FAIL_\(suite.suite)_\(scenario.name)")
                print("FAIL \(suite.suite)/\(scenario.name): \(failure)")
                XCTFail("\(suite.suite)/\(scenario.name): \(failure)")
            } else {
                print("PASS \(suite.suite)/\(scenario.name)")
            }
            results.append(ScenarioResult(suite: suite.suite, name: scenario.name, ok: failure == nil, message: failure ?? ""))
        }
    }

    /// ホストの入力テスト画面に出ているフィクスチャ配置の結果（`e2e.fixture-status`）。読めなければその旨
    private func fixtureStatus() -> String {
        let text = app.staticTexts["e2e.fixture-status"]
        guard text.exists else { return "(e2e.fixture-status なし)" }
        return text.label
    }

    /// 準備ができなかったとき（キーボードが出せないなど）、スイート全体の失敗として記録する
    func recordSetupFailure(_ message: String) {
        print("FAIL setup: \(message)")
        saveScreenshot(name: "FAIL_setup")
        results.append(ScenarioResult(suite: "(setup)", name: "setup", ok: false, message: message))
        XCTFail(message)
    }

    /// アプリを起動し直し、入力欄を空にして Nagi（ひらがな）を出す。シナリオごとに状態を持ち越さない
    func launchFresh() throws {
        app.terminate()
        app.launchArguments = ["-nagi-e2e"]
        app.launch()
        let input = app.textFields[Self.inputIdentifier]
        if let problem = E2EKeyboardSetup.showNagi(in: app, input: input) {
            // 1 回だけ、アプリを起動し直してやり直す（成功するシナリオの実行時間は増えない）
            print("[kbd] retry with relaunch: \(problem)")
            app.terminate()
            app.launch()
            if let second = E2EKeyboardSetup.showNagi(in: app, input: app.textFields[Self.inputIdentifier]) {
                throw E2EError.setup("Nagi キーボードを表示できません（再起動後も失敗）: \(second)")
            }
        }
        if !currentText().isEmpty {
            // 起動し直しているので通常は空。念のためホストの「クリア」を押す
            let clear = E2EKeyboardSetup.element(app, identifier: "e2e.clear")
            if clear.exists { clear.tap() }
        }
    }

    /// 準備段階の確認用: 起動して Nagi が出せるか
    func canShowNagi() -> Bool {
        (try? launchFresh()) != nil
    }

    // MARK: ステップ

    private func perform(_ step: E2EStep) throws {
        if let name = step.key {
            try pressSpecialKey(name)
        } else if let names = step.tap {
            for name in names { try keyElement(name).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap() }
        } else if let flick = step.flick {
            try performFlick(flick)
        } else if let expect = step.expect {
            try check(expect)
        } else if let name = step.screenshot {
            saveScreenshot(name: "\(currentSuite)_\(currentScenario)_\(name)")
        } else if step.type != nil {
            throw E2EError.step("`type` は iOS では使えません（tap / flick を使う）")
        } else {
            throw E2EError.step("未対応のステップです")
        }
    }

    /// `key` ステップの名前 → ソフトキーの識別子の名前（12 キーの既定レイアウト）
    private static let specialKeys: [String: String] = [
        "Space": "空白", "Return": "改行", "Backspace": "⌫", "Left": "←", "Right": "→",
    ]

    private func pressSpecialKey(_ name: String) throws {
        guard let label = Self.specialKeys[name] else {
            throw E2EError.step("iOS では `key` \(name) に対応するソフトキーがありません（対応: \(Self.specialKeys.keys.sorted())）")
        }
        try keyElement(label).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
    }

    private func keyElement(_ name: String) throws -> XCUIElement {
        let element = E2EKeyboardSetup.element(app, identifier: "key.\(name)")
        if element.waitForExistence(timeout: 3) { return element }
        // 識別子で見つからないときはラベル（表示中の文字）でも探す（例: 変換中の「空白」キーは「変換」と表示される）
        let byLabel = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH 'key.' AND label == %@", name)).firstMatch
        if byLabel.exists { return byLabel }
        throw E2EError.step("キー \"\(name)\" が見つかりません（key.\(name)）")
    }

    private func performFlick(_ flick: E2EStep.Flick) throws {
        let element = try keyElement(flick.key)
        // フリックの閾値（既定 14pt）を超え、取り消し（キーの大きさの 2 倍）には届かない距離
        let distance = max(30, min(element.frame.width, element.frame.height) * 0.9)
        let vector: CGVector
        switch flick.dir {
        case "up": vector = CGVector(dx: 0, dy: -distance)
        case "down": vector = CGVector(dx: 0, dy: distance)
        case "left": vector = CGVector(dx: -distance, dy: 0)
        case "right": vector = CGVector(dx: distance, dy: 0)
        default: throw E2EError.step("flick の dir が不正です: \(flick.dir)")
        }
        let start = element.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        start.press(forDuration: 0.1, thenDragTo: start.withOffset(vector))
    }

    // MARK: 検証

    private func check(_ expect: E2EStep.Expect) throws {
        if expect.marked != nil || expect.committed != nil {
            throw E2EError.step("iOS では `marked` / `committed` を取れません。`text` を使ってください")
        }
        let timeout = Double(expect.timeout_ms ?? Self.defaultTimeoutMs) / 1000
        var lastProblem = ""
        let ok = E2EKeyboardSetup.wait(timeout: timeout) {
            lastProblem = self.mismatch(expect)
            return lastProblem.isEmpty
        }
        if !ok { throw E2EError.step(lastProblem) }
    }

    /// 期待と食い違う内容（一致していれば空文字）
    private func mismatch(_ expect: E2EStep.Expect) -> String {
        if let text = expect.text {
            let actual = currentText()
            if actual != text { return "text: 期待 \"\(text)\" / 実測 \"\(actual)\"" }
        }
        if let wanted = expect.candidates_contain {
            let candidates = currentCandidates()
            let missing = wanted.filter { want in !candidates.contains { $0.contains(want) } }
            if !missing.isEmpty { return "candidates_contain: 不足 \(missing) / 実測 \(candidates)" }
        }
        return ""
    }

    /// テキスト欄の表示全体。空のときの値はプレースホルダなので空文字に直す
    private func currentText() -> String {
        let field = app.textFields[Self.inputIdentifier]
        guard field.exists else { return "" }
        let value = (field.value as? String) ?? ""
        if value == field.placeholderValue { return "" }
        return value
    }

    /// 候補バーに見えている候補のラベル（`candidate.<index>`。index 順。展開ボタンは除く）
    private func currentCandidates() -> [String] {
        let predicate = NSPredicate(format: "identifier BEGINSWITH 'candidate.' AND identifier != 'candidate.expand'")
        let elements = app.descendants(matching: .any).matching(predicate).allElementsBoundByIndex
        return elements
            .map { (Int($0.identifier.dropFirst("candidate.".count)) ?? Int.max, $0.label) }
            .sorted { $0.0 < $1.0 }
            .map { $0.1 }
    }

    // MARK: 成果物

    /// スクリーンショットを XCTAttachment（keepAlways）と `screenshots/<name>.png` に残す
    func saveScreenshot(name: String) {
        let screenshot = XCUIScreen.main.screenshot()
        let attachment = XCTAttachment(screenshot: screenshot)
        attachment.name = name
        attachment.lifetime = .keepAlways
        attach(attachment)
        let safe = name.replacingOccurrences(of: "/", with: "_")
        try? screenshot.pngRepresentation.write(
            to: outputDir.appendingPathComponent("screenshots").appendingPathComponent("\(safe).png"))
    }

    /// `results.json`（`[{ suite, name, ok, message }]`）を書き、XCTAttachment にも付ける
    func writeResults() {
        let array: [[String: Any]] = results.map {
            ["suite": $0.suite, "name": $0.name, "ok": $0.ok, "message": $0.message]
        }
        guard let data = try? JSONSerialization.data(
            withJSONObject: array, options: [.prettyPrinted, .sortedKeys]) else { return }
        try? data.write(to: outputDir.appendingPathComponent("results.json"))
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = "results.json"
        attachment.lifetime = .keepAlways
        attach(attachment)
    }
}
