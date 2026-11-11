import Foundation

// 共通シナリオ仕様（tests/e2e/README.md）のモデル。Foundation のみ（macOS 以外でもコンパイルできる）

struct Suite: Decodable {
    let suite: String
    let platforms: [String]?
    let scenarios: [Scenario]

    /// `platforms` 省略時は全プラットフォーム
    func runs(on platform: String) -> Bool {
        guard let platforms else { return true }
        return platforms.contains(platform)
    }
}

struct Scenario: Decodable {
    let name: String
    let steps: [Step]
}

struct Step: Decodable {
    let type: String?
    let key: String?
    let tap: [String]?
    let flick: Flick?
    let expect: Expect?
    let screenshot: String?
}

struct Flick: Decodable {
    let key: String
    let dir: String
}

struct Expect: Decodable {
    let marked: String?
    let committed: String?
    let text: String?
    let candidatesContain: [String]?
    let timeoutMs: Int?

    enum CodingKeys: String, CodingKey {
        case marked, committed, text
        case candidatesContain = "candidates_contain"
        case timeoutMs = "timeout_ms"
    }
}

/// results.json の 1 要素
struct ScenarioResult: Encodable {
    let suite: String
    let name: String
    let ok: Bool
    let message: String
}

/// NagiTestHost が書き出す状態ファイル
struct HostState: Decodable {
    let marked: String
    let committed: String
    let clearCount: Int?
    let active: Bool?

    enum CodingKeys: String, CodingKey {
        case marked, committed, active
        case clearCount = "clear_count"
    }
}

/// ステップの失敗（シナリオはここで打ち切る）
struct StepFailure: Error {
    let message: String
}
