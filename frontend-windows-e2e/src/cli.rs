//! コマンドライン引数。

use std::path::PathBuf;

/// 使い方
pub const USAGE: &str = "\
使い方: nagi-e2e-win --dll <nagi_windows_tsf.dll> [オプション]

  --dll <path>        IME の DLL（同じディレクトリに imserver.exe を置くこと）。必須
  --scenarios <dir>   シナリオのディレクトリ（既定 tests/e2e/scenarios）
  --fixtures <dir>    設定とユーザー辞書（既定 tests/e2e/fixtures/user-dir。一時ディレクトリへコピーして使う）
  --data-dir <dir>    imserver の data-dir（`cargo xtask e2e-fixture-data --out <dir>` の出力。語彙なしの最小辞書。
                      一時の %APPDATA%/Nagi/data へコピーする。未指定なら辞書なし）
  --out <dir>         results.json と screenshots/ の出力先（既定 e2e-out）
  --filter <text>     `スイート名/シナリオ名` に text を含むものだけ実行
  --skip-register     regsvr32 での登録・解除を行わない（登録済みの前提）
  --keep-registered   終了時に登録を解除しない
  --ready-timeout-ms <n>  IME の起動（imserver の接続）を待つ上限（既定 30000）
";

/// 解釈済みの引数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// IME の DLL
    pub dll: PathBuf,
    /// シナリオのディレクトリ
    pub scenarios: PathBuf,
    /// フィクスチャ（user-dir）
    pub fixtures: PathBuf,
    /// imserver の data-dir の元
    pub data_dir: Option<PathBuf>,
    /// 出力先
    pub out: PathBuf,
    /// 絞り込み
    pub filter: Option<String>,
    /// 登録・解除をしない
    pub skip_register: bool,
    /// 終了時に解除しない
    pub keep_registered: bool,
    /// IME 起動待ちの上限（ミリ秒）
    pub ready_timeout_ms: u64,
}

/// 引数を解釈する
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut dll = None;
    let mut out = Args {
        dll: PathBuf::new(),
        scenarios: PathBuf::from("tests/e2e/scenarios"),
        fixtures: PathBuf::from("tests/e2e/fixtures/user-dir"),
        data_dir: None,
        out: PathBuf::from("e2e-out"),
        filter: None,
        skip_register: false,
        keep_registered: false,
        ready_timeout_ms: 30_000,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} に値がありません"));
        match arg.as_str() {
            "--dll" => dll = Some(PathBuf::from(value("--dll")?)),
            "--scenarios" => out.scenarios = PathBuf::from(value("--scenarios")?),
            "--fixtures" => out.fixtures = PathBuf::from(value("--fixtures")?),
            "--data-dir" => out.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--out" => out.out = PathBuf::from(value("--out")?),
            "--filter" => out.filter = Some(value("--filter")?),
            "--ready-timeout-ms" => {
                out.ready_timeout_ms = value("--ready-timeout-ms")?
                    .parse()
                    .map_err(|_| "--ready-timeout-ms は整数で指定してください".to_owned())?;
            }
            "--skip-register" => out.skip_register = true,
            "--keep-registered" => out.keep_registered = true,
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    out.dll = dll.ok_or("--dll は必須です")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Args, String> {
        parse(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_options_and_defaults() {
        let a = p(&["--dll", "a.dll", "--filter", "x", "--keep-registered"]).unwrap();
        assert_eq!(a.dll, PathBuf::from("a.dll"));
        assert_eq!(a.filter.as_deref(), Some("x"));
        assert!(a.keep_registered && !a.skip_register);
        assert_eq!(a.out, PathBuf::from("e2e-out"));
        assert_eq!(a.ready_timeout_ms, 30_000);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(p(&[]).is_err());
        assert!(p(&["--dll"]).is_err());
        assert!(p(&["--dll", "a", "--nope"]).is_err());
        assert!(p(&["--dll", "a", "--ready-timeout-ms", "x"]).is_err());
    }
}
