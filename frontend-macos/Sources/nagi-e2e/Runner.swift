#if os(macOS)
import Foundation

/// コマンドライン引数
struct Config {
    var scenarios: String?
    var app: String?
    var fixtures: String?
    var dataDir: String?
    var testHost: String?
    var out = "e2e-out"
    var filter: String?
    var skipInstall = false
    var warmupMs = 60_000

    static let usage = """
        usage: nagi-e2e --scenarios <dir|file> --app <Nagi.app> --fixtures <dir> [options]
          --scenarios <path>   シナリオ JSON（ディレクトリなら *.json すべて。platforms に macos を含むものだけ実行）
          --app <path>         cargo xtask bundle-macos で作った Nagi.app
          --fixtures <dir>     tests/e2e/fixtures（user-dir/ を一時ディレクトリへコピーして使う）
          --data-dir <dir>     NAGI_DATA_DIR にする辞書ディレクトリ（cargo xtask e2e-fixture-data の出力。未指定は空）
          --test-host <path>   NagiTestHost の実行ファイル（既定: この実行ファイルと同じディレクトリ）
          --out <dir>          results.json / screenshots/ / host.log の出力先（既定: e2e-out）
          --filter <text>      "<suite>/<name>" にこの文字列を含むシナリオだけ実行
          --skip-install       ~/Library/Input Methods への配置・登録を省略（配置済みの Nagi を使う）
          --warmup-ms <n>      初回キー入力（IME・エンジンの起動待ち）の待ち時間（既定 60000）
        """

    static func parse(_ args: [String]) -> Config? {
        var config = Config()
        var i = 0
        func value() -> String? {
            guard i + 1 < args.count else { return nil }
            i += 1
            return args[i]
        }
        while i < args.count {
            switch args[i] {
            case "--scenarios": config.scenarios = value()
            case "--app": config.app = value()
            case "--fixtures": config.fixtures = value()
            case "--data-dir": config.dataDir = value()
            case "--test-host": config.testHost = value()
            case "--out": config.out = value() ?? config.out
            case "--filter": config.filter = value()
            case "--skip-install": config.skipInstall = true
            case "--warmup-ms": config.warmupMs = Int(value() ?? "") ?? config.warmupMs
            case "-h", "--help": return nil
            default:
                FileHandle.standardError.write(Data("unknown argument: \(args[i])\n".utf8))
                return nil
            }
            i += 1
        }
        guard config.scenarios != nil, config.fixtures != nil else { return nil }
        if !config.skipInstall, config.app == nil { return nil }
        return config
    }
}

/// 終了コード: 0 全成功 / 1 シナリオ失敗 / 2 引数・入力の誤り / 3 キー送出または IME に届かない（権限・入力ソース）/ 4 インストール・起動の失敗
enum ExitCode {
    static let failed: Int32 = 1
    static let usage: Int32 = 2
    static let unreachable: Int32 = 3
    static let setup: Int32 = 4
}

final class Runner {
    private let config: Config
    private let fileManager = FileManager.default
    private let poster = KeyPoster()
    private let outDir: URL
    private let screenshotsDir: URL
    private let workDir: URL
    private var host: HostProcess?
    private var bundleID = "com.shiki.inputmethod.Nagi"
    private var results: [ScenarioResult] = []

    private var japaneseInputSourceID: String { "\(bundleID).Japanese" }

    init(config: Config) {
        self.config = config
        outDir = URL(fileURLWithPath: config.out)
        screenshotsDir = outDir.appendingPathComponent("screenshots")
        workDir = fileManager.temporaryDirectory.appendingPathComponent("nagi-e2e-\(UUID().uuidString)")
    }

    // MARK: - 全体

    func run() -> Int32 {
        do {
            try fileManager.createDirectory(at: screenshotsDir, withIntermediateDirectories: true)
        } catch {
            return fail(ExitCode.usage, "出力先を作れません: \(error)")
        }

        let suites: [Suite]
        do {
            suites = try loadSuites()
        } catch let error as StepFailure {
            return fail(ExitCode.usage, error.message)
        } catch {
            return fail(ExitCode.usage, "シナリオを読めません: \(error)")
        }
        let planned = suites.flatMap { suite in
            suite.scenarios.map { (suite.suite, $0) }
        }.filter { config.filter == nil || "\($0.0)/\($0.1.name)".contains(config.filter!) }
        guard !planned.isEmpty else { return fail(ExitCode.usage, "実行するシナリオがありません") }

        print("権限: \(Permissions.summary())")
        if let code = prepareEnvironment() {
            teardown()
            writeResults()
            return code
        }
        defer {
            teardown()
            writeResults()
        }

        if let code = warmUp() { return code }

        for (suiteName, scenario) in planned {
            let result = runScenario(suite: suiteName, scenario: scenario)
            results.append(result)
            print("\(result.ok ? "PASS" : "FAIL") \(suiteName)/\(scenario.name)" + (result.ok ? "" : ": \(result.message)"))
        }

        let failed = results.filter { !$0.ok }.count
        print("--- \(results.count - failed) passed, \(failed) failed (\(results.count) total)")
        return failed == 0 ? 0 : ExitCode.failed
    }

    private func fail(_ code: Int32, _ message: String) -> Int32 {
        FileHandle.standardError.write(Data("error: \(message)\n".utf8))
        return code
    }

    // MARK: - シナリオの読み込み

    private func loadSuites() throws -> [Suite] {
        let path = config.scenarios!
        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: path, isDirectory: &isDirectory) else {
            throw StepFailure(message: "シナリオが見つかりません: \(path)")
        }
        var files: [URL]
        if isDirectory.boolValue {
            files = try fileManager.contentsOfDirectory(
                at: URL(fileURLWithPath: path), includingPropertiesForKeys: nil
            ).filter { $0.pathExtension == "json" }
            files.sort { $0.lastPathComponent < $1.lastPathComponent }
        } else {
            files = [URL(fileURLWithPath: path)]
        }
        var suites: [Suite] = []
        for file in files {
            do {
                let suite = try JSONDecoder().decode(Suite.self, from: Data(contentsOf: file))
                if suite.runs(on: "macos") { suites.append(suite) }
            } catch {
                throw StepFailure(message: "シナリオ \(file.lastPathComponent) を読めません: \(error)")
            }
        }
        return suites
    }

    // MARK: - 環境の準備

    /// 成功なら nil、失敗なら終了コード
    private func prepareEnvironment() -> Int32? {
        // (a) fixtures/user-dir を一時ディレクトリへコピー（エンジンの設定ディレクトリ）
        let userDirSource = URL(fileURLWithPath: config.fixtures!).appendingPathComponent("user-dir")
        let userDir = workDir.appendingPathComponent("user-dir")
        // 語彙なしの最小辞書（ユーザー辞書の重ね合わせに辞書本体が要る）。未指定なら空（辞書なし）
        let dataDir = config.dataDir.map { URL(fileURLWithPath: $0) } ?? workDir.appendingPathComponent("data")
        do {
            try fileManager.createDirectory(at: workDir, withIntermediateDirectories: true)
            guard fileManager.fileExists(atPath: userDirSource.path) else {
                return fail(ExitCode.usage, "fixtures/user-dir が見つかりません: \(userDirSource.path)")
            }
            try fileManager.copyItem(at: userDirSource, to: userDir)
            try fileManager.createDirectory(at: dataDir, withIntermediateDirectories: true)
        } catch {
            return fail(ExitCode.setup, "一時ディレクトリの準備に失敗しました: \(error)")
        }

        // (b) Nagi.app の配置・登録
        if !config.skipInstall, let code = installApp() { return code }

        // IME プロセスは launchd 配下で起動されるため、環境変数は launchctl setenv で渡す。
        // 実行中の IME は古い環境のままなので、設定後に必ず落として再起動させる
        runProcess("/usr/bin/pkill", ["-x", executableName()])
        for (key, value) in [
            ("NAGI_CONFIG_DIR", userDir.path), ("NAGI_USER_DIR", userDir.path),
            ("NAGI_DATA_DIR", dataDir.path),
        ] {
            let result = runProcess("/bin/launchctl", ["setenv", key, value])
            if result.status != 0 {
                return fail(ExitCode.setup, "launchctl setenv \(key) に失敗しました: \(result.output)")
            }
        }

        // 入力ソースの選択（有効化直後は select が -50 になることがあるのでリトライ + 診断 + defaults フォールバック）
        if let code = selectInputSource() { return code }

        // (c) NagiTestHost の起動と前面化
        let hostURL: URL
        if let path = config.testHost {
            hostURL = URL(fileURLWithPath: path)
        } else {
            let own = Bundle.main.executableURL ?? URL(fileURLWithPath: CommandLine.arguments[0])
            hostURL = own.deletingLastPathComponent().appendingPathComponent("NagiTestHost")
        }
        guard fileManager.isExecutableFile(atPath: hostURL.path) else {
            return fail(ExitCode.setup, "NagiTestHost が見つかりません: \(hostURL.path)（swift build -c release --product NagiTestHost）")
        }
        let host = HostProcess(
            executable: hostURL, stateURL: workDir.appendingPathComponent("state.json"),
            logURL: outDir.appendingPathComponent("host.log"))
        self.host = host
        do {
            try host.launch()
        } catch {
            return fail(ExitCode.setup, "NagiTestHost を起動できません: \(error)")
        }
        let active = pumpUntil(15) { host.readState()?.active == true }
        if !active {
            print("警告: NagiTestHost が前面のキーウィンドウになっていません（GUI セッションにログインしていますか）")
            screenshot(named: "host-not-active")
        }
        return nil
    }

    /// 成功なら nil、失敗なら終了コード
    private func selectInputSource() -> Int32? {
        let id = japaneseInputSourceID
        if InputSources.currentID() == id { return nil }
        let report = InputSources.enableAndSelect(id: id)
        print("入力ソース \(id): found=\(report.found) enable=\(report.enableStatus) select=\(report.selectStatus)")
        print("入力ソースの状態（選択前）:")
        InputSources.dump(matching: "nagi").forEach { print($0) }
        if InputSources.selectWithRetry(id: id, timeout: 30, log: { print($0) }) { return nil }

        print("TISSelectInputSource で選べませんでした。defaults (com.apple.HIToolbox) によるフォールバックを試します")
        if InputSources.selectViaDefaults(bundleID: bundleID, modeID: id, log: { print($0) }) { return nil }

        print("入力ソースの状態（失敗後）:")
        InputSources.dump(matching: "nagi").forEach { print($0) }
        screenshot(named: "select-input-source-failed")
        return fail(
            ExitCode.unreachable,
            "入力ソース \(id) を選択できません（現在: \(InputSources.currentID() ?? "nil")）。"
                + "GUI セッションに紐づかない環境、またはシステム設定での手動追加が必要な環境の可能性があります")
    }

    private func executableName() -> String {
        guard let app = config.app else { return "NagiIMEApp" }
        let plist = NSDictionary(contentsOfFile: "\(app)/Contents/Info.plist")
        return (plist?["CFBundleExecutable"] as? String) ?? "NagiIMEApp"
    }

    private func installApp() -> Int32? {
        let app = URL(fileURLWithPath: config.app!)
        guard fileManager.fileExists(atPath: app.appendingPathComponent("Contents/Info.plist").path) else {
            return fail(ExitCode.setup, "Nagi.app が見つかりません: \(app.path)")
        }
        let plist = NSDictionary(contentsOf: app.appendingPathComponent("Contents/Info.plist"))
        if let id = plist?["CFBundleIdentifier"] as? String { bundleID = id }
        let executable = executableName()

        let inputMethods = fileManager.homeDirectoryForCurrentUser.appendingPathComponent("Library/Input Methods")
        let destination = inputMethods.appendingPathComponent(app.lastPathComponent)
        runProcess("/usr/bin/pkill", ["-x", executable])
        do {
            try fileManager.createDirectory(at: inputMethods, withIntermediateDirectories: true)
            if fileManager.fileExists(atPath: destination.path) { try fileManager.removeItem(at: destination) }
            try fileManager.copyItem(at: app, to: destination)
        } catch {
            return fail(ExitCode.setup, "Nagi.app の配置に失敗しました (\(destination.path)): \(error)")
        }

        // `NagiIMEApp --register`: TIS への登録と有効化だけ行って終了する（HostOptions.register）
        let register = runProcess(destination.appendingPathComponent("Contents/MacOS/\(executable)").path, ["--register"])
        print(register.output)
        if register.status != 0 {
            return fail(ExitCode.setup, "NagiIMEApp --register が失敗しました (exit \(register.status))")
        }
        return nil
    }

    private func teardown() {
        host?.terminate()
        runProcess("/usr/bin/pkill", ["-x", executableName()])
        for key in ["NAGI_CONFIG_DIR", "NAGI_USER_DIR", "NAGI_DATA_DIR"] {
            runProcess("/bin/launchctl", ["unsetenv", key])
        }
        try? fileManager.removeItem(at: workDir)
    }

    // MARK: - ウォームアップ

    /// 初回のキー入力は IME プロセスとエンジンの起動を伴うため、長めに待つ。
    /// ここで届かなければ、権限・入力ソース・フォーカスのどれが原因かを診断して終了する
    private func warmUp() -> Int32? {
        if let message = resetEnvironment() { return fail(ExitCode.unreachable, "リセットに失敗: \(message)") }
        poster.post(KeyChord(keyCode: KeyMap.unshifted["a"]!))
        let reached = pumpUntil(TimeInterval(config.warmupMs) / 1000) { host?.readState()?.marked == "あ" }
        if reached {
            print("ウォームアップ: IME 経由で \"a\" -> \"あ\" を確認")
            if let message = resetEnvironment() { return fail(ExitCode.unreachable, "リセットに失敗: \(message)") }
            return nil
        }

        screenshot(named: "warmup-failed")
        let state = host?.readState()
        var lines = [
            "キー入力が IME に届きませんでした（\"a\" を送っても未確定文字列が \"あ\" になりません）。",
            "  実測: marked=\(state?.marked.debugDescription ?? "nil") committed=\(state?.committed.debugDescription ?? "nil") active=\(String(describing: state?.active))",
            "  現在の入力ソース: \(InputSources.currentID() ?? "nil")（期待: \(japaneseInputSourceID)）",
            "  権限: \(Permissions.summary())",
        ]
        if !Permissions.accessibilityTrusted || !Permissions.postEventAllowed {
            lines.append(
                "  原因の可能性が高い: アクセシビリティ（キー送出）の許可がありません。"
                    + "システム設定 > プライバシーとセキュリティ > アクセシビリティ で、このコマンドを実行しているターミナル/シェルを許可してください。"
                    + "CI では .github/workflows/macos-e2e.yml の TCC 付与手順を参照")
        }
        if state?.active != true {
            lines.append("  NagiTestHost が前面のキーウィンドウではありません（ログイン済みの GUI セッションが必要です）")
        }
        if InputSources.currentID() != japaneseInputSourceID {
            lines.append("  Nagi の入力ソースが選択されていません（TISSelectInputSource が拒否された可能性）")
        }
        lines.append("  入力ソース一覧:")
        lines.append(contentsOf: InputSources.dump(matching: "nagi"))
        lines.append("  それ以外: IME プロセス/エンジンの起動失敗。`log show --predicate 'process == \"\(executableName())\"' --last 5m` を確認")
        return fail(ExitCode.unreachable, lines.joined(separator: "\n"))
    }

    // MARK: - 各シナリオ

    /// シナリオ開始時の初期化: 未確定を破棄し、テキスト欄を空にし、日本語（ひらがな）入力ソースへ戻す。
    /// 失敗したら理由を返す
    private func resetEnvironment() -> String? {
        guard let host else { return "ホストが起動していません" }
        // 変換中でも抜けられるよう Escape を 2 回（コンポジションなしなら IME は消費せず、ホストは無視する）
        for _ in 0..<2 {
            poster.post(KeyChord(keyCode: KeyMap.escapeKey))
            pump(0.05)
        }
        pump(0.1)
        let before = host.readState()?.clearCount ?? 0
        host.sendClear()
        guard pumpUntil(3, { (host.readState()?.clearCount ?? 0) > before }) else {
            return "NagiTestHost が clear（SIGUSR1）に応答しません"
        }
        if InputSources.currentID() != japaneseInputSourceID {
            _ = InputSources.enableAndSelect(id: japaneseInputSourceID)
            pump(0.3)
        }
        pump(0.1)
        if let state = host.readState(), !state.marked.isEmpty || !state.committed.isEmpty {
            return "リセット後もテキスト欄が空になりません: marked=\(state.marked.debugDescription) committed=\(state.committed.debugDescription)"
        }
        return nil
    }

    private func runScenario(suite: String, scenario: Scenario) -> ScenarioResult {
        if let message = resetEnvironment() {
            return ScenarioResult(suite: suite, name: scenario.name, ok: false, message: "初期化に失敗: \(message)")
        }
        for (index, step) in scenario.steps.enumerated() {
            do {
                try execute(step, suite: suite, scenario: scenario.name)
            } catch let error as StepFailure {
                screenshot(named: "\(suite)__\(scenario.name)__FAIL")
                return ScenarioResult(
                    suite: suite, name: scenario.name, ok: false,
                    message: "step \(index + 1): \(error.message)")
            } catch {
                return ScenarioResult(
                    suite: suite, name: scenario.name, ok: false, message: "step \(index + 1): \(error)")
            }
        }
        return ScenarioResult(suite: suite, name: scenario.name, ok: true, message: "")
    }

    private func execute(_ step: Step, suite: String, scenario: String) throws {
        if let text = step.type {
            for chord in try KeyMap.chords(forTyping: text) {
                poster.post(chord)
                pump(0.03)
            }
        } else if let key = step.key {
            poster.post(try KeyMap.chord(forKeySpec: key))
            pump(0.05)
        } else if let expect = step.expect {
            try check(expect)
        } else if let name = step.screenshot {
            screenshot(named: "\(suite)__\(scenario)__\(name)")
        } else if step.tap != nil || step.flick != nil {
            throw StepFailure(message: "tap / flick は iOS 専用のステップです")
        } else {
            throw StepFailure(message: "認識できないステップです")
        }
    }

    /// 指定された項目がすべて一致するまで最大 timeout_ms 待つ
    private func check(_ expect: Expect) throws {
        guard let host else { throw StepFailure(message: "ホストが起動していません") }
        if expect.candidatesContain != nil {
            // macOS のドライバーでは候補一覧を取得できない（仕様: 取得できる環境のみ）
            print("  注: candidates_contain は macOS では検証しません（スキップ）")
        }
        var mismatch = "状態ファイルを読めません"
        let ok = pumpUntil(TimeInterval(expect.timeoutMs ?? 2000) / 1000) {
            guard let state = host.readState() else { return false }
            var problems: [String] = []
            if let want = expect.marked, state.marked != want {
                problems.append("marked: expected \(want.debugDescription), actual \(state.marked.debugDescription)")
            }
            if let want = expect.committed, state.committed != want {
                problems.append("committed: expected \(want.debugDescription), actual \(state.committed.debugDescription)")
            }
            if let want = expect.text, state.committed + state.marked != want {
                problems.append("text: expected \(want.debugDescription), actual \((state.committed + state.marked).debugDescription)")
            }
            mismatch = problems.joined(separator: "; ")
            return problems.isEmpty
        }
        if !ok { throw StepFailure(message: mismatch) }
    }

    // MARK: - 成果物

    private func screenshot(named name: String) {
        let safe = name.map { $0.isLetter || $0.isNumber || $0 == "_" || $0 == "-" ? $0 : Character("_") }
        let path = screenshotsDir.appendingPathComponent(String(safe) + ".png").path
        let result = runProcess("/usr/sbin/screencapture", ["-x", "-t", "png", path])
        if result.status != 0 {
            print("  注: screencapture に失敗しました (exit \(result.status)): \(result.output)")
        }
    }

    private func writeResults() {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        do {
            let data = try encoder.encode(results)
            try data.write(to: outDir.appendingPathComponent("results.json"), options: .atomic)
        } catch {
            FileHandle.standardError.write(Data("warning: results.json を書けません: \(error)\n".utf8))
        }
    }
}
#endif
