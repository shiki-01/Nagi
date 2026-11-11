// IME の UI E2E ドライバー（macOS 専用。他 OS では空の実行ファイル）。
// 設計と実行方法: docs/ui-testing.md、シナリオ仕様: tests/e2e/README.md、実行スクリプト: tests/e2e/macos/run.sh
#if os(macOS)
import Foundation

setvbuf(stdout, nil, _IOLBF, 0)

guard let config = Config.parse(Array(CommandLine.arguments.dropFirst())) else {
    FileHandle.standardError.write(Data((Config.usage + "\n").utf8))
    exit(ExitCode.usage)
}
exit(Runner(config: config).run())
#endif
