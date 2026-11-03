import SwiftUI

@main
struct NagiSpikeApp: App {
    @StateObject private var layoutInbox = NagiLayoutURLInbox.shared
    @Environment(\.scenePhase) private var scenePhase

    init() { LaunchBaseline.capture() }

    private static func captureClipboard() {
        ClipboardCapture.captureIfChanged(
            config: NagiIOSSettingsStore.load().clipboardHistory, hasFullAccess: true, isSecure: false,
            store: ClipboardHistoryStore())
    }

    /// なぞり入力がオンで索引が無い・古いとき、バックグラウンドで作る（辞書の更新のあとなど）
    private static func ensureGlideIndex() {
        Task { @MainActor in GlideIndexBuilder.shared.ensureBuiltIfWanted() }
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                // nagi://layout?data=<base64url json>（サイトで作ったレイアウトの 1 タップ取り込み）
                .onOpenURL { url in layoutInbox.handle(url) }
                // クリップボード履歴: アプリが前面になったとき、クリップボードが変わっていれば取り込む（履歴がオンのときだけ）
                .onAppear {
                    Self.captureClipboard()
                    Self.ensureGlideIndex()
                }
                .onChange(of: scenePhase) { phase in
                    if phase == .active {
                        Self.captureClipboard()
                        Self.ensureGlideIndex()
                    }
                }
        }
    }
}
