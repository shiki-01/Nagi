import Foundation

/// UI テスト（XCUITest。`UITests/`）用の起動モード。起動引数 `-nagi-e2e` があるときだけ有効で、
/// 引数が無い通常の起動（TestFlight / App Store を含む）の挙動は変わらない。
/// 有効なときは、初回案内を出さずに「入力テスト」タブを最初に開く（入力欄は `KeyboardTestView` の `e2e.input`）。
/// あわせて、変換を決定的にするフィクスチャ（`tests/e2e/fixtures/user-dir/`）をキーボード拡張が読む場所へ置く
enum NagiE2EMode {
    static let launchArgument = "-nagi-e2e"

    static var isEnabled: Bool {
        ProcessInfo.processInfo.arguments.contains(launchArgument)
    }

    /// バンドルに同梱したフィクスチャ（project.yml で `tests/e2e/fixtures/user-dir` をフォルダ参照として入れる）
    private static var bundledFixtures: URL? {
        Bundle.main.url(forResource: "user-dir", withExtension: nil)
    }

    /// E2E 専用の設定。学習は止める（前のテストの結果で候補の順が変わらないように）。
    /// ライブ変換は iOS の既定のまま（シナリオ側がライブ変換の有無に依存しない書き方をする）
    private static let e2eConfig = """
        # nagi:e2e（UI テスト用。-nagi-e2e のときだけホストアプリが書く）
        [update]
        auto_check = false

        [learning]
        enabled = false

        [input.fuzzy]
        enabled = true
        """

    /// フィクスチャを App Group 内のユーザー領域（`<App Group>/user`。キーボード拡張と共通）へ置く:
    /// `user_dict/*.tsv|txt` をコピーし、`config.toml` を E2E 用に書き換える。`-nagi-e2e` のときだけ呼ぶこと
    static func installFixtures() {
        guard isEnabled else { return }
        let located = NagiUserDictStore.locate()
        guard let userDict = located.userDict, let userDir = located.userDir else {
            NSLog("[nagi-e2e] ユーザー領域を用意できません: %@", located.message ?? "")
            return
        }
        let fileManager = FileManager.default
        do {
            try fileManager.createDirectory(at: userDict, withIntermediateDirectories: true)
            // 前回の実行の辞書を持ち越さない（固定語だけにする）
            for url in (try? fileManager.contentsOfDirectory(at: userDict, includingPropertiesForKeys: nil)) ?? [] {
                try? fileManager.removeItem(at: url)
            }
            if let source = bundledFixtures?.appendingPathComponent("user_dict", isDirectory: true) {
                for file in try fileManager.contentsOfDirectory(at: source, includingPropertiesForKeys: nil) {
                    let ext = file.pathExtension.lowercased()
                    guard ext == "tsv" || ext == "txt" else { continue }
                    try fileManager.copyItem(at: file, to: userDict.appendingPathComponent(file.lastPathComponent))
                }
            } else {
                NSLog("[nagi-e2e] バンドルにフィクスチャ（user-dir）がありません")
            }
            // config.toml は内容が変わるので更新時刻も進み、拡張が focusIn で reload_config を送る
            try e2eConfig.write(to: userDir.appendingPathComponent("config.toml"), atomically: true, encoding: .utf8)
        } catch {
            NSLog("[nagi-e2e] フィクスチャの配置に失敗: %@", "\(error)")
        }
    }
}
