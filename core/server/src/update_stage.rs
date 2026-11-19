//! Windows の自動更新（`docs/decisions.md` H-95）。
//!
//! 流れは 2 段に分かれる。
//!
//! 1. **取得（`imserver --stage-updates`、利用者権限）**: 起動時の更新確認で新版が見つかったら、
//!    [`crate::update_check::download_verified`]（SHA-256 → ed25519）で検証した配布物を
//!    `<user_dir>/update/Nagi-windows-<version>.zip` に原子的に保存し、`ready.json` を書く
//!    （[`stage_update`]）。複数のホストプロセスが同時に走っても壊れないよう、一時ファイル名に pid を含める。
//! 2. **適用前の再検証（`imserver --apply-update-prepare`、昇格後に Program Files 側の imserver が実行）**:
//!    保存済みのファイルを**ゼロから再検証**し、検証に使ったバイト列をそのまま管理者専用の出力先へ
//!    書き出す（[`prepare_apply`]）。展開と配置は `apply-update.ps1` が行う。
//!
//! **ユーザーが書き込める場所（`<user_dir>/update/`）のファイルは信頼しない。** 取得時に検証済みでも、
//! 保存後に差し替えられうる（TOCTOU）ため、適用時に必ず再検証し、検証した同じバイト列を書き出す
//! （検証後に再読込しない）。`ready.json` の版数も署名の対象外なので、zip の中の `VERSION`（署名対象）との
//! 一致は `apply-update.ps1` が確かめる。また、現在の版以下の更新は拒否する（ダウングレード・リプレイ対策）。

use crate::update_check::{
    Manifest, parse_version, sha256_hex, update_public_key_hex, verify_download,
};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// `<user_dir>` の下の保存先ディレクトリ名
pub const DIR_NAME: &str = "update";
/// 保存済みの版の情報（コミットの印。zip を先に置き、これを最後に書く）
pub const READY_FILE: &str = "ready.json";
/// `ready.json` の最大サイズ（これを超えるものは壊れている／不正とみなす）
const MAX_READY_BYTES: u64 = 64 * 1024;
/// 配布物 zip の最大サイズ（メモリに全部載せるため上限を置く）
const MAX_ZIP_BYTES: u64 = 256 * 1024 * 1024;
/// 異常終了で残った一時ファイルを片付ける経過時間
const STALE_TMP_AGE: Duration = Duration::from_secs(60 * 60);

/// `ready.json` の内容。`version` 以外はマニフェストの写し
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyInfo {
    /// 保存した版（"x.y.z"）
    pub version: String,
    /// 配布物の SHA-256（小文字16進）
    pub sha256: String,
    /// 配布物への ed25519 署名（16進）
    pub signature: String,
    /// 取得元
    pub download_url: String,
    /// リリースノートの URL
    #[serde(default)]
    pub notes_url: Option<String>,
    /// 保存した時刻（UNIX 秒）
    pub staged_at: u64,
}

/// [`stage_update`] の結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcome {
    /// すでに同じ版が保存済みだった（取得していない）
    AlreadyStaged,
    /// 取得して保存した
    Staged,
}

/// 版数として安全な文字列か（1〜3 個の数字だけの要素を `.` で連結したもの）。
/// ファイル名に埋め込むため、パス区切りや `..` を通さない
pub fn is_valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    (1..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// 保存する zip のファイル名（`Nagi-windows-<version>.zip`。`cargo xtask bundle-windows` と同じ）
pub fn zip_file_name(version: &str) -> String {
    format!("Nagi-windows-{version}.zip")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 上限つきでファイルを全部読む。上限を超えたらエラー
fn read_capped(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("{} を開けません: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("{} が大きすぎます", path.display()));
    }
    Ok(bytes)
}

/// `ready.json` を読む（無い・壊れている・版数が不正なら `None`）
pub fn read_ready(update_dir: &Path) -> Option<ReadyInfo> {
    let bytes = read_capped(&update_dir.join(READY_FILE), MAX_READY_BYTES).ok()?;
    let info: ReadyInfo = serde_json::from_slice(&bytes).ok()?;
    is_valid_version(&info.version).then_some(info)
}

/// マニフェストと同じ版・同じ配布物がすでに保存済みか（`ready.json` と zip の存在で判断する。
/// 中身の検証は適用時に行う）
pub fn is_staged(update_dir: &Path, manifest: &Manifest) -> bool {
    let (Some(sha), Some(ready)) = (manifest.sha256.as_deref(), read_ready(update_dir)) else {
        return false;
    };
    ready.version == manifest.version
        && ready.sha256.eq_ignore_ascii_case(sha)
        && std::fs::metadata(update_dir.join(zip_file_name(&ready.version)))
            .is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// ファイルの SHA-256 が `expected_sha256` と一致するか
fn file_matches(path: &Path, expected_sha256: &str) -> bool {
    read_capped(path, MAX_ZIP_BYTES)
        .map(|b| sha256_hex(&b).eq_ignore_ascii_case(expected_sha256))
        .unwrap_or(false)
}

/// 一時ファイルに書いてから名前を変える。変更に失敗しても、すでに同じ内容（他のホストが先に
/// 置いたもの）が最終位置にあれば成功とみなす（Windows では使用中のファイルへの rename が失敗しうる）
fn write_atomic(dest: &Path, bytes: &[u8], verify_sha256: &str) -> Result<(), String> {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| "保存先の名前が不正です".to_string())?;
    let tmp = dest.with_file_name(format!("{name}.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, bytes)
        .map_err(|e| format!("{} に書けません: {e}", tmp.display()))
        .and_then(|()| {
            let renamed = std::fs::rename(&tmp, dest);
            // rename 後に確認する: 最終位置の中身が期待どおりであること
            if file_matches(dest, verify_sha256) {
                Ok(())
            } else {
                Err(format!(
                    "{} への保存を確認できません: {}",
                    dest.display(),
                    renamed
                        .err()
                        .map_or("内容が一致しません".to_string(), |e| e.to_string())
                ))
            }
        });
    let _ = std::fs::remove_file(&tmp);
    result
}

/// 版が異なる保存済みの zip と、古い一時ファイルを消す。他のホストが書いている最中の一時ファイルは
/// 触らない（一定時間より新しいものは残す）
fn cleanup_stale(update_dir: &Path, keep_zip: &str) {
    let Ok(entries) = std::fs::read_dir(update_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stale = if name.starts_with("Nagi-windows-") && name.ends_with(".zip") {
            name != keep_zip
        } else if name.ends_with(".tmp") {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STALE_TMP_AGE)
        } else {
            false
        };
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// 新版 `manifest` を保存する。すでに同じ版が保存済みなら取得しない。
/// `fetch` は検証済みの配布物のバイト列を返す（実際は `download_verified`。テストで差し替える）。
/// 保存後に、古い版の保存ファイルを消す
pub fn stage_update(
    update_dir: &Path,
    manifest: &Manifest,
    fetch: impl FnOnce(&Manifest) -> Result<Vec<u8>, String>,
) -> Result<StageOutcome, String> {
    if !is_valid_version(&manifest.version) {
        return Err("マニフェストの版数が不正です".to_string());
    }
    let (Some(sha256), Some(signature), Some(download_url)) = (
        manifest.sha256.as_deref(),
        manifest.signature.as_deref(),
        manifest.download_url.as_deref(),
    ) else {
        return Err(
            "マニフェストに download_url / sha256 / signature が揃っていません".to_string(),
        );
    };
    if is_staged(update_dir, manifest) {
        return Ok(StageOutcome::AlreadyStaged);
    }
    let bytes = fetch(manifest)?;
    if bytes.len() as u64 > MAX_ZIP_BYTES {
        return Err("配布物が大きすぎます".to_string());
    }
    // 取得関数が検証済みでも、ready.json に書く値と中身を一致させておく
    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err(format!("SHA-256 が一致しません（実際 {actual}）"));
    }
    std::fs::create_dir_all(update_dir)
        .map_err(|e| format!("{} を作れません: {e}", update_dir.display()))?;

    let zip_name = zip_file_name(&manifest.version);
    write_atomic(&update_dir.join(&zip_name), &bytes, &actual)?;

    let ready = ReadyInfo {
        version: manifest.version.clone(),
        sha256: actual.clone(),
        signature: signature.to_string(),
        download_url: download_url.to_string(),
        notes_url: manifest.notes_url.clone(),
        staged_at: unix_now(),
    };
    let json = serde_json::to_vec_pretty(&ready).map_err(|e| e.to_string())?;
    let ready_path = update_dir.join(READY_FILE);
    let tmp = update_dir.join(format!("{READY_FILE}.{}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, &json)
        .and_then(|()| std::fs::rename(&tmp, &ready_path))
        .map_err(|e| format!("{} を書けません: {e}", ready_path.display()));
    let _ = std::fs::remove_file(&tmp);
    // rename 後の確認（他のホストが先に同じ内容を置いていれば、rename が失敗しても可）
    if read_ready(update_dir).is_none_or(|r| r.version != ready.version || r.sha256 != actual) {
        return Err(written
            .err()
            .unwrap_or_else(|| "ready.json を確認できません".to_string()));
    }
    cleanup_stale(update_dir, &zip_name);
    Ok(StageOutcome::Staged)
}

/// 保存済みの版が現在の版以下（手動更新済みなど）なら、保存ファイルを消す
pub fn discard_obsolete(update_dir: &Path, current_version: &str) {
    let Some(ready) = read_ready(update_dir) else {
        return;
    };
    if parse_version(&ready.version) <= parse_version(current_version) {
        let _ = std::fs::remove_file(update_dir.join(READY_FILE));
        cleanup_stale(update_dir, "");
    }
}

/// `--apply-update-prepare` の引数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareArgs {
    /// 保存済みの版のあるディレクトリ（利用者の `%LOCALAPPDATA%\Nagi\update`）
    pub update_dir: PathBuf,
    /// 検証済みの zip を書き出す先（管理者専用の staging）
    pub out_dir: PathBuf,
}

/// 引数に `--apply-update-prepare` があれば [`PrepareArgs`] を返す（無ければ `None`）。
/// 受け付ける引数は `--apply-update-prepare` `--update-dir` `--out` のみ
pub fn parse_prepare_args<I: Iterator<Item = String>>(
    args: I,
) -> Result<Option<PrepareArgs>, String> {
    let args: Vec<String> = args.collect();
    if !args.iter().any(|a| a == "--apply-update-prepare") {
        return Ok(None);
    }
    let (mut update_dir, mut out_dir) = (None, None);
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--apply-update-prepare" => {}
            "--update-dir" => {
                update_dir =
                    Some(PathBuf::from(it.next().ok_or_else(|| {
                        "--update-dir に値がありません".to_string()
                    })?));
            }
            "--out" => {
                out_dir = Some(PathBuf::from(
                    it.next()
                        .ok_or_else(|| "--out に値がありません".to_string())?,
                ));
            }
            other => {
                return Err(format!(
                    "--apply-update-prepare と併用できない引数: {other}"
                ));
            }
        }
    }
    Ok(Some(PrepareArgs {
        update_dir: update_dir
            .ok_or_else(|| "--apply-update-prepare には --update-dir が必要です".to_string())?,
        out_dir: out_dir
            .ok_or_else(|| "--apply-update-prepare には --out が必要です".to_string())?,
    }))
}

/// 適用前の再検証。成功したら検証済みの zip を `out_dir/Nagi-windows-<version>.zip` に書き出し、版数を返す。
///
/// `update_dir` は利用者が書き込める場所なので何も信頼しない: `ready.json` と zip を読み、SHA-256 と
/// ed25519 署名（`public_key_hex`）をゼロから検証し、**検証に使ったバイト列をそのまま**書き出す。
/// 版数が `current_version` 以下なら拒否する
pub fn prepare_apply_with(
    args: &PrepareArgs,
    current_version: &str,
    public_key_hex: &str,
) -> Result<String, String> {
    let ready_bytes = read_capped(&args.update_dir.join(READY_FILE), MAX_READY_BYTES)
        .map_err(|e| format!("保存済みの更新がありません: {e}"))?;
    let ready: ReadyInfo = serde_json::from_slice(&ready_bytes)
        .map_err(|e| format!("ready.json を解析できません: {e}"))?;
    if !is_valid_version(&ready.version) {
        return Err("ready.json の版数が不正です".to_string());
    }
    if parse_version(&ready.version) <= parse_version(current_version) {
        return Err(format!(
            "保存済みの版 {} は現在の版 {current_version} より新しくありません（更新しません）",
            ready.version
        ));
    }
    let zip_name = zip_file_name(&ready.version);
    // 以降、この `bytes` だけを使う（検証後にファイルを読み直さない）
    let bytes = read_capped(&args.update_dir.join(&zip_name), MAX_ZIP_BYTES)?;
    verify_download(&bytes, &ready.sha256, &ready.signature, public_key_hex)?;
    std::fs::create_dir_all(&args.out_dir)
        .map_err(|e| format!("{} を作れません: {e}", args.out_dir.display()))?;
    let dest = args.out_dir.join(&zip_name);
    std::fs::write(&dest, &bytes).map_err(|e| format!("{} に書けません: {e}", dest.display()))?;
    Ok(ready.version)
}

/// [`prepare_apply_with`] を、この imserver の版数と組み込みの公開鍵で実行する
pub fn prepare_apply(args: &PrepareArgs) -> Result<String, String> {
    prepare_apply_with(args, env!("CARGO_PKG_VERSION"), update_public_key_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    // update_check のテストと同じ、openssl で作った既知の鍵・メッセージ・署名（秘密鍵は破棄済み）
    const TEST_PUBLIC_KEY_HEX: &str =
        "48880411a8c51a1e707022d84b711e9ed883a20143eb1734da57d11877fa3abc";
    const TEST_MESSAGE: &[u8] = b"nagi-update-test";
    const TEST_SIGNATURE_HEX: &str = "d28314b502a250ea0471de96a5ec949bf254473f0ce3cc4c1078099e1434ef478a56a49bb49a7b60a25409be2342914b7d36782288e088885d1b83f5e824bc02";

    struct Temp(PathBuf);
    impl Temp {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("nagi-update-stage-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Temp(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn manifest(version: &str, signature: &str) -> Manifest {
        Manifest {
            version: version.to_string(),
            notes_url: Some("https://example.invalid/notes".to_string()),
            download_url: Some("https://example.invalid/x.zip".to_string()),
            sha256: Some(sha256_hex(TEST_MESSAGE)),
            signature: Some(signature.to_string()),
        }
    }

    /// ネットワークの代わりに既知のバイト列を返し、実際の検証関数（テスト用の鍵）を通す
    fn fake_fetch(m: &Manifest) -> Result<Vec<u8>, String> {
        let bytes = TEST_MESSAGE.to_vec();
        verify_download(
            &bytes,
            m.sha256.as_deref().unwrap(),
            m.signature.as_deref().unwrap(),
            TEST_PUBLIC_KEY_HEX,
        )?;
        Ok(bytes)
    }

    #[test]
    fn version_validation_rejects_path_like_values() {
        assert!(is_valid_version("0.1.12"));
        assert!(is_valid_version("1.2"));
        assert!(!is_valid_version(""));
        assert!(!is_valid_version("1..2"));
        assert!(!is_valid_version("1.2.3.4"));
        assert!(!is_valid_version("../1.0.0"));
        assert!(!is_valid_version("1.0.0/x"));
        assert!(!is_valid_version("1.0.0-beta"));
    }

    #[test]
    fn stage_writes_zip_and_ready_json() {
        let t = Temp::new("write");
        let dir = t.0.join("update");
        let m = manifest("9.0.0", TEST_SIGNATURE_HEX);
        assert_eq!(
            stage_update(&dir, &m, fake_fetch).unwrap(),
            StageOutcome::Staged
        );
        assert_eq!(
            std::fs::read(dir.join("Nagi-windows-9.0.0.zip")).unwrap(),
            TEST_MESSAGE
        );
        let ready = read_ready(&dir).unwrap();
        assert_eq!(ready.version, "9.0.0");
        assert_eq!(ready.sha256, sha256_hex(TEST_MESSAGE));
        assert_eq!(ready.signature, TEST_SIGNATURE_HEX);
        assert_eq!(ready.download_url, "https://example.invalid/x.zip");
        assert!(ready.notes_url.is_some());
        assert!(is_staged(&dir, &m));
        // 一時ファイルが残らない
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn stage_skips_fetch_when_same_version_is_already_staged() {
        let t = Temp::new("again");
        let dir = t.0.join("update");
        let m = manifest("9.0.0", TEST_SIGNATURE_HEX);
        stage_update(&dir, &m, fake_fetch).unwrap();
        let outcome = stage_update(&dir, &m, |_| panic!("再取得してはいけない")).unwrap();
        assert_eq!(outcome, StageOutcome::AlreadyStaged);
    }

    #[test]
    fn stage_does_not_save_when_signature_is_bad() {
        let t = Temp::new("badsig");
        let dir = t.0.join("update");
        // 別の署名（先頭 1 バイトを変えて不正にする）
        let bad = format!("00{}", &TEST_SIGNATURE_HEX[2..]);
        let m = manifest("9.0.0", &bad);
        let err = stage_update(&dir, &m, fake_fetch).unwrap_err();
        assert!(err.contains("署名"), "{err}");
        assert!(!dir.join("Nagi-windows-9.0.0.zip").exists());
        assert!(!dir.join(READY_FILE).exists());
    }

    #[test]
    fn stage_rejects_unsafe_version_and_missing_fields() {
        let t = Temp::new("unsafe");
        let dir = t.0.join("update");
        let m = manifest("../../evil", TEST_SIGNATURE_HEX);
        assert!(stage_update(&dir, &m, |_| panic!("取得してはいけない")).is_err());
        let mut m = manifest("9.0.0", TEST_SIGNATURE_HEX);
        m.download_url = None;
        assert!(stage_update(&dir, &m, |_| panic!("取得してはいけない")).is_err());
    }

    #[test]
    fn stage_removes_older_staged_versions() {
        let t = Temp::new("older");
        let dir = t.0.join("update");
        stage_update(&dir, &manifest("9.0.0", TEST_SIGNATURE_HEX), fake_fetch).unwrap();
        stage_update(&dir, &manifest("9.1.0", TEST_SIGNATURE_HEX), fake_fetch).unwrap();
        assert!(!dir.join("Nagi-windows-9.0.0.zip").exists());
        assert!(dir.join("Nagi-windows-9.1.0.zip").exists());
        assert_eq!(read_ready(&dir).unwrap().version, "9.1.0");
    }

    #[test]
    fn discard_obsolete_removes_current_or_older_only() {
        let t = Temp::new("discard");
        let dir = t.0.join("update");
        stage_update(&dir, &manifest("9.0.0", TEST_SIGNATURE_HEX), fake_fetch).unwrap();
        discard_obsolete(&dir, "8.9.9");
        assert!(read_ready(&dir).is_some());
        discard_obsolete(&dir, "9.0.0");
        assert!(read_ready(&dir).is_none());
        assert!(!dir.join("Nagi-windows-9.0.0.zip").exists());
    }

    #[test]
    fn parse_prepare_args_cases() {
        let v = |a: &[&str]| {
            a.iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .into_iter()
        };
        assert_eq!(parse_prepare_args(v(&["--data-dir", "x"])).unwrap(), None);
        let a = parse_prepare_args(v(&[
            "--apply-update-prepare",
            "--update-dir",
            "u",
            "--out",
            "o",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(a.update_dir, PathBuf::from("u"));
        assert_eq!(a.out_dir, PathBuf::from("o"));
        assert!(parse_prepare_args(v(&["--apply-update-prepare", "--out", "o"])).is_err());
        assert!(parse_prepare_args(v(&["--apply-update-prepare", "--no-llm"])).is_err());
    }

    #[test]
    fn stage_updates_flag_is_off_by_default_and_coexists_with_no_llm() {
        let parse = |a: &[&str]| {
            crate::setup::parse_args(
                a.iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .into_iter(),
            )
            .unwrap()
        };
        assert!(!parse(&["--data-dir", "d"]).stage_updates);
        let o = parse(&["--data-dir", "d", "--stage-updates", "--no-llm"]);
        assert!(o.stage_updates && !o.use_llm);
    }

    fn staged_args(t: &Temp, version: &str) -> PrepareArgs {
        let dir = t.0.join("update");
        stage_update(&dir, &manifest(version, TEST_SIGNATURE_HEX), fake_fetch).unwrap();
        PrepareArgs {
            update_dir: dir,
            out_dir: t.0.join("staging"),
        }
    }

    #[test]
    fn prepare_writes_verified_zip() {
        let t = Temp::new("prep-ok");
        let args = staged_args(&t, "9.0.0");
        let v = prepare_apply_with(&args, "0.1.0", TEST_PUBLIC_KEY_HEX).unwrap();
        assert_eq!(v, "9.0.0");
        assert_eq!(
            std::fs::read(args.out_dir.join("Nagi-windows-9.0.0.zip")).unwrap(),
            TEST_MESSAGE
        );
    }

    #[test]
    fn prepare_rejects_same_or_older_version() {
        let t = Temp::new("prep-down");
        let args = staged_args(&t, "9.0.0");
        assert!(prepare_apply_with(&args, "9.0.0", TEST_PUBLIC_KEY_HEX).is_err());
        assert!(prepare_apply_with(&args, "9.5.0", TEST_PUBLIC_KEY_HEX).is_err());
        assert!(!args.out_dir.join("Nagi-windows-9.0.0.zip").exists());
    }

    #[test]
    fn prepare_detects_tampering_after_staging() {
        let t = Temp::new("prep-tamper");
        let args = staged_args(&t, "9.0.0");
        // 保存後に zip が差し替えられた
        std::fs::write(args.update_dir.join("Nagi-windows-9.0.0.zip"), b"evil").unwrap();
        let err = prepare_apply_with(&args, "0.1.0", TEST_PUBLIC_KEY_HEX).unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert!(!args.out_dir.join("Nagi-windows-9.0.0.zip").exists());

        // ready.json の sha256 も合わせて差し替えられても、署名で弾く
        let mut ready = read_ready(&args.update_dir).unwrap();
        ready.sha256 = sha256_hex(b"evil");
        std::fs::write(
            args.update_dir.join(READY_FILE),
            serde_json::to_vec(&ready).unwrap(),
        )
        .unwrap();
        let err = prepare_apply_with(&args, "0.1.0", TEST_PUBLIC_KEY_HEX).unwrap_err();
        assert!(err.contains("署名"), "{err}");
        assert!(!args.out_dir.join("Nagi-windows-9.0.0.zip").exists());
    }

    #[test]
    fn prepare_rejects_wrong_key_and_missing_ready() {
        let t = Temp::new("prep-key");
        let args = staged_args(&t, "9.0.0");
        assert!(prepare_apply_with(&args, "0.1.0", &"0".repeat(64)).is_err());
        let empty = PrepareArgs {
            update_dir: t.0.join("none"),
            out_dir: t.0.join("o"),
        };
        assert!(prepare_apply_with(&empty, "0.1.0", TEST_PUBLIC_KEY_HEX).is_err());
    }
}
