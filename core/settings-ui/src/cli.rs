//! コマンドライン引数と、既定の場所の解決。

use std::path::{Path, PathBuf};

/// 引数
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    /// `--imserver <path>`: imserver の場所（既定は自身と同じディレクトリ）
    pub imserver: Option<PathBuf>,
    /// `--data-dir <dir>`
    pub data_dir: Option<PathBuf>,
    /// `--user-dir <dir>`
    pub user_dir: Option<PathBuf>,
    /// `--web`: ウィンドウではなく、従来の Web 画面（ローカル HTTP + 既定のブラウザ）で開く。
    /// Windows 以外では常に Web 画面
    pub web: bool,
    /// `--native`: Win32 のネイティブ画面で開く（Windows のみ。WebView2 があっても使わない）
    pub native: bool,
    /// `--webview`: WebView2 のウィンドウで開く（Windows のみ。使えなければフォールバックせずエラーにする）
    pub webview: bool,
    /// `--no-browser`: ブラウザを開かない（検証用。`--web` のときだけ意味を持つ）
    pub no_browser: bool,
    /// `--url-file <path>`: 画面の URL（トークンつき）をこのファイルに書く（検証用。既定では出力しない。`--web` のときだけ）
    pub url_file: Option<PathBuf>,
    /// `--update-dir <dir>`: 取得した更新の保存先（既定は Windows の `%LOCALAPPDATA%\Nagi\update`）
    pub update_dir: Option<PathBuf>,
    /// `--section <name>`: 開いたときに表示する節（`update` のみ）
    pub section: Option<String>,
    /// `--auto-check`: 開いたら更新の確認を自動で実行する（`--section update` と併用）
    pub auto_check: bool,
    /// `--diag`: 画面を開かず、実機の診断（`diag`）を実行してレポートを出力する
    pub diag: bool,
    /// `--diag-item <id>`: 診断する項目を 1 つに絞る（`--diag` のとき）
    pub diag_item: Option<String>,
    /// `--diag-out <dir>`: レポートの保存先（`--diag` のとき。既定はログフォルダ）
    pub diag_out: Option<PathBuf>,
    /// `--diag-json`: レポートを JSON で標準出力に出す（`--diag` のとき）
    pub diag_json: bool,
    /// `--no-network`: 診断で実サーバーに通信しない（`--diag` のとき）
    pub diag_no_network: bool,
}

impl Args {
    /// 更新ページを開いた状態で始めるか（`--section update` または `--auto-check`）
    pub fn opens_update_page(&self) -> bool {
        self.section.as_deref() == Some("update") || self.auto_check
    }

    /// ブラウザで開く URL に付ける断片（`update` = 更新セクションへスクロール、
    /// `update-check` = さらに確認を自動実行）。`--auto-check` だけでも更新セクションを開く
    pub fn start_fragment(&self) -> Option<&'static str> {
        match (self.section.as_deref(), self.auto_check) {
            (Some("update"), true) | (None, true) => Some("update-check"),
            (Some("update"), false) => Some("update"),
            _ => None,
        }
    }
}

/// 引数を解析する
pub fn parse_args<I: Iterator<Item = String>>(mut args: I) -> Result<Args, String> {
    let mut out = Args::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{name} に値がありません"))
        };
        match arg.as_str() {
            "--imserver" => out.imserver = Some(value("--imserver")?),
            "--data-dir" => out.data_dir = Some(value("--data-dir")?),
            "--user-dir" => out.user_dir = Some(value("--user-dir")?),
            "--url-file" => out.url_file = Some(value("--url-file")?),
            "--update-dir" => out.update_dir = Some(value("--update-dir")?),
            "--section" => {
                let name = value("--section")?.to_string_lossy().into_owned();
                if name != "update" {
                    return Err(format!("--section は update だけです: {name}"));
                }
                out.section = Some(name);
            }
            "--auto-check" => out.auto_check = true,
            "--diag" => out.diag = true,
            "--diag-item" => {
                out.diag_item = Some(value("--diag-item")?.to_string_lossy().into_owned());
            }
            "--diag-out" => out.diag_out = Some(value("--diag-out")?),
            "--diag-json" => out.diag_json = true,
            "--no-network" => out.diag_no_network = true,
            "--no-browser" => out.no_browser = true,
            "--web" => out.web = true,
            "--native" => out.native = true,
            "--webview" => out.webview = true,
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    if [out.web, out.native, out.webview]
        .iter()
        .filter(|&&flag| flag)
        .count()
        > 1
    {
        return Err("--web / --native / --webview は 1 つだけ指定できます".to_owned());
    }
    Ok(out)
}

/// `imserver` の実行ファイル名
pub fn imserver_file_name() -> &'static str {
    if cfg!(windows) {
        "imserver.exe"
    } else {
        "imserver"
    }
}

/// 既定の `imserver` の場所（自身の exe と同じディレクトリ）
pub fn default_imserver(current_exe: &Path) -> Option<PathBuf> {
    Some(current_exe.parent()?.join(imserver_file_name()))
}

/// `--data-dir` が無いときの既定。`NAGI_DATA_DIR` があれば `None`（imserver 自身が読む）。
/// `NAGI_CONFIG_DIR` があれば `<それ>/data`、Windows は `%APPDATA%\Nagi\data`（TSF 側の既定と同じ）。
/// 決まらなければ `None`（imserver に任せる）
pub fn default_data_dir(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if env("NAGI_DATA_DIR").is_some() {
        return None;
    }
    if let Some(dir) = env("NAGI_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("data"));
    }
    if os == "windows" {
        let app_data = env("APPDATA")?;
        // Windows の区切り文字は実行 OS に依存させない（他 OS 上のテストでも同じ結果にする）
        return Some(PathBuf::from(format!(
            "{}\\Nagi\\data",
            app_data.trim_end_matches(['\\', '/'])
        )));
    }
    None
}

/// 取得した更新の既定の保存先。Windows は `%LOCALAPPDATA%\Nagi\update`
/// （TSF 側が imserver に渡す `--update-dir` と同じ場所。`frontend-windows-tsf` の `engine_location`）。
/// それ以外・決まらなければ `None`（imserver の既定 = `<user-dir>/update`）
pub fn default_update_dir(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if os != "windows" {
        return None;
    }
    let local = env("LOCALAPPDATA")?;
    Some(PathBuf::from(format!(
        "{}\\Nagi\\update",
        local.trim_end_matches(['\\', '/'])
    )))
}

/// `apply-update.ps1` の場所（自身の exe と同じディレクトリ）
pub fn default_apply_script(current_exe: &Path) -> Option<PathBuf> {
    Some(current_exe.parent()?.join("apply-update.ps1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_all_options() {
        let a = parse(&[
            "--imserver",
            "/x/imserver",
            "--data-dir",
            "/d",
            "--user-dir",
            "/u",
            "--no-browser",
            "--url-file",
            "/f",
            "--web",
        ])
        .unwrap();
        assert!(a.web);
        assert_eq!(a.imserver, Some(PathBuf::from("/x/imserver")));
        assert_eq!(a.data_dir, Some(PathBuf::from("/d")));
        assert_eq!(a.user_dir, Some(PathBuf::from("/u")));
        assert!(a.no_browser);
        assert_eq!(a.url_file, Some(PathBuf::from("/f")));
        assert_eq!(parse(&[]).unwrap(), Args::default());
        let b = parse(&["--section", "update", "--auto-check", "--update-dir", "/up"]).unwrap();
        assert_eq!(b.section.as_deref(), Some("update"));
        assert!(b.auto_check);
        assert_eq!(b.update_dir, Some(PathBuf::from("/up")));
        assert!(parse(&["--section", "other"]).is_err());
        let d = parse(&[
            "--diag",
            "--diag-item",
            "env.arch",
            "--diag-out",
            "/o",
            "--diag-json",
            "--no-network",
        ])
        .unwrap();
        assert!(d.diag && d.diag_json && d.diag_no_network);
        assert_eq!(d.diag_item.as_deref(), Some("env.arch"));
        assert_eq!(d.diag_out, Some(PathBuf::from("/o")));
        assert!(!parse(&[]).unwrap().diag);
        assert!(parse(&["--data-dir"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
    }

    #[test]
    fn update_page_flags_and_exclusive_mode_flags() {
        let none = parse(&[]).unwrap();
        assert!(!none.web && !none.native && !none.webview);
        assert!(parse(&["--native"]).unwrap().native);
        assert!(parse(&["--webview"]).unwrap().webview);
        // 画面の方式は 1 つだけ
        assert!(parse(&["--web", "--native"]).is_err());
        assert!(parse(&["--native", "--webview"]).is_err());
        assert!(parse(&["--web", "--webview"]).is_err());
        assert!(!none.opens_update_page());
        assert!(parse(&["--section", "update"]).unwrap().opens_update_page());
        assert!(parse(&["--auto-check"]).unwrap().opens_update_page());
    }

    #[test]
    fn start_fragment_follows_section_and_auto_check() {
        let f = |a: &[&str]| parse(a).unwrap().start_fragment();
        assert_eq!(f(&[]), None);
        assert_eq!(f(&["--section", "update"]), Some("update"));
        assert_eq!(
            f(&["--section", "update", "--auto-check"]),
            Some("update-check")
        );
        assert_eq!(f(&["--auto-check"]), Some("update-check"));
    }

    #[test]
    fn default_update_dir_is_local_app_data_on_windows_only() {
        let env =
            |k: &str| (k == "LOCALAPPDATA").then(|| "C:\\Users\\x\\AppData\\Local\\".to_owned());
        assert_eq!(
            default_update_dir("windows", &env),
            Some(PathBuf::from("C:\\Users\\x\\AppData\\Local\\Nagi\\update"))
        );
        assert_eq!(default_update_dir("linux", &env), None);
        assert_eq!(default_update_dir("windows", &|_| None), None);
    }

    #[test]
    fn default_apply_script_is_next_to_the_exe() {
        assert_eq!(
            default_apply_script(Path::new("/pf/Nagi/nagi-settings.exe")),
            Some(Path::new("/pf/Nagi").join("apply-update.ps1"))
        );
    }

    #[test]
    fn default_imserver_is_next_to_the_exe() {
        let p = default_imserver(Path::new("/pf/Nagi/nagi-settings.exe")).unwrap();
        assert_eq!(p, Path::new("/pf/Nagi").join(imserver_file_name()));
    }

    #[test]
    fn default_data_dir_follows_env_then_appdata() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            default_data_dir(
                "windows",
                &env(&[("APPDATA", "C:\\Users\\x\\AppData\\Roaming\\")])
            ),
            Some(PathBuf::from("C:\\Users\\x\\AppData\\Roaming\\Nagi\\data"))
        );
        assert_eq!(
            default_data_dir(
                "windows",
                &env(&[("NAGI_DATA_DIR", "/d"), ("APPDATA", "C:\\a")])
            ),
            None
        );
        assert_eq!(
            default_data_dir(
                "windows",
                &env(&[("NAGI_CONFIG_DIR", "/c"), ("APPDATA", "C:\\a")])
            ),
            Some(PathBuf::from("/c").join("data"))
        );
        assert_eq!(default_data_dir("linux", &env(&[("APPDATA", "x")])), None);
        assert_eq!(default_data_dir("windows", &env(&[])), None);
    }
}
