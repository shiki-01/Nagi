//! 診断の実行コンテキスト（場所・タイムアウト・OS への問い合わせ）。

use super::sys::{RealSystem, System, WinFacts};
use crate::cli::{Args, default_data_dir, default_imserver, default_update_dir};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// 待ち時間の上限
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// imserver の `initialize`（辞書・モデルの読み込みを待つ）
    pub initialize: Duration,
    /// 通常の JSON-RPC 要求
    pub request: Duration,
    /// 初回の `reconvert`（逆引き索引の構築を待つことがある）
    pub reconvert: Duration,
    /// 終了・親消滅の検出
    pub exit: Duration,
    /// 更新確認（`imserver --check-update`）
    pub update: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            initialize: Duration::from_secs(60),
            request: Duration::from_secs(15),
            reconvert: Duration::from_secs(60),
            exit: Duration::from_secs(10),
            update: Duration::from_secs(40),
        }
    }
}

/// 診断の実行コンテキスト。フィールドは公開（UI・CLI が上書きしてよい）
pub struct Ctx {
    /// インストール先（`nagi-settings.exe` と同じディレクトリ）
    pub install_dir: PathBuf,
    /// `imserver` の実行ファイル
    pub imserver_exe: PathBuf,
    /// 辞書・モデルの置き場（`--data-dir`）。決まらなければ `None`
    pub data_dir: Option<PathBuf>,
    /// 利用者の設定ディレクトリ（`config.toml` など）。診断は読み取りと書き込みテストだけで、
    /// imserver には渡さない（学習データを汚さない）
    pub user_dir: Option<PathBuf>,
    /// 更新の保存先（実機の値。診断の更新確認は一時ディレクトリを使う）
    pub update_dir: Option<PathBuf>,
    /// ログの置き場（`%LOCALAPPDATA%\Nagi\logs`）
    pub log_dir: Option<PathBuf>,
    /// 診断用の作業ディレクトリ（imserver の `--user-dir`・更新確認の保存先。実行ごとに作り、終わったら消す）
    pub scratch_root: PathBuf,
    /// OS 名（`windows` など。`std::env::consts::OS`）
    pub os: String,
    /// この診断プログラムの CPU（`std::env::consts::ARCH`）
    pub arch: String,
    /// 実際のサーバーへの通信を許すか（`false` なら更新確認を実サーバーに送らない）
    pub allow_network: bool,
    /// 待ち時間
    pub timeouts: Timeouts,
    /// OS への問い合わせ
    pub system: Arc<dyn System>,
    /// 中断の合図（`run_all` が項目の間で見る。UI のキャンセルボタン用）
    pub cancel: Arc<AtomicBool>,
    /// 表示用に置き換えるパスの接頭辞（長い順。ユーザー名を報告に残さない）
    aliases: Vec<(String, &'static str)>,
    facts: OnceLock<Option<WinFacts>>,
}

impl Ctx {
    /// 引数（`nagi-settings` のコマンドライン）と環境から作る
    pub fn from_args(args: &Args) -> Result<Ctx, String> {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Ok(Ctx::resolve(
            args,
            &env,
            std::env::current_exe().ok(),
            std::env::consts::OS,
            Arc::new(RealSystem),
        ))
    }

    /// 環境・OS 名を注入して作る（テストでも使う）
    pub fn resolve(
        args: &Args,
        env: &dyn Fn(&str) -> Option<String>,
        current_exe: Option<PathBuf>,
        os: &str,
        system: Arc<dyn System>,
    ) -> Ctx {
        let install_dir = current_exe
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let imserver_exe = args
            .imserver
            .clone()
            .or_else(|| current_exe.as_deref().and_then(default_imserver))
            .unwrap_or_else(|| PathBuf::from(crate::cli::imserver_file_name()));
        let data_dir = args
            .data_dir
            .clone()
            .or_else(|| env("NAGI_DATA_DIR").map(PathBuf::from))
            .or_else(|| default_data_dir(os, env));
        let user_dir = args
            .user_dir
            .clone()
            .or_else(|| env("NAGI_CONFIG_DIR").map(PathBuf::from))
            .or_else(|| {
                (os == "windows")
                    .then(|| env("APPDATA"))
                    .flatten()
                    .map(|a| join_windows(&a, "Nagi"))
            });
        let update_dir = args
            .update_dir
            .clone()
            .or_else(|| default_update_dir(os, env));
        let log_dir =
            env("LOCALAPPDATA").map(|l| join_windows(&join_windows_str(&l, "Nagi"), "logs"));
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let scratch_root = std::env::temp_dir().join(format!(
            "nagi-diag-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut aliases: Vec<(String, &'static str)> = Vec::new();
        for (var, label) in [
            ("LOCALAPPDATA", "%LOCALAPPDATA%"),
            ("APPDATA", "%APPDATA%"),
            ("ProgramFiles", "%ProgramFiles%"),
            ("USERPROFILE", "%USERPROFILE%"),
            ("TEMP", "%TEMP%"),
        ] {
            if let Some(v) = env(var) {
                let v = v.trim_end_matches(['\\', '/']).to_owned();
                if !v.is_empty() {
                    aliases.push((v, label));
                }
            }
        }
        aliases.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        Ctx {
            install_dir,
            imserver_exe,
            data_dir,
            user_dir,
            update_dir,
            log_dir,
            scratch_root,
            os: os.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            allow_network: true,
            timeouts: Timeouts::default(),
            system,
            cancel: Arc::new(AtomicBool::new(false)),
            aliases,
            facts: OnceLock::new(),
        }
    }

    /// Windows か
    pub fn is_windows(&self) -> bool {
        self.system.is_windows()
    }

    /// Windows の事実（最初の呼び出しで PowerShell を 1 回だけ実行する）
    pub fn facts(&self) -> Option<&WinFacts> {
        self.facts
            .get_or_init(|| self.system.facts(&self.install_dir))
            .as_ref()
    }

    /// 中断が要求されたか
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// 作業ディレクトリの下に名前つきのサブディレクトリを作って返す
    pub fn scratch(&self, name: &str) -> std::io::Result<PathBuf> {
        let dir = self.scratch_root.join(name);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 作業ディレクトリを消す
    pub fn cleanup_scratch(&self) {
        let _ = std::fs::remove_dir_all(&self.scratch_root);
    }

    /// パスを報告用に整える（ユーザー名を含む接頭辞を `%USERPROFILE%` などに置き換える）
    pub fn show(&self, path: &Path) -> String {
        self.redact(&path.display().to_string())
    }

    /// 文字列の中のユーザー固有のパスを置き換える（大文字小文字を区別しない）
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (prefix, label) in &self.aliases {
            out = replace_ignore_ascii_case(&out, prefix, label);
        }
        out
    }
}

/// `base` と `child` を Windows の区切り（`\`）で結ぶ（実行 OS に依存させない）
fn join_windows(base: &str, child: &str) -> PathBuf {
    PathBuf::from(join_windows_str(base, child))
}

fn join_windows_str(base: &str, child: &str) -> String {
    format!("{}\\{child}", base.trim_end_matches(['\\', '/']))
}

/// `from` を（ASCII の大文字小文字を無視して）すべて `to` に置き換える
fn replace_ignore_ascii_case(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_owned();
    }
    // ASCII の小文字化はバイト位置を変えないので、小文字版で探した位置を元の文字列にそのまま使える
    let hay = text.to_ascii_lowercase();
    let needle = from.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    while let Some(pos) = hay[last..].find(&needle) {
        let start = last + pos;
        out.push_str(&text[last..start]);
        out.push_str(to);
        last = start + needle.len();
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::diag::testutil::FakeSystem;

    pub(crate) fn env_of(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<String> {
        move |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn resolve_uses_windows_defaults() {
        let env = env_of(&[
            ("APPDATA", "C:\\Users\\alice\\AppData\\Roaming"),
            ("LOCALAPPDATA", "C:\\Users\\alice\\AppData\\Local"),
            ("USERPROFILE", "C:\\Users\\alice"),
        ]);
        let ctx = Ctx::resolve(
            &Args::default(),
            &env,
            Some(PathBuf::from("/pf/Nagi/nagi-settings.exe")),
            "windows",
            Arc::new(FakeSystem::default()),
        );
        assert_eq!(ctx.install_dir, PathBuf::from("/pf/Nagi"));
        assert_eq!(
            ctx.data_dir,
            Some(PathBuf::from(
                "C:\\Users\\alice\\AppData\\Roaming\\Nagi\\data"
            ))
        );
        assert_eq!(
            ctx.user_dir,
            Some(PathBuf::from("C:\\Users\\alice\\AppData\\Roaming\\Nagi"))
        );
        assert_eq!(
            ctx.update_dir,
            Some(PathBuf::from(
                "C:\\Users\\alice\\AppData\\Local\\Nagi\\update"
            ))
        );
        assert_eq!(
            ctx.log_dir,
            Some(PathBuf::from(
                "C:\\Users\\alice\\AppData\\Local\\Nagi\\logs"
            ))
        );
        assert!(ctx.imserver_exe.ends_with(crate::cli::imserver_file_name()));
    }

    #[test]
    fn explicit_args_win() {
        let args = Args {
            imserver: Some(PathBuf::from("/x/imserver")),
            data_dir: Some(PathBuf::from("/d")),
            user_dir: Some(PathBuf::from("/u")),
            ..Args::default()
        };
        let ctx = Ctx::resolve(
            &args,
            &env_of(&[("NAGI_DATA_DIR", "/env-data")]),
            None,
            "linux",
            Arc::new(FakeSystem::default()),
        );
        assert_eq!(ctx.imserver_exe, PathBuf::from("/x/imserver"));
        assert_eq!(ctx.data_dir, Some(PathBuf::from("/d")));
        assert_eq!(ctx.user_dir, Some(PathBuf::from("/u")));
        let ctx = Ctx::resolve(
            &Args::default(),
            &env_of(&[("NAGI_DATA_DIR", "/env-data")]),
            None,
            "linux",
            Arc::new(FakeSystem::default()),
        );
        assert_eq!(ctx.data_dir, Some(PathBuf::from("/env-data")));
    }

    #[test]
    fn redact_hides_user_specific_prefixes_longest_first_and_case_insensitively() {
        let env = env_of(&[
            ("LOCALAPPDATA", "C:\\Users\\alice\\AppData\\Local"),
            ("USERPROFILE", "C:\\Users\\alice"),
            ("ProgramFiles", "C:\\Program Files"),
        ]);
        let ctx = Ctx::resolve(
            &Args::default(),
            &env,
            None,
            "windows",
            Arc::new(FakeSystem::default()),
        );
        assert_eq!(
            ctx.redact("c:\\users\\ALICE\\appdata\\local\\Nagi\\logs and C:\\Users\\alice\\x"),
            "%LOCALAPPDATA%\\Nagi\\logs and %USERPROFILE%\\x"
        );
        assert_eq!(
            ctx.redact("C:\\Program Files\\Nagi"),
            "%ProgramFiles%\\Nagi"
        );
        assert_eq!(ctx.redact("nothing"), "nothing");
    }

    #[test]
    fn replace_handles_non_ascii_neighbours() {
        assert_eq!(
            replace_ignore_ascii_case("あAbcい abc", "ABC", "X"),
            "あXい X"
        );
    }
}
