//! 人の操作が必要な項目（手順の案内のみ）。結果は `Status::Manual`。
//!
//! 一部は自動で拾える根拠（`tsf.log` の記録・CPU）があるときだけ、状態を変える
//! （言語のオプションから起動した記録があれば OK、ARM64 でなければスキップ）。

use super::checks::{Arch, os_arch};
use super::ctx::Ctx;
use super::tsflog;
use super::types::{ItemId, ItemResult};

/// 言語のオプション（`ITfFnConfigure`）。tsf.log に起動の記録があれば確認済みとする
pub fn judge_language_options(configure_requested: bool, configure_failed: bool) -> ItemResult {
    let id = ItemId::ManualLanguageOptions;
    if configure_failed {
        return ItemResult::ng(
            id,
            "言語のオプションから起動を試みた記録がありますが、失敗しています（nagi-settings.exe が見つからない）",
            "インストール先に nagi-settings.exe があるか確認し、無ければ Nagi を入れ直してください。",
        );
    }
    if configure_requested {
        return ItemResult::ok(
            id,
            "言語のオプションから設定画面を起動した記録が tsf.log にあります（確認済み）",
        );
    }
    ItemResult::manual(
        id,
        "言語のオプションから設定画面が開くかは、画面を操作しないと確認できません",
        "1. 設定 → 時刻と言語 → 言語と地域 を開く\n\
         2. 「日本語」の右の「…」→「言語のオプション」\n\
         3. 「キーボード」の一覧で Nagi を選び、「オプション」（または「詳細オプション」）を押す\n\
         4. 「Nagi 設定」の画面が開けば OK。項目が見つからない・何も起きない場合は NG として、その旨を報告する\n\
         ※ 確認したあとにもう一度この診断を実行すると、tsf.log の記録から自動で OK になります。",
    )
}

/// 手動項目の案内を作る
pub fn guide(ctx: &Ctx, id: ItemId) -> ItemResult {
    match id {
        ItemId::ManualLanguageOptions => {
            let analysis = ctx
                .log_dir
                .as_ref()
                .and_then(|d| std::fs::read(d.join("tsf.log")).ok())
                .map(|b| tsflog::analyze(&String::from_utf8_lossy(&b)));
            match analysis {
                Some(a) => judge_language_options(a.configure_requested, a.configure_failed),
                None => judge_language_options(false, false),
            }
        }
        ItemId::ManualReconversion => ItemResult::manual(
            id,
            "実アプリ（Office・ブラウザ・メモ帳など）での再変換は、アプリを操作しないと確認できません。エンジン側の再変換（input.reconvert）は自動で確認済みです",
            "1. Word などで「日本語」と入力して確定する\n\
             2. その文字列を選択し、再変換（変換キー、または右クリック「再変換」）を実行する\n\
             3. 候補の一覧が出て、選ぶと置き換わりキャレットが末尾に来れば OK\n\
             4. 複数の文節にまたがる範囲でも、選んだ候補以外の部分が消えないこと・直後の入力が通常どおりであることを確認する\n\
             結果（アプリ名・OK/NG・様子）を報告する。",
        ),
        ItemId::ManualMultiDpi => {
            let monitors = ctx.facts().and_then(|f| f.monitors);
            let note = match monitors {
                Some(n) if n >= 2 => format!("モニターは {n} 枚検出されています"),
                Some(_) => {
                    "モニターが 1 枚しか検出されていないため、この PC では確認できません".to_owned()
                }
                None => "モニター数を取得できませんでした".to_owned(),
            };
            ItemResult::manual(
                id,
                format!(
                    "拡大率が違うモニター間の移動は、実際にウィンドウを動かさないと確認できません（{note}）"
                ),
                "1. 拡大率が違う 2 枚のモニターで、スタートメニューの「Nagi 設定」を開く\n\
                 2. ウィンドウをもう一方のモニターへドラッグする\n\
                 3. 文字が切れない・ぼやけない・ボタンが重ならないことを確認する\n\
                 4. 設定画面を開いたまま更新確認を実行しても問題ないことも確認する（更新ページ）",
            )
        }
        ItemId::ManualSmartScreen => ItemResult::manual(
            id,
            "SmartScreen の表示（文言）は、ダウンロード・実行のときに画面で見ないと記録できません",
            "1. Edge または Chrome で配布物（Nagi-setup-<版>.exe または zip）をダウンロードする\n\
             2. 実行（または zip 内の install.ps1 を実行）し、SmartScreen / Defender の警告が出るか確認する\n\
             3. 出た場合は、画面の文言と、「詳細情報」→「実行」で先へ進めるかを記録する（スクリーンショット可）\n\
             4. 出なかった場合は「出なかった」と記録する",
        ),
        ItemId::ManualApplyUpdate => ItemResult::manual(
            id,
            "更新の適用（UAC・再起動）と UAC 拒否の挙動は、昇格の確認画面を操作しないと確認できません。更新の取得・検証の失敗系は自動で確認済みです（update.* の項目）",
            "1. 更新ページで「更新を確認」→ 新しい版があれば取得し、「更新を適用…」を押す\n\
             2. UAC の確認で「いいえ」を選ぶ → エラーで終わり、何も変わらない（版が戻らない・ファイルが壊れない）ことを確認する\n\
             3. もう一度適用して「はい」を選ぶ → 完了メッセージが出て、サインアウトまたは再起動後に新しい版になることを確認する\n\
             4. 再起動後にインストール先へ imserver.exe.old が残っていないこと（この診断の「更新の残り」）を確認する",
        ),
        ItemId::ManualArm64Emulation => match ctx.facts().map(os_arch) {
            Some(Arch::Arm64) => ItemResult::manual(
                id,
                "この PC は ARM64 です。ARM64 ネイティブ／x64 エミュレーションのアプリでの入力は、アプリを操作しないと確認できません",
                "1. ARM64 ネイティブのアプリ（メモ帳など）で Nagi に切り替えて入力できることを確認する\n\
                 2. x64 エミュレーションで動くアプリでは Nagi が出ない（既知の制約）ことを確認する。ここで出た場合は想定外なので報告する\n\
                 3. 更新の確認が latest-arm64.json を見ること（更新ページの結果）を確認する\n\
                 4. x64 版の zip を実行すると拒否されること（install.ps1 の ARCH 不一致）を確認する",
            ),
            _ => ItemResult::skipped(id, "ARM64 の PC でのみ確認する項目です"),
        },
        other => ItemResult::skipped(other, "手動項目ではありません"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::sys::WinFacts;
    use crate::diag::testutil::{FakeSystem, ctx_with_system};
    use crate::diag::types::{Kind, Status};

    #[test]
    fn language_options_follows_the_log() {
        assert_eq!(judge_language_options(false, false).status, Status::Manual);
        assert_eq!(judge_language_options(true, false).status, Status::Ok);
        assert_eq!(judge_language_options(true, true).status, Status::Ng);
        let m = judge_language_options(false, false);
        assert!(m.advice.unwrap().contains("言語のオプション"));
    }

    #[test]
    fn every_guide_item_returns_a_manual_or_skipped_result_with_steps() {
        let ctx = ctx_with_system(FakeSystem::windows(WinFacts {
            os_arch: "Arm64".into(),
            monitors: Some(2),
            ..WinFacts::default()
        }));
        for id in ItemId::ALL.iter().filter(|i| i.kind() == Kind::Guide) {
            let r = guide(&ctx, *id);
            assert_eq!(r.id, *id);
            assert!(
                matches!(r.status, Status::Manual | Status::Ok),
                "{id:?}: {r:?}"
            );
            if r.status == Status::Manual {
                assert!(
                    r.advice.as_deref().is_some_and(|a| a.contains("1.")),
                    "{id:?}"
                );
            }
        }
    }

    #[test]
    fn arm64_item_is_skipped_on_x64() {
        let ctx = ctx_with_system(FakeSystem::windows(WinFacts {
            os_arch: "X64".into(),
            ..WinFacts::default()
        }));
        assert_eq!(
            guide(&ctx, ItemId::ManualArm64Emulation).status,
            Status::Skipped
        );
    }
}
