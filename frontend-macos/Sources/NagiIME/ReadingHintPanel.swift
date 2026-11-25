#if os(macOS)
import AppKit

/// 変換された文節の読みを未確定文字の下に小さく出すパネル（設定 `input.show_reading`）。
/// 背景は StatusToast（HUD）、文字は CandidatePanel の補助表示に揃える。
/// フォーカスを奪わず、マウス操作も受けない。メインスレッドからのみ使う
final class ReadingHintPanel: NSPanel {
    private let label = NSTextField(labelWithString: "")
    private static let font = NSFont.systemFont(ofSize: 12)
    private static let horizontalPadding: CGFloat = 8
    private static let verticalPadding: CGFloat = 3

    init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 100, height: 100),
            styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        isFloatingPanel = true
        level = .popUpMenu
        hidesOnDeactivate = false
        hasShadow = true
        isOpaque = false
        backgroundColor = .clear
        ignoresMouseEvents = true

        let background = NSVisualEffectView()
        background.material = .hudWindow
        background.state = .active
        background.wantsLayer = true
        background.layer?.cornerRadius = 6
        background.layer?.masksToBounds = true
        label.font = Self.font
        label.textColor = .secondaryLabelColor
        label.lineBreakMode = .byClipping
        background.addSubview(label)
        contentView = background
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    /// `text` を `caret`（スクリーン座標のカーソル行矩形）の直下に出す
    func show(text: String, near caret: NSRect) {
        let textSize = (text as NSString).size(withAttributes: [.font: Self.font])
        let textWidth = ceil(textSize.width)
        let textHeight = ceil(textSize.height)
        let size = NSSize(
            width: textWidth + Self.horizontalPadding * 2,
            height: textHeight + Self.verticalPadding * 2)
        label.stringValue = text
        label.frame = NSRect(
            x: Self.horizontalPadding, y: Self.verticalPadding, width: textWidth, height: textHeight)
        setContentSize(size)
        setFrameOrigin(PanelPlacement.origin(for: size, below: PanelPlacement.anchor(for: caret)))
        orderFrontRegardless()
    }

    func hide() {
        orderOut(nil)
    }
}
#endif
