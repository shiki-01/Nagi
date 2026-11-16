<#
.SYNOPSIS
  Nagi（Windows 版）の「堅牢性・環境」の実機検証を自動化する（管理者権限は不要）。

.DESCRIPTION
  サブコマンドごとに「操作 → 待機 → tsf.log を読んで OK / NG を表示 → NG のとき報告に貼るログ行を表示」
  までを行います。最後に報告用の「結果サマリ（コピー用）」を出力します。
  手順書: docs/windows-verification.md の「堅牢性・環境の簡単な検証（verify.ps1）」

    status                  現在の状態の要約（DLL を読み込んだプロセス・imserver・VERSION・tsf.log 末尾）
    crash [-Repeat N]       imserver を強制終了し、自動再起動とセッション復元を確認する。
                            -Repeat 6 で連続 6 回終了させ、minimal プロファイルへの移行を確認する
    env <名前>              環境の擬似（debug-env.json。検証専用）: battery / saver / memory-warning /
                            memory-critical / thermal-serious / invalid / clear
    sleep                   スリープ前の記録（復帰後に sleep -After を実行する）
    sleep -After            復帰後の判定
    password                パスワード欄の判定（secure_input）を確認する
    all                     status・crash・env・password を順に案内する（sleep は別に実行）

  使い方:
    powershell -NoProfile -ExecutionPolicy Bypass -File "C:\Program Files\Nagi\verify.ps1" status

  注意:
    - debug-env.json（%LOCALAPPDATA%\Nagi\debug-env.json）は検証専用です。置くと OS の検出値を上書きします。
      検証が終わったら `verify.ps1 env clear` で必ず消してください。
    - ログには入力内容は含まれません（列挙値・PID・実行ファイル名のみ）。
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)][string]$Command = 'help',
    [Parameter(Position = 1)][string]$Name = '',
    [string]$InstallDir = 'C:\Program Files\Nagi',
    [string]$UserDir = (Join-Path $env:APPDATA 'Nagi'),
    [string]$LogDir = (Join-Path $env:LOCALAPPDATA 'Nagi\logs'),
    [int]$Repeat = 1,
    [int]$TimeoutSec = 0,
    [switch]$After
)

$ErrorActionPreference = 'Continue'

$script:Results = New-Object System.Collections.ArrayList
$script:Interactive = $false

# ---- 共通 ----

function Get-TsfLogPath { return (Join-Path $LogDir 'tsf.log') }
function Get-DebugEnvPath { return (Join-Path (Split-Path -Parent $LogDir) 'debug-env.json') }
function Get-SleepStatePath { return (Join-Path $env:TEMP 'nagi-verify-sleep.json') }

function Write-Title([string]$Text) {
    Write-Host ''
    Write-Host ('=== ' + $Text + ' ===') -ForegroundColor Cyan
}

function Write-Info([string]$Text) { Write-Host ('  ' + $Text) }

function Add-Result([string]$Item, [string]$Status, [string]$Detail, [string[]]$Logs) {
    $entry = New-Object PSObject -Property @{ Item = $Item; Status = $Status; Detail = $Detail; Logs = @($Logs) }
    [void]$script:Results.Add($entry)
    $color = 'White'
    if ($Status -eq 'OK') { $color = 'Green' }
    elseif ($Status -eq 'NG') { $color = 'Red' }
    elseif ($Status -eq 'WARN') { $color = 'Yellow' }
    Write-Host ('  [' + $Status + '] ' + $Item + ': ' + $Detail) -ForegroundColor $color
    if ($Status -eq 'NG' -and @($Logs).Count -gt 0) {
        Write-Host '  --- 報告に貼るログ行 ---' -ForegroundColor Red
        foreach ($l in $Logs) { Write-Host ('  ' + $l) }
        Write-Host '  ------------------------' -ForegroundColor Red
    }
}

function Pause-Step([string]$Message) {
    if ($script:Interactive) { [void](Read-Host $Message) }
}

# 前提: Nagi が導入済みで tsf.log がある。満たさなければ理由を表示して $false
function Test-Prerequisite {
    $dll = Join-Path $InstallDir 'nagi_windows_tsf.dll'
    if (-not (Test-Path -LiteralPath $dll)) {
        Write-Host ('Nagi が導入されていません（' + $dll + ' がありません）。install.ps1 でインストールするか、-InstallDir でインストール先を指定してください。') -ForegroundColor Red
        return $false
    }
    if (-not (Test-Path -LiteralPath (Get-TsfLogPath))) {
        Write-Host ('tsf.log がありません（' + (Get-TsfLogPath) + '）。Nagi に切り替えて何か入力してから再実行してください（入力方式の登録・切り替えは docs/windows-verification.md 段階 D）。') -ForegroundColor Red
        return $false
    }
    return $true
}

# ---- tsf.log の読み取り（バイト位置で「この操作の後に出た行」を取る） ----

function Get-LogLength {
    $p = Get-TsfLogPath
    if (Test-Path -LiteralPath $p) { return [long](Get-Item -LiteralPath $p).Length }
    return [long]0
}

function Read-LogSince([long]$Position) {
    $p = Get-TsfLogPath
    if (-not (Test-Path -LiteralPath $p)) { return @() }
    $fs = $null
    try {
        $share = [System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete
        $fs = New-Object System.IO.FileStream($p, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, $share)
        # ローテーション（tsf.log.1 へ回った）で短くなっていたら先頭から読む
        if ($Position -gt $fs.Length) { $Position = 0 }
        [void]$fs.Seek($Position, [System.IO.SeekOrigin]::Begin)
        $size = [int]($fs.Length - $Position)
        if ($size -le 0) { return @() }
        $buf = New-Object byte[] $size
        $read = $fs.Read($buf, 0, $size)
        $text = [System.Text.Encoding]::UTF8.GetString($buf, 0, $read)
        return @($text -split "`r?`n" | Where-Object { $_ -ne '' })
    }
    catch { return @() }
    finally { if ($fs -ne $null) { $fs.Dispose() } }
}

# 行のパターンに合うものが出るまで待つ（最大 $Seconds 秒）。見つかった行の配列を返す（無ければ空）
function Wait-ForLog([long]$Position, [string]$Pattern, [int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ($true) {
        $hits = @(Read-LogSince $Position | Where-Object { $_ -match $Pattern })
        if ($hits.Count -gt 0) { Write-Host ''; return ,$hits }
        if ((Get-Date) -ge $deadline) { break }
        Write-Host '.' -NoNewline
        Start-Sleep -Milliseconds 1000
    }
    Write-Host ''
    return ,@()
}

# NG のときに貼るログ行（操作以降の [engine] [env] [ctx] 行の末尾）
function Get-Evidence([long]$Position) {
    $lines = @(Read-LogSince $Position | Where-Object { $_ -match '\[(engine|env|ctx|tsf)\]' })
    if ($lines.Count -eq 0) { return @('(操作以降に該当する tsf.log の行はありません)') }
    if ($lines.Count -gt 25) { $lines = $lines[($lines.Count - 25)..($lines.Count - 1)] }
    return $lines
}

# ---- プロセス ----

function Get-ImserverProcesses { return @(Get-Process -Name imserver -ErrorAction SilentlyContinue) }

function Format-Process($p) {
    $start = '?'
    try { $start = $p.StartTime.ToString('yyyy-MM-dd HH:mm:ss') } catch { }
    return ('PID=' + $p.Id + ' 起動=' + $start)
}

# 親プロセスごとの imserver の数（Win32_Process）
function Get-ImserverByParent {
    try {
        $procs = @(Get-CimInstance -ClassName Win32_Process -Filter "Name='imserver.exe'" -ErrorAction Stop)
        return @($procs | Group-Object -Property ParentProcessId)
    }
    catch { return @() }
}

# ---- status ----

function Invoke-Status {
    Write-Title 'status: 現在の状態'
    $installed = Test-Path -LiteralPath (Join-Path $InstallDir 'nagi_windows_tsf.dll')
    $versionFile = Join-Path $InstallDir 'VERSION'
    $version = '(不明)'
    if (Test-Path -LiteralPath $versionFile) { $version = (Get-Content -LiteralPath $versionFile -TotalCount 1).Trim() }
    Write-Info ('インストール先: ' + $InstallDir + '  VERSION: ' + $version)
    Write-Info ('ユーザーデータ: ' + $UserDir + '  ログ: ' + $LogDir)
    if ($installed) { Add-Result 'Nagi の導入' 'OK' ('VERSION ' + $version) @() }
    else { Add-Result 'Nagi の導入' 'NG' ('nagi_windows_tsf.dll がありません: ' + $InstallDir) @() }

    Write-Info 'Nagi の DLL を読み込んでいるプロセス（tasklist /m）:'
    $listed = @(& tasklist.exe /m nagi_windows_tsf.dll 2>&1)
    foreach ($l in $listed) { Write-Info ('  ' + $l) }
    $loaded = @($listed | Where-Object { $_ -match '\.exe' -and $_ -notmatch '^\s*(Image Name|イメージ名)' })
    if ($loaded.Count -gt 0) { Add-Result 'DLL の読み込み' 'OK' ($loaded.Count.ToString() + ' プロセス') @() }
    else { Add-Result 'DLL の読み込み' 'WARN' 'DLL を読み込んだプロセスがありません（Nagi に切り替えて入力すると読み込まれます）' @() }

    $procs = Get-ImserverProcesses
    if ($procs.Count -gt 0) {
        foreach ($p in $procs) { Write-Info ('imserver: ' + (Format-Process $p)) }
        Add-Result 'imserver' 'OK' ($procs.Count.ToString() + ' プロセス') @()
    }
    else { Add-Result 'imserver' 'WARN' '起動していません（Nagi で入力するとホストごとに起動します）' @() }

    $log = Get-TsfLogPath
    if (Test-Path -LiteralPath $log) {
        Add-Result 'tsf.log' 'OK' $log @()
        $engine = @(Read-LogSince 0 | Where-Object { $_ -match '\[engine\]' })
        Write-Info '直近の [engine] 行:'
        $tail = $engine
        if ($engine.Count -gt 8) { $tail = $engine[($engine.Count - 8)..($engine.Count - 1)] }
        foreach ($l in $tail) { Write-Info ('  ' + $l) }
    }
    else { Add-Result 'tsf.log' 'NG' ($log + ' がありません') @() }

    $imLogs = @()
    if (Test-Path -LiteralPath $LogDir) { $imLogs = @(Get-ChildItem -LiteralPath $LogDir -Filter 'imserver-*.log' -ErrorAction SilentlyContinue) }
    if ($imLogs.Count -gt 0) { Write-Info ('imserver ログ: ' + $imLogs.Count + ' ファイル（' + $LogDir + '）') }
    else { Write-Info ('imserver ログ: なし（' + $LogDir + '）') }
    $debugEnv = Get-DebugEnvPath
    if (Test-Path -LiteralPath $debugEnv) {
        Add-Result 'debug-env.json' 'WARN' ('検証用の環境上書きが有効です: ' + $debugEnv + '（verify.ps1 env clear で消す）') @()
    }
}

# ---- crash ----

function Invoke-Crash {
    Write-Title 'crash: imserver の強制終了と自動復旧'
    if (-not (Test-Prerequisite)) { return }
    Write-Info '事前準備: メモ帳を開いて Nagi に切り替え、ひらがなを 3 文字打って「未確定のまま」にしてから、このスクリプトを実行してください。'
    Write-Info '（Nagi を使っているアプリが複数あると imserver が複数います。このスクリプトはすべて終了させます）'
    Pause-Step '準備ができたら Enter'

    $pos = Get-LogLength
    $before = Get-ImserverProcesses
    if ($before.Count -eq 0) {
        Add-Result 'crash/事前' 'NG' 'imserver が起動していません。Nagi に切り替えて文字を入力してから実行してください' (Get-Evidence $pos)
        return
    }
    $seen = New-Object System.Collections.ArrayList
    foreach ($p in $before) { [void]$seen.Add($p.Id) }
    $wait = 30
    if ($TimeoutSec -gt 0) { $wait = $TimeoutSec }
    if ($Repeat -lt 1) { $Repeat = 1 }

    for ($i = 1; $i -le $Repeat; $i++) {
        $targets = Get-ImserverProcesses | Where-Object { $seen -contains $_.Id }
        if ($i -gt 1) { $targets = Get-ImserverProcesses }
        foreach ($t in $targets) { if ($seen -notcontains $t.Id) { [void]$seen.Add($t.Id) } }
        $ids = @($targets | ForEach-Object { $_.Id })
        if ($ids.Count -eq 0) { break }
        Write-Info ('[' + $i + '/' + $Repeat + '] imserver を強制終了: PID=' + ($ids -join ','))
        Stop-Process -Id $ids -Force -ErrorAction SilentlyContinue

        # 新しい PID が現れるまで待つ（連続終了のときは、起動直後の短い生存で次を終了させる）
        $deadline = (Get-Date).AddSeconds($wait)
        $fresh = @()
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 300
            $fresh = @(Get-ImserverProcesses | Where-Object { $seen -notcontains $_.Id })
            if ($fresh.Count -gt 0) { break }
        }
        if ($fresh.Count -eq 0) {
            if ($i -lt $Repeat -and (@(Read-LogSince $pos | Where-Object { $_ -match 'entering minimal profile' }).Count -gt 0)) { break }
            Add-Result 'crash/再起動' 'NG' ($wait.ToString() + ' 秒待っても新しい imserver が現れません（' + $i + ' 回目）') (Get-Evidence $pos)
            return
        }
        foreach ($f in $fresh) { [void]$seen.Add($f.Id) }
        Write-Info ('  新しい imserver: ' + (($fresh | ForEach-Object { Format-Process $_ }) -join ' / '))
    }

    $started = @(Wait-ForLog $pos '\[engine\] started' 10)
    if ($started.Count -gt 0) { Add-Result 'crash/再起動' 'OK' '新しい imserver が起動し [engine] started が記録された' @() }
    else { Add-Result 'crash/再起動' 'NG' '新しい PID は現れたが [engine] started が tsf.log に出ていない' (Get-Evidence $pos) }

    $crashed = @(Read-LogSince $pos | Where-Object { $_ -match 'stopped \(crash or start failure\)' })
    if ($crashed.Count -gt 0) { Add-Result 'crash/検出' 'OK' '[engine] stopped (crash or start failure) が記録された' @() }
    else { Add-Result 'crash/検出' 'NG' 'クラッシュが tsf.log に記録されていない' (Get-Evidence $pos) }

    if ($Repeat -eq 1) {
        $restored = @(Wait-ForLog $pos 'session restored' 10)
        if ($restored.Count -gt 0) { Add-Result 'crash/セッション復元' 'OK' 'session restored が記録された。メモ帳の未確定文字が残っていることも目視で確認してください' @() }
        else {
            $failed = @(Read-LogSince $pos | Where-Object { $_ -match 'session restore failed' })
            if ($failed.Count -gt 0) { Add-Result 'crash/セッション復元' 'NG' '復元に失敗した' (Get-Evidence $pos) }
            else { Add-Result 'crash/セッション復元' 'WARN' 'session restored が出ていません（未確定文字が無い状態なら正常。あったなら NG として報告）' (Get-Evidence $pos) }
        }
    }

    if ($Repeat -ge 5) {
        $minimal = @(Wait-ForLog $pos 'entering minimal profile' 30)
        if ($minimal.Count -gt 0) { Add-Result 'crash/minimal 移行' 'OK' ('連続 ' + $Repeat + ' 回の終了で minimal に入った') @() }
        else { Add-Result 'crash/minimal 移行' 'NG' ('連続 ' + $Repeat + ' 回の終了で entering minimal profile が出ない（起動後 30 秒以上生きるとカウントが戻るため、間隔が空いていないかも確認）') (Get-Evidence $pos) }
        Write-Info 'minimal のままになります。元に戻すには、Nagi を使っているアプリを閉じて開き直してください。'
    }
}

# ---- env ----

function Invoke-Env {
    Write-Title ('env ' + $Name + ': 環境の擬似（検証専用の debug-env.json）')
    if (-not (Test-Prerequisite)) { return }
    $path = Get-DebugEnvPath
    $json = $null
    $pattern = $null
    switch ($Name) {
        'battery' { $json = '{"on_battery":true}'; $pattern = '\[env\] sent ac=battery .*source=override' }
        'saver' { $json = '{"battery_saver":true}'; $pattern = '\[env\] sent .*battery_saver=true .*source=override' }
        'memory-warning' { $json = '{"memory":"warning"}'; $pattern = '\[env\] sent .*memory=warning .*source=override' }
        'memory-critical' { $json = '{"memory":"critical"}'; $pattern = '\[env\] sent .*memory=critical .*source=override' }
        'thermal-serious' { $json = '{"thermal":"serious"}'; $pattern = '\[env\] sent .*thermal=serious source=override' }
        'invalid' { $json = '{ this is not json'; $pattern = '\[env\] invalid debug-env\.json' }
        'clear' { }
        default {
            Write-Host 'env の引数: battery | saver | memory-warning | memory-critical | thermal-serious | invalid | clear' -ForegroundColor Red
            return
        }
    }
    $wait = 40
    if ($TimeoutSec -gt 0) { $wait = $TimeoutSec }
    $pos = Get-LogLength

    if ($Name -eq 'clear') {
        if (-not (Test-Path -LiteralPath $path)) {
            Add-Result 'env/clear' 'OK' 'debug-env.json は元から存在しません（実値のまま）' @()
            return
        }
        Remove-Item -LiteralPath $path -Force
        Write-Info ('debug-env.json を削除しました。最大 ' + $wait + ' 秒待って実値に戻った送信を確認します。')
        $hits = @(Wait-ForLog $pos '\[env\] sent .*source=real' $wait)
        if ($hits.Count -gt 0) { Add-Result 'env/clear' 'OK' '実値が再送された（source=real）' $hits }
        else { Add-Result 'env/clear' 'WARN' '実値への変化の送信が出ていません（上書き中と実値が同じなら変化なしで正常）' (Get-Evidence $pos) }
        return
    }

    $dir = Split-Path -Parent $path
    if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    # BOM なしの UTF-8（BOM があると JSON として読めない）
    [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Info ('debug-env.json を書きました: ' + $json)
    Write-Info ('最大 ' + $wait + ' 秒待ちます（環境は約 30 秒ごとと電源通知で確認されます。Nagi を有効にしたアプリが 1 つ必要です）')
    $hits = @(Wait-ForLog $pos $pattern $wait)
    if ($hits.Count -gt 0) {
        if ($Name -eq 'invalid') { Add-Result 'env/invalid' 'OK' '不正な JSON は無視され、警告が 1 回出た' $hits }
        else { Add-Result ('env/' + $Name) 'OK' '対応する [env] sent が source=override で記録された' $hits }
    }
    else { Add-Result ('env/' + $Name) 'NG' ($wait.ToString() + ' 秒待っても対応する [env] 行が出ない') (Get-Evidence $pos) }

    if ($Name -eq 'invalid') {
        Start-Sleep -Seconds 35
        $warns = @(Read-LogSince $pos | Where-Object { $_ -match '\[env\] invalid debug-env\.json' })
        if ($warns.Count -eq 1) { Add-Result 'env/invalid の警告回数' 'OK' '35 秒後も警告は 1 回のまま' @() }
        else { Add-Result 'env/invalid の警告回数' 'NG' ('警告が ' + $warns.Count + ' 回出ている（1 回のはず）') $warns }
    }
    else {
        Write-Info '目視: 言語バー（タスクトレイの「あ」）を右クリックし、メニューの「プロファイル: …」の行が環境に応じた値になっているか確認してください。'
        Write-Info '  battery/saver/memory-*/thermal-serious はプロファイルが balanced より軽く（light など）なる想定です。結果を報告に添えてください。'
    }
    Write-Info '終了したら必ず  verify.ps1 env clear  を実行してください（検証専用ファイルを残さない）。'
}

# ---- sleep ----

function Invoke-Sleep {
    Write-Title 'sleep: スリープ復帰後の再起動'
    if (-not (Test-Prerequisite)) { return }
    $state = Get-SleepStatePath
    if (-not $After) {
        $pids = @(Get-ImserverProcesses | ForEach-Object { $_.Id })
        $obj = @{ position = (Get-LogLength); pids = $pids; time = (Get-Date).ToString('o') }
        [System.IO.File]::WriteAllText($state, (ConvertTo-Json -InputObject $obj), (New-Object System.Text.UTF8Encoding($false)))
        Write-Info ('記録しました: imserver PID=' + ($pids -join ',') + ' / tsf.log の位置=' + $obj.position)
        Write-Info '手順: メモ帳で Nagi に切り替えてひらがなを 3 文字打ち（未確定のまま）、PC をスリープしてください。'
        Write-Info '復帰したら次を実行します:  verify.ps1 sleep -After'
        return
    }
    if (-not (Test-Path -LiteralPath $state)) {
        Write-Host 'スリープ前の記録がありません。先に  verify.ps1 sleep  を実行してください。' -ForegroundColor Red
        return
    }
    $saved = Get-Content -LiteralPath $state -Raw | ConvertFrom-Json
    $pos = [long]$saved.position
    $oldPids = @($saved.pids)
    Write-Info '復帰後の再起動を最大 60 秒待ちます。'
    $hits = @(Wait-ForLog $pos 'restart requested \(resume: (os-notification|tick-gap)\)' 60)
    if ($hits.Count -gt 0) {
        $routes = @($hits | ForEach-Object { if ($_ -match 'resume: ([a-z-]+)') { $Matches[1] } } | Select-Object -Unique)
        Add-Result 'sleep/復帰の検出' 'OK' ('再起動の要求が記録された（検出経路: ' + ($routes -join ', ') + '）') $hits
    }
    else { Add-Result 'sleep/復帰の検出' 'NG' 'restart requested (resume: …) が出ない（電源通知もタイマーの飛びも検出していない）' (Get-Evidence $pos) }

    # 同じホストが 2 回要求していないか（pid= ごと。復帰は 60 秒に 1 回だけ報告する設計）
    $byPid = @{}
    foreach ($h in $hits) { if ($h -match 'pid=(\d+) ') { $k = $Matches[1]; if ($byPid.ContainsKey($k)) { $byPid[$k]++ } else { $byPid[$k] = 1 } } }
    $dups = @($byPid.Keys | Where-Object { $byPid[$_] -gt 1 })
    if ($dups.Count -eq 0) { Add-Result 'sleep/二重検出' 'OK' '同じホストでの再起動要求は 1 回だけ' @() }
    else { Add-Result 'sleep/二重検出' 'NG' ('同じホスト（pid=' + ($dups -join ',') + '）が復帰を複数回検出した') $hits }

    Start-Sleep -Seconds 5
    $now = Get-ImserverProcesses
    $fresh = @($now | Where-Object { $oldPids -notcontains $_.Id })
    if ($now.Count -gt 0 -and $fresh.Count -gt 0) { Add-Result 'sleep/imserver の入れ替え' 'OK' ('新しい imserver: ' + (($fresh | ForEach-Object { Format-Process $_ }) -join ' / ')) @() }
    else { Add-Result 'sleep/imserver の入れ替え' 'NG' '復帰後に新しい imserver が見つからない' (Get-Evidence $pos) }

    $groups = @(Get-ImserverByParent)
    $multi = @($groups | Where-Object { $_.Count -gt 1 })
    if ($multi.Count -eq 0) { Add-Result 'sleep/imserver の重複' 'OK' 'ホスト 1 つにつき imserver は 1 つだけ' @() }
    else { Add-Result 'sleep/imserver の重複' 'NG' ('同じ親に imserver が複数: 親 PID=' + (($multi | ForEach-Object { $_.Name }) -join ',')) (Get-Evidence $pos) }

    $restored = @(Read-LogSince $pos | Where-Object { $_ -match 'session restored' })
    if ($restored.Count -gt 0) { Add-Result 'sleep/セッション復元' 'OK' 'session restored が記録された。メモ帳の未確定文字が残っていることも目視で確認してください' @() }
    else { Add-Result 'sleep/セッション復元' 'WARN' 'session restored が出ていません（未確定文字があったなら NG として報告）' (Get-Evidence $pos) }
    Remove-Item -LiteralPath $state -Force -ErrorAction SilentlyContinue
}

# ---- password ----

function Invoke-Password {
    Write-Title 'password: パスワード欄の判定（secure_input）'
    if (-not (Test-Prerequisite)) { return }
    $self = (Get-Process -Id $PID).ProcessName
    $pos = Get-LogLength
    Write-Info '小さなウィンドウが開きます。Nagi に切り替えて、上の欄・下（パスワード）の欄にそれぞれ 1 文字入力し、「閉じる」を押してください。'
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'Nagi 検証: パスワード欄'
    $form.Width = 440
    $form.Height = 250
    $form.StartPosition = 'CenterScreen'
    $form.TopMost = $true
    $l1 = New-Object System.Windows.Forms.Label
    $l1.Text = '通常のテキストボックス（Nagi に切り替えて 1 文字入力）'
    $l1.Left = 12; $l1.Top = 12; $l1.Width = 400
    $t1 = New-Object System.Windows.Forms.TextBox
    $t1.Left = 12; $t1.Top = 36; $t1.Width = 400
    $l2 = New-Object System.Windows.Forms.Label
    $l2.Text = 'パスワードボックス（1 文字入力）'
    $l2.Left = 12; $l2.Top = 72; $l2.Width = 400
    $t2 = New-Object System.Windows.Forms.TextBox
    $t2.Left = 12; $t2.Top = 96; $t2.Width = 400
    $t2.UseSystemPasswordChar = $true
    $btn = New-Object System.Windows.Forms.Button
    $btn.Text = '閉じる'
    $btn.Left = 12; $btn.Top = 140; $btn.Width = 120
    $btn.Add_Click({ $form.Close() })
    foreach ($c in @($l1, $t1, $l2, $t2, $btn)) { $form.Controls.Add($c) }
    [void]$form.ShowDialog()
    $form.Dispose()

    Start-Sleep -Seconds 2
    $lines = @(Read-LogSince $pos)
    $hit = @($lines | Where-Object { $_ -match ('\[ctx\] secure_input=true app=' + [regex]::Escape($self)) })
    if ($hit.Count -gt 0) { Add-Result 'password/パスワード欄' 'OK' ('secure_input=true が ' + $self + ' で記録された') $hit }
    else {
        Add-Result 'password/パスワード欄' 'NG' ('[ctx] secure_input=true app=' + $self + ' が出ていません。このウィンドウ（WinForms の ES_PASSWORD）では TSF が InputScope を返さない、または Nagi が無効化されている可能性があります。その旨を報告してください') (Get-Evidence $pos)
    }

    Write-Info '次に、メモ帳を開き、通常の入力欄に Nagi で 1 文字入力してください（パスワード欄ではないことの対照）。'
    [void](Read-Host '入力したら Enter')
    Start-Sleep -Seconds 1
    $wrong = @(Read-LogSince $pos | Where-Object { $_ -match '\[ctx\] secure_input=true app=notepad' })
    if ($wrong.Count -eq 0) { Add-Result 'password/メモ帳の通常欄' 'OK' 'メモ帳では secure_input=true が出ていない' @() }
    else { Add-Result 'password/メモ帳の通常欄' 'NG' '通常のテキスト欄が機密欄と判定された' $wrong }
}

# ---- サマリ ----

function Write-Summary {
    Write-Host ''
    Write-Host '---- 結果サマリ（コピー用）----' -ForegroundColor Cyan
    Write-Host ('日時: ' + (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'))
    Write-Host ('OS: ' + [Environment]::OSVersion.VersionString + ' / PowerShell ' + $PSVersionTable.PSVersion)
    $versionFile = Join-Path $InstallDir 'VERSION'
    if (Test-Path -LiteralPath $versionFile) { Write-Host ('Nagi VERSION: ' + (Get-Content -LiteralPath $versionFile -TotalCount 1).Trim()) }
    Write-Host ('実行: verify.ps1 ' + $Command + ' ' + $Name)
    foreach ($r in $script:Results) {
        Write-Host ('[' + $r.Status + '] ' + $r.Item + ': ' + $r.Detail)
        if ($r.Status -eq 'NG') { foreach ($l in @($r.Logs)) { Write-Host ('    ' + $l) } }
    }
    $ng = @($script:Results | Where-Object { $_.Status -eq 'NG' }).Count
    $warn = @($script:Results | Where-Object { $_.Status -eq 'WARN' }).Count
    Write-Host ('NG: ' + $ng + ' / WARN: ' + $warn + ' / 全体: ' + $script:Results.Count)
    Write-Host '---- ここまで ----' -ForegroundColor Cyan
    if ($ng -gt 0) { return 1 }
    return 0
}

function Show-Help {
    Write-Host 'verify.ps1 <status | crash | env <名前> | sleep [-After] | password | all> [-Repeat N] [-TimeoutSec N] [-InstallDir <dir>] [-UserDir <dir>] [-LogDir <dir>]'
    Write-Host '  env の名前: battery | saver | memory-warning | memory-critical | thermal-serious | invalid | clear'
    Write-Host '  詳細: Get-Help .\verify.ps1 -Full / docs/windows-verification.md'
}

# ---- 実行 ----

switch ($Command) {
    'status' { Invoke-Status }
    'crash' { Invoke-Crash }
    'env' { Invoke-Env }
    'sleep' { Invoke-Sleep }
    'password' { Invoke-Password }
    'all' {
        $script:Interactive = $true
        Invoke-Status
        if (Test-Prerequisite) {
            Invoke-Crash
            foreach ($n in @('battery', 'saver', 'memory-warning', 'memory-critical', 'thermal-serious', 'invalid')) {
                Pause-Step ('次: env ' + $n + '（Enter で続行）')
                $Name = $n
                Invoke-Env
            }
            Pause-Step '次: env clear（Enter で続行）'
            $Name = 'clear'
            Invoke-Env
            Pause-Step '次: password（Enter で続行）'
            Invoke-Password
        }
        Write-Info 'sleep はスリープが要るため all に含めません: verify.ps1 sleep → スリープ → verify.ps1 sleep -After'
        $Name = ''
    }
    default { Show-Help; exit 0 }
}

$code = Write-Summary
exit $code
