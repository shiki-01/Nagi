import Foundation
import NagiClient
import XCTest

@testable import NagiIME

final class ReadingHintTests: XCTestCase {
    private func seg(_ surface: String, _ reading: String, _ kind: SegmentKind, _ start: UInt32)
        -> SegmentView
    {
        SegmentView(
            surface: surface, reading: reading, kind: kind,
            range: RangeU16(start: start, end: start + UInt32(surface.utf16.count)))
    }

    private func preedit(_ segments: [SegmentView]) -> PreeditView {
        let text = segments.map(\.surface).joined()
        return PreeditView(
            text: text, segments: segments, cursor: UInt32(text.utf16.count),
            pending: nil, focus: nil, selection: nil)
    }

    func testConvertedSegmentsJoinedInOrder() {
        let p = preedit([
            seg("今日は", "きょうは", .japanese, 0),
            seg("いい天気", "いいてんき", .japanese, 3),
        ])
        XCTAssertEqual(ReadingHint.text(for: p), "きょうは｜いいてんき")
    }

    func testKanaSegmentsAreSkipped() {
        let p = preedit([
            seg("きょう", "きょう", .japanese, 0),
            seg("天気", "てんき", .japanese, 4),
        ])
        XCTAssertEqual(ReadingHint.text(for: p), "てんき")
    }

    func testKatakanaConversionIsShown() {
        let p = preedit([seg("キョウ", "きょう", .japanese, 0)])
        XCTAssertEqual(ReadingHint.text(for: p), "きょう")
    }

    func testNothingWhenAllKanaOrEmpty() {
        XCTAssertNil(ReadingHint.text(for: preedit([])))
        XCTAssertNil(ReadingHint.text(for: preedit([seg("きょう", "きょう", .japanese, 0)])))
    }

    func testLiteralFixedAndStyledAreExcluded() {
        let p = preedit([
            seg("ABC", "abc", .literal, 0),
            seg("部分", "ぶぶん", .fixed, 3),
            seg("変換", "へんかん", .styled, 5),
        ])
        XCTAssertNil(ReadingHint.text(for: p))
    }

    func testIsEnabledReadsBoolSetting() {
        func snapshot(_ value: SettingValue?) -> SettingsSnapshot {
            SettingsSnapshot(
                items: [
                    SettingItem(
                        key: ReadingHint.settingKey, group: "入力", label: "", description: "",
                        kind: .bool, value: value, default: .bool(false), explicit: false,
                        requires_restart: false, level: .detail, platforms: .desktop)
                ],
                error: nil)
        }
        XCTAssertTrue(ReadingHint.isEnabled(in: snapshot(.bool(true))))
        XCTAssertFalse(ReadingHint.isEnabled(in: snapshot(.bool(false))))
        XCTAssertFalse(ReadingHint.isEnabled(in: snapshot(nil)))
        XCTAssertFalse(ReadingHint.isEnabled(in: SettingsSnapshot(items: [], error: nil)))
    }
}
