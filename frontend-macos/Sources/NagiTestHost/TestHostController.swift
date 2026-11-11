#if os(macOS)
import AppKit

/// 状態ファイルへ書き出す JSON: `{"marked","committed","clear_count","active","seq"}`。
/// ドライバー（nagi-e2e）が polling して検証する。`marked` / `committed` が共通シナリオ仕様の値
final class StateWriter {
    private let url: URL
    private var seq = 0

    init(path: String) {
        url = URL(fileURLWithPath: path)
    }

    func write(committed: String, marked: String, clearCount: Int, active: Bool) {
        seq += 1
        let object: [String: Any] = [
            "marked": marked,
            "committed": committed,
            "clear_count": clearCount,
            "active": active,
            "seq": seq,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
            // 書き込み途中を読ませないよう原子的に置き換える
            try data.write(to: url, options: .atomic)
        } catch {
            NSLog("NagiTestHost: failed to write state file: \(error)")
        }
    }
}

/// ウィンドウ・テキスト欄・状態ファイル・リセット用シグナル（SIGUSR1）の組み立て。
final class TestHostController: NSObject, NSApplicationDelegate {
    private let writer: StateWriter
    private let view = TextInputView(frame: NSRect(x: 0, y: 0, width: 640, height: 240))
    private var window: NSWindow?
    private var signalSource: DispatchSourceSignal?
    private var keepActiveTimer: Timer?
    private var clearCount = 0
    private var observers: [NSObjectProtocol] = []

    init(statePath: String) {
        writer = StateWriter(path: statePath)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(
            contentRect: NSRect(x: 200, y: 400, width: 640, height: 240),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "NagiTestHost"
        window.isReleasedWhenClosed = false
        window.contentView = view
        self.window = window
        view.onChange = { [weak self] in self?.writeState() }

        // 前面化。入力ソースの切替・キー送出は前面アプリに対して行われる
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(view)

        // 別アプリにフォーカスを奪われても取り戻す（CI の通知ダイアログ等への備え）
        keepActiveTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            guard let self, let window = self.window else { return }
            if !NSApp.isActive {
                NSApp.activate(ignoringOtherApps: true)
            }
            if !window.isKeyWindow { window.makeKeyAndOrderFront(nil) }
            if window.firstResponder !== self.view { window.makeFirstResponder(self.view) }
        }

        let center = NotificationCenter.default
        for name in [
            NSApplication.didBecomeActiveNotification, NSApplication.didResignActiveNotification,
            NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification,
        ] {
            observers.append(
                center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                    self?.writeState()
                })
        }

        // SIGUSR1 でリセット（IME のコンポジションを破棄し、内容を空にして clear_count を進める）
        signal(SIGUSR1, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: SIGUSR1, queue: .main)
        source.setEventHandler { [weak self] in self?.handleClear() }
        source.resume()
        signalSource = source

        writeState()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private func handleClear() {
        view.reset()
        clearCount += 1
        writeState()
    }

    private func writeState() {
        let active =
            NSApp.isActive && (window?.isKeyWindow ?? false) && window?.firstResponder === view
        writer.write(
            committed: view.committed, marked: view.marked, clearCount: clearCount, active: active)
    }
}
#endif
