#if os(macOS)
import ApplicationServices
import Carbon
import CoreGraphics
import Foundation

/// RunLoop を回しながら待つ（TIS の状態更新は RunLoop 経由で反映されることがあるため、単純な sleep にしない）
func pump(_ seconds: TimeInterval) {
    let deadline = Date(timeIntervalSinceNow: seconds)
    while Date() < deadline {
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01))
    }
}

/// 条件が成立するまで待つ。タイムアウトしたら false
@discardableResult
func pumpUntil(_ timeout: TimeInterval, _ condition: () -> Bool) -> Bool {
    let deadline = Date(timeIntervalSinceNow: timeout)
    while true {
        if condition() { return true }
        if Date() >= deadline { return false }
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02))
    }
}

struct ProcessOutput {
    var status: Int32
    var output: String
}

/// 外部コマンドを実行して標準出力+標準エラーを集める
@discardableResult
func runProcess(_ path: String, _ arguments: [String]) -> ProcessOutput {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: path)
    process.arguments = arguments
    let pipe = Pipe()
    process.standardOutput = pipe
    process.standardError = pipe
    do {
        try process.run()
    } catch {
        return ProcessOutput(status: -1, output: "failed to run \(path): \(error)")
    }
    let data = pipe.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    return ProcessOutput(status: process.terminationStatus, output: String(decoding: data, as: UTF8.self))
}

// MARK: - キー送出

/// CGEvent でキーを送出する。システム全体の HID イベントとして流れ、前面アプリ（NagiTestHost）→ IMKit に届く。
/// 要アクセシビリティ許可（送出元プロセスまたはその責任プロセス）
final class KeyPoster {
    private let source = CGEventSource(stateID: .hidSystemState)

    func post(_ chord: KeyChord) {
        var flags: CGEventFlags = []
        var pressed: [(UInt16, CGEventFlags)] = []
        if chord.shift { pressed.append((KeyMap.shiftKey, .maskShift)) }
        if chord.control { pressed.append((KeyMap.controlKey, .maskControl)) }
        if chord.option { pressed.append((KeyMap.optionKey, .maskAlternate)) }
        if chord.command { pressed.append((KeyMap.commandKey, .maskCommand)) }

        for (code, flag) in pressed {
            flags.insert(flag)
            send(code, down: true, flags: flags)
        }
        send(chord.keyCode, down: true, flags: flags)
        send(chord.keyCode, down: false, flags: flags)
        for (code, flag) in pressed.reversed() {
            flags.remove(flag)
            send(code, down: false, flags: flags)
        }
    }

    private func send(_ code: UInt16, down: Bool, flags: CGEventFlags) {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(code), keyDown: down)
        else { return }
        event.flags = flags
        event.post(tap: .cghidEventTap)
        usleep(8_000)
    }
}

// MARK: - 入力ソース（TIS）

enum InputSources {
    private static let keyInputSourceID = "TISPropertyInputSourceID"

    private static func find(_ filter: CFDictionary?) -> [TISInputSource] {
        guard let list = TISCreateInputSourceList(filter, true)?.takeRetainedValue() else { return [] }
        var result: [TISInputSource] = []
        for i in 0..<CFArrayGetCount(list) {
            guard let pointer = CFArrayGetValueAtIndex(list, i) else { continue }
            result.append(Unmanaged<TISInputSource>.fromOpaque(pointer).takeUnretainedValue())
        }
        return result
    }

    private static func property(_ source: TISInputSource, _ key: String) -> String? {
        guard let raw = TISGetInputSourceProperty(source, key as CFString) else { return nil }
        return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
    }

    /// 現在選ばれているキーボード入力ソースの ID
    static func currentID() -> String? {
        guard let source = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue() else { return nil }
        return property(source, keyInputSourceID)
    }

    struct SelectReport {
        var found: Bool
        var enableStatus: Int32
        var selectStatus: Int32
    }

    private static func boolProperty(_ source: TISInputSource, _ key: String) -> Bool? {
        guard let raw = TISGetInputSourceProperty(source, key as CFString) else { return nil }
        return CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(raw).takeUnretainedValue())
    }

    /// id 一致の入力ソース（includeAllInstalled: 無効なものも含む）
    private static func source(id: String) -> TISInputSource? {
        find([keyInputSourceID: id] as CFDictionary).first
    }

    /// 1 行の説明（ID / 種別 / enabled / select-capable）
    static func describe(_ source: TISInputSource) -> String {
        func flag(_ value: Bool?) -> String { value.map { $0 ? "yes" : "no" } ?? "-" }
        return "\(property(source, keyInputSourceID) ?? "?") type=\(property(source, "TISPropertyInputSourceType") ?? "-")"
            + " enabled=\(flag(boolProperty(source, "TISPropertyInputSourceIsEnabled")))"
            + " enableCapable=\(flag(boolProperty(source, "TISPropertyInputSourceIsEnableCapable")))"
            + " selectCapable=\(flag(boolProperty(source, "TISPropertyInputSourceIsSelectCapable")))"
            + " selected=\(flag(boolProperty(source, "TISPropertyInputSourceIsSelected")))"
    }

    /// 診断: Nagi 関連（bundle id / ID に "com.shiki.inputmethod.Nagi" を含む）と、キーボードレイアウト以外の
    /// 全入力ソースだけを列挙する（レイアウトは列挙しない）
    static func dump() -> [String] {
        let all = find(nil)
        var lines = ["入力ソース総数: \(all.count)（Nagi 関連と非レイアウト型のみ表示）"]
        var shown = 0
        for source in all {
            let id = property(source, keyInputSourceID) ?? ""
            let bundle = property(source, "TISPropertyBundleID") ?? ""
            let type = property(source, "TISPropertyInputSourceType") ?? ""
            let isNagi = id.contains("com.shiki.inputmethod.Nagi") || bundle.contains("com.shiki.inputmethod.Nagi")
            if isNagi || type != "TISTypeKeyboardLayout" {
                lines.append("  " + describe(source) + (bundle.isEmpty ? "" : " bundle=\(bundle)"))
                shown += 1
            }
        }
        if shown == 0 { lines.append("  (該当なし: Nagi は TIS の一覧に存在しない)") }
        return lines
    }

    /// 親の入力メソッド（bundle id そのもの）を enable + select してみる
    static func selectParent(bundleID: String) -> String {
        guard let parent = source(id: bundleID) else { return "親(\(bundleID)) は一覧に無い" }
        let enable = TISEnableInputSource(parent)
        let select = TISSelectInputSource(parent)
        return "親(\(bundleID)) enable=\(enable) select=\(select) | \(describe(parent))"
    }

    /// ドライバー自身から Nagi.app の URL で TISRegisterInputSource を呼ぶ
    static func register(appURL: URL) -> Int32 {
        TISRegisterInputSource(appURL as CFURL)
    }

    /// 入力ソース（入力モード）を有効化して選択する（1 回だけ試す）
    static func enableAndSelect(id: String) -> SelectReport {
        guard let source = source(id: id) else {
            return SelectReport(found: false, enableStatus: -1, selectStatus: -1)
        }
        let enable = TISEnableInputSource(source)
        let select = TISSelectInputSource(source)
        return SelectReport(found: true, enableStatus: enable, selectStatus: select)
    }

    /// 有効化 → 待機 → 選択をリトライし、currentID() が id になるまで待つ（最大 timeout 秒）。
    /// 有効化直後は TIS の一覧に反映されず select が -50 になることがあるため。ログを `log` に積む
    static func selectWithRetry(id: String, timeout: TimeInterval, log: (String) -> Void) -> Bool {
        let deadline = Date(timeIntervalSinceNow: timeout)
        var attempt = 0
        while Date() < deadline {
            attempt += 1
            if currentID() == id { return true }
            guard let target = source(id: id) else {
                log("  試行 \(attempt): 入力ソースが見つかりません")
                pump(1)
                continue
            }
            // 3 回目以降は一度標準の英字キーボードへ切り替えてから選ぶ（現在値と同一視されるのを避ける）
            if attempt >= 3, attempt % 2 == 1, let us = source(id: "com.apple.keylayout.US") {
                log("  試行 \(attempt): いったん US を選択 -> \(TISSelectInputSource(us))")
                pump(0.5)
            }
            let enable = TISEnableInputSource(target)
            pump(0.5)
            let select = TISSelectInputSource(target)
            pump(0.5)
            log("  試行 \(attempt): enable=\(enable) select=\(select) current=\(currentID() ?? "nil") | \(describe(target))")
            if select == 0, currentID() == id { return true }
            pump(1)
        }
        return currentID() == id
    }

    /// 最後の手段: com.apple.HIToolbox の AppleEnabledInputSources / AppleSelectedInputSources を書き換えて
    /// 選択させる。cfprefsd と入力メニューのエージェントを再起動して反映を促す
    static func selectViaDefaults(bundleID: String, modeID: String, log: (String) -> Void) -> Bool {
        let dict =
            "<dict><key>InputSourceKind</key><string>Input Mode</string>"
            + "<key>Bundle ID</key><string>\(bundleID)</string>"
            + "<key>Input Mode</key><string>\(modeID)</string></dict>"
        // 有効一覧には追記、選択中は Nagi だけにする
        for args in [
            ["write", "com.apple.HIToolbox", "AppleEnabledInputSources", "-array-add", dict],
            ["write", "com.apple.HIToolbox", "AppleSelectedInputSources", "-array", dict],
        ] {
            let result = runProcess("/usr/bin/defaults", args)
            log("  defaults \(args[0...2].joined(separator: " ")) -> \(result.status) \(result.output)")
        }
        runProcess("/usr/bin/killall", ["cfprefsd"])
        runProcess("/usr/bin/killall", ["TextInputMenuAgent"])
        runProcess("/usr/bin/killall", ["SystemUIServer"])
        pump(3)
        // 反映後に再度 TIS 経由の選択も試す
        let ok = selectWithRetry(id: modeID, timeout: 10, log: log)
        log("  defaults 後の現在の入力ソース: \(currentID() ?? "nil")")
        return ok
    }
}

// MARK: - 権限の診断

enum Permissions {
    static var accessibilityTrusted: Bool { AXIsProcessTrusted() }
    static var postEventAllowed: Bool { CGPreflightPostEventAccess() }

    static func summary() -> String {
        "AXIsProcessTrusted=\(accessibilityTrusted), CGPreflightPostEventAccess=\(postEventAllowed)"
    }
}

// MARK: - NagiTestHost のプロセス

final class HostProcess {
    private let process = Process()
    let stateURL: URL

    init(executable: URL, stateURL: URL, logURL: URL) {
        self.stateURL = stateURL
        process.executableURL = executable
        process.arguments = ["--state-file", stateURL.path]
        FileManager.default.createFile(atPath: logURL.path, contents: nil)
        if let handle = try? FileHandle(forWritingTo: logURL) {
            process.standardOutput = handle
            process.standardError = handle
        }
    }

    func launch() throws { try process.run() }

    var isRunning: Bool { process.isRunning }

    func readState() -> HostState? {
        guard let data = try? Data(contentsOf: stateURL) else { return nil }
        return try? JSONDecoder().decode(HostState.self, from: data)
    }

    /// SIGUSR1: ホストの内容と IME のコンポジションを破棄させる
    func sendClear() {
        kill(process.processIdentifier, SIGUSR1)
    }

    func terminate() {
        if process.isRunning {
            process.terminate()
            process.waitUntilExit()
        }
    }
}
#endif
