import XCTest

/// グライド入力（なぞり入力）の純粋なロジック: ジェスチャ判定（なぞり / タップの分離）、点列の間引き・座標の収め方・上限、
/// 送る型への変換、対象レイアウトの判定、候補の「なぞり」表示
final class GlideGestureTests: XCTestCase {
    /// 幅 400 × 高さ 200 の領域に、1 行 10 キー（幅 40・高さ 50）を 4 行並べたキー矩形
    private func keys() -> [LayoutGeometry.KeyRect] {
        var result: [LayoutGeometry.KeyRect] = []
        for row in 0..<4 {
            for col in 0..<10 {
                result.append(LayoutGeometry.KeyRect(
                    id: "k\(row)\(col)", label: "a", x: Double(col) * 40, y: Double(row) * 50, w: 40, h: 50
                ))
            }
        }
        return result
    }

    private func tracker(minPath: Double = GlideGesture.minPathLength) -> GlideStrokeTracker {
        GlideStrokeTracker(keys: keys(), width: 400, height: 200, minPathLength: minPath)
    }

    // MARK: なぞり / タップの分離

    func testTapStaysATap() {
        var t = tracker()
        XCTAssertFalse(t.add(x: 20, y: 25, timeMs: 0))
        XCTAssertFalse(t.add(x: 22, y: 26, timeMs: 30))
        XCTAssertFalse(t.add(x: 21, y: 25, timeMs: 60))
        XCTAssertFalse(t.isGlide)
        XCTAssertNil(t.finish())
    }

    func testLongMoveWithinOneKeyIsNotGlide() {
        // 道のりが長くても 1 キーの中だけ（行ったり来たり）ならなぞりではない
        var t = tracker(minPath: 30)
        t.add(x: 5, y: 25, timeMs: 0)
        for i in 1...6 {
            t.add(x: i % 2 == 0 ? 5 : 35, y: 25, timeMs: Double(i) * 16)
        }
        XCTAssertGreaterThan(t.pathLength, 100)
        XCTAssertEqual(t.visitedKeyCount, 1)
        XCTAssertFalse(t.isGlide)
        XCTAssertNil(t.finish())
    }

    func testShortMoveAcrossTwoKeysIsNotGlide() {
        // 2 キーを通っても道のりがしきい値に届かない（隣のキーへ少しずれたタップ）はなぞりではない
        var t = tracker()
        t.add(x: 35, y: 25, timeMs: 0)
        t.add(x: 45, y: 25, timeMs: 16)
        XCTAssertEqual(t.visitedKeyCount, 2)
        XCTAssertLessThan(t.pathLength, GlideGesture.minPathLength)
        XCTAssertFalse(t.isGlide)
    }

    func testGlideStartsOnceWhenBothConditionsAreMet() {
        var t = tracker()
        XCTAssertFalse(t.add(x: 20, y: 25, timeMs: 0))
        XCTAssertFalse(t.add(x: 40, y: 25, timeMs: 16))   // 道のり 20 < 40
        XCTAssertFalse(t.isGlide)
        XCTAssertTrue(t.add(x: 70, y: 25, timeMs: 32))    // 道のり 50 ≥ 40 かつ 2 キー通過 → なぞり
        XCTAssertTrue(t.isGlide)
        XCTAssertFalse(t.add(x: 110, y: 25, timeMs: 48))  // 判定は 1 回だけ
        XCTAssertTrue(t.isGlide)
        XCTAssertEqual(t.visitedKeyCount, 3)
    }

    func testStrokeStartingOutsideLetterKeysIsNeverGlide() {
        // 始点がキーの上でない（⇧・⌫・スペースなど。キー矩形に無い）ストロークは、長く動かしてもなぞりにならない
        var t = GlideStrokeTracker(keys: Array(keys().prefix(10)), width: 400, height: 200)
        t.add(x: 20, y: 150, timeMs: 0)
        for i in 1...10 { t.add(x: 20 + Double(i) * 30, y: Double(150 - i * 10), timeMs: Double(i) * 16) }
        XCTAssertFalse(t.isGlide)
        XCTAssertNil(t.finish())
        XCTAssertTrue(t.samples.isEmpty)
    }

    func testGlideReturnsSamplesIncludingFirstAndLast() throws {
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 1000)
        t.add(x: 60, y: 25, timeMs: 1016)
        t.add(x: 100, y: 25, timeMs: 1032)
        t.add(x: 100.3, y: 25, timeMs: 1048)   // 直前の点に近すぎて間引かれる最後の点
        let samples = try XCTUnwrap(t.finish())
        XCTAssertEqual(samples.first, GlideSample(x: 20, y: 25, t: 0))
        // 間引かれた最後の点も、終了時には含める
        XCTAssertEqual(samples.last, GlideSample(x: 100.3, y: 25, t: 48))
    }

    // MARK: 点列の間引き・座標・時刻・上限

    func testCloseSamplesAreThinnedButPathLengthCounts() {
        var t = tracker()
        t.add(x: 10, y: 25, timeMs: 0)
        for i in 1...10 { t.add(x: 10 + Double(i) * 0.3, y: 25, timeMs: Double(i)) }
        // 直前に残した点から 1pt 未満の点は捨てる（10 → 11.2 → 12.4）
        XCTAssertEqual(t.samples.count, 3)
        XCTAssertEqual(t.samples[1].x, 11.2, accuracy: 0.0001)
        XCTAssertEqual(t.samples[2].x, 12.4, accuracy: 0.0001)
        XCTAssertEqual(t.pathLength, 3.0, accuracy: 0.0001)
    }

    func testCoordinatesAreClampedToTheKeyArea() throws {
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 0)
        t.add(x: -30, y: 25, timeMs: 16)      // 左へはみ出す → x = 0
        t.add(x: 150, y: 260, timeMs: 400)    // 下へはみ出す → y = 200（時間が空いているので飛びではない）
        for sample in t.trail { XCTAssertTrue((0...400).contains(sample.x) && (0...200).contains(sample.y)) }
        XCTAssertEqual(t.trail.map(\.x), [20, 0, 150])
        XCTAssertEqual(t.trail.map(\.y), [25, 25, 200])
    }

    func testTimeIsRelativeAndMonotonic() {
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 5000)
        t.add(x: 60, y: 25, timeMs: 5016)
        t.add(x: 100, y: 25, timeMs: 5010)  // 逆戻りは直前の時刻に揃える
        XCTAssertEqual(t.samples.map(\.t), [0, 16, 16])
    }

    func testNonFiniteInputIsIgnored() {
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 0)
        XCTAssertFalse(t.add(x: .nan, y: 25, timeMs: 16))
        XCTAssertFalse(t.add(x: 60, y: .infinity, timeMs: 32))
        XCTAssertFalse(t.add(x: 60, y: 25, timeMs: .nan))
        XCTAssertEqual(t.samples.count, 1)
        XCTAssertEqual(t.pathLength, 0)
    }

    func testPointCountIsCappedAndLastPointKept() throws {
        // 往復を繰り返して大量の点を作る（毎回 2pt 以上動く）。上限を超えない・最初と最後の点は残る
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 0)
        var x = 20.0
        var direction = 1.0
        for i in 1...20_000 {
            x += 3 * direction
            if x > 380 || x < 20 { direction = -direction; x += 6 * direction }
            t.add(x: x, y: 25, timeMs: Double(i) * 8)
        }
        XCTAssertTrue(t.isGlide)
        let samples = try XCTUnwrap(t.finish())
        XCTAssertLessThanOrEqual(samples.count, GlideGesture.maxPoints)
        XCTAssertGreaterThan(samples.count, GlideGesture.maxPoints / 4)
        XCTAssertEqual(samples.first?.t, 0)
        XCTAssertEqual(samples.last?.x ?? -1, x, accuracy: 0.0001)
        // 時刻は単調増加
        XCTAssertEqual(samples.map(\.t), samples.map(\.t).sorted())
    }

    func testTrailIsCappedAndSpaced() {
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 0)
        var x = 20.0
        var direction = 1.0
        for i in 1...2000 {
            x += 4 * direction
            if x > 380 || x < 20 { direction = -direction; x += 8 * direction }
            t.add(x: x, y: 25, timeMs: Double(i) * 8)
        }
        XCTAssertLessThanOrEqual(t.trail.count, GlideGesture.maxTrailPoints)
        XCTAssertEqual(t.trail.count, GlideGesture.maxTrailPoints)
    }

    // MARK: 指の飛び

    func testJumpBeforeGlideInvalidatesTheStroke() {
        // 2 本目の指に追従先が移った想定: 数 ms で 3 キー幅を超えて飛ぶ
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 0)
        t.add(x: 300, y: 25, timeMs: 5)
        XCTAssertTrue(t.isInvalid)
        t.add(x: 340, y: 25, timeMs: 21)
        XCTAssertFalse(t.isGlide)
        XCTAssertNil(t.finish())
    }

    func testSlowLongStepIsNotAJump() {
        // 間隔が空いていれば（イベントが間引かれた）長い一歩も有効
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 0)
        XCTAssertTrue(t.add(x: 160, y: 25, timeMs: 100))
        XCTAssertFalse(t.isInvalid)
        XCTAssertTrue(t.isGlide)
    }

    func testJumpAfterGlideDropsOnlyThePoint() {
        var t = tracker()
        t.add(x: 20, y: 25, timeMs: 0)
        t.add(x: 60, y: 25, timeMs: 16)
        t.add(x: 100, y: 25, timeMs: 32)
        XCTAssertTrue(t.isGlide)
        t.add(x: 395, y: 175, timeMs: 36)   // 飛びの点は取り込まない
        XCTAssertFalse(t.isInvalid)
        XCTAssertTrue(t.isGlide)
        XCTAssertEqual(t.samples.last?.x, 100)
    }

    // MARK: キーの当たり判定・送る型

    func testHitKeyPrefersNearestCenterAndAllowsSlack() {
        let ks = keys()
        XCTAssertEqual(GlideGesture.hitKey(x: 20, y: 25, keys: ks), 0)
        XCTAssertEqual(GlideGesture.hitKey(x: 61, y: 25, keys: ks), 1)
        // 隙間（キー矩形の外 2pt）でも最寄りのキー
        let spaced = [LayoutGeometry.KeyRect(id: "a", label: "a", x: 0, y: 0, w: 30, h: 30)]
        XCTAssertEqual(GlideGesture.hitKey(x: 32, y: 15, keys: spaced), 0)
        XCTAssertNil(GlideGesture.hitKey(x: 40, y: 15, keys: spaced))
        XCTAssertNil(GlideGesture.hitKey(x: 10, y: 10, keys: []))
    }

    func testPointsMatchProtocolType() {
        let points = GlideGesture.points([GlideSample(x: 1.5, y: 2.5, t: 0), GlideSample(x: 3, y: 4, t: 16)])
        XCTAssertEqual(points.count, 2)
        XCTAssertEqual(points[0].x, 1.5)
        XCTAssertEqual(points[1].y, 4)
        XCTAssertEqual(points[1].t, 16)
        // JSON のキーは docs/glide-input.md の {x, y, t}
        let json = try? JSONEncoder().encode(points[0])
        let object = json.flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Double]
        XCTAssertEqual(object, ["x": 1.5, "y": 2.5, "t": 0])
    }

    // MARK: 対象レイアウト

    private func letterKey(_ text: String) -> LayoutKeyDef {
        var key = LayoutKeyDef(id: text, kind: .text, label: nil, action: nil, width: 1)
        key.text = text
        return key
    }

    private func layout(mode: LayoutMode, letters: String) -> KeyboardLayoutDef {
        KeyboardLayoutDef(id: "t", name: "t", mode: mode, rows: [
            LayoutRow(height: 1, keys: letters.map { letterKey(String($0)) }),
        ])
    }

    func testOnlyKanaQwertyLayoutsSupportGlide() {
        let alphabet = "abcdefghijklmnopqrstuvwxyz"
        XCTAssertTrue(LayoutGeometry.supportsGlide(layout: layout(mode: .kana, letters: alphabet)))
        // 英字（qwerty-alpha）・数字記号では無効
        XCTAssertFalse(LayoutGeometry.supportsGlide(layout: layout(mode: .alpha, letters: alphabet)))
        XCTAssertFalse(LayoutGeometry.supportsGlide(layout: layout(mode: .number, letters: alphabet)))
        // 英字キーが 20 個に満たない（12 キーのかななど）
        XCTAssertFalse(LayoutGeometry.supportsGlide(layout: layout(mode: .kana, letters: "abcdefghij")))
        XCTAssertTrue(LayoutGeometry.supportsGlide(layout: layout(mode: .kana, letters: String(alphabet.prefix(20)))))
        XCTAssertFalse(LayoutGeometry.supportsGlide(layout: layout(mode: .kana, letters: String(alphabet.prefix(19)))))
    }

    /// 上段 q〜p が `flick`（中央が英字、上フリックが数字）、2〜3 段目が `text` のユーザーのレイアウト
    private func flickTopLayout(mode: LayoutMode = .kana) -> KeyboardLayoutDef {
        let digits = Array("1234567890")
        let top = "qwertyuiop".enumerated().map { index, ch -> LayoutKeyDef in
            var key = LayoutKeyDef(id: String(ch), kind: .flick, label: nil, action: nil, width: 1)
            key.center = FlickChar(text: String(ch))
            key.up = FlickChar(text: String(digits[index]))
            return key
        }
        return KeyboardLayoutDef(id: "t", name: "t", mode: mode, rows: [
            LayoutRow(height: 1, keys: top),
            LayoutRow(height: 1, keys: "asdfghjkl".map { letterKey(String($0)) }),
            LayoutRow(height: 1, keys: "zxcvbnm".map { letterKey(String($0)) }),
        ])
    }

    func testFlickTopRowQwertyLayoutSupportsGlide() {
        let layout = flickTopLayout()
        XCTAssertTrue(LayoutGeometry.supportsGlide(layout: layout))
        XCTAssertFalse(LayoutGeometry.supportsGlide(layout: flickTopLayout(mode: .alpha)))
        let rects = LayoutGeometry.rects(layout: layout, size: CGSize(width: 400, height: 150), keySpacing: 4, rowSpacing: 4)
        XCTAssertEqual(rects.count, 26)
        XCTAssertEqual(Set(rects.map(\.label)), Set("abcdefghijklmnopqrstuvwxyz".map { String($0) }))
        let snapshot = LayoutGeometry.snapshot(layout: layout, size: CGSize(width: 400, height: 150), keySpacing: 4, rowSpacing: 4)
        XCTAssertEqual(snapshot?.keys.count, 26)
    }

    func testNonLetterFlickKeysAreNotCounted() {
        // 12 キーのかな（中央が「あ」など）や、英字 1 文字でない flick は英字キーに数えない
        var kana = LayoutKeyDef(id: "a", kind: .flick, label: nil, action: nil, width: 1)
        kana.center = FlickChar(text: "あ")
        XCTAssertNil(LayoutGeometry.letterLabel(kana))
        var abc = LayoutKeyDef(id: "abc", kind: .flick, label: "abc", action: nil, width: 1)
        abc.center = FlickChar(text: "abc")
        XCTAssertNil(LayoutGeometry.letterLabel(abc))
        let twelve = KeyboardLayoutDef(id: "t", name: "t", mode: .kana, rows: [
            LayoutRow(height: 1, keys: Array(repeating: kana, count: 12)),
        ])
        XCTAssertFalse(LayoutGeometry.supportsGlide(layout: twelve))
        XCTAssertNil(LayoutGeometry.snapshot(layout: twelve, size: CGSize(width: 400, height: 150), keySpacing: 4, rowSpacing: 4))
    }

    // MARK: 候補の「なぞり」表示

    func testGlideHint() {
        XCTAssertNil(glideHint(sources: [.systemDict], surface: "感じ", reading: "かんじ"))
        XCTAssertNil(glideHint(sources: [.fuzzy], surface: "感じ", reading: "かんじ"))
        XCTAssertEqual(glideHint(sources: [.glide], surface: "感じ", reading: "かんじ"), "なぞり: かんじ")
        XCTAssertEqual(glideHint(sources: [.glide, .llm], surface: "感じ", reading: "かんじ"), "なぞり: かんじ")
        // 読みが無い・表記と同じ（ひらがなのまま）なら出所だけ
        XCTAssertEqual(glideHint(sources: [.glide], surface: "かんじ", reading: "かんじ"), "なぞり")
        XCTAssertEqual(glideHint(sources: [.glide], surface: "感じ", reading: nil), "なぞり")
        XCTAssertEqual(glideHint(sources: [.glide], surface: "感じ", reading: ""), "なぞり")
    }

    func testGlideCandidatesAreConversionAndNeverPinned() {
        let items = [
            CandidateView(surface: "感じ", sources: [.glide], covers: nil, reading: "かんじ"),
            CandidateView(surface: "幹事", sources: [.glide], covers: nil, reading: "かんじ"),
        ]
        let view = CandidatesView(items: items, selected: 0, page_size: 9, visible: true, unselected: nil)
        XCTAssertEqual(classifyCandidates(view, hasPreedit: true), .conversion)
        // 右端に固定するのは曖昧入力（fuzzy）の候補だけ。グライドの候補は一覧に並ぶ
        let hints = items.map { fuzzyHint(sources: $0.sources, reading: $0.reading) }
        XCTAssertNil(pinnedFuzzyIndex(hints: hints, nextWord: false))
    }
}
