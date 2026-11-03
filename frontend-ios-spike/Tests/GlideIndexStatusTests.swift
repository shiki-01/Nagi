import XCTest

/// グライド入力の索引 `nagi-glide.bin` の配置と、辞書との整合の判定（`GlideIndexStatus`）
final class GlideIndexStatusTests: XCTestCase {
    private func tempDataDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("nagi-glide-index-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(
            at: dir.appendingPathComponent("dict", isDirectory: true), withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func writeDict(_ dataDir: URL, _ bytes: [UInt8], modified: Date? = nil) throws {
        let url = GlideIndexStatus.dictURL(dataDir: dataDir)
        try Data(bytes).write(to: url)
        if let modified {
            try FileManager.default.setAttributes([.modificationDate: modified], ofItemAtPath: url.path)
        }
    }

    /// 索引を作ったことにする（生成の前に取った指紋で付属ファイルを書く）
    private func installIndex(_ dataDir: URL, bytes: [UInt8] = [1, 2, 3, 4]) throws {
        let fingerprint = try XCTUnwrap(GlideIndexStatus.dictFingerprint(dataDir: dataDir))
        try Data(bytes).write(to: GlideIndexStatus.indexURL(dataDir: dataDir))
        try GlideIndexStatus.writeMeta(dataDir: dataDir, dictFingerprint: fingerprint)
    }

    func testPaths() {
        let dataDir = URL(fileURLWithPath: "/tmp/data", isDirectory: true)
        XCTAssertEqual(GlideIndexStatus.indexURL(dataDir: dataDir).path, "/tmp/data/dict/nagi-glide.bin")
        XCTAssertEqual(GlideIndexStatus.dictURL(dataDir: dataDir).path, "/tmp/data/dict/nagi-dict.bin")
        XCTAssertEqual(GlideIndexStatus.metaURL(dataDir: dataDir).path, "/tmp/data/dict/nagi-glide.meta")
    }

    func testNoDictThenMissingThenReady() throws {
        let dataDir = try tempDataDir()
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .noDict)
        XCTAssertNil(GlideIndexStatus.dictFingerprint(dataDir: dataDir))
        try writeDict(dataDir, [9, 9, 9])
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .missing)
        XCTAssertFalse(GlideIndexStatus.indexExists(dataDir: dataDir))
        try installIndex(dataDir)
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .ready)
        XCTAssertTrue(GlideIndexStatus.indexExists(dataDir: dataDir))
    }

    func testDictUpdateMakesIndexStale() throws {
        let dataDir = try tempDataDir()
        let t0 = Date(timeIntervalSince1970: 1_700_000_000)
        try writeDict(dataDir, [1, 2, 3], modified: t0)
        try installIndex(dataDir)
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .ready)
        // サイズが同じでも、更新時刻が変われば古い
        try writeDict(dataDir, [4, 5, 6], modified: t0.addingTimeInterval(60))
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .stale)
        // 更新時刻が同じでも、サイズが変われば古い
        try writeDict(dataDir, [4, 5, 6, 7], modified: t0)
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .stale)
        // 索引を作り直せば最新（索引ファイルは拡張が使い続けられる）
        XCTAssertTrue(GlideIndexStatus.indexExists(dataDir: dataDir))
        try installIndex(dataDir)
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .ready)
    }

    func testFingerprintTakenBeforeBuildDetectsDictReplacedDuringBuild() throws {
        let dataDir = try tempDataDir()
        let t0 = Date(timeIntervalSince1970: 1_700_000_000)
        try writeDict(dataDir, [1, 2, 3], modified: t0)
        let before = try XCTUnwrap(GlideIndexStatus.dictFingerprint(dataDir: dataDir))
        // 生成中に辞書が差し替わった
        try writeDict(dataDir, [1, 2, 3, 4], modified: t0.addingTimeInterval(10))
        try Data([1, 2, 3, 4]).write(to: GlideIndexStatus.indexURL(dataDir: dataDir))
        try GlideIndexStatus.writeMeta(dataDir: dataDir, dictFingerprint: before)
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .stale)
    }

    func testMissingOrBrokenMetaIsStale() throws {
        let dataDir = try tempDataDir()
        try writeDict(dataDir, [1, 2, 3])
        try Data([1, 2, 3, 4]).write(to: GlideIndexStatus.indexURL(dataDir: dataDir))
        // 付属ファイルが無い（書く前にアプリが終了した）
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .stale)
        // 壊れている
        try Data("garbage".utf8).write(to: GlideIndexStatus.metaURL(dataDir: dataDir))
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .stale)
        // 索引のサイズが記録と違う（書きかけ・差し替え）
        try installIndex(dataDir)
        try Data([1, 2]).write(to: GlideIndexStatus.indexURL(dataDir: dataDir))
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .stale)
    }

    func testEmptyIndexFileIsMissing() throws {
        let dataDir = try tempDataDir()
        try writeDict(dataDir, [1])
        try Data().write(to: GlideIndexStatus.indexURL(dataDir: dataDir))
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .missing)
        XCTAssertFalse(GlideIndexStatus.indexExists(dataDir: dataDir))
    }

    func testMetaRoundTripAndVersionCheck() {
        let text = GlideIndexStatus.metaText(dictFingerprint: "123:456", indexSize: 39_166_255)
        let parsed = GlideIndexStatus.parseMeta(text)
        XCTAssertEqual(parsed?.dict, "123:456")
        XCTAssertEqual(parsed?.indexSize, 39_166_255)
        XCTAssertNil(GlideIndexStatus.parseMeta("version=99\ndict=1:2\nindex_size=3\n"))
        XCTAssertNil(GlideIndexStatus.parseMeta("version=1\nindex_size=3\n"))
        XCTAssertNil(GlideIndexStatus.parseMeta(""))
    }

    func testRemoveDeletesIndexAndMeta() throws {
        let dataDir = try tempDataDir()
        try writeDict(dataDir, [1])
        try installIndex(dataDir)
        GlideIndexStatus.remove(dataDir: dataDir)
        XCTAssertEqual(GlideIndexStatus.state(dataDir: dataDir), .missing)
        XCTAssertFalse(FileManager.default.fileExists(atPath: GlideIndexStatus.metaURL(dataDir: dataDir).path))
        // 辞書は消さない
        XCTAssertTrue(FileManager.default.fileExists(atPath: GlideIndexStatus.dictURL(dataDir: dataDir).path))
    }

    func testStatusTextCoversAllStates() {
        let texts = [GlideIndexStatus.State.noDict, .missing, .stale, .ready].map(GlideIndexStatus.statusText)
        XCTAssertEqual(Set(texts).count, 4)
    }
}
