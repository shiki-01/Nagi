<#
.SYNOPSIS
  取得済みの Nagi の更新を適用する（UAC の確認は 1 回）。

.DESCRIPTION
  インストール先（%ProgramFiles%\Nagi）に置かれる。スタートメニューの「Nagi を更新」、または入力インジケーターの
  右クリックメニューの「更新を適用」から実行する。

  imserver（--stage-updates）が、署名を検証した新版を %LOCALAPPDATA%\Nagi\update に取得している前提で、
  管理者権限（UAC の確認を 1 回出す）で次を行う。

    1. %ProgramFiles%\Nagi\staging（管理者だけが書ける場所）を作る
    2. Program Files 側の imserver.exe --apply-update-prepare で、取得済みの zip を SHA-256 と ed25519 署名で
       ゼロから再検証し、検証したバイト列を staging に書き出す
       （利用者が書き込める場所のファイルは信頼しない。取得後に差し替えられていても、ここで弾く）
    3. staging で zip を展開し、zip の中の VERSION が検証済みの版と一致し、現在の版より新しいことを確かめる
    4. 展開した install.ps1 を、すでに昇格済みなので直接実行して配置・登録する
    5. 成功したら staging と、取得済みの更新（%LOCALAPPDATA%\Nagi\update の ready.json と zip）を消す

  適用後は、サインアウト（または再起動）で、開いているすべてのアプリが新しい版になる。

.PARAMETER UpdateDir
  取得済みの更新の置き場所。既定は %LOCALAPPDATA%\Nagi\update。昇格後は別のユーザーになりうるため、
  昇格前の（元のユーザーの）値を引数で渡す。

.PARAMETER Elevated
  内部用。昇格した子プロセスが適用だけを行う。
#>
[CmdletBinding()]
param(
    [string]$UpdateDir = '',
    [switch]$Elevated
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

# このスクリプトは install.ps1 がインストール先に置く。その場所を「現在のインストール先」とみなす
$InstallDir = $PSScriptRoot
$UninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Nagi'

function Say([string]$Message) { Write-Host "nagi: $Message" }

function Test-Administrator {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# 版数（x.y.z、1〜3 個の数字）として安全か。ファイル名・比較に使う前に必ず通す
function Test-VersionText([string]$Text) {
    return [bool]($Text -match '^[0-9]{1,9}(\.[0-9]{1,9}){0,2}$')
}

# "0.2" のような短い版数も 3 要素にそろえて [version] にする
function ConvertTo-Version([string]$Text) {
    $parts = @($Text.Split('.'))
    while ($parts.Count -lt 3) { $parts += '0' }
    return [version]($parts -join '.')
}

function Get-CurrentVersion {
    $text = (Get-Content -LiteralPath (Join-Path $InstallDir 'VERSION') -TotalCount 1).Trim()
    if (-not (Test-VersionText $text)) { throw "現在の版を読めません: $InstallDir\VERSION" }
    return $text
}

# 更新の置き場所の形（...\Nagi\update）。昇格後の削除の対象を、このディレクトリの中の決まった名前のファイルだけに
# 絞るための確認でもある（引数で任意のディレクトリを渡されても、広い範囲を消さない）
function Assert-UpdateDirShape([string]$Dir) {
    if (-not $Dir -or -not [IO.Path]::IsPathRooted($Dir)) {
        throw "更新の置き場所が絶対パスではありません: $Dir"
    }
    $full = [IO.Path]::GetFullPath($Dir).TrimEnd('\')
    if ($full -notmatch '\\Nagi\\update$') {
        throw "更新の置き場所が想定外です（...\Nagi\update のはず）: $Dir"
    }
}

# 取得済みの更新の版数（ready.json）。無い・壊れている・版数が不正なら $null。
# 利用者が書き換えられるファイルなので、ここでの判定は案内のためだけ（適用時に imserver が再検証する）
function Get-StagedVersion([string]$Dir) {
    $path = Join-Path $Dir 'ready.json'
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    try {
        $ready = Get-Content -LiteralPath $path -Raw -Encoding UTF8 | ConvertFrom-Json
    } catch {
        return $null
    }
    if ($null -eq $ready -or -not $ready.PSObject.Properties['version']) { return $null }
    $version = [string]$ready.version
    if (-not (Test-VersionText $version)) { return $null }
    return $version
}

# 昇格後の安全確認: このスクリプトが、登録されたインストール先（HKLM = 管理者だけが書ける）にあり、
# Program Files の下であること。利用者が書き込める場所に置いた複製や、偽の imserver.exe では適用しない
function Assert-RegisteredInstall {
    if (-not [Environment]::Is64BitProcess) {
        throw '64bit の PowerShell で実行してください'
    }
    $info = Get-ItemProperty -Path $UninstallKey -ErrorAction SilentlyContinue
    if ($null -eq $info -or -not $info.PSObject.Properties['InstallLocation']) {
        throw 'Nagi のインストール情報が見つかりません。install.ps1 で入れ直してください'
    }
    $registered = [IO.Path]::GetFullPath([string]$info.InstallLocation).TrimEnd('\')
    $here = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\')
    if ($registered -ne $here) {
        throw "このスクリプトはインストール先（$registered）にありません: $here"
    }
    $programFiles = $env:ProgramW6432
    if (-not $programFiles) { $programFiles = $env:ProgramFiles }
    $programFiles = [IO.Path]::GetFullPath($programFiles).TrimEnd('\')
    if (-not $here.StartsWith($programFiles + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "自動更新は Program Files への導入だけに対応しています（現在: $here）。install.ps1 を実行し直してください"
    }
}

# 取得済みの更新を消す（ready.json と Nagi-windows-*.zip だけ。ディレクトリごとは消さない）
function Remove-StagedUpdate([string]$Dir) {
    Remove-Item -LiteralPath (Join-Path $Dir 'ready.json') -Force -ErrorAction SilentlyContinue
    Get-ChildItem -LiteralPath $Dir -Filter 'Nagi-windows-*.zip' -File -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

# 管理者権限が要る適用の本体
function Invoke-Apply([string]$Dir) {
    Assert-UpdateDirShape $Dir
    Assert-RegisteredInstall
    $Dir = [IO.Path]::GetFullPath($Dir).TrimEnd('\')

    # 管理者だけが書ける作業場所。Program Files の継承された ACL のまま（Users に書き込みを付けない）。
    # 前回の残りがあれば消してから作る
    $staging = Join-Path $InstallDir 'staging'
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    New-Item -ItemType Directory -Path $staging | Out-Null
    try {
        # 再検証: 利用者の領域のファイルは信頼せず、Program Files 側の imserver が SHA-256 と署名をゼロから検証して、
        # 検証に使ったバイト列を staging に書き出す。成功なら標準出力に版数、失敗なら標準エラーに理由
        $exe = Join-Path $InstallDir 'imserver.exe'
        $outFile = Join-Path $staging 'prepare.out.txt'
        $errFile = Join-Path $staging 'prepare.err.txt'
        $argumentList = '--apply-update-prepare --update-dir "{0}" --out "{1}"' -f $Dir, $staging
        Say '取得済みの更新を検証しています（SHA-256 と署名）'
        $prepare = Start-Process -FilePath $exe -ArgumentList $argumentList -Wait -PassThru -NoNewWindow `
            -RedirectStandardOutput $outFile -RedirectStandardError $errFile
        if ($prepare.ExitCode -ne 0) {
            $reason = ''
            if (Test-Path -LiteralPath $errFile) {
                $reason = ((Get-Content -LiteralPath $errFile -Encoding UTF8) -join ' ').Trim()
            }
            throw "更新を検証できませんでした（適用しません）: $reason"
        }
        $version = ''
        if (Test-Path -LiteralPath $outFile) {
            $version = ((Get-Content -LiteralPath $outFile -TotalCount 1) -join '').Trim()
        }
        if (-not (Test-VersionText $version)) { throw "検証結果の版数が不正です: $version" }
        $current = Get-CurrentVersion
        if ((ConvertTo-Version $version) -le (ConvertTo-Version $current)) {
            throw "取得済みの版 $version は現在の版 $current より新しくありません（更新しません）"
        }
        $zip = Join-Path $staging "Nagi-windows-$version.zip"
        if (-not (Test-Path -LiteralPath $zip)) { throw "検証済みの zip がありません: $zip" }

        Say "Nagi $version を展開しています"
        $extract = Join-Path $staging 'x'
        Expand-Archive -LiteralPath $zip -DestinationPath $extract -Force
        $bundle = Join-Path $extract "Nagi-windows-$version"
        $installer = Join-Path $bundle 'install.ps1'
        if (-not (Test-Path -LiteralPath $installer)) { throw 'zip に install.ps1 がありません' }
        # ready.json の版数は署名の対象外。署名された中身の VERSION と一致することで、古い版の zip の使い回し
        # （ダウングレード）を弾く
        $bundleVersion = (Get-Content -LiteralPath (Join-Path $bundle 'VERSION') -TotalCount 1).Trim()
        if ($bundleVersion -ne $version) {
            throw "zip の版（$bundleVersion）が検証済みの版（$version）と一致しません（適用しません）"
        }

        # すでに昇格済みなので、install.ps1 の配置と登録の手順を直接実行する（UAC は再度出ない）
        Say "Nagi $version を配置しています（配置先: $InstallDir）"
        $global:LASTEXITCODE = 0
        & $installer -Elevated -InstallDir $InstallDir
        if ($LASTEXITCODE -ne 0) {
            throw "配置と登録に失敗しました（終了コード: $LASTEXITCODE）"
        }
    } finally {
        Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    }

    Remove-StagedUpdate $Dir
    Say "Nagi $version に更新しました。サインアウトまたは再起動で全アプリが新しい版になります"
}

# 昇格した子プロセス: 適用だけをして、結果を読めるよう窓を閉じる前に待つ
if ($Elevated) {
    $code = 0
    try {
        if (-not (Test-Administrator)) { throw '管理者権限で実行されていません' }
        Invoke-Apply $UpdateDir
    } catch {
        Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
        $code = 1
    }
    Read-Host 'Enter キーで閉じます' | Out-Null
    exit $code
}

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows 専用です' }
    if (-not $PSCommandPath) { throw 'スクリプトファイルとして実行してください（-File .\apply-update.ps1）' }

    # 昇格すると別のユーザーになりうるので、元のユーザーの更新の置き場所を、昇格前に解決して渡す
    if (-not $UpdateDir) {
        if (-not $env:LOCALAPPDATA) { throw '%LOCALAPPDATA% が分かりません。-UpdateDir を指定してください' }
        $UpdateDir = Join-Path $env:LOCALAPPDATA 'Nagi\update'
    }
    Assert-UpdateDirShape $UpdateDir
    $UpdateDir = [IO.Path]::GetFullPath($UpdateDir).TrimEnd('\')

    $current = Get-CurrentVersion
    $staged = Get-StagedVersion $UpdateDir
    if ($null -eq $staged) {
        Say "取得済みの更新がありません（$UpdateDir）。更新があると、IME の起動時に自動で取得されます（24 時間に 1 回確認）"
        exit 0
    }
    if ((ConvertTo-Version $staged) -le (ConvertTo-Version $current)) {
        Say "取得済みの版 $staged は、現在の版 $current と同じかそれより古いため、適用しません"
        exit 0
    }
    Say "Nagi $current から $staged へ更新します"

    if (Test-Administrator) {
        Invoke-Apply $UpdateDir
    } else {
        Say '管理者権限が必要です。ユーザーアカウント制御の確認が表示されます'
        $argumentList = '-NoProfile -ExecutionPolicy Bypass -File "{0}" -Elevated -UpdateDir "{1}"' -f $PSCommandPath, $UpdateDir
        $child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') `
            -ArgumentList $argumentList -Verb RunAs -Wait -PassThru
        if ($child.ExitCode -ne 0) {
            throw "更新の適用に失敗しました（終了コード: $($child.ExitCode)。詳細は管理者の窓に表示されていました）"
        }
        Say "Nagi $staged に更新しました。サインアウトまたは再起動で全アプリが新しい版になります"
    }
    exit 0
} catch {
    Write-Host "nagi: エラー: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
