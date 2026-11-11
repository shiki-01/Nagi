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

    /// 入力ソース（入力モード）を有効化して選択する
    static func enableAndSelect(id: String) -> SelectReport {
        let filter = [keyInputSourceID: id] as CFDictionary
        guard let source = find(filter).first else {
            return SelectReport(found: false, enableStatus: -1, selectStatus: -1)
        }
        let enable = TISEnableInputSource(source)
        let select = TISSelectInputSource(source)
        return SelectReport(found: true, enableStatus: enable, selectStatus: select)
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
