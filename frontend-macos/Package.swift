// swift-tools-version:5.9
import PackageDescription

// Xcode プロジェクトは使わず SwiftPM のみで構成する。
// NagiProtocol / NagiClient は Foundation のみに依存し、Linux でもビルド・テストできる。
let package = Package(
    name: "NagiFrontend",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "NagiClient", targets: ["NagiClient"]),
        .library(name: "NagiIME", targets: ["NagiIME"]),
        .executable(name: "NagiIMEApp", targets: ["NagiIMEApp"]),
    ],
    targets: [
        // typeshare 生成物（core/protocol の Rust 型から生成。編集禁止）
        .target(
            name: "NagiProtocol",
            path: "Sources/Generated"
        ),
        .target(
            name: "NagiClient",
            dependencies: ["NagiProtocol"],
            path: "Sources/NagiClient"
        ),
        // IMKit 層。AppKit / IMKit に触れるファイルは `#if os(macOS)` で囲む。
        // キー正規化・preedit 装飾・候補ページング等の純粋ロジックは Linux でもビルド・テストできる
        .target(
            name: "NagiIME",
            dependencies: ["NagiClient"],
            path: "Sources/NagiIME"
        ),
        // 入力メソッドアプリの main（macOS 以外では空の実行ファイル）
        .executableTarget(
            name: "NagiIMEApp",
            dependencies: ["NagiIME"],
            path: "Sources/NagiIMEApp"
        ),
        // E2E テスト用（`docs/ui-testing.md` / `tests/e2e/`）。macOS 以外では空の実行ファイル。
        // NagiTestHost: NSTextInputClient を実装したテスト専用ホスト。確定/未確定文字列を状態ファイルへ書き出す
        .executableTarget(
            name: "NagiTestHost",
            path: "Sources/NagiTestHost"
        ),
        // nagi-e2e: シナリオ JSON を読み、CGEvent でキーを送出して NagiTestHost の状態を検証するドライバー
        .executableTarget(
            name: "nagi-e2e",
            path: "Sources/nagi-e2e"
        ),
        .testTarget(
            name: "NagiIMETests",
            dependencies: ["NagiIME", "NagiClient"],
            path: "Tests/NagiIMETests"
        ),
        .testTarget(
            name: "NagiClientTests",
            dependencies: ["NagiClient"],
            path: "Tests/NagiClientTests"
        ),
    ]
)
