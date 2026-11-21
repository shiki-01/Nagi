//! 自動更新の診断（H-95・H-100）: 実サーバーでの更新確認と、失敗系のシミュレーション。
//!
//! 失敗系は 127.0.0.1 の小さな HTTP サーバー（`tiny_http`）を立て、診断用の `config.toml`
//! （`[update] check_url`）でそこへ向けた `imserver --check-update --stage-updates` を実行して、
//! **拒否されること**と**何も保存されないこと**を確認する（`apply-update.ps1` の昇格・適用は実行しない）。
//! 保存先は診断用の作業ディレクトリで、利用者の更新フォルダ（`%LOCALAPPDATA%\Nagi\update`）には触れない。

use super::ctx::Ctx;
use super::sha256::sha256_hex;
use super::types::{ItemId, ItemResult};
use crate::backend::{UpdateCheck, UpdateStatus, parse_update_output};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// 偽の配布物の細工
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tamper {
    /// チェックサムが配布物と合わない
    BadChecksum,
    /// チェックサムは合うが、署名が不正
    BadSignature,
}

/// 偽のマニフェスト・配布物を返すローカルサーバー
pub struct MockServer {
    port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

const FAKE_VERSION: &str = "99.0.0";
const FAKE_ZIP: &str = "Nagi-windows-99.0.0.zip";

impl MockServer {
    /// 起動する
    pub fn start(tamper: Tamper) -> Result<Self, String> {
        let server = tiny_http::Server::http("127.0.0.1:0")
            .map_err(|e| format!("診断用サーバーを起動できません: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| "診断用サーバーのポートを取得できません".to_owned())?;
        let payload: Vec<u8> = b"nagi-diag dummy payload (not a real update)\n".repeat(64);
        let sha = match tamper {
            Tamper::BadChecksum => "00".repeat(32),
            Tamper::BadSignature => sha256_hex(&payload),
        };
        let manifest = serde_json::json!({
            "version": FAKE_VERSION,
            "download_url": format!("http://127.0.0.1:{port}/{FAKE_ZIP}"),
            "sha256": sha,
            "signature": "00".repeat(64),
        })
        .to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(100)) else {
                    continue;
                };
                let path = request.url().split('?').next().unwrap_or("").to_owned();
                let response = match path.as_str() {
                    "/latest.json" => tiny_http::Response::from_data(manifest.clone().into_bytes())
                        .with_status_code(200),
                    p if p == format!("/{FAKE_ZIP}") => {
                        tiny_http::Response::from_data(payload.clone()).with_status_code(200)
                    }
                    _ => tiny_http::Response::from_data(Vec::new()).with_status_code(404),
                };
                let _ = request.respond(response);
            }
        });
        Ok(Self {
            port,
            stop,
            handle: Some(handle),
        })
    }

    /// マニフェストの URL
    pub fn manifest_url(&self) -> String {
        format!("http://127.0.0.1:{}/latest.json", self.port)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// 閉じたポート（誰も待ち受けていない）の URL。オフラインの擬似に使う
pub fn closed_port_url() -> Result<String, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("ポートを確保できません（{}）", e.kind()))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("ポートを取得できません（{}）", e.kind()))?
        .port();
    drop(listener);
    Ok(format!("http://127.0.0.1:{port}/latest.json"))
}

/// `imserver --check-update` を実行する。`check_url` があれば診断用の設定で差し替える
fn run_check(
    ctx: &Ctx,
    scratch_name: &str,
    check_url: Option<&str>,
    stage: bool,
) -> Result<(UpdateCheck, PathBuf), String> {
    if !ctx.imserver_exe.is_file() {
        return Err(format!(
            "imserver が見つかりません（{}）",
            ctx.show(&ctx.imserver_exe)
        ));
    }
    let dir = ctx
        .scratch(scratch_name)
        .map_err(|e| format!("作業フォルダを作れません（{}）", e.kind()))?;
    let update_dir = dir.join("update");
    if let Some(url) = check_url {
        std::fs::write(
            dir.join("config.toml"),
            format!("[update]\nauto_check = true\ncheck_url = \"{url}\"\n"),
        )
        .map_err(|e| format!("診断用の設定を書けません（{}）", e.kind()))?;
    }
    let mut command = Command::new(&ctx.imserver_exe);
    command.arg("--check-update");
    if stage {
        command.arg("--stage-updates");
    }
    command
        .arg("--update-dir")
        .arg(&update_dir)
        .arg("--user-dir")
        .arg(&dir);
    if let Some(d) = &ctx.data_dir {
        command.arg("--data-dir").arg(d);
    }
    command
        .arg("--parent-pid")
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("更新の確認を起動できません（{}）", e.kind()))?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Err("更新の確認の出力を開けません".to_owned());
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    match rx.recv_timeout(ctx.timeouts.update) {
        Ok(text) => {
            let _ = child.wait();
            let check = parse_update_output(&text).map_err(|e| e.message)?;
            Ok((check, update_dir))
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!(
                "更新の確認が {} 秒以内に終わりませんでした（固まった）",
                ctx.timeouts.update.as_secs()
            ))
        }
    }
}

/// 保存先に残っているファイル名（`.tmp`・`ready.json`・zip の有無を見る）
fn staged_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// 実サーバーでの更新確認の判定
pub fn judge_real_check(check: &UpdateCheck) -> ItemResult {
    let id = ItemId::UpdateCheck;
    let mut lines = vec![format!("現在の版: {}", check.current)];
    if let Some(l) = &check.latest {
        lines.push(format!("最新版: {l}"));
    }
    match check.status {
        UpdateStatus::UpToDate => {
            ItemResult::ok(id, format!("最新版です（{}）", check.current)).with_evidence(lines)
        }
        UpdateStatus::Available | UpdateStatus::Staged => ItemResult::ok(
            id,
            format!(
                "新しい版 {} があります。確認自体は成功しました",
                check.latest.as_deref().unwrap_or("?")
            ),
        )
        .with_evidence(lines),
        UpdateStatus::Error => ItemResult::warn(
            id,
            format!(
                "更新を確認できませんでした: {}",
                check.error.as_deref().unwrap_or("理由不明")
            ),
            "インターネットに接続できるか、会社のプロキシ・ファイアウォールが nagi.shiki-01.com を遮断していないか確認してください。接続できているのに失敗する場合は、この理由を報告してください。",
        )
        .with_evidence(lines),
    }
}

/// 実サーバーでの更新確認（取得はしない）
pub fn check_real(ctx: &Ctx) -> ItemResult {
    let id = ItemId::UpdateCheck;
    if !ctx.allow_network {
        return ItemResult::skipped(id, "通信を許可していないため実サーバーには確認しません");
    }
    match run_check(ctx, "update-real", None, false) {
        Ok((check, _)) => judge_real_check(&check),
        Err(e) => ItemResult::ng(
            id,
            e,
            "imserver の起動項目（engine.start）を確認してください。",
        ),
    }
}

/// オフラインの判定
pub fn judge_offline(check: &UpdateCheck, leftovers: &[String]) -> ItemResult {
    let id = ItemId::UpdateOffline;
    let lines = vec![
        format!(
            "status: {:?} / 理由: {}",
            check.status,
            check.error.as_deref().unwrap_or("なし")
        ),
        format!("保存先に残ったファイル: {}", leftovers.len()),
    ];
    if check.status != UpdateStatus::Error {
        return ItemResult::ng(
            id,
            "到達できないサーバーなのに、エラーになりませんでした",
            "更新確認の失敗の扱いに不具合があります。報告してください。",
        )
        .with_evidence(lines);
    }
    if check.error.as_deref().is_none_or(str::is_empty) {
        return ItemResult::warn(
            id,
            "失敗しましたが、理由のメッセージが空です（利用者に原因が伝わりません）",
            "報告してください。",
        )
        .with_evidence(lines);
    }
    if !leftovers.is_empty() {
        return ItemResult::warn(
            id,
            "失敗したのに保存先にファイルが残っています",
            "報告してください。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(
        id,
        "到達できないサーバーに対して、固まらずに理由つきのエラーで終了しました",
    )
    .with_evidence(lines)
}

/// オフラインのシミュレーション
pub fn check_offline(ctx: &Ctx) -> ItemResult {
    let id = ItemId::UpdateOffline;
    let url = match closed_port_url() {
        Ok(u) => u,
        Err(e) => return ItemResult::warn(id, e, "もう一度実行してください。"),
    };
    match run_check(ctx, "update-offline", Some(&url), true) {
        Ok((check, dir)) => judge_offline(&check, &staged_entries(&dir)),
        Err(e) => ItemResult::ng(
            id,
            e,
            "到達できないサーバーで固まる場合は、ネットワークが不通のときに更新確認が固まる不具合です。報告してください。",
        ),
    }
}

/// 細工した配布物の判定。拒否され、何も保存されていなければ合格
pub fn judge_tampered(
    id: ItemId,
    tamper: Tamper,
    check: &UpdateCheck,
    leftovers: &[String],
) -> ItemResult {
    let keyword = match tamper {
        Tamper::BadChecksum => "SHA-256",
        Tamper::BadSignature => "署名",
    };
    let what = match tamper {
        Tamper::BadChecksum => "チェックサムが合わない配布物",
        Tamper::BadSignature => "署名が不正な配布物",
    };
    let lines = vec![
        format!(
            "status: {:?} / 理由: {}",
            check.status,
            check.error.as_deref().unwrap_or("なし")
        ),
        format!(
            "保存先のファイル: {}",
            if leftovers.is_empty() {
                "なし".to_owned()
            } else {
                leftovers.join(", ")
            }
        ),
    ];
    if check.status != UpdateStatus::Error {
        return ItemResult::ng(
            id,
            format!("{what}を受け入れました（status = {:?}）", check.status),
            "重大です。偽の更新を取得・保存できる状態です。すぐに報告してください（更新機能は使わないでください）。",
        )
        .with_evidence(lines);
    }
    if !leftovers.is_empty() {
        return ItemResult::ng(
            id,
            format!("{what}は拒否しましたが、保存先にファイルが残りました"),
            "検証に失敗した配布物が保存されると、後で適用される恐れがあります。報告してください。",
        )
        .with_evidence(lines);
    }
    let reason = check.error.as_deref().unwrap_or("");
    if !reason.contains(keyword) {
        return ItemResult::warn(
            id,
            format!("{what}は拒否されましたが、理由が想定と違います（{reason}）"),
            "診断用サーバーに届いていない可能性があります（プロキシ設定・セキュリティソフトが 127.0.0.1 への通信を遮断していないか）。理由を報告してください。",
        )
        .with_evidence(lines);
    }
    ItemResult::ok(
        id,
        format!("{what}を拒否し、何も保存しませんでした（理由に「{keyword}」を含む）"),
    )
    .with_evidence(lines)
}

fn check_tampered(ctx: &Ctx, id: ItemId, tamper: Tamper, scratch: &str) -> ItemResult {
    let server = match MockServer::start(tamper) {
        Ok(s) => s,
        Err(e) => return ItemResult::warn(id, e, "もう一度実行してください。"),
    };
    let result = run_check(ctx, scratch, Some(&server.manifest_url()), true);
    drop(server);
    match result {
        Ok((check, dir)) => judge_tampered(id, tamper, &check, &staged_entries(&dir)),
        Err(e) => ItemResult::ng(
            id,
            e,
            "imserver の起動項目（engine.start）を確認してください。",
        ),
    }
}

/// 署名不正・チェックサム不正のシミュレーション（署名不正）
pub fn check_bad_signature(ctx: &Ctx) -> ItemResult {
    let id = ItemId::UpdateBadSignature;
    // チェックサム不正と署名不正の両方を確認し、悪い方を返す
    let a = check_tampered(ctx, id, Tamper::BadChecksum, "update-bad-sha");
    let b = check_tampered(ctx, id, Tamper::BadSignature, "update-bad-sig");
    merge_tamper_results(id, a, b)
}

/// 2 つの結果を 1 項目にまとめる（悪い方の状態。詳細・根拠は両方）
pub fn merge_tamper_results(id: ItemId, a: ItemResult, b: ItemResult) -> ItemResult {
    use super::types::Status;
    let rank = |s: Status| match s {
        Status::Ng => 3,
        Status::Warn => 2,
        Status::Skipped | Status::Manual => 1,
        Status::Ok => 0,
    };
    let worst = if rank(b.status) > rank(a.status) {
        &b
    } else {
        &a
    };
    let mut merged = ItemResult::new(
        id,
        worst.status,
        if a.status == Status::Ok && b.status == Status::Ok {
            "チェックサム不正・署名不正のどちらの偽の更新も拒否し、何も保存しませんでした"
                .to_owned()
        } else {
            format!("チェックサム不正: {} / 署名不正: {}", a.detail, b.detail)
        },
    );
    merged.advice = worst.advice.clone();
    merged.evidence = a
        .evidence
        .iter()
        .map(|l| format!("[チェックサム不正] {l}"))
        .chain(b.evidence.iter().map(|l| format!("[署名不正] {l}")))
        .collect();
    merged
}

/// 更新の保存先の状態を判定する
pub fn judge_state(
    ready_version: Option<&str>,
    entries: &[String],
    current: &str,
    apply_script: Option<bool>,
) -> ItemResult {
    let id = ItemId::UpdateState;
    let mut lines = vec![format!(
        "保存先のファイル: {}",
        if entries.is_empty() {
            "なし".to_owned()
        } else {
            entries.join(", ")
        }
    )];
    if let Some(v) = ready_version {
        lines.push(format!("取得済みの版: {v}（現在 {current}）"));
    }
    let tmp: Vec<&String> = entries
        .iter()
        .filter(|e| e.ends_with(".tmp") || e.contains(".tmp"))
        .collect();
    if !tmp.is_empty() {
        return ItemResult::warn(
            id,
            "取得の途中のファイル（.tmp）が残っています",
            "更新の取得が中断された跡です。更新ページでもう一度「更新を確認」を実行すると片付きます。",
        )
        .with_evidence(lines);
    }
    if apply_script == Some(false) {
        return ItemResult::warn(
            id,
            "apply-update.ps1 がインストール先にありません（更新を適用できません）",
            "Nagi を入れ直してください。",
        )
        .with_evidence(lines);
    }
    match ready_version {
        Some(v) => ItemResult::ok(
            id,
            format!(
                "取得済みの更新 {v} があります。スタートメニューの「Nagi を更新」で適用できます"
            ),
        )
        .with_evidence(lines),
        None => ItemResult::ok(id, "適用待ちの更新はなく、保存先も正常です").with_evidence(lines),
    }
}

/// 更新の保存先の項目
pub fn check_state(ctx: &Ctx) -> ItemResult {
    let id = ItemId::UpdateState;
    let Some(dir) = &ctx.update_dir else {
        return ItemResult::skipped(id, "更新の保存先が決まりません（Windows のみ）");
    };
    let entries = staged_entries(dir);
    let ready_version = std::fs::read_to_string(dir.join("ready.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("version").and_then(|v| v.as_str()).map(str::to_owned));
    let apply = ctx
        .is_windows()
        .then(|| ctx.install_dir.join("apply-update.ps1").is_file());
    judge_state(
        ready_version.as_deref(),
        &entries,
        env!("CARGO_PKG_VERSION"),
        apply,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::types::Status;

    fn check(status: UpdateStatus, error: Option<&str>) -> UpdateCheck {
        UpdateCheck {
            current: "0.1.17".into(),
            latest: Some("99.0.0".into()),
            status,
            notes_url: None,
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn real_check_judgement() {
        assert_eq!(
            judge_real_check(&check(UpdateStatus::UpToDate, None)).status,
            Status::Ok
        );
        assert_eq!(
            judge_real_check(&check(UpdateStatus::Available, None)).status,
            Status::Ok
        );
        let r = judge_real_check(&check(UpdateStatus::Error, Some("接続できません")));
        assert_eq!(r.status, Status::Warn);
        assert!(r.detail.contains("接続できません"));
    }

    #[test]
    fn offline_judgement() {
        let e = check(UpdateStatus::Error, Some("リクエストに失敗"));
        assert_eq!(judge_offline(&e, &[]).status, Status::Ok);
        assert_eq!(judge_offline(&e, &["x.tmp".into()]).status, Status::Warn);
        assert_eq!(
            judge_offline(&check(UpdateStatus::Error, None), &[]).status,
            Status::Warn
        );
        assert_eq!(
            judge_offline(&check(UpdateStatus::UpToDate, None), &[]).status,
            Status::Ng
        );
    }

    #[test]
    fn tampered_judgement() {
        let id = ItemId::UpdateBadSignature;
        let sha = check(
            UpdateStatus::Error,
            Some("新しい版の取得に失敗しました: SHA-256 が一致しません"),
        );
        assert_eq!(
            judge_tampered(id, Tamper::BadChecksum, &sha, &[]).status,
            Status::Ok
        );
        let sig = check(
            UpdateStatus::Error,
            Some("新しい版の取得に失敗しました: 署名検証に失敗しました"),
        );
        assert_eq!(
            judge_tampered(id, Tamper::BadSignature, &sig, &[]).status,
            Status::Ok
        );
        // 受け入れたら NG
        let accepted = check(UpdateStatus::Staged, None);
        let r = judge_tampered(id, Tamper::BadSignature, &accepted, &[]);
        assert_eq!(r.status, Status::Ng);
        assert!(r.advice.unwrap().contains("重大"));
        // 拒否したが保存が残ったら NG
        assert_eq!(
            judge_tampered(id, Tamper::BadSignature, &sig, &["ready.json".into()]).status,
            Status::Ng
        );
        // 想定外の理由（通信失敗）は要確認
        let other = check(
            UpdateStatus::Error,
            Some("配布物の取得に失敗: 接続できません"),
        );
        assert_eq!(
            judge_tampered(id, Tamper::BadSignature, &other, &[]).status,
            Status::Warn
        );
    }

    #[test]
    fn merge_takes_worst() {
        let id = ItemId::UpdateBadSignature;
        let ok = ItemResult::ok(id, "a");
        let ng = ItemResult::ng(id, "b", "fix");
        assert_eq!(
            merge_tamper_results(id, ok.clone(), ok.clone()).status,
            Status::Ok
        );
        let m = merge_tamper_results(id, ok, ng);
        assert_eq!(m.status, Status::Ng);
        assert_eq!(m.advice.as_deref(), Some("fix"));
    }

    #[test]
    fn state_judgement() {
        assert_eq!(
            judge_state(None, &[], "0.1.17", Some(true)).status,
            Status::Ok
        );
        assert_eq!(
            judge_state(Some("0.1.18"), &["ready.json".into()], "0.1.17", Some(true)).status,
            Status::Ok
        );
        assert_eq!(
            judge_state(None, &["a.zip.123.tmp".into()], "0.1.17", Some(true)).status,
            Status::Warn
        );
        assert_eq!(
            judge_state(None, &[], "0.1.17", Some(false)).status,
            Status::Warn
        );
    }

    #[test]
    fn mock_server_serves_manifest_and_payload() {
        let server = MockServer::start(Tamper::BadSignature).unwrap();
        let url = server.manifest_url();
        let (host, rest) = url
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap();
        let get = |path: &str| -> String {
            use std::io::{Read, Write};
            let mut s = std::net::TcpStream::connect(host).unwrap();
            write!(
                s,
                "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let manifest = get(&format!("/{rest}"));
        assert!(manifest.starts_with("HTTP/1.1 200"), "{manifest}");
        assert!(manifest.contains("\"version\":\"99.0.0\""));
        assert!(manifest.contains(&format!("/{FAKE_ZIP}")));
        assert!(get("/nope").starts_with("HTTP/1.1 404"));
        let zip = get(&format!("/{FAKE_ZIP}"));
        assert!(zip.starts_with("HTTP/1.1 200"));
        drop(server);
        // 閉じたあとは接続できない
        assert!(std::net::TcpStream::connect(host).is_err());
    }

    #[test]
    fn closed_port_refuses_connections() {
        let url = closed_port_url().unwrap();
        let host = url
            .strip_prefix("http://")
            .unwrap()
            .split_once('/')
            .unwrap()
            .0;
        assert!(std::net::TcpStream::connect(host).is_err());
    }
}
