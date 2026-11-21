<#
.SYNOPSIS
  Nagi（日本語入力）をインストールする。

.DESCRIPTION
  展開した zip（Nagi-windows-X.Y.Z / ARM64 は Nagi-windows-arm64-X.Y.Z.zip の中の Nagi-windows-X.Y.Z）の中から実行する。
  zip のアーキテクチャ（ARCH ファイル）が PC と合わなければ配置の前に止める。
    powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1

  1. 同梱ファイルを SHA256SUMS で検証する
  2. 管理者権限で %ProgramFiles%\Nagi に配置し、regsvr32 で入力方式（TSF）を登録する
     （管理者でなければ UAC の確認を出して、この手順だけを昇格して実行する）
     あわせてスタートメニューに「Nagi」フォルダ（設定・設定フォルダ・ログフォルダ・状態確認・更新）を作る
  3. 辞書とモデルを配布元から取得する（現在のユーザーの %APPDATA%\Nagi\data。昇格しないで実行する）

  辞書とモデルは同梱・再配布しない。この PC が配布元から直接取得し、SHA-256 を検証して辞書を組み立てる。
  入力内容や個人情報は送らない。更新は同じ手順の再実行（上書き）か、取得済みの更新を適用する
  スタートメニューの「Nagi を更新」（apply-update.ps1）で行う。

.PARAMETER InstallDir
  配置先。既定は %ProgramFiles%\Nagi。UWP / 保護されたアプリにも DLL を読ませるため、
  利用者のプロファイルの外（Program Files）に置く。

.PARAMETER Model
  取得する変換モデル。small（約 74MB、既定）/ xsmall / none（辞書だけ）。

.PARAMETER SkipData
  辞書とモデルを取得しない。

.PARAMETER Elevated
  内部用。昇格した子プロセスが配置と登録だけを行う（apply-update.ps1 と、セットアップ exe が使う）。

  以下はセットアップ exe（Nagi-setup-X.Y.Z.exe。frontend-windows/installer/nagi.iss）が使う内部用の指定。
  exe は UI・昇格・CPU に合う zip の展開だけを行い、配置・登録・スタートメニュー・辞書とモデルの取得は
  このスクリプトを呼んで行う（実装を重複させない。正はこのスクリプト）。

.PARAMETER NoPause
  -Elevated の失敗時に Enter 待ちをしない（窓を出さずに実行されるため）。

.PARAMETER LogPath
  指定すると、その場所に実行の記録（Start-Transcript）を残す。

.PARAMETER StopEngines
  -Elevated のとき、配置の前にインストール先の旧版の imserver を止める（apply-update.ps1 と同じ判定）。

.PARAMETER HideUninstallEntry
  「アプリと機能」に Nagi の項目を 2 つ出さないよう、このスクリプトが作る項目（apply-update.ps1 が参照する
  登録情報として必要）を非表示（SystemComponent=1）にする。セットアップ exe が自前の項目を持つとき用。

.PARAMETER DataOnly
  辞書とモデルの取得だけを行う（昇格しない。-InstallDir の imserver.exe を使う）。
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:ProgramFiles 'Nagi'),
    [ValidateSet('small', 'xsmall', 'none')]
    [string]$Model = 'small',
    [switch]$SkipData,
    [switch]$Elevated,
    [switch]$NoPause,
    [string]$LogPath = '',
    [switch]$StopEngines,
    [switch]$HideUninstallEntry,
    [switch]$DataOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$DllName = 'nagi_windows_tsf.dll'
$ExeName = 'imserver.exe'
# 配置するファイル（SHA256SUMS に載っているもの + SHA256SUMS 自身）
$PayloadFiles = @(
    'nagi_windows_tsf.dll', 'imserver.exe', 'nagi-settings.exe', 'uninstall.ps1', 'verify.ps1', 'apply-update.ps1', 'README.md',
    'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD_PARTY_LICENSES.md',
    'VERSION', 'ARCH', 'SHA256SUMS'
)
$UninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Nagi'

function Say([string]$Message) { Write-Host "nagi: $Message" }

function Test-Administrator {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# 64bit の regsvr32。32bit の PowerShell から呼ぶと System32 が SysWOW64 に読み替えられるため Sysnative を使う
function Get-Regsvr32 {
    if ([Environment]::Is64BitProcess) {
        return (Join-Path $env:SystemRoot 'System32\regsvr32.exe')
    }
    return (Join-Path $env:SystemRoot 'Sysnative\regsvr32.exe')
}

# この PC の CPU のアーキテクチャ（x86_64 / aarch64）。ARM64 Windows で x64 エミュレーションの PowerShell から
# 実行されても aarch64 と判定する（環境変数 PROCESSOR_ARCHITECTURE はエミュレーション下だと AMD64 になる）。
# IsWow64Process2 の nativeMachine は、プロセスのエミュレーションによらず本来の CPU を返す（Windows 10 1709 以降）
function Get-NativeArchitecture {
    try {
        if (-not ('Nagi.NativeMachine' -as [type])) {
            Add-Type -Namespace Nagi -Name NativeMachine -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
public static extern bool IsWow64Process2(System.IntPtr process, out ushort processMachine, out ushort nativeMachine);
'@
        }
        $processMachine = [uint16]0
        $nativeMachine = [uint16]0
        $self = [System.Diagnostics.Process]::GetCurrentProcess().Handle
        if ([Nagi.NativeMachine]::IsWow64Process2($self, [ref]$processMachine, [ref]$nativeMachine)) {
            if ($nativeMachine -eq 0xAA64) { return 'aarch64' }
            if ($nativeMachine -eq 0x8664) { return 'x86_64' }
        }
    } catch {
        # 取れなければ環境変数で判定する
    }
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    if ($arch -eq 'ARM64' -or $env:PROCESSOR_IDENTIFIER -like 'ARM*') { return 'aarch64' }
    if ($arch -eq 'AMD64') { return 'x86_64' }
    return $arch
}

# zip のアーキテクチャ（ARCH ファイル。無い版は x86_64）
function Get-PayloadArchitecture([string]$SourceDir) {
    $path = Join-Path $SourceDir 'ARCH'
    if (-not (Test-Path -LiteralPath $path)) { return 'x86_64' }
    return (Get-Content -LiteralPath $path -TotalCount 1).Trim()
}

# zip と PC のアーキテクチャが合っているか。TSF の DLL は同じアーキテクチャのプロセスにしか読み込まれず、
# 取り違えると入力方式が出ない（regsvr32 の登録も失敗する）ので、配置の前に止める
function Assert-ArchitectureMatches([string]$SourceDir) {
    $native = Get-NativeArchitecture
    $payload = Get-PayloadArchitecture $SourceDir
    if ($native -ne $payload) {
        throw "この zip は $payload 用ですが、この PC は $native です。$native 用の zip（ARM64 は Nagi-windows-arm64-X.Y.Z.zip）を使ってください"
    }
}

function Test-Platform {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'Windows 専用です'
    }
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw '64bit 版の Windows が必要です'
    }
    $arch = Get-NativeArchitecture
    if ($arch -ne 'x86_64' -and $arch -ne 'aarch64') {
        throw "この PC のアーキテクチャには未対応です（この PC: $arch）。対応: x64 / ARM64"
    }
    # Windows 10 22H2 = 19045、Windows 11 = 22000 以降
    if ([Environment]::OSVersion.Version.Build -lt 19045) {
        throw 'Windows 10 22H2 以降が必要です'
    }
}

# SHA256SUMS と実ファイルを突き合わせる（破損・取り違えの検出。改ざん対策ではない）
function Test-Payload([string]$SourceDir) {
    $sumsPath = Join-Path $SourceDir 'SHA256SUMS'
    if (-not (Test-Path -LiteralPath $sumsPath)) {
        throw "SHA256SUMS が見つかりません: $sumsPath（zip を展開し直してください）"
    }
    $listed = @{}
    foreach ($line in Get-Content -LiteralPath $sumsPath -Encoding UTF8) {
        if ($line -match '^([0-9a-f]{64})  (.+)$') {
            $listed[$Matches[2]] = $Matches[1]
        }
    }
    foreach ($name in @($DllName, $ExeName)) {
        if (-not $listed.ContainsKey($name)) { throw "SHA256SUMS に $name がありません" }
    }
    foreach ($name in $listed.Keys) {
        $path = Join-Path $SourceDir $name
        if (-not (Test-Path -LiteralPath $path)) { throw "ファイルがありません: $name" }
        $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $listed[$name]) { throw "SHA-256 が一致しません: $name（zip を取得し直してください）" }
    }
}

# 使用中（読み込み中の DLL・実行中の exe）でも置き換えられるよう、古いファイルは名前を変えて退避する
function Copy-Replacing([string]$From, [string]$To) {
    if (Test-Path -LiteralPath $To) {
        $old = "$To.old"
        Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
        # 前回の退避ファイルがまだ使用中で消せないときは、重ならない名前に退避する
        if (Test-Path -LiteralPath $old) {
            $old = '{0}.{1}.old' -f $To, (Get-Date -Format 'yyyyMMddHHmmssfff')
        }
        try {
            Move-Item -LiteralPath $To -Destination $old -Force
        } catch {
            throw "既存のファイルを置き換えられません: $To（$($_.Exception.Message)）"
        }
    }
    Copy-Item -LiteralPath $From -Destination $To -Force
    Unblock-File -LiteralPath $To -ErrorAction SilentlyContinue
}

# スタートメニュー（全ユーザー）の「Nagi」フォルダに設定への入口を作る。OS の入力インジケーターのメニューに
# 頼らず設定・ログ・状態確認を開けるようにするため（decisions.md H-94）。
# ユーザーデータの場所は、昇格後は別ユーザーのことがあるため、ショートカットの中で環境変数を展開させる。
# 失敗しても導入は失敗にしない（警告のみ）
function New-StartMenuShortcuts([string]$TargetInstallDir) {
    try {
        $folder = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Nagi'
        New-Item -ItemType Directory -Force -Path $folder | Out-Null
        $shell = New-Object -ComObject WScript.Shell
        $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
        $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $verify = Join-Path $TargetInstallDir 'verify.ps1'
        $applyUpdate = Join-Path $TargetInstallDir 'apply-update.ps1'
        $settingsGui = Join-Path $TargetInstallDir 'nagi-settings.exe'
        # 旧版が作った名前（「Nagi 設定（config.toml）」）は、新しい名前に置き換わるので消す
        Remove-Item -LiteralPath (Join-Path $folder 'Nagi 設定（config.toml）.lnk') -Force -ErrorAction SilentlyContinue
        # 「Nagi 設定」は設定画面（nagi-settings.exe。ローカルの Web 画面を既定のブラウザで開く。H-97）。
        # Arguments の環境変数はシェルが展開しないので、config.toml だけ cmd 経由（cmd が %APPDATA% を展開する）で
        # メモ帳を開く。無いファイルはメモ帳が作成を尋ねる
        $items = @(
            @{ Name = 'Nagi 設定'; Target = $settingsGui; Arguments = ''; Minimized = $false },
            # 更新を手動で確認する（設定画面の更新セクションを開き、確認を自動で実行する。H-100）
            @{ Name = 'Nagi 更新の確認'; Target = $settingsGui; Arguments = '--section update --auto-check'; Minimized = $false },
            @{ Name = 'Nagi 設定ファイル（config.toml）'; Target = $cmd; Arguments = '/c start "" notepad.exe "%APPDATA%\Nagi\config.toml"'; Minimized = $true },
            @{ Name = 'Nagi 設定フォルダ'; Target = '%APPDATA%\Nagi'; Arguments = ''; Minimized = $false },
            @{ Name = 'Nagi ログフォルダ'; Target = '%LOCALAPPDATA%\Nagi\logs'; Arguments = ''; Minimized = $false },
            @{ Name = 'Nagi 状態確認（verify）'; Target = $powershell; Arguments = ('-NoProfile -ExecutionPolicy Bypass -NoExit -File "{0}" status' -f $verify); Minimized = $false },
            # 取得済みの更新を適用する（UAC が 1 回出る。更新の保存先は %LOCALAPPDATA%\Nagi\update が既定なので、
            # 環境変数が展開されないショートカットの引数では渡さない。H-95）
            @{ Name = 'Nagi を更新'; Target = $powershell; Arguments = ('-NoProfile -ExecutionPolicy Bypass -NoExit -File "{0}"' -f $applyUpdate); Minimized = $false }
        )
        foreach ($item in $items) {
            $link = $shell.CreateShortcut((Join-Path $folder ($item.Name + '.lnk')))
            $link.TargetPath = $item.Target
            if ($item.Arguments) { $link.Arguments = $item.Arguments }
            if ($item.Minimized) { $link.WindowStyle = 7 }
            $link.Save()
        }
    } catch {
        Say "警告: スタートメニューのショートカットを作れませんでした（導入には影響しません）: $($_.Exception.Message)"
    }
}

# インストール先の imserver（通常名・`.old` に退避されたもの）のプロセスを、パスで列挙する
# （プロセス名では判定しない。apply-update.ps1 と同じ。decisions.md H-96）
function Get-InstalledEngines([string]$Dir) {
    $prefix = (Join-Path $Dir $ExeName)
    return @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $path = $null
        try { $path = $_.Path } catch { }
        $path -and $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)
    })
}

# 旧版の imserver を止める（-StopEngines）。配置で imserver.exe が `.old` に改名される前に呼ぶ。
# 止めたエンジンは各ホストの supervisor が自動で再起動し、そのとき新しい imserver.exe が使われる
function Stop-InstalledEngines([string]$Dir) {
    $engines = Get-InstalledEngines $Dir
    if ($engines.Count -eq 0) { return }
    Say "動作中の旧版のエンジン（imserver）$($engines.Count) 個を止めます（止めたエンジンは自動で再起動し、新しい版が使われます）"
    foreach ($engine in $engines) {
        Stop-Process -Id $engine.Id -Force -ErrorAction SilentlyContinue
    }
    foreach ($engine in $engines) {
        try { [void]$engine.WaitForExit(3000) } catch { }
    }
}

function Start-InstallLog {
    if (-not $LogPath) { return }
    try {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $LogPath) | Out-Null
        Start-Transcript -LiteralPath $LogPath -Force | Out-Null
    } catch {
        # 記録が取れなくても導入は続ける
    }
}

# セットアップ exe（Inno Setup）が作る「アプリと機能」の項目の表示バージョンを、いま配置した版にそろえる。
# 自動更新（apply-update.ps1）はこのスクリプトを再実行するため、exe の項目だけ古い版のままになるのを防ぐ。
# 項目は Inno の "Inno Setup: App Path" がインストール先と一致するものを探す（見つからなければ何もしない）
function Sync-SetupEntryVersion([string]$TargetInstallDir, [string]$Version) {
    try {
        $root = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
        $here = [IO.Path]::GetFullPath($TargetInstallDir).TrimEnd('\')
        foreach ($key in Get-ChildItem -Path $root -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like '*_is1' }) {
            $appPath = $key.GetValue('Inno Setup: App Path')
            if ($appPath -and ([IO.Path]::GetFullPath([string]$appPath).TrimEnd('\') -eq $here)) {
                New-ItemProperty -Path $key.PSPath -Name 'DisplayVersion' -Value $Version -PropertyType String -Force | Out-Null
            }
        }
    } catch {
        # 表示だけの同期。失敗しても導入は失敗にしない
    }
}

# 辞書とモデルを配布元から取得する（現在のユーザーの %APPDATA%\Nagi\data。昇格しないで実行する）。戻り値: 成功したか
function Install-Data([string]$Dir) {
    $dataDir = Join-Path $env:APPDATA 'Nagi\data'
    Say '辞書とモデルを取得しています（約 150MB。取得済みのものは飛ばします）'
    # 標準出力は進捗の JSON（アプリ向け）なので捨て、標準エラーの人向けのログだけを表示する
    & (Join-Path $Dir $ExeName) --install-data --data-dir $dataDir --model $Model | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Say "辞書とモデルを入れました: $dataDir"
        return $true
    }
    Say '警告: 辞書とモデルを入れられませんでした。ネットワークを確認して、このスクリプトをもう一度実行してください'
    return $false
}

# 管理者権限が要る手順: 配置・登録・「アプリと機能」への登録・スタートメニューのショートカット
function Install-Privileged([string]$SourceDir) {
    Test-Payload $SourceDir
    Assert-ArchitectureMatches $SourceDir

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($name in $PayloadFiles) {
        # ARCH が無い版（x64 のみだった頃の zip）もそのまま入れられるようにする
        if ($name -eq 'ARCH' -and -not (Test-Path -LiteralPath (Join-Path $SourceDir $name))) { continue }
        Copy-Replacing (Join-Path $SourceDir $name) (Join-Path $InstallDir $name)
    }
    # 前回までの退避ファイル（使用中でなくなったもの）を片付ける
    Get-ChildItem -LiteralPath $InstallDir -Filter '*.old' -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue

    $dll = Join-Path $InstallDir $DllName
    $process = Start-Process -FilePath (Get-Regsvr32) -ArgumentList @('/s', ('"{0}"' -f $dll)) -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "regsvr32 による登録に失敗しました（終了コード: $($process.ExitCode)）"
    }

    $version = (Get-Content -LiteralPath (Join-Path $InstallDir 'VERSION') -TotalCount 1).Trim()
    $uninstallScript = Join-Path $InstallDir 'uninstall.ps1'
    New-Item -Path $UninstallKey -Force | Out-Null
    $values = @{
        DisplayName     = 'Nagi（日本語入力）'
        DisplayVersion  = $version
        Publisher       = 'shiki-01'
        InstallLocation = $InstallDir
        URLInfoAbout    = 'https://nagi.shiki-01.com'
        UninstallString = ('powershell.exe -NoProfile -ExecutionPolicy Bypass -File "{0}"' -f $uninstallScript)
    }
    foreach ($key in $values.Keys) {
        New-ItemProperty -Path $UninstallKey -Name $key -Value $values[$key] -PropertyType String -Force | Out-Null
    }
    foreach ($key in @('NoModify', 'NoRepair')) {
        New-ItemProperty -Path $UninstallKey -Name $key -Value 1 -PropertyType DWord -Force | Out-Null
    }
    if ($HideUninstallEntry) {
        # 一覧には出さない（セットアップ exe の項目だけを見せる）。InstallLocation は apply-update.ps1 が読むので残す
        New-ItemProperty -Path $UninstallKey -Name 'SystemComponent' -Value 1 -PropertyType DWord -Force | Out-Null
    }
    Sync-SetupEntryVersion $InstallDir $version

    New-StartMenuShortcuts $InstallDir
}

# 昇格した子プロセスから呼ばれたとき: 配置だけして終わる（失敗は窓が閉じる前に読めるよう待つ。-NoPause なら待たない）
if ($Elevated) {
    $code = 0
    try {
        Start-InstallLog
        if ($StopEngines) { Stop-InstalledEngines $InstallDir }
        Install-Privileged $PSScriptRoot
    } catch {
        Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
        $code = 1
    }
    try { Stop-Transcript | Out-Null } catch { }
    if ($code -ne 0 -and -not $NoPause) {
        Read-Host 'Enter キーで閉じます' | Out-Null
    }
    exit $code
}

# 辞書とモデルの取得だけ（セットアップ exe が、元のユーザーの権限で呼ぶ。窓に進み具合が出る）
if ($DataOnly) {
    try {
        if (Install-Data $InstallDir) { exit 0 }
        exit 1
    } catch {
        Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
        exit 1
    }
}

try {
    Test-Platform
    if (-not $PSCommandPath) { throw 'スクリプトファイルとして実行してください（-File .\install.ps1）' }
    Test-Payload $PSScriptRoot

    $version = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'VERSION') -TotalCount 1).Trim()
    Say "Nagi $version をインストールします（配置先: $InstallDir）"

    if (Test-Administrator) {
        Install-Privileged $PSScriptRoot
    } else {
        Say '管理者権限が必要です。ユーザーアカウント制御の確認が表示されます'
        $argumentList = '-NoProfile -ExecutionPolicy Bypass -File "{0}" -Elevated -InstallDir "{1}"' -f $PSCommandPath, $InstallDir
        $child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $argumentList -Verb RunAs -Wait -PassThru
        if ($child.ExitCode -ne 0) {
            throw "配置と登録に失敗しました（終了コード: $($child.ExitCode)）"
        }
    }
    Say "配置と入力方式の登録を終えました: $InstallDir"

    # Visual C++ 再頒布可能パッケージ（imserver が使う。ほとんどの PC に入っている）
    $redist = 'https://aka.ms/vs/17/release/vc_redist.x64.exe'
    if ((Get-PayloadArchitecture $PSScriptRoot) -eq 'aarch64') { $redist = 'https://aka.ms/vs/17/release/vc_redist.arm64.exe' }
    foreach ($runtime in @('vcruntime140.dll', 'msvcp140.dll')) {
        if (-not (Test-Path -LiteralPath (Join-Path $env:SystemRoot "System32\$runtime"))) {
            Say "警告: $runtime が見つかりません。$redist を入れてください"
        }
    }

    $dataOk = $false
    if ($SkipData) {
        Say '辞書とモデルの取得を飛ばしました（-SkipData）'
    } else {
        $dataOk = Install-Data $InstallDir
    }

    Write-Host ''
    Say "Nagi $version をインストールしました"
    Write-Host @'

次の手順:
  1. 設定 > 時刻と言語 > 言語と地域 > 日本語 >「言語のオプション」>「キーボードを追加する」で
     「Nagi」を選んでください。Win+Space で入力方式を切り替えられます。
  2. すでに開いているアプリでは、アプリを開き直すと反映されます。
     更新の場合は、サインアウト（または再起動）すると新しい版にそろいます。
  3. 辞書が無い間は、入力した読みがそのまま出ます。
'@
    if (-not $dataOk -and -not $SkipData) {
        Write-Host '  ※ 辞書の取得に失敗しています。もう一度このスクリプトを実行してください。'
    }
    exit 0
} catch {
    Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
