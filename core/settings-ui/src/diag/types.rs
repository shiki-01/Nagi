//! 診断の結果型（OS 非依存）。項目 ID・状態・詳細・推奨対処を構造化して持つ。

use serde::{Deserialize, Serialize};

/// 診断項目の ID（安定した文字列 ID を持つ。UI・ファイル・報告で同じ ID を使う）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemId {
    /// CPU アーキテクチャ（ARM64 判定）と配布物の整合
    EnvArch,
    /// 実行ユーザー（標準ユーザー / 管理者）と書き込み先
    EnvUser,
    /// VC++ ランタイムの有無
    EnvVcRuntime,
    /// DPI・テーマ・ハイコントラスト
    EnvDisplay,
    /// インストール先のファイル構成
    InstallFiles,
    /// `.old` の残り・旧版の imserver の残存
    InstallOldFiles,
    /// Authenticode 署名の状態（記録のみ）
    InstallSignature,
    /// 辞書・モデルの有無
    DataFiles,
    /// TSF の COM 登録（CLSID・プロファイル・カテゴリ）
    TsfRegistration,
    /// 現在のユーザーで入力方式が有効か
    TsfUserProfile,
    /// TIP の COM インターフェイス（`ITfFnConfigure` を含む）
    TsfInterfaces,
    /// `ITfFunctionProvider` の登録（Activate の記録）と DLL を読んだホスト
    TsfFunctionProvider,
    /// 言語のオプションに「オプション」「削除」が出ない原因の切り分け（登録値・ACL・MS-IME との差分）
    TsfLanguageOptions,
    /// `tsf.log` の健全性（起動失敗・登録失敗の記録）
    TsfLog,
    /// imserver の起動・初期化・応答
    EngineStart,
    /// imserver の終了（shutdown・stdin の EOF）
    EngineShutdown,
    /// 親プロセスの消滅で imserver が終了するか
    EngineParentExit,
    /// imserver の常駐メモリ
    EngineMemory,
    /// 曖昧入力（H-88・H-89）
    InputFuzzy,
    /// 確定済みテキストの再変換（`reconvert`）
    InputReconvert,
    /// 更新確認（実際のサーバー）
    UpdateCheck,
    /// 更新確認の失敗系: オフライン
    UpdateOffline,
    /// 更新確認の失敗系: 署名・チェックサム不正
    UpdateBadSignature,
    /// 更新の保存先・適用スクリプトの状態
    UpdateState,
    /// 手動: 言語のオプションから設定 GUI を開く
    ManualLanguageOptions,
    /// 手動: 実アプリでの再変換
    ManualReconversion,
    /// 手動: 別 DPI のモニター間移動
    ManualMultiDpi,
    /// 手動: SmartScreen の文言の記録
    ManualSmartScreen,
    /// 手動: 更新の適用と UAC 拒否
    ManualApplyUpdate,
    /// 手動: ARM64 のエミュレーション下アプリ
    ManualArm64Emulation,
}

impl Serialize for ItemId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ItemId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ItemId::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("不明な項目 ID: {s}")))
    }
}

/// 項目の種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 自動で判定する
    Auto,
    /// 人の操作が必要（手順の案内のみ。結果は `Status::Manual`）
    Guide,
}

impl ItemId {
    /// 実行順の全項目（`run_all` の順序）
    pub const ALL: &'static [ItemId] = &[
        ItemId::EnvArch,
        ItemId::EnvUser,
        ItemId::EnvVcRuntime,
        ItemId::EnvDisplay,
        ItemId::InstallFiles,
        ItemId::InstallOldFiles,
        ItemId::InstallSignature,
        ItemId::DataFiles,
        ItemId::TsfRegistration,
        ItemId::TsfUserProfile,
        ItemId::TsfInterfaces,
        ItemId::TsfFunctionProvider,
        ItemId::TsfLanguageOptions,
        ItemId::TsfLog,
        ItemId::EngineStart,
        ItemId::EngineMemory,
        ItemId::InputFuzzy,
        ItemId::InputReconvert,
        ItemId::EngineShutdown,
        ItemId::EngineParentExit,
        ItemId::UpdateCheck,
        ItemId::UpdateOffline,
        ItemId::UpdateBadSignature,
        ItemId::UpdateState,
        ItemId::ManualLanguageOptions,
        ItemId::ManualReconversion,
        ItemId::ManualMultiDpi,
        ItemId::ManualSmartScreen,
        ItemId::ManualApplyUpdate,
        ItemId::ManualArm64Emulation,
    ];

    /// 安定した ID 文字列（`env.arch` など）
    pub fn as_str(self) -> &'static str {
        match self {
            ItemId::EnvArch => "env.arch",
            ItemId::EnvUser => "env.user",
            ItemId::EnvVcRuntime => "env.vcruntime",
            ItemId::EnvDisplay => "env.display",
            ItemId::InstallFiles => "install.files",
            ItemId::InstallOldFiles => "install.old_files",
            ItemId::InstallSignature => "install.signature",
            ItemId::DataFiles => "data.files",
            ItemId::TsfRegistration => "tsf.registration",
            ItemId::TsfUserProfile => "tsf.user_profile",
            ItemId::TsfInterfaces => "tsf.interfaces",
            ItemId::TsfFunctionProvider => "tsf.function_provider",
            ItemId::TsfLanguageOptions => "tsf.language_options",
            ItemId::TsfLog => "tsf.log",
            ItemId::EngineStart => "engine.start",
            ItemId::EngineShutdown => "engine.shutdown",
            ItemId::EngineParentExit => "engine.parent_exit",
            ItemId::EngineMemory => "engine.memory",
            ItemId::InputFuzzy => "input.fuzzy",
            ItemId::InputReconvert => "input.reconvert",
            ItemId::UpdateCheck => "update.check",
            ItemId::UpdateOffline => "update.offline",
            ItemId::UpdateBadSignature => "update.bad_signature",
            ItemId::UpdateState => "update.state",
            ItemId::ManualLanguageOptions => "manual.language_options",
            ItemId::ManualReconversion => "manual.reconversion",
            ItemId::ManualMultiDpi => "manual.multi_dpi",
            ItemId::ManualSmartScreen => "manual.smartscreen",
            ItemId::ManualApplyUpdate => "manual.apply_update",
            ItemId::ManualArm64Emulation => "manual.arm64_emulation",
        }
    }

    /// 文字列 ID から項目を引く
    pub fn parse(s: &str) -> Option<ItemId> {
        Self::ALL.iter().copied().find(|id| id.as_str() == s)
    }

    /// 利用者に見せる項目名（日本語）
    pub fn title(self) -> &'static str {
        match self {
            ItemId::EnvArch => "CPU の種類と配布物の整合（ARM64 判定）",
            ItemId::EnvUser => "実行ユーザーと書き込み先（標準ユーザー）",
            ItemId::EnvVcRuntime => "VC++ ランタイム",
            ItemId::EnvDisplay => "画面の拡大率（DPI）・テーマ",
            ItemId::InstallFiles => "インストール先のファイル",
            ItemId::InstallOldFiles => "更新の残り（.old・旧版のエンジン）",
            ItemId::InstallSignature => "Authenticode 署名の状態",
            ItemId::DataFiles => "辞書・モデル",
            ItemId::TsfRegistration => "入力方式の登録（COM・プロファイル）",
            ItemId::TsfUserProfile => "このユーザーで Nagi が有効か",
            ItemId::TsfInterfaces => "TIP のインターフェイス（言語のオプション用 ITfFnConfigure）",
            ItemId::TsfFunctionProvider => "再変換の入口（ITfFunctionProvider）",
            ItemId::TsfLanguageOptions => {
                "言語のオプションのプローブ（オプション・削除が出ない原因）"
            }
            ItemId::TsfLog => "tsf.log の記録",
            ItemId::EngineStart => "変換エンジン（imserver）の起動と応答",
            ItemId::EngineShutdown => "変換エンジンの終了",
            ItemId::EngineParentExit => "親プロセスが消えたときのエンジン終了",
            ItemId::EngineMemory => "変換エンジンの常駐メモリ",
            ItemId::InputFuzzy => "曖昧入力（もしかして候補）",
            ItemId::InputReconvert => "確定済みテキストの再変換（エンジン側）",
            ItemId::UpdateCheck => "更新の確認（実サーバー）",
            ItemId::UpdateOffline => "更新確認の失敗系: オフライン",
            ItemId::UpdateBadSignature => "更新確認の失敗系: 署名・チェックサム不正",
            ItemId::UpdateState => "更新の保存先と適用スクリプト",
            ItemId::ManualLanguageOptions => "言語のオプションから設定画面を開く",
            ItemId::ManualReconversion => "実アプリでの再変換（Office など）",
            ItemId::ManualMultiDpi => "別 DPI のモニター間の移動",
            ItemId::ManualSmartScreen => "SmartScreen の表示の記録",
            ItemId::ManualApplyUpdate => "更新の適用と UAC の拒否",
            ItemId::ManualArm64Emulation => "ARM64: x64 エミュレーションのアプリ",
        }
    }

    /// 自動判定か、案内のみか
    pub fn kind(self) -> Kind {
        match self {
            ItemId::ManualLanguageOptions
            | ItemId::ManualReconversion
            | ItemId::ManualMultiDpi
            | ItemId::ManualSmartScreen
            | ItemId::ManualApplyUpdate
            | ItemId::ManualArm64Emulation => Kind::Guide,
            _ => Kind::Auto,
        }
    }
}

/// 項目の判定結果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// 期待どおり
    Ok,
    /// 要確認（問題とは言い切れない・判定材料が足りない）
    Warn,
    /// 期待と違う（不具合・環境の問題）
    Ng,
    /// この環境では判定できない（Windows 以外・前提が無い）
    Skipped,
    /// 人の操作が必要（手順の案内のみ）
    Manual,
}

impl Status {
    /// 表示ラベル
    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Warn => "要確認",
            Status::Ng => "NG",
            Status::Skipped => "スキップ",
            Status::Manual => "手動確認",
        }
    }

    /// 全体の判定にするときの重さ（大きいほど悪い。`Manual`・`Skipped` は全体に影響しない）
    fn severity(self) -> u8 {
        match self {
            Status::Ng => 3,
            Status::Warn => 2,
            Status::Ok => 1,
            Status::Skipped | Status::Manual => 0,
        }
    }
}

/// 1 項目の結果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemResult {
    /// 項目 ID
    pub id: ItemId,
    /// 項目名（日本語）
    pub title: String,
    /// 判定
    pub status: Status,
    /// 詳細（何を見てどうだったか）。入力内容は含めない
    pub detail: String,
    /// 推奨される対処（`Ok` では通常なし）
    pub advice: Option<String>,
    /// 根拠の短い行（ログの抜粋など。入力内容は含めない）
    pub evidence: Vec<String>,
    /// 所要時間（ミリ秒）
    pub elapsed_ms: u64,
}

impl ItemResult {
    /// 結果を作る
    pub fn new(id: ItemId, status: Status, detail: impl Into<String>) -> Self {
        Self {
            id,
            title: id.title().to_owned(),
            status,
            detail: detail.into(),
            advice: None,
            evidence: Vec::new(),
            elapsed_ms: 0,
        }
    }

    /// 期待どおり
    pub fn ok(id: ItemId, detail: impl Into<String>) -> Self {
        Self::new(id, Status::Ok, detail)
    }

    /// 要確認
    pub fn warn(id: ItemId, detail: impl Into<String>, advice: impl Into<String>) -> Self {
        Self::new(id, Status::Warn, detail).with_advice(advice)
    }

    /// NG
    pub fn ng(id: ItemId, detail: impl Into<String>, advice: impl Into<String>) -> Self {
        Self::new(id, Status::Ng, detail).with_advice(advice)
    }

    /// スキップ
    pub fn skipped(id: ItemId, reason: impl Into<String>) -> Self {
        Self::new(id, Status::Skipped, reason)
    }

    /// 手動確認の案内（`steps` は手順）
    pub fn manual(id: ItemId, detail: impl Into<String>, steps: impl Into<String>) -> Self {
        Self::new(id, Status::Manual, detail).with_advice(steps)
    }

    /// 推奨対処を付ける
    pub fn with_advice(mut self, advice: impl Into<String>) -> Self {
        self.advice = Some(advice.into());
        self
    }

    /// 根拠の行を付ける
    pub fn with_evidence(mut self, lines: impl IntoIterator<Item = String>) -> Self {
        self.evidence.extend(lines);
        self
    }
}

/// 状態ごとの件数
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// OK の件数
    pub ok: usize,
    /// 要確認の件数
    pub warn: usize,
    /// NG の件数
    pub ng: usize,
    /// スキップの件数
    pub skipped: usize,
    /// 手動確認の件数
    pub manual: usize,
}

/// 診断レポート全体
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// 診断を実行した `nagi-settings` の版
    pub tool_version: String,
    /// 実行時刻（UTC。`YYYY-MM-DDTHH:MM:SSZ`）
    pub generated_at: String,
    /// 実行環境の要約（OS・CPU。ユーザー名・パスを含めない）
    pub environment: String,
    /// 項目ごとの結果（実行順）
    pub items: Vec<ItemResult>,
}

impl Report {
    /// 状態ごとの件数
    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        for item in &self.items {
            match item.status {
                Status::Ok => s.ok += 1,
                Status::Warn => s.warn += 1,
                Status::Ng => s.ng += 1,
                Status::Skipped => s.skipped += 1,
                Status::Manual => s.manual += 1,
            }
        }
        s
    }

    /// 全体の判定（自動判定した項目のうち最悪のもの。自動判定が 1 つも無ければ `Skipped`）
    pub fn overall(&self) -> Status {
        self.items
            .iter()
            .map(|i| i.status)
            .filter(|s| s.severity() > 0)
            .max_by_key(|s| s.severity())
            .unwrap_or(Status::Skipped)
    }

    /// 項目を ID で引く
    pub fn get(&self, id: ItemId) -> Option<&ItemResult> {
        self.items.iter().find(|i| i.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_parse_round_trips() {
        let mut seen = std::collections::HashSet::new();
        for id in ItemId::ALL {
            assert!(seen.insert(id.as_str()), "重複: {}", id.as_str());
            assert_eq!(ItemId::parse(id.as_str()), Some(*id));
            assert!(!id.title().is_empty());
        }
        assert_eq!(ItemId::parse("nope"), None);
    }

    #[test]
    fn serde_uses_the_dotted_id() {
        for id in ItemId::ALL {
            let json = serde_json::to_string(id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.as_str()));
            assert_eq!(serde_json::from_str::<ItemId>(&json).unwrap(), *id);
        }
        assert!(serde_json::from_str::<ItemId>("\"x.y\"").is_err());
    }

    #[test]
    fn manual_items_are_guides() {
        for id in ItemId::ALL {
            assert_eq!(
                id.kind() == Kind::Guide,
                id.as_str().starts_with("manual."),
                "{}",
                id.as_str()
            );
        }
    }

    #[test]
    fn overall_takes_the_worst_automatic_status() {
        let item = |s| ItemResult::new(ItemId::EnvArch, s, "x");
        let report = |statuses: &[Status]| Report {
            tool_version: "0".into(),
            generated_at: "t".into(),
            environment: "e".into(),
            items: statuses.iter().map(|s| item(*s)).collect(),
        };
        assert_eq!(
            report(&[Status::Ok, Status::Warn, Status::Manual]).overall(),
            Status::Warn
        );
        assert_eq!(
            report(&[Status::Ok, Status::Ng, Status::Warn]).overall(),
            Status::Ng
        );
        assert_eq!(report(&[Status::Ok]).overall(), Status::Ok);
        assert_eq!(
            report(&[Status::Skipped, Status::Manual]).overall(),
            Status::Skipped
        );
        let s = report(&[Status::Ok, Status::Ok, Status::Ng, Status::Manual]).summary();
        assert_eq!((s.ok, s.ng, s.manual, s.warn, s.skipped), (2, 1, 1, 0, 0));
    }
}
