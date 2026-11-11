#if os(macOS)
import AppKit

/// NSTextInputClient を実装した単純なテキスト欄。
/// 内容は「確定済み文字列（committed）」の末尾に「未確定文字列（marked）」が続く、というモデルだけを持つ。
/// カーソル移動・範囲選択・途中挿入は扱わない（IME の E2E に必要な範囲に限る）。
final class TextInputView: NSView, NSTextInputClient {
    private(set) var committed = ""
    private(set) var marked = ""
    /// marked 内での選択範囲（setMarkedText の selectedRange）
    private var markedSelection = NSRange(location: 0, length: 0)

    /// 内容が変わるたびに呼ぶ（メインスレッド）
    var onChange: (() -> Void)?

    override var acceptsFirstResponder: Bool { true }
    override func becomeFirstResponder() -> Bool { true }

    // MARK: - 描画（スクリーンショットでの目視確認用）

    override func draw(_ dirtyRect: NSRect) {
        NSColor.white.setFill()
        bounds.fill()
        let font = NSFont.systemFont(ofSize: 22)
        let text = NSMutableAttributedString(
            string: committed,
            attributes: [.font: font, .foregroundColor: NSColor.black])
        text.append(
            NSAttributedString(
                string: marked,
                attributes: [
                    .font: font, .foregroundColor: NSColor.black,
                    .underlineStyle: NSUnderlineStyle.single.rawValue,
                ]))
        text.draw(in: bounds.insetBy(dx: 8, dy: 8))
    }

    // MARK: - キー入力

    override func keyDown(with event: NSEvent) {
        // IME（入力コンテキスト）へ渡す。IME が消費しなければ insertText / doCommand(by:) が呼ばれる
        interpretKeyEvents([event])
    }

    override func doCommand(by selector: Selector) {
        // IME が処理しなかった場合の最小限の編集動作（確定済み文字列のみ）
        if selector == #selector(NSResponder.insertNewline(_:)) {
            committed += "\n"
        } else if selector == #selector(NSResponder.insertTab(_:)) {
            committed += "\t"
        } else if selector == #selector(NSResponder.deleteBackward(_:)) {
            if marked.isEmpty, !committed.isEmpty { committed.removeLast() }
        } else {
            return
        }
        changed()
    }

    /// テスト開始時のリセット。IME 側のコンポジションを破棄し、内容を空にする
    func reset() {
        inputContext?.discardMarkedText()
        committed = ""
        marked = ""
        markedSelection = NSRange(location: 0, length: 0)
        changed()
    }

    private func changed() {
        needsDisplay = true
        onChange?()
    }

    private func plain(_ value: Any) -> String {
        if let attributed = value as? NSAttributedString { return attributed.string }
        if let string = value as? String { return string }
        return String(describing: value)
    }

    private var committedLength: Int { (committed as NSString).length }

    /// 確定済み文字列内の有効な置換範囲か
    private func isValidCommittedRange(_ range: NSRange) -> Bool {
        range.location != NSNotFound && range.location >= 0
            && range.location + range.length <= committedLength
    }

    // MARK: - NSTextInputClient

    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = plain(string)
        if marked.isEmpty, isValidCommittedRange(replacementRange) {
            committed = (committed as NSString).replacingCharacters(in: replacementRange, with: text)
        } else {
            // マークドテキストがあればそれを置き換えて確定する
            committed += text
        }
        marked = ""
        markedSelection = NSRange(location: 0, length: 0)
        changed()
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        // 再変換（確定済みテキストの範囲を置換して未確定にする）。マークドは末尾に付く簡易モデルなので、
        // 範囲を確定済み文字列から取り除くだけに留める
        if marked.isEmpty, isValidCommittedRange(replacementRange), replacementRange.length > 0 {
            committed = (committed as NSString).replacingCharacters(in: replacementRange, with: "")
        }
        marked = plain(string)
        markedSelection = selectedRange.location == NSNotFound ? NSRange(location: 0, length: 0) : selectedRange
        changed()
    }

    func unmarkText() {
        // 未確定文字列をそのまま確定する（NSTextInputClient の規定の動作）
        if !marked.isEmpty {
            committed += marked
            marked = ""
            markedSelection = NSRange(location: 0, length: 0)
            changed()
        }
    }

    func selectedRange() -> NSRange {
        if marked.isEmpty { return NSRange(location: committedLength, length: 0) }
        return NSRange(location: committedLength + markedSelection.location, length: markedSelection.length)
    }

    func markedRange() -> NSRange {
        if marked.isEmpty { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: committedLength, length: (marked as NSString).length)
    }

    func hasMarkedText() -> Bool { !marked.isEmpty }

    func attributedSubstring(
        forProposedRange range: NSRange, actualRange: NSRangePointer?
    ) -> NSAttributedString? {
        let full = (committed + marked) as NSString
        guard range.location != NSNotFound, range.location <= full.length else { return nil }
        let clipped = NSIntersectionRange(range, NSRange(location: 0, length: full.length))
        actualRange?.pointee = clipped
        return NSAttributedString(string: full.substring(with: clipped))
    }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] {
        [.underlineStyle, .underlineColor, .foregroundColor, .backgroundColor, .markedClauseSegment]
    }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let window else { return .zero }
        actualRange?.pointee = range
        // 欄の左上付近（候補ウィンドウの表示位置の基準）
        let local = NSRect(x: 8, y: max(bounds.height - 36, 0), width: 2, height: 28)
        return window.convertToScreen(convert(local, to: nil))
    }

    func characterIndex(for point: NSPoint) -> Int { 0 }
}
#endif
