//! 利用者に見せる状態の文言・入力モードの表示（OS 非依存）。macOS 版 `StatusMessages.swift` に相当する。
//! 入力内容は含めない。表示（トースト・言語バー・ツールチップ）は次の層が担う。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nagi_protocol::{InputMode, NotifyLevel, StatusResult};

use crate::supervisor::EngineEvent;

/// エンジンのイベントのうち、利用者に知らせるべきものの文言。`None` なら知らせない
pub fn toast_for(event: &EngineEvent) -> Option<String> {
    match event {
        EngineEvent::Notify(n) => {
            // 起動時のアップデート確認（コアの `update.auto_check`）は info でも知らせる。
            // フロントは imserver に `--stage-updates` を付けるので、新版はバックグラウンドで取得される
            // （decisions.md H-95）。取得できたら `update_ready`
            match n.code.as_deref() {
                Some("update_available") => {
                    return Some(format!(
                        "{}。取得中です。取得できたらスタートメニューの「Nagi を更新」から適用できます",
                        n.message
                    ));
                }
                Some("update_ready") => {
                    return Some(format!(
                        "{}。スタートメニューの「Nagi を更新」から適用できます",
                        n.message
                    ));
                }
                _ => {}
            }
            // ほかの info は頻繁に来うるため出さない（ログのみ）
            (n.level != NotifyLevel::Info).then(|| n.message.clone())
        }
        EngineEvent::Crashed { attempt, .. } => {
            (*attempt == 1).then(|| "変換エンジンが停止しました。再起動しています".to_owned())
        }
        EngineEvent::EnteredMinimalMode { restarts } => Some(format!(
            "変換エンジンが {restarts} 回続けて停止したため、辞書のみで動作しています"
        )),
        // 直らない種類の起動失敗（このアプリでは子プロセスを作れない）は、利用者にできることが無く、
        // 入力しないアプリ（ブラウザの子プロセス等）でも出うるので知らせない（ログのみ。H-125）
        EngineEvent::LaunchFailed(error) => (crate::supervisor::spawn_error_code(error)
            .is_none_or(|c| !crate::supervisor::is_unrecoverable_spawn_code(c)))
        .then(|| "変換エンジンを起動できません。再試行しています".to_owned()),
        EngineEvent::RestoreFailed { .. } => Some(
            "変換エンジンの再起動で入力中の文字列を復元できなかったため、そのまま確定しました"
                .to_owned(),
        ),
        EngineEvent::Started { .. }
        | EngineEvent::Restored { .. }
        | EngineEvent::UpdatePreedit(_)
        | EngineEvent::UpdateCandidates(_)
        | EngineEvent::ProcessExited(_)
        | EngineEvent::MinimalSwitchReport { .. }
        | EngineEvent::SpawnRetry { .. }
        | EngineEvent::SpawnBlocked { .. }
        | EngineEvent::Diagnostic(_) => None,
    }
}

/// 状態を取得できなかったときの行
pub const UNAVAILABLE: &str = "変換エンジンに接続していません";

/// 理由文の短い表記の最大文字数（これを超える未知の理由文は切り詰める）
pub const SHORT_REASON_MAX_CHARS: usize = 24;

/// `s` の最初の `ms` の直前にある数字列（`48ms` → `48`）。無ければ `None`
fn ms_value(s: &str) -> Option<&str> {
    let end = s.find("ms")?;
    let head = &s[..end];
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i)?;
    Some(&head[start..])
}

/// 文字の連なりの途中で切ってはいけない文字（結合文字・異体字選択子・ZWJ）
fn is_extender(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{3099}' | '\u{309A}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{200D}')
}

/// `s` を最大 `max` 文字（Unicode のスカラー値単位。結合文字の途中では切らない）にし、
/// 超えたときは末尾に「…」を付ける
fn truncate_chars(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let mut out: String = chars.by_ref().take(max).collect();
    let mut rest = chars.peekable();
    if rest.peek().is_none() {
        return out;
    }
    while let Some(c) = rest.next_if(|c| is_extender(*c)) {
        out.push(c);
    }
    if rest.peek().is_some() {
        out.push('…');
    }
    out
}

/// 自動切替の理由（`{理由}のため {p} に切り替えました` の `{理由}`）の短い表記
fn short_switch_cause(cause: &str) -> &str {
    match cause {
        "バッテリー駆動" => "バッテリー",
        "低電力モード" => "省電力",
        "サーマル状態の悪化" => "発熱",
        "サーマル状態が危険" => "発熱（危険）",
        "メモリ逼迫" => "メモリ逼迫",
        "メモリ逼迫（危険）" => "メモリ逼迫（危険）",
        c if c.starts_with("推論レイテンシ") => "遅延超過",
        c => c,
    }
}

/// コアの `profile_reason`（長い理由文）をメニュー用の短い表記にする（OS 非依存。H-103）。
/// 例: 「キャリブレーション推論 48ms から balanced を選びました」→「推論 48ms」。
/// 知らない文言は [`SHORT_REASON_MAX_CHARS`] 文字で切り詰めて「…」を付ける。
/// `profile` は現状の変換に使っていない（将来、プロファイル別の表記のために受け取る）
pub fn short_profile_reason(_profile: &str, reason: &str) -> String {
    let reason = reason.trim();
    let short = if reason.starts_with("設定（profile.mode") {
        Some("設定で固定".to_owned())
    } else if reason.starts_with("メモリ上限") {
        Some("メモリ上限".to_owned())
    } else if let Some(rest) = reason.strip_prefix("キャリブレーション推論") {
        // 「… 48ms から {p} を選びました」/「…は 39ms ですが、メモリ … に制限しました」
        ms_value(rest).map(|ms| {
            if reason.ends_with("に制限しました") {
                format!("推論 {ms}ms・メモリ制限")
            } else {
                format!("推論 {ms}ms")
            }
        })
    } else if reason.starts_with("実測なし") {
        Some("実測なし".to_owned())
    } else if reason.ends_with("軽量プロファイルにしました") {
        Some("低メモリ".to_owned())
    } else if reason.contains("読み込み中") {
        Some("モデル読込中".to_owned())
    } else if reason.starts_with("変換モデルが使えない") {
        Some("モデルなし".to_owned())
    } else if reason.contains("に切り替えました") {
        reason.find("のため").map(|end| {
            let causes: Vec<&str> = reason[..end].split('・').map(short_switch_cause).collect();
            let joined = causes.join("・");
            if joined.chars().count() > SHORT_REASON_MAX_CHARS && causes.len() > 2 {
                format!("{}ほか", causes[..2].join("・"))
            } else {
                joined
            }
        })
    } else {
        None
    };
    truncate_chars(short.as_deref().unwrap_or(reason), SHORT_REASON_MAX_CHARS)
}

/// メニューに出す状態の行（選択不可の項目として並べる。各行は短く保つ）
pub fn menu_lines(status: &StatusResult) -> Vec<String> {
    let mut lines = vec![format!(
        "プロファイル: {}（{}）",
        status.profile,
        short_profile_reason(&status.profile, &status.profile_reason)
    )];
    lines.push(
        if status.llm {
            "変換: LLM + 辞書"
        } else {
            "変換: 辞書のみ"
        }
        .to_owned(),
    );
    lines.push(if status.learning {
        format!("学習: 有効（{} 件）", status.learned_entries)
    } else {
        "学習: 無効".to_owned()
    });
    lines.push(format!("ユーザー辞書: {} 語", status.user_dict_entries));
    if status.config_error {
        lines.push("設定エラー（以前の設定で動作中）".to_owned());
    }
    lines
}

/// 言語バー・トレイに出す入力モードの短い表示
pub fn mode_indicator(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Hiragana | InputMode::Katakana | InputMode::HankakuKatakana => "あ",
        InputMode::Alphanumeric | InputMode::ZenkakuAlnum | InputMode::HankakuAlnum => "A",
    }
}

/// ツールチップ・アクセシビリティ用のモード名
pub fn mode_name(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Hiragana | InputMode::Katakana | InputMode::HankakuKatakana => "ひらがな",
        InputMode::Alphanumeric | InputMode::ZenkakuAlnum | InputMode::HankakuAlnum => "英数",
    }
}

/// 言語バーのボタンのメニューで選べる操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    /// 入力モードを切り替える
    SetMode(InputMode),
    /// 設定画面（`nagi-settings.exe`。ローカルの Web 画面を既定のブラウザで開く）を起動する
    OpenSettingsGui,
    /// 設定ファイル（`config.toml`）をメモ帳で開く
    OpenSettings,
    /// 設定フォルダをエクスプローラーで開く
    OpenSettingsFolder,
    /// ユーザー辞書フォルダをエクスプローラーで開く
    OpenUserDictFolder,
    /// ログフォルダをエクスプローラーで開く
    OpenLogFolder,
    /// 状態確認（`verify.ps1 status`）を新しいコンソールで起動する
    RunVerify,
    /// 取得済みの更新を適用する（`apply-update.ps1` を新しいコンソールで起動。UAC が出る）
    ApplyUpdate,
    /// 更新を手動で確認する（`nagi-settings.exe --section update --auto-check`。設定画面の更新セクションを開き、
    /// 確認を自動で実行する。decisions.md H-100）
    CheckUpdate,
}

impl MenuCommand {
    /// ログ用の短い名前（`[menu] open failed kind=…`）
    pub fn kind(self) -> &'static str {
        match self {
            MenuCommand::SetMode(_) => "mode",
            MenuCommand::OpenSettingsGui => "settings_gui",
            MenuCommand::OpenSettings => "settings",
            MenuCommand::OpenSettingsFolder => "settings_folder",
            MenuCommand::OpenUserDictFolder => "user_dict_folder",
            MenuCommand::OpenLogFolder => "log_folder",
            MenuCommand::RunVerify => "verify",
            MenuCommand::ApplyUpdate => "apply_update",
            MenuCommand::CheckUpdate => "check_update",
        }
    }
}

/// 言語バーのメニューの 1 行
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// 選べる項目。`id` は `ITfMenu::AddMenuItem` の識別子（[`MenuCommand`] と 1 対 1）
    Command {
        /// メニュー項目の識別子
        id: u32,
        /// 表示文言
        label: String,
        /// 現在の選択（チェック）
        checked: bool,
        /// 選べるか
        enabled: bool,
    },
    /// 選べない説明の行（状態の表示）
    Info(String),
    /// 区切り線
    Separator,
}

const ID_HIRAGANA: u32 = 1;
const ID_ALPHANUMERIC: u32 = 2;
const ID_SETTINGS: u32 = 3;
const ID_SETTINGS_FOLDER: u32 = 4;
const ID_USER_DICT_FOLDER: u32 = 5;
const ID_LOG_FOLDER: u32 = 6;
const ID_VERIFY: u32 = 7;
const ID_APPLY_UPDATE: u32 = 8;
const ID_SETTINGS_GUI: u32 = 9;
const ID_CHECK_UPDATE: u32 = 10;

/// 設定画面の実行ファイル名（DLL と同じディレクトリに置く。`install.ps1` の `$PayloadFiles`）
pub const SETTINGS_GUI_EXE: &str = "nagi-settings.exe";

/// 「更新を確認…」で設定画面に渡す引数（更新セクションを開き、確認を自動で実行する）
pub const CHECK_UPDATE_ARGUMENTS: &str = "--section update --auto-check";

/// 操作 → メニュー項目の識別子
pub fn command_id(command: MenuCommand) -> u32 {
    match command {
        MenuCommand::SetMode(mode) if mode.is_kana() => ID_HIRAGANA,
        MenuCommand::SetMode(_) => ID_ALPHANUMERIC,
        MenuCommand::OpenSettingsGui => ID_SETTINGS_GUI,
        MenuCommand::OpenSettings => ID_SETTINGS,
        MenuCommand::OpenSettingsFolder => ID_SETTINGS_FOLDER,
        MenuCommand::OpenUserDictFolder => ID_USER_DICT_FOLDER,
        MenuCommand::OpenLogFolder => ID_LOG_FOLDER,
        MenuCommand::RunVerify => ID_VERIFY,
        MenuCommand::ApplyUpdate => ID_APPLY_UPDATE,
        MenuCommand::CheckUpdate => ID_CHECK_UPDATE,
    }
}

/// メニュー項目の識別子 → 操作（未知の識別子は `None`）
pub fn command_from_id(id: u32) -> Option<MenuCommand> {
    match id {
        ID_HIRAGANA => Some(MenuCommand::SetMode(InputMode::Hiragana)),
        ID_ALPHANUMERIC => Some(MenuCommand::SetMode(InputMode::Alphanumeric)),
        ID_SETTINGS_GUI => Some(MenuCommand::OpenSettingsGui),
        ID_SETTINGS => Some(MenuCommand::OpenSettings),
        ID_SETTINGS_FOLDER => Some(MenuCommand::OpenSettingsFolder),
        ID_USER_DICT_FOLDER => Some(MenuCommand::OpenUserDictFolder),
        ID_LOG_FOLDER => Some(MenuCommand::OpenLogFolder),
        ID_VERIFY => Some(MenuCommand::RunVerify),
        ID_APPLY_UPDATE => Some(MenuCommand::ApplyUpdate),
        ID_CHECK_UPDATE => Some(MenuCommand::CheckUpdate),
        _ => None,
    }
}

/// ボタンを押したときの切り替え先（ひらがな ⇄ 英数）
pub fn toggled(mode: InputMode) -> InputMode {
    match mode {
        InputMode::Hiragana | InputMode::Katakana | InputMode::HankakuKatakana => {
            InputMode::Alphanumeric
        }
        InputMode::Alphanumeric | InputMode::ZenkakuAlnum | InputMode::HankakuAlnum => {
            InputMode::Hiragana
        }
    }
}

/// ボタンのツールチップ
pub fn button_tooltip(mode: Option<InputMode>) -> String {
    match mode {
        Some(m) => format!("Nagi: {}", mode_name(m)),
        None => "Nagi".to_owned(),
    }
}

/// ボタンに出す短い表示（接続前などモード不明のときは既定のひらがな）
pub fn button_text(mode: Option<InputMode>) -> &'static str {
    mode_indicator(mode.unwrap_or(InputMode::Hiragana))
}

/// 設定ファイルの場所。コアが教える `config_path` が無ければ開けない
pub fn settings_path(status: Option<&StatusResult>) -> Option<&str> {
    status?.config_path.as_deref().filter(|p| !p.is_empty())
}

/// 環境変数・DLL の場所（OS から読んだ値。テストでは注入する）
#[derive(Debug, Clone, Default)]
pub struct PathEnv {
    /// `%APPDATA%`
    pub app_data: Option<PathBuf>,
    /// `%LOCALAPPDATA%`
    pub local_app_data: Option<PathBuf>,
    /// DLL（`nagi_windows_tsf.dll`）のあるディレクトリ。`verify.ps1` も同じ場所に置かれる
    pub module_dir: Option<PathBuf>,
    /// 取得済みの更新の `ready.json`（`%LOCALAPPDATA%\Nagi\update\ready.json`）の中身。無ければ `None`
    pub staged_ready_json: Option<String>,
    /// この DLL の版数（`x.y.z`）。取得済みの更新がこれより新しいときだけ「更新を適用」を出す
    pub current_version: Option<String>,
}

/// メニューの各「開く」項目の対象。`None` は開けない（項目を灰色にする）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuTargets {
    /// 設定画面の実行ファイル（DLL と同じディレクトリの `nagi-settings.exe`。存在するときだけ）
    pub settings_gui: Option<PathBuf>,
    /// 設定ファイル（`config.toml`）。ファイル自体が無くてもメモ帳が作成を尋ねるので存在は問わない
    pub settings_file: Option<PathBuf>,
    /// 設定フォルダ（存在するときだけ）
    pub settings_dir: Option<PathBuf>,
    /// ユーザー辞書フォルダ（`<user-dir>/user_dict`、無ければ user-dir。存在するときだけ）
    pub user_dict_dir: Option<PathBuf>,
    /// ログフォルダ（存在するときだけ）
    pub log_dir: Option<PathBuf>,
    /// `verify.ps1`（存在するときだけ）
    pub verify_script: Option<PathBuf>,
    /// `apply-update.ps1`（存在し、かつ現在の版より新しい更新が取得済みのときだけ）
    pub apply_update_script: Option<PathBuf>,
    /// 取得済みの更新の版数（`apply_update_script` があるときだけ）
    pub staged_version: Option<String>,
}

/// 版数文字列（`x.y.z`、1〜3 要素の数字）を比較用の組にする。不正なら `None`
fn version_tuple(v: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = v.trim().split('.').collect();
    if !(1..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let n = |i: usize| parts.get(i).map_or(0, |p| p.parse().unwrap_or(0));
    Some((n(0), n(1), n(2)))
}

/// 取得済みの更新（`ready.json` の中身）の版数。壊れている・版数が不正なら `None`。
/// ファイルは利用者が書き換えられるので表示用にしか使わない（適用時にコアが再検証する）
pub fn staged_version(ready_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(ready_json).ok()?;
    let version = value.get("version")?.as_str()?;
    version_tuple(version).map(|_| version.trim().to_owned())
}

impl MenuTargets {
    /// `command` が開く対象。モード切り替えや、対象が無いときは `None`
    pub fn target(&self, command: MenuCommand) -> Option<&Path> {
        match command {
            MenuCommand::SetMode(_) => None,
            MenuCommand::OpenSettingsGui | MenuCommand::CheckUpdate => self.settings_gui.as_deref(),
            MenuCommand::OpenSettings => self.settings_file.as_deref(),
            MenuCommand::OpenSettingsFolder => self.settings_dir.as_deref(),
            MenuCommand::OpenUserDictFolder => self.user_dict_dir.as_deref(),
            MenuCommand::OpenLogFolder => self.log_dir.as_deref(),
            MenuCommand::RunVerify => self.verify_script.as_deref(),
            MenuCommand::ApplyUpdate => self.apply_update_script.as_deref(),
        }
    }
}

/// メニューの対象を決める。`config_path` はコアが教える設定ファイルの場所（user-dir はその親とみなす。
/// 無ければ `%APPDATA%\Nagi`）。`exists` はディレクトリ・ファイルの存在判定（作成はしない）
pub fn resolve_targets(
    config_path: Option<&str>,
    env: &PathEnv,
    exists: impl Fn(&Path) -> bool,
) -> MenuTargets {
    let settings_file = config_path.filter(|p| !p.is_empty()).map(PathBuf::from);
    let default_user_dir = env.app_data.as_ref().map(|d| d.join("Nagi"));
    // user-dir: 設定ファイルの親（存在するとき）→ %APPDATA%\Nagi
    let user_dir = settings_file
        .as_deref()
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty() && exists(p))
        .map(Path::to_path_buf)
        .or_else(|| default_user_dir.filter(|p| exists(p)));
    let user_dict_dir = user_dir.as_ref().map(|d| {
        let sub = d.join("user_dict");
        if exists(&sub) { sub } else { d.clone() }
    });
    let log_dir = env
        .local_app_data
        .as_ref()
        .map(|d| d.join("Nagi").join("logs"))
        .filter(|p| exists(p));
    let settings_gui = env
        .module_dir
        .as_ref()
        .map(|d| d.join(SETTINGS_GUI_EXE))
        .filter(|p| exists(p));
    let verify_script = env
        .module_dir
        .as_ref()
        .map(|d| d.join("verify.ps1"))
        .filter(|p| exists(p));
    // 取得済みの更新は、現在の版より新しいときだけ（手動で更新した後の古い ready.json は無視する）
    let staged = env
        .staged_ready_json
        .as_deref()
        .and_then(staged_version)
        .filter(|staged| {
            let current = env.current_version.as_deref().and_then(version_tuple);
            matches!((version_tuple(staged), current), (Some(s), Some(c)) if s > c)
        });
    let apply_update_script = staged.as_ref().and_then(|_| {
        env.module_dir
            .as_ref()
            .map(|d| d.join("apply-update.ps1"))
            .filter(|p| exists(p))
    });
    let staged_version = apply_update_script.as_ref().and(staged);
    MenuTargets {
        settings_gui,
        settings_file,
        settings_dir: user_dir,
        user_dict_dir,
        log_dir,
        verify_script,
        apply_update_script,
        staged_version,
    }
}

/// `verify.ps1 status` を起動する PowerShell の引数（実行後もコンソールを閉じない）
pub fn verify_arguments(script: &Path) -> String {
    format!(
        "-NoProfile -ExecutionPolicy Bypass -NoExit -File \"{}\" status",
        script.display()
    )
}

/// `apply-update.ps1` を起動する PowerShell の引数（実行後もコンソールを閉じない。昇格はスクリプトが行う。
/// 更新の保存先は既定の `%LOCALAPPDATA%\Nagi\update`）
pub fn apply_update_arguments(script: &Path) -> String {
    format!(
        "-NoProfile -ExecutionPolicy Bypass -NoExit -File \"{}\"",
        script.display()
    )
}

/// 言語バーのボタンのメニュー。現在のモードにチェックを付け、状態の行（`status` が取れなければ
/// [`UNAVAILABLE`]）を挟み、設定・フォルダ・確認の項目は対象が分かるときだけ選べる
pub fn menu_entries(
    mode: Option<InputMode>,
    status: Option<&StatusResult>,
    targets: &MenuTargets,
) -> Vec<MenuEntry> {
    let mode_item = |target: InputMode| MenuEntry::Command {
        id: command_id(MenuCommand::SetMode(target)),
        label: mode_name(target).to_owned(),
        checked: mode == Some(target),
        enabled: true,
    };
    let mut entries = vec![
        mode_item(InputMode::Hiragana),
        mode_item(InputMode::Alphanumeric),
        MenuEntry::Separator,
    ];
    match status {
        Some(s) => entries.extend(menu_lines(s).into_iter().map(MenuEntry::Info)),
        None => entries.push(MenuEntry::Info(UNAVAILABLE.to_owned())),
    }
    entries.push(MenuEntry::Separator);
    let open_items = [
        (MenuCommand::OpenSettingsGui, "設定…"),
        (MenuCommand::CheckUpdate, "更新を確認…"),
        (
            MenuCommand::OpenSettings,
            "設定ファイル（config.toml）を直接開く",
        ),
        (MenuCommand::OpenSettingsFolder, "設定フォルダを開く"),
        (
            MenuCommand::OpenUserDictFolder,
            "ユーザー辞書フォルダを開く",
        ),
        (MenuCommand::OpenLogFolder, "ログフォルダを開く"),
        (MenuCommand::RunVerify, "状態を確認（verify.ps1）"),
    ];
    entries.extend(open_items.map(|(command, label)| MenuEntry::Command {
        id: command_id(command),
        label: label.to_owned(),
        checked: false,
        enabled: targets.target(command).is_some(),
    }));
    // 取得済みの新版があるときだけ「更新を適用」を出す（適用時に UAC の確認が出る）
    if let (Some(_), Some(version)) = (&targets.apply_update_script, &targets.staged_version) {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Command {
            id: command_id(MenuCommand::ApplyUpdate),
            label: format!("更新を適用（{version}）…"),
            checked: false,
            enabled: true,
        });
    }
    entries
}

/// 同じ文言を短時間に繰り返し出さないための間引き
#[derive(Debug, Clone)]
pub struct ToastThrottle {
    interval: Duration,
    last_shown: HashMap<String, Instant>,
}

impl Default for ToastThrottle {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl ToastThrottle {
    /// 間引きの間隔を指定して作る（既定は 30 秒）
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_shown: HashMap::new(),
        }
    }

    /// `message` を今出してよいか。出してよければ記録する
    pub fn should_show(&mut self, message: &str, now: Instant) -> bool {
        if let Some(last) = self.last_shown.get(message)
            && now.saturating_duration_since(*last) < self.interval
        {
            return false;
        }
        self.last_shown.insert(message.to_owned(), now);
        true
    }
}

/// 同じ座標の右クリックが短時間に繰り返し届いたとき、2 回目以降を無視するための判定。
/// 入力インジケーターを 1 回右クリックしても `OnClick` が 3 回呼ばれることがあり（実機 0.1.12）、
/// そのたびにポップアップメニューを出さないために使う（H-96）
#[derive(Debug, Default)]
pub struct ClickDedup {
    last: Option<(Instant, i32, i32)>,
}

impl ClickDedup {
    /// 重複を無視する間隔
    pub const WINDOW: Duration = Duration::from_millis(300);

    /// 直近の記録から [`WINDOW`](Self::WINDOW) 以内で同じ座標なら `true`（無視する）。
    /// いずれの場合も、時刻と座標を記録し直す（メニューを閉じた直後の到着も重複として扱うため、
    /// 呼び出し側は閉じたときにも [`touch`](Self::touch) する）
    pub fn is_duplicate(&mut self, now: Instant, x: i32, y: i32) -> bool {
        let duplicate = self.last.is_some_and(|(at, lx, ly)| {
            lx == x && ly == y && now.saturating_duration_since(at) < Self::WINDOW
        });
        self.last = Some((now, x, y));
        duplicate
    }

    /// 最後の操作の時刻を更新する（座標は変えない）
    pub fn touch(&mut self, now: Instant) {
        if let Some((_, x, y)) = self.last {
            self.last = Some((now, x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientError;
    use nagi_protocol::NotifyParams;

    fn notify(level: NotifyLevel, code: Option<&str>) -> EngineEvent {
        EngineEvent::Notify(NotifyParams {
            level,
            message: "メッセージ".into(),
            code: code.map(str::to_owned),
        })
    }

    #[test]
    fn notify_levels() {
        assert_eq!(toast_for(&notify(NotifyLevel::Info, None)), None);
        assert_eq!(
            toast_for(&notify(NotifyLevel::Warning, None)).as_deref(),
            Some("メッセージ")
        );
        assert!(toast_for(&notify(NotifyLevel::Error, None)).is_some());
        let update = toast_for(&notify(NotifyLevel::Info, Some("update_available"))).unwrap();
        assert!(update.starts_with("メッセージ。"));
        assert!(update.contains("取得中です"), "{update}");
        assert!(update.contains("「Nagi を更新」"), "{update}");
        let ready = toast_for(&notify(NotifyLevel::Info, Some("update_ready"))).unwrap();
        assert_eq!(
            ready,
            "メッセージ。スタートメニューの「Nagi を更新」から適用できます"
        );
    }

    #[test]
    fn engine_failures() {
        let crashed = |attempt| EngineEvent::Crashed {
            attempt,
            delay: Duration::ZERO,
        };
        assert!(toast_for(&crashed(1)).is_some());
        assert_eq!(toast_for(&crashed(2)), None);
        assert!(
            toast_for(&EngineEvent::EnteredMinimalMode { restarts: 5 })
                .unwrap()
                .contains("5 回")
        );
        assert!(toast_for(&EngineEvent::LaunchFailed(ClientError::NotConnected)).is_some());
        // 直らない起動失敗（os error 15623 等）は知らせない
        let blocked = ClientError::SendFailed("spawn: x (os error 15623)".into());
        assert_eq!(toast_for(&EngineEvent::LaunchFailed(blocked)), None);
        assert_eq!(
            toast_for(&EngineEvent::SpawnBlocked {
                code: 15623,
                recheck_in: Duration::from_secs(300)
            }),
            None
        );
        // 番号が直る種類でなければ従来どおり
        let missing = ClientError::SendFailed("spawn: x (os error 2)".into());
        assert!(toast_for(&EngineEvent::LaunchFailed(missing)).is_some());
    }

    #[test]
    fn menu_lines_reflect_status() {
        let mut s = StatusResult {
            profile: "balanced".into(),
            profile_reason: "メモリ 16GB".into(),
            backend: "x".into(),
            llm: true,
            learning: true,
            learned_entries: 12,
            user_dict_entries: 3,
            config_path: None,
            config_error: false,
        };
        let lines = menu_lines(&s);
        assert_eq!(lines[0], "プロファイル: balanced（メモリ 16GB）");
        assert_eq!(lines[1], "変換: LLM + 辞書");
        assert_eq!(lines[2], "学習: 有効（12 件）");
        assert_eq!(lines[3], "ユーザー辞書: 3 語");
        assert_eq!(lines.len(), 4);
        s.llm = false;
        s.learning = false;
        s.config_error = true;
        let lines = menu_lines(&s);
        assert_eq!(lines[1], "変換: 辞書のみ");
        assert_eq!(lines[2], "学習: 無効");
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4], "設定エラー（以前の設定で動作中）");
    }

    #[test]
    fn short_reasons_for_known_phrases() {
        let cases = [
            (
                "キャリブレーション推論 48ms から balanced を選びました",
                "推論 48ms",
            ),
            (
                "キャリブレーション推論は 39ms ですが、メモリ 8192MB・4 コアのため light に制限しました",
                "推論 39ms・メモリ制限",
            ),
            (
                "実測なしのためハードウェアから balanced を選びました",
                "実測なし",
            ),
            (
                "メモリ 2048MB・1 コアのため軽量プロファイルにしました",
                "低メモリ",
            ),
            (
                "変換モデルを読み込み中です（辞書のみで動作中）",
                "モデル読込中",
            ),
            ("変換モデルが使えないため辞書のみで動作します", "モデルなし"),
            ("メモリ逼迫のため light に切り替えました", "メモリ逼迫"),
            ("バッテリー駆動のため light に切り替えました", "バッテリー"),
            ("低電力モードのため light に切り替えました", "省電力"),
            ("サーマル状態の悪化のため light に切り替えました", "発熱"),
            (
                "サーマル状態が危険のため minimal に切り替えました",
                "発熱（危険）",
            ),
            (
                "メモリ逼迫（危険）のため minimal に切り替えました",
                "メモリ逼迫（危険）",
            ),
            (
                "推論レイテンシが目標（100ms）を超過のため light に切り替えました",
                "遅延超過",
            ),
            (
                "バッテリー駆動・低電力モード・サーマル状態の悪化のため light に切り替えました",
                "バッテリー・省電力・発熱",
            ),
            (
                "バッテリー駆動・低電力モードのため light に切り替えました",
                "バッテリー・省電力",
            ),
            (
                "バッテリー駆動・低電力モード・サーマル状態が危険・メモリ逼迫（危険）のため minimal に切り替えました",
                "バッテリー・省電力ほか",
            ),
            (
                "設定（profile.mode = quality）で固定されています",
                "設定で固定",
            ),
            (
                "メモリ上限 1500MB（2048MB 未満）のため balanced に制限しました",
                "メモリ上限",
            ),
            (
                "メモリ上限 500MB（800MB 未満）のため辞書のみで動作します",
                "メモリ上限",
            ),
        ];
        for (reason, expected) in cases {
            assert_eq!(
                short_profile_reason("balanced", reason),
                expected,
                "{reason}"
            );
        }
    }

    #[test]
    fn unknown_reason_is_truncated_by_chars() {
        // 24 文字以下はそのまま
        let exact: String = "あ".repeat(SHORT_REASON_MAX_CHARS);
        assert_eq!(short_profile_reason("light", &exact), exact);
        // 超えると 24 文字で切って「…」
        let long = "未知の理由がとても長く書かれていてメニューが横に伸びてしまう文言";
        let short = short_profile_reason("light", long);
        assert_eq!(short.chars().count(), SHORT_REASON_MAX_CHARS + 1);
        assert!(short.ends_with('…'));
        assert!(long.starts_with(short.trim_end_matches('…')));
        // 結合文字（濁点）の直前では切らない
        let combining = format!(
            "{}か\u{3099}あいう",
            "あ".repeat(SHORT_REASON_MAX_CHARS - 1)
        );
        let short = short_profile_reason("light", &combining);
        assert!(short.contains("か\u{3099}"), "{short}");
        assert!(short.ends_with('…'));
        // 絵文字（サロゲートペア相当の補助平面）も 1 文字として数える
        let emoji = "😀".repeat(30);
        assert_eq!(
            short_profile_reason("light", &emoji).chars().count(),
            SHORT_REASON_MAX_CHARS + 1
        );
    }

    #[test]
    fn menu_lines_stay_short_for_long_reasons() {
        let reasons = [
            "キャリブレーション推論は 39ms ですが、メモリ 8192MB・4 コアのため light に制限しました",
            "バッテリー駆動・低電力モード・サーマル状態の悪化・メモリ逼迫のため minimal に切り替えました",
            "メモリ上限 1500MB（2048MB 未満）のため balanced に制限しました",
            "これは未知でとても長い理由文であり切り詰められるべきものです。まだまだ続きます",
        ];
        for reason in reasons {
            let mut s = sample_status(None);
            s.profile_reason = reason.into();
            s.config_error = true;
            s.llm = true;
            for line in menu_lines(&s) {
                assert!(line.chars().count() <= 48, "{line}");
            }
            let entries = menu_entries(None, Some(&s), &MenuTargets::default());
            assert!(entries.iter().all(|e| match e {
                MenuEntry::Info(l) => l.chars().count() <= 48,
                _ => true,
            }));
        }
        let mut s = sample_status(None);
        s.profile_reason = "キャリブレーション推論 48ms から balanced を選びました".into();
        assert_eq!(menu_lines(&s)[0], "プロファイル: balanced（推論 48ms）");
    }

    #[test]
    fn mode_labels() {
        assert_eq!(mode_indicator(InputMode::Hiragana), "あ");
        assert_eq!(mode_indicator(InputMode::Alphanumeric), "A");
        assert_eq!(mode_name(InputMode::Alphanumeric), "英数");
    }

    fn sample_status(config_path: Option<&str>) -> StatusResult {
        StatusResult {
            profile: "balanced".into(),
            profile_reason: "r".into(),
            backend: "x".into(),
            llm: false,
            learning: true,
            learned_entries: 1,
            user_dict_entries: 0,
            config_path: config_path.map(str::to_owned),
            config_error: false,
        }
    }

    #[test]
    fn menu_ids_round_trip_and_are_unique() {
        let commands = [
            MenuCommand::SetMode(InputMode::Hiragana),
            MenuCommand::SetMode(InputMode::Alphanumeric),
            MenuCommand::OpenSettingsGui,
            MenuCommand::OpenSettings,
            MenuCommand::OpenSettingsFolder,
            MenuCommand::OpenUserDictFolder,
            MenuCommand::OpenLogFolder,
            MenuCommand::RunVerify,
            MenuCommand::ApplyUpdate,
            MenuCommand::CheckUpdate,
        ];
        let mut ids: Vec<u32> = commands.iter().map(|c| command_id(*c)).collect();
        for c in commands {
            assert_eq!(command_from_id(command_id(c)), Some(c));
        }
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), commands.len());
        assert_eq!(command_from_id(0), None);
        assert_eq!(command_from_id(999), None);
    }

    #[test]
    fn toggle_and_button_labels() {
        assert_eq!(toggled(InputMode::Hiragana), InputMode::Alphanumeric);
        assert_eq!(toggled(toggled(InputMode::Hiragana)), InputMode::Hiragana);
        assert_eq!(button_text(Some(InputMode::Alphanumeric)), "A");
        assert_eq!(button_text(None), "あ");
        assert_eq!(button_tooltip(Some(InputMode::Hiragana)), "Nagi: ひらがな");
        assert_eq!(button_tooltip(None), "Nagi");
    }

    fn all_targets() -> MenuTargets {
        let p = |s: &str| Some(PathBuf::from(s));
        MenuTargets {
            settings_gui: p("C:/p/nagi-settings.exe"),
            settings_file: p("C:/n/config.toml"),
            settings_dir: p("C:/n"),
            user_dict_dir: p("C:/n/user_dict"),
            log_dir: p("C:/l/Nagi/logs"),
            verify_script: p("C:/p/verify.ps1"),
            ..MenuTargets::default()
        }
    }

    fn command_entries(entries: &[MenuEntry]) -> Vec<(MenuCommand, bool)> {
        entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { id, enabled, .. } => {
                    Some((command_from_id(*id).expect("既知の ID"), *enabled))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn menu_checks_current_mode_and_lists_open_items() {
        let status = sample_status(Some("C:/Users/x/AppData/Roaming/Nagi/config.toml"));
        let entries = menu_entries(Some(InputMode::Alphanumeric), Some(&status), &all_targets());
        let checked: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { label, checked, .. } if *checked => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(checked, ["英数"]);
        // 末尾の 7 項目は開く操作（順序込み）で、対象があるので全部選べる
        let commands = command_entries(&entries);
        assert_eq!(
            commands[2..],
            [
                (MenuCommand::OpenSettingsGui, true),
                (MenuCommand::CheckUpdate, true),
                (MenuCommand::OpenSettings, true),
                (MenuCommand::OpenSettingsFolder, true),
                (MenuCommand::OpenUserDictFolder, true),
                (MenuCommand::OpenLogFolder, true),
                (MenuCommand::RunVerify, true),
            ]
        );
        assert!(matches!(
            entries.iter().rev().nth(6),
            Some(MenuEntry::Command { label, .. }) if label == "設定…"
        ));
        assert!(matches!(
            entries.iter().rev().nth(5),
            Some(MenuEntry::Command { label, .. }) if label == "更新を確認…"
        ));
        assert!(matches!(
            entries.iter().rev().nth(4),
            Some(MenuEntry::Command { label, .. }) if label == "設定ファイル（config.toml）を直接開く"
        ));
        // 状態の行（menu_lines）が入る
        assert!(entries.contains(&MenuEntry::Info("変換: 辞書のみ".into())));
        // 区切り線は 2 本だけ（長くしすぎない）
        assert_eq!(
            entries
                .iter()
                .filter(|e| matches!(e, MenuEntry::Separator))
                .count(),
            2
        );

        // 対象が無い・状態が取れないときは開く項目が灰色（モードは選べる）
        for st in [Some(sample_status(None)), None] {
            let entries = menu_entries(None, st.as_ref(), &MenuTargets::default());
            let commands = command_entries(&entries);
            assert!(commands[..2].iter().all(|(_, enabled)| *enabled));
            assert!(commands[2..].iter().all(|(_, enabled)| !*enabled));
            // モード不明ならチェックは付けない
            assert!(
                !entries
                    .iter()
                    .any(|e| matches!(e, MenuEntry::Command { checked: true, .. }))
            );
        }
        let none = menu_entries(Some(InputMode::Hiragana), None, &all_targets());
        assert!(none.contains(&MenuEntry::Info(UNAVAILABLE.into())));
    }

    fn env() -> PathEnv {
        PathEnv {
            app_data: Some(PathBuf::from("/ad")),
            local_app_data: Some(PathBuf::from("/lad")),
            module_dir: Some(PathBuf::from("/pf/Nagi")),
            ..PathEnv::default()
        }
    }

    fn existing<'a>(paths: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
        move |p| paths.iter().any(|x| Path::new(x) == p)
    }

    #[test]
    fn resolve_targets_prefers_config_parent_and_user_dict() {
        let t = resolve_targets(
            Some("/u/Nagi/config.toml"),
            &env(),
            existing(&[
                "/u/Nagi",
                "/u/Nagi/user_dict",
                "/lad/Nagi/logs",
                "/pf/Nagi/verify.ps1",
                "/pf/Nagi/nagi-settings.exe",
            ]),
        );
        assert_eq!(
            t.settings_gui.as_deref(),
            Some(Path::new("/pf/Nagi/nagi-settings.exe"))
        );
        assert_eq!(
            t.target(MenuCommand::OpenSettingsGui),
            Some(Path::new("/pf/Nagi/nagi-settings.exe"))
        );
        assert_eq!(
            t.settings_file.as_deref(),
            Some(Path::new("/u/Nagi/config.toml"))
        );
        assert_eq!(t.settings_dir.as_deref(), Some(Path::new("/u/Nagi")));
        assert_eq!(
            t.user_dict_dir.as_deref(),
            Some(Path::new("/u/Nagi/user_dict"))
        );
        assert_eq!(t.log_dir.as_deref(), Some(Path::new("/lad/Nagi/logs")));
        assert_eq!(
            t.verify_script.as_deref(),
            Some(Path::new("/pf/Nagi/verify.ps1"))
        );
        assert_eq!(
            t.target(MenuCommand::RunVerify),
            Some(Path::new("/pf/Nagi/verify.ps1"))
        );
        assert_eq!(t.target(MenuCommand::SetMode(InputMode::Hiragana)), None);
    }

    #[test]
    fn resolve_targets_falls_back_and_never_requires_creation() {
        // user_dict が無ければ user-dir、config_path が無ければ %APPDATA%\Nagi
        let t = resolve_targets(None, &env(), existing(&["/ad/Nagi"]));
        assert_eq!(t.settings_file, None);
        assert_eq!(t.settings_dir.as_deref(), Some(Path::new("/ad/Nagi")));
        assert_eq!(t.user_dict_dir.as_deref(), Some(Path::new("/ad/Nagi")));
        assert_eq!(t.log_dir, None);
        assert_eq!(t.verify_script, None);
        assert_eq!(t.settings_gui, None);

        // config_path の親が無ければ %APPDATA%\Nagi へ。空の config_path は無いものとして扱う
        let t = resolve_targets(Some("/gone/config.toml"), &env(), existing(&["/ad/Nagi"]));
        assert_eq!(t.settings_dir.as_deref(), Some(Path::new("/ad/Nagi")));
        assert!(t.settings_file.is_some());
        let t = resolve_targets(Some(""), &env(), existing(&[]));
        assert_eq!(t, MenuTargets::default());

        // 環境変数が全く無い
        let t = resolve_targets(None, &PathEnv::default(), existing(&["/ad/Nagi"]));
        assert_eq!(t, MenuTargets::default());
    }

    #[test]
    fn verify_arguments_quote_the_script_and_keep_the_console() {
        let a = verify_arguments(Path::new("C:/Program Files/Nagi/verify.ps1"));
        assert_eq!(
            a,
            "-NoProfile -ExecutionPolicy Bypass -NoExit -File \"C:/Program Files/Nagi/verify.ps1\" status"
        );
    }

    #[test]
    fn apply_update_arguments_quote_the_script_and_keep_the_console() {
        assert_eq!(
            apply_update_arguments(Path::new("C:/Program Files/Nagi/apply-update.ps1")),
            "-NoProfile -ExecutionPolicy Bypass -NoExit -File \"C:/Program Files/Nagi/apply-update.ps1\""
        );
    }

    fn staged_env(ready: Option<&str>, current: Option<&str>) -> PathEnv {
        PathEnv {
            staged_ready_json: ready.map(str::to_owned),
            current_version: current.map(str::to_owned),
            ..env()
        }
    }

    #[test]
    fn staged_version_reads_and_validates_ready_json() {
        assert_eq!(
            staged_version(r#"{"version":"0.2.0","sha256":"x"}"#).as_deref(),
            Some("0.2.0")
        );
        assert_eq!(staged_version(r#"{"version":"../0.2.0"}"#), None);
        assert_eq!(staged_version(r#"{"version":2}"#), None);
        assert_eq!(staged_version("not json"), None);
        assert_eq!(staged_version("{}"), None);
    }

    #[test]
    fn apply_update_needs_newer_staged_version_and_script() {
        let present = ["/pf/Nagi/apply-update.ps1"];
        let ready = r#"{"version":"0.2.0"}"#;
        // 新しい版が取得済みで、スクリプトもある
        let t = resolve_targets(
            None,
            &staged_env(Some(ready), Some("0.1.12")),
            existing(&present),
        );
        assert_eq!(
            t.target(MenuCommand::ApplyUpdate),
            Some(Path::new("/pf/Nagi/apply-update.ps1"))
        );
        assert_eq!(t.staged_version.as_deref(), Some("0.2.0"));
        // 現在の版と同じ・古い（手動で更新済み）・ready.json が無い・版が不明・スクリプトが無い
        for (ready, current, files) in [
            (Some(ready), Some("0.2.0"), &present[..]),
            (Some(ready), Some("0.10.0"), &present[..]),
            (None, Some("0.1.0"), &present[..]),
            (Some(ready), None, &present[..]),
            (Some(ready), Some("0.1.0"), &[][..]),
        ] {
            let t = resolve_targets(None, &staged_env(ready, current), existing(files));
            assert_eq!(t.apply_update_script, None);
            assert_eq!(t.staged_version, None);
        }
        // 0.9 < 0.10（文字列ではなく数値で比べる）
        let t = resolve_targets(
            None,
            &staged_env(Some(r#"{"version":"0.10.0"}"#), Some("0.9.0")),
            existing(&present),
        );
        assert_eq!(t.staged_version.as_deref(), Some("0.10.0"));
    }

    #[test]
    fn menu_shows_apply_update_only_when_staged() {
        let without = menu_entries(None, None, &all_targets());
        assert!(
            !command_entries(&without)
                .iter()
                .any(|(c, _)| *c == MenuCommand::ApplyUpdate)
        );
        let mut targets = all_targets();
        targets.apply_update_script = Some(PathBuf::from("C:/p/apply-update.ps1"));
        targets.staged_version = Some("0.2.0".into());
        let with = menu_entries(None, None, &targets);
        assert_eq!(
            command_entries(&with).last(),
            Some(&(MenuCommand::ApplyUpdate, true))
        );
        assert!(matches!(
            with.last(),
            Some(MenuEntry::Command { label, .. }) if label == "更新を適用（0.2.0）…"
        ));
    }

    #[test]
    fn command_kinds_are_distinct() {
        let mut kinds = vec![
            MenuCommand::SetMode(InputMode::Hiragana).kind(),
            MenuCommand::OpenSettings.kind(),
            MenuCommand::OpenSettingsFolder.kind(),
            MenuCommand::OpenUserDictFolder.kind(),
            MenuCommand::OpenLogFolder.kind(),
            MenuCommand::RunVerify.kind(),
            MenuCommand::ApplyUpdate.kind(),
        ];
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), 7);
    }

    #[test]
    fn throttle_suppresses_repeats_within_interval() {
        let mut t = ToastThrottle::new(Duration::from_secs(30));
        let t0 = Instant::now();
        assert!(t.should_show("a", t0));
        assert!(!t.should_show("a", t0 + Duration::from_secs(10)));
        assert!(t.should_show("b", t0 + Duration::from_secs(10)));
        assert!(t.should_show("a", t0 + Duration::from_secs(30)));
    }

    #[test]
    fn click_dedup_ignores_same_point_within_the_window() {
        let t0 = Instant::now();
        let mut d = ClickDedup::default();
        assert!(!d.is_duplicate(t0, 10, 20));
        assert!(d.is_duplicate(t0 + Duration::from_millis(100), 10, 20));
        // 記録は更新されるので、最後の重複から 300ms 未満ならまた重複
        assert!(d.is_duplicate(t0 + Duration::from_millis(350), 10, 20));
        // 座標が違えば別の操作
        assert!(!d.is_duplicate(t0 + Duration::from_millis(360), 11, 20));
        // 間隔が空けば別の操作
        assert!(!d.is_duplicate(t0 + Duration::from_millis(700), 11, 20));
    }

    #[test]
    fn click_dedup_touch_extends_the_window() {
        let t0 = Instant::now();
        let mut d = ClickDedup::default();
        assert!(!d.is_duplicate(t0, 1, 2));
        d.touch(t0 + Duration::from_secs(5));
        assert!(d.is_duplicate(
            t0 + Duration::from_secs(5) + Duration::from_millis(50),
            1,
            2
        ));
    }
}
