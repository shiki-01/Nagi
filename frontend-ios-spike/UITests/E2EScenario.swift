import Foundation

// 共通シナリオ（tests/e2e/scenarios/*.json。スキーマは tests/e2e/README.md）のモデルと読み込み。
// Foundation だけに依存する。

struct E2ESuite: Decodable {
    let suite: String
    /// 省略時は全プラットフォーム
    let platforms: [String]?
    let scenarios: [E2EScenario]

    func runsOn(_ platform: String) -> Bool {
        platforms?.contains(platform) ?? true
    }
}

struct E2EScenario: Decodable {
    let name: String
    let steps: [E2EStep]
}

/// steps の要素。次のいずれか 1 つだけが入る
struct E2EStep: Decodable {
    struct Flick: Decodable {
        let key: String
        let dir: String
    }

    struct Expect: Decodable {
        let marked: String?
        let committed: String?
        let text: String?
        let candidates_contain: [String]?
        let timeout_ms: Int?
    }

    let type: String?
    let key: String?
    let tap: [String]?
    let flick: Flick?
    let expect: Expect?
    let screenshot: String?

    /// 失敗メッセージ用の短い説明
    var summary: String {
        if let type = type { return "type \"\(type)\"" }
        if let key = key { return "key \(key)" }
        if let tap = tap { return "tap \(tap)" }
        if let flick = flick { return "flick \(flick.key) \(flick.dir)" }
        if expect != nil { return "expect" }
        if let screenshot = screenshot { return "screenshot \(screenshot)" }
        return "(empty step)"
    }
}

enum E2EScenarioLoader {
    /// テストバンドルの `scenarios/` フォルダから、`platform` で動くスイートを名前順に読む。
    /// フォルダが無い・JSON が読めないときは理由つきで throw する（黙って 0 件にしない）
    static func load(bundle: Bundle, platform: String) throws -> [E2ESuite] {
        guard let dir = bundle.url(forResource: "scenarios", withExtension: nil) else {
            throw E2EError.setup("テストバンドルに scenarios/ フォルダがありません（tests/e2e/scenarios が無いか、project.yml の同梱設定が効いていません）")
        }
        let files = try FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "json" }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
        var suites: [E2ESuite] = []
        for file in files {
            do {
                let suite = try JSONDecoder().decode(E2ESuite.self, from: Data(contentsOf: file))
                if suite.runsOn(platform) { suites.append(suite) }
            } catch {
                throw E2EError.setup("シナリオを読めません: \(file.lastPathComponent): \(error)")
            }
        }
        return suites
    }
}

enum E2EError: Error, CustomStringConvertible {
    /// 準備（シナリオの読み込み・キーボードの有効化など）の失敗
    case setup(String)
    /// ステップの失敗（期待値との不一致・要素が見つからないなど）
    case step(String)

    var description: String {
        switch self {
        case .setup(let message), .step(let message): return message
        }
    }
}
