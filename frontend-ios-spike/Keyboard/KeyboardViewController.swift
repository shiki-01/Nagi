import UIKit
import SwiftUI

/// 製品版キーボードと検証用の画面（Probe）の切り替え
final class KeyboardScreen: ObservableObject {
    @Published var showProbes = false
}

class KeyboardViewController: UIInputViewController, KeyboardHost {
    private let store = SharedStore.shared
    private var model: KeyboardModel?
    private let screen = KeyboardScreen()
    /// 読み込み中はプレースホルダ、コアの準備ができたら `KeyboardRoot` に差し替えるため AnyView で持つ
    private var hostingController: UIHostingController<AnyView>?
    /// キーボード全体の高さの制約（設定の高さの倍率で、focusIn のたびに更新する）
    private var heightConstraint: NSLayoutConstraint?
    /// 直近に見た入力欄の識別子（`textDocumentProxy.documentIdentifier`）。変わったら入力欄が切り替わっている
    private var lastDocumentIdentifier: UUID?

    /// このプロセスで作った VC の数（診断ログでインスタンスを区別する。メインスレッドからのみ読み書きする）
    nonisolated(unsafe) private static var instanceCounter = 0
    /// この VC の通し番号（生成時に採番する）
    private let instanceNumber: Int = {
        KeyboardViewController.instanceCounter += 1
        return KeyboardViewController.instanceCounter
    }()

    /// 表示中のキーボードのプロセス ID を置く App Group の UserDefaults のキー。
    /// 表示中のままプロセスが終わった（viewWillDisappear が来なかった）ことを、次のプロセスで検出する
    private static let visiblePidKey = "keyboardVisiblePid"

    /// viewDidLoad の時刻（コア準備完了までの所要時間の診断ログ用）
    private var loadStartedAt = Date()
    /// viewWillAppear から viewWillDisappear の間（コアの準備が表示後に終わったとき、focusIn が必要かの判定）
    private var isAppeared = false
    /// コア生成の依頼の世代（古い完了を捨てる）
    private var coreGeneration = 0
    /// 高さの異常（想定の 1.5 倍超）の警告を出したか（1 回だけ）
    private var loggedOversize = false

    /// コアを生成中か（メインスレッドからのみ読み書きする。同時に 2 つのコアを作らない）
    nonisolated(unsafe) private static var coreCreating = false
    /// 生成中に作られた最新の VC が、完了時に呼ばれる処理を置く（古い VC のものは上書きして捨てる）
    nonisolated(unsafe) private static var pendingAttach: ((KeyboardModel) -> Void)?

    /// 共有のモデルの現在のホストが自分か（旧 VC が遅れて呼ばれても、新しい VC の状態を壊さないための判定）
    private var isCurrentHost: Bool { model?.host === self }

    override func viewDidLoad() {
        LaunchBaseline.capture()
        super.viewDidLoad()
        loadStartedAt = Date()

        // 重い初期化の前に、高さと透明な背景を確定させる（高さ未確定の間に背景だけが広がるのを防ぐ）。
        // 優先度を下げて OS の制約と衝突しないようにする
        view.backgroundColor = .clear
        let height = view.heightAnchor.constraint(equalToConstant: CGFloat(NagiKeyMetrics.totalHeight))
        height.priority = UILayoutPriority(999)
        height.isActive = true
        heightConstraint = height

        let reuseModel = KeyboardModel.processShared != nil
        let stats = MemoryProbe.getStats()
        DiagLog.log("viewDidLoad instance=#\(instanceNumber) pid=\(getpid()) reuseModel=\(reuseModel) footprint=\(Int(stats.physFootprintMB))MB available=\(Int(stats.availableMemoryMB))MB")
        if !reuseModel {
            // 申告した拡張属性が実際のバンドルに入っているかの確認用（H-35）
            let ext = Bundle.main.object(forInfoDictionaryKey: "NSExtension") as? [String: Any]
            let attrs = ext?["NSExtensionAttributes"] as? [String: Any] ?? [:]
            DiagLog.log("extension attributes \(attrs.map { "\($0.key)=\($0.value)" }.sorted().joined(separator: " "))")
        }
        setupUI()
    }

    deinit {
        DiagLog.log("keyboard VC deinit instance=#\(instanceNumber)")
    }

    override func viewWillAppear(_ animated: Bool) {
        DiagLog.log("viewWillAppear instance=#\(instanceNumber) currentHost=\(isCurrentHost)")
        super.viewWillAppear(animated)
        isAppeared = true
        // ホストアプリのセットアップ状況表示用のマーカー（OnboardingView.swift の NagiSetupMarker が読む）
        if let defaults = UserDefaults(suiteName: SharedStore.shared.appGroupId) {
            let now = Date().timeIntervalSince1970
            defaults.set(now, forKey: "keyboardLastSeen")
            if hasFullAccess { defaults.set(now, forKey: "fullAccessLastSeen") }
            defaults.set(Int(getpid()), forKey: Self.visiblePidKey)
        }
        lastDocumentIdentifier = textDocumentProxy.documentIdentifier
        // 非表示の間に捨てた背景画像を、必要になったときに読み直せるようにする
        KeyImageCache.shared.resume()
        // 再表示で別の VC から戻る場合に備え、常に自分をホストにしてから focusIn する
        model?.host = self
        model?.focusIn()
        // 触覚・クリック音: フルアクセスの有無を渡し、focusIn で読み直した設定でジェネレータを準備する
        NagiKeyFeedback.fullAccess = hasFullAccess
        NagiKeyFeedback.prepare()
        // focusIn で読み直した設定の高さを反映する
        heightConstraint?.constant = CGFloat(NagiKeyMetrics.totalHeight)
        applyTheme()
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        // 画面の端（候補バーの展開ボタン・端のキー）の押下が遅れる・取りこぼされるのを防ぐ。
        // システムの端のジェスチャ（ホームインジケーター・端からのスワイプ）の認識器が、キーボードの window で
        // touchesBegan を遅らせるため、遅らせないようにする（カスタムキーボードで一般的な対処）
        view.window?.gestureRecognizers?.forEach { $0.delaysTouchesBegan = false }
    }

    /// 設定 `keyboard.theme` の外観を反映する（system は端末に従う）
    private func applyTheme() {
        hostingController?.overrideUserInterfaceStyle = NagiTheme.interfaceStyle(NagiKeyMetrics.current.theme)
    }

    override func viewWillDisappear(_ animated: Bool) {
        let current = isCurrentHost
        DiagLog.log("viewWillDisappear instance=#\(instanceNumber) currentHost=\(current)")
        isAppeared = false
        if model == nil {
            let elapsed = Int(Date().timeIntervalSince(loadStartedAt) * 1000)
            DiagLog.log("viewWillDisappear while placeholder (core not ready) instance=#\(instanceNumber) sinceLoad=\(elapsed)ms")
        }
        // 変換中の文字列は確定してから離れる。LLM も速やかに手放す（再表示の focusIn で読み直す）。
        // 共有のモデルが既に別の VC のものなら触らない（新しい VC の状態を壊さない）
        if current {
            model?.focusOut()
            model?.suspendLLM()
            // 背景画像のデコード結果を手放す（H-26 と同じ観点: 非表示の間はメモリを持たない）
            KeyImageCache.shared.suspend()
            UserDefaults(suiteName: SharedStore.shared.appGroupId)?.removeObject(forKey: Self.visiblePidKey)
        }
        super.viewWillDisappear(animated)
    }

    private func setupUI() {
        // まず軽いプレースホルダを出す（コアの生成は重いので、画面を先に出す）
        let hostView = UIHostingController(rootView: AnyView(KeyboardPlaceholderView()))
        hostingController = hostView
        // 背景を透明にする（既定のままだと不透明で黒く見える。システムのキーボードの背景（ブラー）を見せる）
        hostView.view.backgroundColor = .clear
        addChild(hostView)

        self.view.addSubview(hostView.view)
        hostView.view.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            hostView.view.leadingAnchor.constraint(equalTo: self.view.leadingAnchor),
            hostView.view.trailingAnchor.constraint(equalTo: self.view.trailingAnchor),
            hostView.view.topAnchor.constraint(equalTo: self.view.topAnchor),
            hostView.view.bottomAnchor.constraint(equalTo: self.view.bottomAnchor)
        ])
        hostView.didMove(toParent: self)
        applyTheme()
        // 触覚ジェネレータを結びつけるビュー（iOS 17.5 以降）
        NagiKeyFeedback.anchorView = view

        if let existing = KeyboardModel.processShared {
            // 同じプロセスで VC が作り直された: コアを作らずに引き継ぐ（旧 VC が残っていてもコアを二重に持たない）
            existing.handOverToNewHost()
            attachModel(existing)
        } else {
            startCoreCreation()
        }
    }

    /// コアの生成をメインスレッドの外で行う。完了したらメインスレッドで `KeyboardModel` を作り、
    /// 最新の VC に渡す。生成中に別の VC ができても、コアは 1 つだけ作る
    private func startCoreCreation() {
        coreGeneration += 1
        let generation = coreGeneration
        KeyboardViewController.pendingAttach = { [weak self] model in
            guard let self = self, generation == self.coreGeneration else { return }
            self.coreReady(model)
        }
        guard !KeyboardViewController.coreCreating else {
            DiagLog.log("core creation already running; waiting instance=#\(instanceNumber)")
            return
        }
        KeyboardViewController.coreCreating = true
        reportPreviousProcessEndedWhileVisible()
        let store = self.store
        let instance = instanceNumber
        // コアは以前メインスレッドの大きなスタックで動いていたので、GCD の既定（512KB）ではなく
        // スタックを明示した専用スレッドで生成する
        let thread = Thread {
            let started = Date()
            // コアのセッション。辞書やモデルが無ければ、画面にメッセージを出す
            // 空きメモリが足りないとき（別アプリから切り替えた直後など）は LLM を読み込まず辞書のみで始め、
            // 空きができてから読み込む（旧インスタンスと合わせてメモリが倍になるのを避ける。H-26）
            let deferLLM = ProductSession.shouldDeferLLM(store: store)
            let outcome = ProductSession.make(store: store, noLLM: deferLLM)
            let makeMs = Int(Date().timeIntervalSince(started) * 1000)
            DispatchQueue.main.async {
                DiagLog.log("core created instance=#\(instance) makeMs=\(makeMs) session=\(outcome.session != nil) deferLLM=\(deferLLM)")
                KeyboardViewController.finishCoreCreation(outcome: outcome, deferLLM: deferLLM)
            }
        }
        thread.name = "com.shiki.nagi.core-create"
        thread.stackSize = 8 * 1024 * 1024
        thread.qualityOfService = .userInitiated
        thread.start()
    }

    /// メインスレッド: 生成したコアからモデルを作り、待っている最新の VC へ渡す（VC が既に無ければ共有に置くだけ）
    private static func finishCoreCreation(outcome: ProductSession.Outcome, deferLLM: Bool) {
        coreCreating = false
        let model: KeyboardModel
        if let existing = KeyboardModel.processShared {
            model = existing
        } else {
            model = KeyboardModel(outcome: outcome, llmDeferred: deferLLM)
            KeyboardModel.processShared = model
        }
        let attach = pendingAttach
        pendingAttach = nil
        attach?(model)
    }

    /// メインスレッド: コアの準備ができた（プレースホルダの表示中に生成が終わった）
    private func coreReady(_ model: KeyboardModel) {
        let elapsed = Int(Date().timeIntervalSince(loadStartedAt) * 1000)
        DiagLog.log("core ready instance=#\(instanceNumber) sinceLoad=\(elapsed)ms appeared=\(isAppeared)")
        attachModel(model)
    }

    /// モデルをこの VC のホストにして、画面をプレースホルダからキーボードへ差し替える。
    /// 既に表示中（viewWillAppear が先に来ていた）なら、viewWillAppear で出来なかった focusIn もここで行う
    private func attachModel(_ model: KeyboardModel) {
        model.host = self
        self.model = model
        let probeView = KeyboardSwiftUIView(
            store: store,
            insertTextHandler: { [weak self] text in
                self?.textDocumentProxy.insertText(text)
            },
            deleteBackwardHandler: { [weak self] in
                self?.textDocumentProxy.deleteBackward()
            },
            advanceToNextInputModeHandler: { [weak self] in
                self?.advanceToNextInputMode()
            }
        )
        hostingController?.rootView = AnyView(KeyboardRoot(model: model, screen: screen, probeView: probeView))
        if isAppeared {
            model.focusIn()
        }
        // focusIn で読み直した設定の高さを反映する
        heightConstraint?.constant = CGFloat(NagiKeyMetrics.totalHeight)
        applyTheme()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        // 想定の高さの 1.5 倍を超えたら 1 回だけ記録する（背景だけが画面いっぱいに広がる不具合の切り分け用）
        guard !loggedOversize else { return }
        let expected = CGFloat(NagiKeyMetrics.totalHeight)
        let actual = view.bounds.height
        if actual > expected * 1.5 {
            loggedOversize = true
            let elapsed = Int(Date().timeIntervalSince(loadStartedAt) * 1000)
            DiagLog.log("layout oversize instance=#\(instanceNumber) height=\(Int(actual)) expected=\(Int(expected)) modelReady=\(model != nil) sinceLoad=\(elapsed)ms")
        }
    }

    /// 前のプロセスが表示中のまま終わった（viewWillDisappear が来なかった）なら記録して、印を消す
    private func reportPreviousProcessEndedWhileVisible() {
        guard let defaults = UserDefaults(suiteName: SharedStore.shared.appGroupId),
              let old = defaults.object(forKey: Self.visiblePidKey) as? Int else { return }
        if old != Int(getpid()) {
            DiagLog.log("previous keyboard process pid=\(old) ended while visible (no viewWillDisappear)")
        }
        defaults.removeObject(forKey: Self.visiblePidKey)
    }

    // MARK: - KeyboardHost

    func hostInsertText(_ text: String) {
        textDocumentProxy.insertText(text)
    }

    func hostDeleteBackward() {
        textDocumentProxy.deleteBackward()
    }

    func hostSetMarkedText(_ text: String, cursor: Int) {
        let position = min(max(cursor, 0), text.utf16.count)
        textDocumentProxy.setMarkedText(text, selectedRange: NSRange(location: position, length: 0))
    }

    func hostUnmarkText() {
        textDocumentProxy.unmarkText()
    }

    func hostClearMarkedText() {
        textDocumentProxy.setMarkedText("", selectedRange: NSRange(location: 0, length: 0))
        textDocumentProxy.unmarkText()
    }

    func hostAdjustCursor(by offset: Int) {
        textDocumentProxy.adjustTextPosition(byCharacterOffset: offset)
    }

    func hostAdvanceToNextInputMode() {
        advanceToNextInputMode()
    }

    func hostDismissKeyboard() {
        dismissKeyboard()
    }

    func hostContextBeforeInput() -> String? {
        textDocumentProxy.documentContextBeforeInput
    }

    func hostContextAfterInput() -> String? {
        textDocumentProxy.documentContextAfterInput
    }

    func hostKeyboardType() -> UIKeyboardType? {
        textDocumentProxy.keyboardType
    }

    func hostReturnKeyType() -> UIReturnKeyType? {
        textDocumentProxy.returnKeyType
    }

    func hostHasFullAccess() -> Bool {
        hasFullAccess
    }

    func hostIsSecure() -> Bool {
        let proxy = textDocumentProxy
        if proxy.isSecureTextEntry ?? false { return true }
        // パスワード・ワンタイムコードの入力欄（secure でなくても文脈・学習の対象にしない）
        let type = proxy.textContentType
        return type == UITextContentType.password
            || type == UITextContentType.newPassword
            || type == UITextContentType.oneTimeCode
    }

    // MARK: - メモリ警告

    /// OS からのメモリ警告。コアに `memory_pressure: critical` を送って LLM を手放させ、キャッシュを捨てる
    /// （強制終了される前に軽くする）
    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        // 背景画像は真っ先に手放す（色だけで描く）。プロセス共通のキャッシュなので現在のホストかは問わない
        KeyImageCache.shared.suspend()
        // 共有のモデルは現在のホストの VC だけが扱う（同じプロセスの VC には同じ警告が届く）
        guard isCurrentHost else { return }
        model?.handleMemoryWarning()
    }

    // MARK: - ホストの入力欄の変化

    /// 入力欄の内容やカーソルが変わった（外部からの変更、カーソル移動、自分の操作の結果）。
    /// 変換中でなければ、直前のテキストを文脈としてコアに渡し直す
    override func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        // 入力欄そのものが切り替わったときは、状態を初期化する（入力中の文字は前の欄で確定扱い）
        let identifier = textDocumentProxy.documentIdentifier
        // 旧 VC に遅れて届いた通知では、共有のモデル（新しい VC の状態）に触らない
        let current = isCurrentHost
        if let last = lastDocumentIdentifier, last != identifier {
            lastDocumentIdentifier = identifier
            if current { model?.hostDocumentChanged() }
        } else if lastDocumentIdentifier == nil {
            lastDocumentIdentifier = identifier
        }
        if current { model?.hostTextChanged() }
    }
}

/// コアの準備ができるまでの間に出す軽いプレースホルダ（背景は透明。高さは VC の制約で確保済み）
struct KeyboardPlaceholderView: View {
    var body: some View {
        Text("読み込み中…")
            .font(.system(size: 13))
            .foregroundColor(.secondary)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// 製品版キーボードと検証用の画面を切り替えるルート
struct KeyboardRoot: View {
    @ObservedObject var model: KeyboardModel
    @ObservedObject var screen: KeyboardScreen
    let probeView: KeyboardSwiftUIView

    var body: some View {
        if screen.showProbes && DiagLog.isTestBuild && model.ios.developerMode {
            ZStack(alignment: .topTrailing) {
                probeView
                Button("閉じる") { screen.showProbes = false }
                    .font(.system(size: 13))
                    .padding(8)
            }
        } else {
            NagiKeyboardView(model: model, onShowProbes: { screen.showProbes = true })
        }
    }
}

// MARK: - SwiftUI 部分

struct KeyboardSwiftUIView: View {
    let store: SharedStore
    let insertTextHandler: (String) -> Void
    let deleteBackwardHandler: () -> Void
    let advanceToNextInputModeHandler: () -> Void

    @State private var results: [ProbeResult] = []
    @State private var selectedProbe: String?
    @State private var isRunning = false
    @State private var showInsertResultsAlert = false
    @State private var hasRunAutoProbe = false
    /// 画面に出す診断ログ（保存や挿入に失敗しても状況が見えるようにする）
    @State private var logLines: [String] = []
    @State private var runningProbe: String?

    /// 実行中の Probe 名を保存するキー。次回起動時に残っていれば、実行中に強制終了されたと判断する
    private static let runningMarkerKey = "nagi.keyboard.runningProbe"

    var body: some View {
        VStack(spacing: 0) {
            // ヘッダ
            VStack {
                Text("Nagi Keyboard Probe")
                    .font(.headline)
                Text(runningProbe.map { "実行中: \($0)" } ?? "各ボタンで検証項目を実行")
                    .font(.caption)
                    .foregroundColor(.gray)
                Text(statusLine())
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundColor(.gray)
            }
            .frame(maxWidth: .infinity)
            .padding()
            .background(Color(.systemGray6))

            // 診断ログ（直近 8 行）
            if !logLines.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(logLines.enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(.system(size: 10, design: .monospaced))
                            .foregroundColor(.black)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.horizontal)
                .padding(.vertical, 4)
                .background(Color(.systemYellow).opacity(0.25))
            }

            // Probe ボタン
            ScrollView {
                VStack(spacing: 8) {
                    ForEach(Array(allProbesExcludingMemoryRamp.keys.sorted()), id: \.self) { probeId in
                        Button(action: { runProbe(probeId) }) {
                            HStack {
                                Text(probeId)
                                Spacer()
                                if let latest = results.last(where: { $0.id == probeId }) {
                                    Image(systemName: latest.ok ? "checkmark.circle" : "xmark.circle")
                                        .foregroundColor(latest.ok ? .green : .red)
                                }
                            }
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color(.systemGray5))
                            .foregroundColor(.black)
                            .cornerRadius(8)
                        }
                        .disabled(isRunning)
                    }

                    Button(action: { runAllProbes() }) {
                        Text("すべて実行（memory_ramp 除外）")
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color.blue)
                            .foregroundColor(.white)
                            .cornerRadius(8)
                    }
                    .disabled(isRunning)

                    Button(action: { runProbe("memory_ramp") }) {
                        Text("Memory Ramp（警告: 強制終了の可能性）")
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color.orange)
                            .foregroundColor(.white)
                            .cornerRadius(8)
                    }
                    .disabled(isRunning)
                }
                .padding()
            }

            Divider()

            Divider()

            // テスト入力ボタン・結果挿入ボタン
            VStack(spacing: 8) {
                HStack(spacing: 4) {
                    Button("あ") { insertTextHandler("あ") }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("か") { insertTextHandler("か") }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("Space") { insertTextHandler(" ") }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("⌫") { deleteBackwardHandler() }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("Next") { advanceToNextInputModeHandler() }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)
                }

                // 入力欄への挿入は DEBUG / TestFlight のビルドだけ（App Store 版では出さない）
                if DiagLog.isTestBuild {
                    Button(action: { insertResultsToTextField() }) {
                        Text("結果を入力欄に挿入")
                            .frame(maxWidth: .infinity)
                            .padding(8)
                            .background(Color.green)
                            .foregroundColor(.white)
                            .cornerRadius(4)
                    }
                }
            }
            .padding()
        }
        .onAppear {
            reportPreviousTermination()
            loadResults()
            autoRunBaselineMemory()
        }
    }

    // MARK: - 診断

    private func appendLog(_ line: String) {
        logLines.append(line)
        if logLines.count > 8 { logLines.removeFirst(logLines.count - 8) }
    }

    private func statusLine() -> String {
        let stats = MemoryProbe.getStats()
        let group = store.containerURL != nil ? "AppGroup:OK" : "AppGroup:NG"
        return String(format: "%@ footprint=%.0fMB available=%.0fMB", group, stats.physFootprintMB, stats.availableMemoryMB)
    }

    private func setRunningMarker(_ probeId: String?) {
        let defaults = UserDefaults.standard
        if let probeId {
            defaults.set(probeId, forKey: Self.runningMarkerKey)
        } else {
            defaults.removeObject(forKey: Self.runningMarkerKey)
        }
        defaults.synchronize()
    }

    /// 前回の実行が終わらないまま拡張が終了していれば、その Probe 名を表示する
    private func reportPreviousTermination() {
        let defaults = UserDefaults.standard
        if let probeId = defaults.string(forKey: Self.runningMarkerKey) {
            appendLog("前回 \(probeId) の実行中に終了（メモリ超過の疑い）")
            defaults.removeObject(forKey: Self.runningMarkerKey)
            defaults.synchronize()
        }
    }

    /// Probe を 1 件実行し、開始・終了・保存失敗を画面ログに残す
    private func execute(_ probeId: String, context: ProbeContext) async {
        runningProbe = probeId
        setRunningMarker(probeId)
        appendLog("開始: \(probeId)")
        let result: ProbeResult
        if probeId == "memory_ramp" {
            result = await MemoryRampProbe(maxLimitMB: 1024).run(context: context)
        } else if let probeRunner = allProbes[probeId] {
            result = await probeRunner(context)
        } else {
            appendLog("未知の Probe: \(probeId)")
            setRunningMarker(nil)
            runningProbe = nil
            return
        }
        setRunningMarker(nil)
        runningProbe = nil
        let metrics = result.metrics
            .sorted { $0.key < $1.key }
            .map { "\($0.key)=\(String(format: "%.1f", $0.value))" }
            .joined(separator: " ")
        appendLog("\(result.ok ? "OK" : "NG"): \(probeId) \(metrics)")
        do {
            try store.append(result: result)
        } catch {
            appendLog("保存失敗: \(error.localizedDescription)")
        }
    }

    private func runProbe(_ probeId: String) {
        isRunning = true
        Task {
            let context = ProbeContext(store: store, processType: "keyboard", engine: nil)
            await execute(probeId, context: context)
            loadResults()
            isRunning = false
        }
    }

    private func runAllProbes() {
        isRunning = true
        Task {
            for probeId in allProbesExcludingMemoryRamp.keys.sorted() {
                let context = ProbeContext(store: store, processType: "keyboard", engine: nil)
                await execute(probeId, context: context)
            }
            loadResults()
            isRunning = false
        }
    }

    private func autoRunBaselineMemory() {
        guard !hasRunAutoProbe else { return }
        hasRunAutoProbe = true

        Task {
            let context = ProbeContext(store: store, processType: "keyboard", engine: nil)
            let result = await BaselineMemoryProbe().run(context: context)
            do {
                try store.append(result: result)
            } catch {
                appendLog("保存失敗(baseline): \(error.localizedDescription)")
            }
            loadResults()
        }
    }

    private func loadResults() {
        results = store.read(last: 100).filter { $0.process == "keyboard" }
    }

    private func insertResultsToTextField() {
        let resultLines = results.map { result in
            let status = result.ok ? "ok" : "ng"
            let metricsStr = result.metrics
                .sorted { $0.key < $1.key }
                .map { "\($0.key)=\(String(format: "%.2f", $0.value))" }
                .joined(separator: ", ")
            return "\(result.id) | \(result.process) | \(status) | \(metricsStr)"
        }

        let insertText = (logLines + resultLines).joined(separator: "\n")
        if !insertText.isEmpty {
            // テキスト入力（行が長い場合は複数行に分割）
            let lines = insertText.split(separator: "\n", omittingEmptySubsequences: true)
            for (index, line) in lines.enumerated() {
                if index > 0 {
                    // 複数行の場合は前の行の後に改行を挿入（省略）
                }
            }
            // 一度に全て挿入
            insertTextHandler(insertText)
        }
    }
}

#Preview {
    KeyboardSwiftUIView(
        store: SharedStore.shared,
        insertTextHandler: { _ in },
        deleteBackwardHandler: { },
        advanceToNextInputModeHandler: { }
    )
}
