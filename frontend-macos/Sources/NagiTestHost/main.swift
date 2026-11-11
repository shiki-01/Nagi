// IME の E2E テスト専用ホスト。macOS 以外では空の実行ファイル。
//
// 使い方: NagiTestHost --state-file <path>
//   - 確定文字列(committed)と未確定文字列(marked)が変わるたびに {"marked":..., "committed":...} を原子的に書き出す
//   - SIGUSR1 を受けると IME のコンポジションを破棄して内容を空にし、clear_count を 1 増やして書き出す
#if os(macOS)
import AppKit

var statePathArgument: String?
let arguments = Array(CommandLine.arguments.dropFirst())
var index = 0
while index < arguments.count {
    if arguments[index] == "--state-file", index + 1 < arguments.count {
        statePathArgument = arguments[index + 1]
        index += 1
    }
    index += 1
}

guard let statePath = statePathArgument else {
    FileHandle.standardError.write(Data("usage: NagiTestHost --state-file <path>\n".utf8))
    exit(2)
}

let app = NSApplication.shared
let controller = TestHostController(statePath: statePath)
app.delegate = controller
app.setActivationPolicy(.regular)
app.run()
#endif
