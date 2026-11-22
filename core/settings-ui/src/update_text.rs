//! 更新確認の結果の画面表示（OS 非依存）。Web 画面の「更新」ページが使う文言と見出しの決め方。

use crate::backend::{UpdateCheck, UpdateStatus};
use crate::diag_model::BadgeKind;

/// 更新確認の結果の画面表示
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOutcome {
    /// 結果の文言
    pub text: String,
    /// 「更新を適用…」を出すか（取得済みのときだけ）
    pub can_apply: bool,
    /// 「リリースノートを開く」の URL（最新・失敗のときは `None`）
    pub notes_url: Option<String>,
}

/// 更新確認の結果を文言にする。`notes_url` はここでは絞らない
/// （呼び出し側が `is_safe_notes_url` を通す）
pub fn describe_update(check: &UpdateCheck) -> UpdateOutcome {
    let latest = check.latest.as_deref().unwrap_or("");
    let (text, can_apply) = match check.status {
        UpdateStatus::UpToDate => (format!("最新です（{}）。", check.current), false),
        UpdateStatus::Available => (format!("新しい版 {latest} があります。"), false),
        UpdateStatus::Staged => (
            format!(
                "新しい版 {latest} を取得しました。「更新を適用…」で入れ替えられます（管理者の確認が出ます）。"
            ),
            true,
        ),
        UpdateStatus::Error => (
            format!(
                "更新を確認できませんでした: {}",
                check.error.as_deref().unwrap_or("原因不明のエラーです")
            ),
            false,
        ),
    };
    let show_notes = matches!(check.status, UpdateStatus::Available | UpdateStatus::Staged);
    UpdateOutcome {
        text,
        can_apply,
        notes_url: check.notes_url.clone().filter(|_| show_notes),
    }
}

/// 更新ページの結果の見出し（状態のバッジ + 短い見出し）。詳しい文言は `text` をそのまま別に出す
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateHeadline {
    /// 色分け
    pub badge: BadgeKind,
    /// バッジの文字（色に頼らず文字でも示す）
    pub label: &'static str,
    /// 見出し
    pub title: &'static str,
}

/// 更新ページの状態（確認中・結果の文言）から、カードに出す見出しを決める。何も無ければ `None`。
/// 文言は [`describe_update`] などが作るもので、ここでは先頭の言い回しで種類を判断する
pub fn update_headline(busy: bool, text: &str) -> Option<UpdateHeadline> {
    use BadgeKind::{Info, Neutral, Ng, Ok, Warn};
    let (badge, label, title) = if busy {
        (Info, "確認中", "更新を確認しています")
    } else if text.is_empty() {
        return None;
    } else if text.starts_with("最新です") {
        (Ok, "最新", "最新の版です")
    } else if text.starts_with("新しい版") && text.contains("取得しました") {
        (Warn, "適用待ち", "新しい版を適用できます")
    } else if text.starts_with("新しい版") {
        (Info, "更新あり", "新しい版があります")
    } else if text.starts_with("更新の適用を開始") {
        (Info, "適用中", "更新を適用しています")
    } else if text.contains("できませんでした") {
        (Ng, "失敗", "更新に失敗しました")
    } else {
        (Neutral, "お知らせ", "更新の状態")
    };
    Some(UpdateHeadline {
        badge,
        label,
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_headline_follows_the_outcome_text() {
        let h = |busy, text: &str| update_headline(busy, text);
        assert_eq!(h(false, ""), None);
        assert_eq!(h(true, "").map(|x| x.badge), Some(BadgeKind::Info));
        assert_eq!(
            h(false, "最新です（0.1.19）。").map(|x| x.badge),
            Some(BadgeKind::Ok)
        );
        assert_eq!(
            h(false, "新しい版 0.2.0 があります。").map(|x| x.label),
            Some("更新あり")
        );
        assert_eq!(
            h(
                false,
                "新しい版 0.2.0 を取得しました。「更新を適用…」で入れ替えられます。"
            )
            .map(|x| x.badge),
            Some(BadgeKind::Warn)
        );
        assert_eq!(
            h(false, "更新を確認できませんでした: x").map(|x| x.badge),
            Some(BadgeKind::Ng)
        );
        assert_eq!(
            h(false, "更新を適用できませんでした: x").map(|x| x.badge),
            Some(BadgeKind::Ng)
        );
        assert_eq!(
            h(false, "更新の適用を開始しました。").map(|x| x.label),
            Some("適用中")
        );
        // 確認中は結果の文言より優先
        assert_eq!(h(true, "最新です").map(|x| x.label), Some("確認中"));
        // 文字（ラベル）は必ずある。色だけに頼らない
        assert!(h(false, "??").is_some_and(|x| !x.label.is_empty()));
    }

    fn check(status: UpdateStatus) -> UpdateCheck {
        UpdateCheck {
            current: "0.1.15".into(),
            latest: Some("0.2.0".into()),
            status,
            notes_url: Some("https://example.com/notes".into()),
            error: None,
        }
    }

    #[test]
    fn update_outcomes_match_the_web_wording() {
        let up = describe_update(&check(UpdateStatus::UpToDate));
        assert_eq!(up.text, "最新です（0.1.15）。");
        assert!(!up.can_apply);
        assert_eq!(up.notes_url, None);
        let avail = describe_update(&check(UpdateStatus::Available));
        assert_eq!(avail.text, "新しい版 0.2.0 があります。");
        assert!(!avail.can_apply);
        assert_eq!(
            avail.notes_url.as_deref(),
            Some("https://example.com/notes")
        );
        let staged = describe_update(&check(UpdateStatus::Staged));
        assert!(staged.can_apply);
        assert!(staged.text.contains("0.2.0 を取得しました"));
        assert!(staged.notes_url.is_some());
        let mut err = check(UpdateStatus::Error);
        err.error = Some("ネットワークに接続できません".into());
        let e = describe_update(&err);
        assert_eq!(
            e.text,
            "更新を確認できませんでした: ネットワークに接続できません"
        );
        assert!(!e.can_apply);
        assert_eq!(e.notes_url, None);
        err.error = None;
        assert!(describe_update(&err).text.ends_with("原因不明のエラーです"));
    }
}
