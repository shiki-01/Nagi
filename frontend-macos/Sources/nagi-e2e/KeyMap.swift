import Foundation

/// 1 回のキー押下（修飾付き）。keyCode は macOS の仮想キーコード（kVK_*、US 配列）
struct KeyChord: Equatable {
    var keyCode: UInt16
    var shift = false
    var control = false
    var option = false
    var command = false
}

/// US 配列の仮想キーコード対応表と、シナリオのキー指定の解釈。Foundation のみ（macOS 以外でもコンパイルできる）
enum KeyMap {
    // 修飾キー自体の仮想キーコード
    static let shiftKey: UInt16 = 56
    static let controlKey: UInt16 = 59
    static let optionKey: UInt16 = 58
    static let commandKey: UInt16 = 55
    static let escapeKey: UInt16 = 53

    /// 文字（Shift なし）→ キーコード
    static let unshifted: [Character: UInt16] = {
        let pairs: [(Character, UInt16)] = [
            ("a", 0), ("s", 1), ("d", 2), ("f", 3), ("h", 4), ("g", 5), ("z", 6), ("x", 7),
            ("c", 8), ("v", 9), ("b", 11), ("q", 12), ("w", 13), ("e", 14), ("r", 15), ("y", 16),
            ("t", 17), ("o", 31), ("u", 32), ("i", 34), ("p", 35), ("l", 37), ("j", 38),
            ("k", 40), ("n", 45), ("m", 46),
            ("1", 18), ("2", 19), ("3", 20), ("4", 21), ("6", 22), ("5", 23), ("9", 25),
            ("7", 26), ("8", 28), ("0", 29),
            ("=", 24), ("-", 27), ("]", 30), ("[", 33), ("'", 39), (";", 41), ("\\", 42),
            (",", 43), ("/", 44), (".", 47), ("`", 50), (" ", 49),
        ]
        var table: [Character: UInt16] = [:]
        for (character, code) in pairs { table[character] = code }
        return table
    }()

    /// Shift で入力する記号 → Shift なしの文字
    static let shiftedSymbols: [Character: Character] = {
        let pairs: [(Character, Character)] = [
            ("!", "1"), ("@", "2"), ("#", "3"), ("$", "4"), ("%", "5"), ("^", "6"), ("&", "7"),
            ("*", "8"), ("(", "9"), (")", "0"), ("_", "-"), ("+", "="), ("{", "["), ("}", "]"),
            ("|", "\\"), (":", ";"), ("\"", "'"), ("<", ","), (">", "."), ("?", "/"), ("~", "`"),
        ]
        var table: [Character: Character] = [:]
        for (shifted, base) in pairs { table[shifted] = base }
        return table
    }()

    /// 特殊キー名（小文字）→ キーコード
    static let named: [String: UInt16] = [
        "return": 36, "enter": 36, "tab": 48, "space": 49, "backspace": 51, "escape": 53,
        "esc": 53, "delete": 117, "left": 123, "right": 124, "down": 125, "up": 126,
        "home": 115, "end": 119, "pageup": 116, "pagedown": 121,
    ]

    /// `{ "type": "..." }` 用: ASCII 文字列を物理キー列にする
    static func chords(forTyping text: String) throws -> [KeyChord] {
        try text.map { try chord(forCharacter: $0) }
    }

    static func chord(forCharacter character: Character) throws -> KeyChord {
        if let code = unshifted[character] { return KeyChord(keyCode: code) }
        if let base = shiftedSymbols[character], let code = unshifted[base] {
            return KeyChord(keyCode: code, shift: true)
        }
        if character.isUppercase, character.isASCII,
            let code = unshifted[Character(character.lowercased())]
        {
            return KeyChord(keyCode: code, shift: true)
        }
        throw StepFailure(message: "type で送れない文字です（US 配列の ASCII のみ）: \"\(character)\"")
    }

    /// `{ "key": "Shift+Left" }` 用。`+` で連結し、最後の要素がキー本体
    static func chord(forKeySpec spec: String) throws -> KeyChord {
        var parts = spec.components(separatedBy: "+")
        // 末尾が "+" 自体（例: "Shift++"）の場合は空要素が 2 つ並ぶ
        let body: String
        if parts.count >= 2, parts.last == "", parts[parts.count - 2] == "" {
            parts.removeLast(2)
            body = "+"
        } else {
            body = parts.removeLast()
        }
        guard !body.isEmpty else { throw StepFailure(message: "キー指定が空です: \"\(spec)\"") }

        var result: KeyChord
        if let code = named[body.lowercased()] {
            result = KeyChord(keyCode: code)
        } else if body.count == 1, let character = body.first {
            result = try self.chord(forCharacter: character)
        } else {
            throw StepFailure(message: "未知のキー名です: \"\(body)\"（\"\(spec)\"）")
        }

        for modifier in parts {
            switch modifier.lowercased() {
            case "shift": result.shift = true
            case "ctrl", "control": result.control = true
            case "alt", "option", "opt": result.option = true
            case "cmd", "command", "meta": result.command = true
            default: throw StepFailure(message: "未知の修飾キーです: \"\(modifier)\"（\"\(spec)\"）")
            }
        }
        return result
    }
}
