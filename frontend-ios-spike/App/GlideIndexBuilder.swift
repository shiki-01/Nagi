import Foundation
import NagiCore
import UIKit

/// グライド入力（なぞり入力）の索引 `nagi-glide.bin` の生成。ホストアプリのバックグラウンドで、辞書
/// （`nagi-dict.bin`）から作って App Group の `data/dict/` に原子的に置く（コアの FFI が一時ファイルに書いて
/// rename する）。キーボード拡張は索引をメモリ上に作らず、このファイルがあるときだけ使う（decisions.md H-84）。
///
/// 作るタイミング（`input.glide` がオンのときだけ自動。オフなら手動のボタンだけ）:
/// 設定で `input.glide` をオンにしたとき、ホームを開いたとき・アプリが前面になったとき、辞書を取り込んだ・更新したとき。
/// 辞書の指紋（サイズ + 更新時刻）が索引を作ったときと違えば作り直す（`GlideIndexStatus`）。
@MainActor
final class GlideIndexBuilder: ObservableObject {
    static let shared = GlideIndexBuilder()

    enum Phase: Equatable {
        case idle
        case building
        /// 直近の生成が失敗した理由（既存の索引は残る）
        case failed(String)
    }

    @Published private(set) var phase: Phase = .idle
    @Published private(set) var state: GlideIndexStatus.State = .missing
    @Published private(set) var indexBytes: Int64 = 0

    /// 生成中に辞書が更新された（終わったら判定し直す）
    private var recheckAfterBuild = false

    private init() { refresh() }

    /// 索引を置く data ディレクトリ（App Group があるときだけ。ないと拡張と共有できない）
    var dataDir: URL? {
        SharedStore.shared.containerURL?.appendingPathComponent("data", isDirectory: true)
    }

    /// ファイルの状態を読み直す（軽い。サイズと更新時刻を見るだけ）
    func refresh() {
        guard let dataDir else {
            state = .noDict
            indexBytes = 0
            return
        }
        state = GlideIndexStatus.state(dataDir: dataDir)
        indexBytes = GlideIndexStatus.fileSize(GlideIndexStatus.indexURL(dataDir: dataDir)) ?? 0
    }

    /// `input.glide` がオンで、索引が無い・古いときに作る（自動。オフなら何もしない）
    func ensureBuiltIfWanted() {
        guard GlideIndexPreference.isGlideSettingOn() else { return }
        refresh()
        if state == .missing || state == .stale { start() }
    }

    /// 辞書を取り込んだ・更新したあとに呼ぶ。索引があれば古くなるので、オンなら作り直す
    func dictionaryChanged() {
        if phase == .building {
            recheckAfterBuild = true
            return
        }
        ensureBuiltIfWanted()
        refresh()
    }

    /// 手動（ボタン）。状態にかかわらず作り直す
    func rebuild() {
        start()
    }

    private func start() {
        guard phase != .building else {
            recheckAfterBuild = true
            return
        }
        guard let dataDir else { return }
        let dictURL = GlideIndexStatus.dictURL(dataDir: dataDir)
        // 指紋は生成の前に取る（生成中に辞書が差し替わっても、次の判定で古い索引とわかる）
        guard let fingerprint = GlideIndexStatus.dictFingerprint(dataDir: dataDir) else {
            state = .noDict
            phase = .failed("辞書がありません。先に辞書を取り込んでください")
            return
        }
        phase = .building
        recheckAfterBuild = false
        let outURL = GlideIndexStatus.indexURL(dataDir: dataDir)
        Task.detached(priority: .utility) { [weak self] in
            // 生成の途中でアプリが閉じられても大丈夫なように、終わるまで OS にバックグラウンド実行を頼む
            let background = BackgroundTaskToken(name: "nagi.glide-index")
            defer { background.end() }
            var failure = Self.build(dict: dictURL.path, out: outURL.path)
            if failure == nil {
                do {
                    try GlideIndexStatus.writeMeta(dataDir: dataDir, dictFingerprint: fingerprint)
                } catch {
                    failure = "付属ファイルを書けません（\(error.localizedDescription)）"
                }
            }
            await self?.finish(failure: failure)
        }
    }

    private func finish(failure: String?) {
        if let failure {
            phase = .failed(failure)
        } else {
            phase = .idle
        }
        refresh()
        if recheckAfterBuild {
            recheckAfterBuild = false
            // 生成中に辞書が更新されていた。指紋が違えば（stale）作り直す
            if failure == nil, state == .stale { start() }
        }
    }

    /// コアの FFI で索引を作る。失敗の理由、成功なら nil。重い（約 2 秒・メモリのピークは約 180MB）ので
    /// メインスレッドで呼ばない
    nonisolated static func build(dict: String, out: String) -> String? {
        var failure: String?
        dict.withCString { d in
            out.withCString { o in
                if let ptr = nagi_build_glide_index(d, o) {
                    failure = String(cString: ptr)
                    nagi_string_free(ptr)
                }
            }
        }
        return failure
    }
}

/// `UIApplication.beginBackgroundTask` の小さな入れ物（生成中にアプリがバックグラウンドに移っても、少しの間は続けさせる）
final class BackgroundTaskToken: @unchecked Sendable {
    private var identifier: UIBackgroundTaskIdentifier = .invalid
    private let lock = NSLock()

    init(name: String) {
        // UIApplication はメインスレッドで触る
        DispatchQueue.main.sync {
            self.identifier = UIApplication.shared.beginBackgroundTask(withName: name) { [weak self] in
                self?.end()
            }
        }
    }

    func end() {
        lock.lock()
        let id = identifier
        identifier = .invalid
        lock.unlock()
        guard id != .invalid else { return }
        DispatchQueue.main.async { UIApplication.shared.endBackgroundTask(id) }
    }
}
