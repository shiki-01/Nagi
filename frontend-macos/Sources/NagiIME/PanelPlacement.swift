#if os(macOS)
import AppKit

/// 未確定文字の近くに出す小さなパネルの位置決め（候補パネル・読みヒントで共通）
enum PanelPlacement {
    /// `caret` はスクリーン座標のカーソル行矩形。取れないクライアント（`.zero`）ではマウス位置で代用する
    static func anchor(for caret: NSRect) -> NSRect {
        guard caret == .zero else { return caret }
        let p = NSEvent.mouseLocation
        return NSRect(x: p.x, y: p.y, width: 0, height: 0)
    }

    /// `anchor` の直下に `size` のパネルを置く原点。画面下端に収まらなければ上側に出し、左右は画面内に寄せる
    static func origin(for size: NSSize, below anchor: NSRect) -> NSPoint {
        var origin = NSPoint(x: anchor.minX, y: anchor.minY - size.height - 2)
        let screen =
            NSScreen.screens.first(where: { $0.frame.contains(anchor.origin) }) ?? NSScreen.main
        if let visible = screen?.visibleFrame {
            if origin.y < visible.minY { origin.y = anchor.maxY + 2 }
            origin.x = max(visible.minX, min(origin.x, visible.maxX - size.width))
        }
        return origin
    }
}
#endif
