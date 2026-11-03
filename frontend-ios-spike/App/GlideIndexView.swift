import SwiftUI

/// ホームタブの「なぞり入力の索引」: グライド入力（`input.glide`）が使う索引 `nagi-glide.bin`（約 39MB）の状態と、
/// 手動の作成・作り直し。索引は辞書から本体アプリが作り、キーボード拡張は読むだけ（decisions.md H-84）。
/// 設定 `input.glide` をオンにしたとき・辞書を更新したときは自動で作る。
struct NagiGlideIndexSection: View {
    @ObservedObject private var builder = GlideIndexBuilder.shared

    var body: some View {
        Section("なぞり入力の索引") {
            NagiGlideIndexRows(builder: builder)
            Text("なぞり入力（設定の「入力」）が使う索引です。辞書から約 2 秒で作り、約 39MB になります。作成は「なぞり入力」をオンにしたときと辞書を更新したときに自動で行います（オフの間は自動では作りません）。索引が無い間、キーボードのなぞり入力は働きません。")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .onAppear {
            Task { @MainActor in
                builder.refresh()
                builder.ensureBuiltIfWanted()
            }
        }
    }
}

/// 設定画面の「なぞり入力」の項目の下に出す、索引の状態（`input.glide` の行のすぐ下）
struct NagiGlideIndexNote: View {
    @ObservedObject private var builder = GlideIndexBuilder.shared

    var body: some View {
        NagiGlideIndexRows(builder: builder)
            .onAppear { Task { @MainActor in builder.refresh() } }
    }
}

/// 状態の行・失敗の理由・作成ボタン（ホームと設定で共通）
private struct NagiGlideIndexRows: View {
    @ObservedObject var builder: GlideIndexBuilder

    var body: some View {
        HStack {
            Text("索引")
            Spacer()
            if builder.phase == .building {
                ProgressView()
                Text("作成中").foregroundColor(.secondary)
            } else {
                Image(systemName: builder.state == .ready ? "checkmark.circle.fill" : "xmark.circle.fill")
                    .foregroundColor(builder.state == .ready ? .green : .orange)
                Text(GlideIndexStatus.statusText(builder.state)).foregroundColor(.secondary)
            }
        }
        if builder.indexBytes > 0 {
            HStack {
                Text("サイズ")
                Spacer()
                Text(NagiFormat.bytes(builder.indexBytes)).foregroundColor(.secondary)
            }
        }
        if builder.state != .ready && builder.phase != .building {
            Text(hint)
                .font(.caption)
                .foregroundColor(.orange)
        }
        if case .failed(let reason) = builder.phase {
            Text("作成に失敗しました: \(reason)")
                .font(.caption)
                .foregroundColor(.red)
        }
        if builder.dataDir == nil {
            Text("App Group コンテナが取得できないため、キーボード拡張と共有できません")
                .font(.caption)
                .foregroundColor(.orange)
        } else {
            Button(builder.state == .missing || builder.state == .noDict ? "索引を作成" : "索引を作り直す") {
                Task { @MainActor in builder.rebuild() }
            }
            .buttonStyle(.borderless)
            .disabled(builder.phase == .building || builder.state == .noDict)
        }
    }

    private var hint: String {
        switch builder.state {
        case .noDict: return "辞書がありません。先に辞書を取り込んでください"
        case .missing: return "索引を作成してください。ないと、キーボードのなぞり入力は働きません"
        case .stale: return "辞書が更新されています。索引を作り直してください（古い索引でも使えますが、新しい読みを拾えません）"
        case .ready: return ""
        }
    }
}
