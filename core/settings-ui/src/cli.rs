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
    /// `--no-browser`: ブラウザを開かない（検証用）
    pub no_browser: bool,
    /// `--url-file <path>`: 画面の URL（トークンつき）をこのファイルに書く（検証用。既定では出力しない）
    pub url_file: Option<PathBuf>,
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
            "--no-browser" => out.no_browser = true,
            other => return Err(format!("不明な引数: {other}")),
        }
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
        ])
        .unwrap();
        assert_eq!(a.imserver, Some(PathBuf::from("/x/imserver")));
        assert_eq!(a.data_dir, Some(PathBuf::from("/d")));
        assert_eq!(a.user_dir, Some(PathBuf::from("/u")));
        assert!(a.no_browser);
        assert_eq!(a.url_file, Some(PathBuf::from("/f")));
        assert_eq!(parse(&[]).unwrap(), Args::default());
        assert!(parse(&["--data-dir"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
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
