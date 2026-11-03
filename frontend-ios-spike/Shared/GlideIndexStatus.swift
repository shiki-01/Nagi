import Foundation

/// グライド入力（なぞり入力）の索引 `nagi-glide.bin`（約 39MB）の配置と、辞書との整合の判定。
/// 索引は辞書から作る派生データで、ホストアプリが生成して App Group の `data/dict/` に置く（`GlideIndexBuilder`）。
/// キーボード拡張はこれを mmap で読むだけで、メモリ上には作らない（拡張のメモリ上限のため。decisions.md H-84）。
/// コア・ネットワークに依存しない純粋なロジック（生成そのものはコアの FFI `nagi_build_glide_index`）。
enum GlideIndexStatus {
    static let indexFileName = "nagi-glide.bin"
    /// 索引を作ったときの辞書の指紋などを書いた付属ファイル（索引と同じディレクトリ）
    static let metaFileName = "nagi-glide.meta"
    static let dictFileName = "nagi-dict.bin"
    private static let metaVersion = 1

    enum State: Equatable {
        /// 辞書が無い（索引の材料が無い）
        case noDict
        /// 索引が無い
        case missing
        /// 索引はあるが、今の辞書から作ったものではない（辞書の更新後・付属ファイルが無い・壊れている）
        case stale
        /// 今の辞書から作った索引がある
        case ready
    }

    static func dictURL(dataDir: URL) -> URL {
        dataDir.appendingPathComponent("dict", isDirectory: true).appendingPathComponent(dictFileName)
    }

    static func indexURL(dataDir: URL) -> URL {
        dataDir.appendingPathComponent("dict", isDirectory: true).appendingPathComponent(indexFileName)
    }

    static func metaURL(dataDir: URL) -> URL {
        dataDir.appendingPathComponent("dict", isDirectory: true).appendingPathComponent(metaFileName)
    }

    /// ファイルのサイズ。無い・読めなければ nil
    static func fileSize(_ url: URL) -> Int64? {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
              let size = attrs[.size] as? NSNumber else { return nil }
        return size.int64Value
    }

    /// 辞書の指紋（サイズ + 更新時刻〔ミリ秒〕）。辞書全体のハッシュは取らない（数十 MB を読むため）。
    /// 辞書を差し替えれば、サイズか更新時刻のどちらかが変わる。無ければ nil
    static func dictFingerprint(dataDir: URL) -> String? {
        let url = dictURL(dataDir: dataDir)
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
              let size = attrs[.size] as? NSNumber else { return nil }
        let mtime = (attrs[.modificationDate] as? Date).map { Int64(($0.timeIntervalSince1970 * 1000).rounded(.down)) } ?? 0
        return "\(size.int64Value):\(mtime)"
    }

    /// 拡張が使える索引ファイルがあるか（存在して空でない。中身の検査はコアが読み込むときに行う）
    static func indexExists(dataDir: URL) -> Bool {
        (fileSize(indexURL(dataDir: dataDir)) ?? 0) > 0
    }

    /// 付属ファイルの内容（行ごとの `key=value`）
    static func metaText(dictFingerprint: String, indexSize: Int64) -> String {
        "version=\(metaVersion)\ndict=\(dictFingerprint)\nindex_size=\(indexSize)\n"
    }

    /// 付属ファイルを読む。(辞書の指紋, 索引のサイズ)。読めない・版が違う・欠けていれば nil
    static func parseMeta(_ text: String) -> (dict: String, indexSize: Int64)? {
        var fields: [String: String] = [:]
        for line in text.split(separator: "\n") {
            guard let eq = line.firstIndex(of: "=") else { continue }
            fields[String(line[line.startIndex..<eq])] = String(line[line.index(after: eq)...])
        }
        guard fields["version"] == String(metaVersion),
              let dict = fields["dict"], !dict.isEmpty,
              let size = fields["index_size"].flatMap({ Int64($0) }) else { return nil }
        return (dict, size)
    }

    static func state(dataDir: URL) -> State {
        guard let fingerprint = dictFingerprint(dataDir: dataDir) else { return .noDict }
        guard let indexSize = fileSize(indexURL(dataDir: dataDir)), indexSize > 0 else { return .missing }
        guard let text = try? String(contentsOf: metaURL(dataDir: dataDir), encoding: .utf8),
              let meta = parseMeta(text),
              meta.dict == fingerprint,
              meta.indexSize == indexSize else { return .stale }
        return .ready
    }

    /// 索引を作ったあとに付属ファイルを書く。`dictFingerprint` は**生成を始める前**に取った値
    /// （生成中に辞書が差し替わっていたら、次の判定で `stale` になって作り直される）
    static func writeMeta(dataDir: URL, dictFingerprint: String) throws {
        let size = fileSize(indexURL(dataDir: dataDir)) ?? 0
        try metaText(dictFingerprint: dictFingerprint, indexSize: size)
            .write(to: metaURL(dataDir: dataDir), atomically: true, encoding: .utf8)
    }

    /// 索引と付属ファイルを消す
    static func remove(dataDir: URL) {
        try? FileManager.default.removeItem(at: indexURL(dataDir: dataDir))
        try? FileManager.default.removeItem(at: metaURL(dataDir: dataDir))
    }

    /// 画面に出す状態の説明
    static func statusText(_ state: State) -> String {
        switch state {
        case .noDict: return "辞書がありません"
        case .missing: return "未作成"
        case .stale: return "作り直しが必要"
        case .ready: return "作成済み"
        }
    }
}

/// 設定 `input.glide` がオンか、の記録（App Group の UserDefaults）。設定画面がコアから値を読んだとき・変えたときに書く。
/// ホストアプリは、起動のたびにコアを開かずに、これを見て索引を自動で作るか決める
enum GlideIndexPreference {
    private static let wantedKey = "glide_index_wanted"

    static func record(enabled: Bool, appGroupId: String = SharedStore.shared.appGroupId) {
        UserDefaults(suiteName: appGroupId)?.set(enabled, forKey: wantedKey)
    }

    static func isGlideSettingOn(appGroupId: String = SharedStore.shared.appGroupId) -> Bool {
        UserDefaults(suiteName: appGroupId)?.bool(forKey: wantedKey) ?? false
    }
}
