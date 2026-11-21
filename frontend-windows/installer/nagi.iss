; Nagi for Windows のセットアップ exe（Inno Setup 6.3 以降）。docs/windows-install.md・docs/decisions.md H-105。
;
; 1 個の exe で x64 と ARM64 の両方に対応する。CPU に合う zip の中身（Nagi-windows[-arm64]-X.Y.Z/）を
; 一時フォルダへ展開し、実際の配置・TSF 登録・スタートメニュー・辞書とモデルの取得は、その中の install.ps1 を
; 呼んで行う。つまり配置の実装は install.ps1 が正で、この exe は UI・昇格・CPU の選択・アンインストーラだけを持つ
; （自動更新 apply-update.ps1 が install.ps1 を再実行するので、結果が必ず同じになる）。
;
; ビルド（CI: .github/workflows/windows-release.yml の installer ジョブ）:
;   iscc /DAppVersion=0.1.16 /DSrcX64=<x64 zip の展開先>\Nagi-windows-0.1.16 ^
;        /DSrcArm64=<arm64 zip の展開先>\Nagi-windows-0.1.16 /DOutputDir=<出力先> nagi.iss
; SrcArm64 を省くと x64 専用（ARM64 の PC では導入できない）になる。

#ifndef AppVersion
  #error AppVersion を /DAppVersion=X.Y.Z で渡してください
#endif
#ifndef SrcX64
  #error SrcX64 を /DSrcX64=<x64 の Nagi-windows-X.Y.Z フォルダ> で渡してください
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist-installer"
#endif

[Setup]
; AppId は「アプリと機能」の項目・上書き更新・アンインストールの同一性の鍵。一度公開したら絶対に変えない。
AppId={{CD1EA9FD-9645-4EB3-8FA7-49E425886CD8}
AppName=Nagi
AppVersion={#AppVersion}
AppVerName=Nagi {#AppVersion}
AppPublisher=shiki-01
AppPublisherURL=https://nagi.shiki-01.com
AppSupportURL=https://nagi.shiki-01.com/support
AppUpdatesURL=https://nagi.shiki-01.com
VersionInfoVersion={#AppVersion}.0
VersionInfoDescription=Nagi（日本語入力）セットアップ
UninstallDisplayName=Nagi（日本語入力）
UninstallDisplayIcon={app}\nagi-settings.exe
; セットアップ exe・アンインストーラ（unins000.exe）のアイコン。Nagi のロゴ（frontend-windows/assets/nagi.ico。
; 絵柄は core の nagi-windows の icon モジュールが生成する。H-115）。パスはこのスクリプトからの相対
SetupIconFile=..\assets\nagi.ico
; 配置先は install.ps1 の既定（%ProgramFiles%\Nagi）に固定する。自動更新（apply-update.ps1）が
; Program Files への導入だけに対応するため、変更させない
DefaultDirName={commonpf64}\Nagi
DisableDirPage=yes
DisableProgramGroupPage=yes
; スタートメニューは install.ps1 が作る（Inno の [Icons] は使わない）
OutputDir={#OutputDir}
OutputBaseFilename=Nagi-setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
MinVersion=10.0.19045
; ARM64 の PC では x64compatible にも該当するため、導入の分岐は [Files] の Check（IsArm64）で行う
#ifdef SrcArm64
ArchitecturesAllowed=x64compatible arm64
ArchitecturesInstallIn64BitMode=x64compatible arm64
#else
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
#endif
; 入力方式の DLL はあらゆるアプリに読み込まれる。Inno の再起動マネージャでアプリを閉じさせない
; （使用中のファイルは install.ps1 が .old に退避して置き換える）
CloseApplications=no
RestartIfNeededByRun=no
; 署名の差し込み位置: 署名するときだけ CI が /DSignToolName=nagisign と /Snagisign="<コマンド>" を渡す（windows-release.yml 参照）
#ifdef SignToolName
SignTool={#SignToolName}
SignedUninstaller=yes
#endif

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: "getdata"; Description: "辞書とモデルをダウンロードする（約 150MB・推奨。後からでも取得できます）"; GroupDescription: "追加の作業:"

[Files]
; 配置はここでは行わない。install.ps1 が読めるよう、zip の中身を一時フォルダ（終了時に自動で消える）へ展開するだけ
#ifdef SrcArm64
Source: "{#SrcArm64}\*"; DestDir: "{tmp}\payload"; Flags: ignoreversion recursesubdirs createallsubdirs; Check: IsArm64
Source: "{#SrcX64}\*"; DestDir: "{tmp}\payload"; Flags: ignoreversion recursesubdirs createallsubdirs; Check: not IsArm64
#else
Source: "{#SrcX64}\*"; DestDir: "{tmp}\payload"; Flags: ignoreversion recursesubdirs createallsubdirs
#endif

[UninstallRun]
; 入力方式の登録解除（regsvr32 /u）・imserver の停止・ファイル削除・スタートメニューの削除は uninstall.ps1 が行う。
; unins* とフォルダ本体は残し、この exe のアンインストーラが消す
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\uninstall.ps1"" -KeepInstallerFiles -InstallDir ""{app}"""; Flags: runhidden waituntilterminated; RunOnceId: "NagiUnregister"

[UninstallDelete]
; 再起動待ちで残った退避ファイルや、導入時の記録（ProgramData\Nagi\setup）の後始末
Type: filesandordirs; Name: "{app}"
Type: filesandordirs; Name: "{commonappdata}\Nagi\setup"

[Code]
var
  DataFailed: Boolean;

function SetupWorkDir: String;
begin
  Result := ExpandConstant('{commonappdata}\Nagi\setup');
end;

function PowerShellExe: String;
begin
  Result := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  Params, Payload, LogPath, DataScript: String;
begin
  if CurStep <> ssPostInstall then
    Exit;

  Payload := ExpandConstant('{tmp}\payload');
  ForceDirectories(SetupWorkDir);
  LogPath := SetupWorkDir + '\install.log';

  { 1. 配置・TSF 登録・「アプリと機能」の登録情報・スタートメニュー（管理者権限。窓は出さない） }
  WizardForm.StatusLabel.Caption := 'ファイルを配置し、入力方式を登録しています...';
  WizardForm.FilenameLabel.Caption := '';
  Params := '-NoProfile -ExecutionPolicy Bypass -File "' + Payload + '\install.ps1" -Elevated -NoPause -StopEngines -HideUninstallEntry' +
            ' -InstallDir "' + ExpandConstant('{app}') + '" -LogPath "' + LogPath + '"';
  if (not Exec(PowerShellExe, Params, '', SW_HIDE, ewWaitUntilTerminated, ResultCode)) or (ResultCode <> 0) then
    RaiseException('配置と入力方式の登録に失敗しました（終了コード: ' + IntToStr(ResultCode) + '）。' + #13#10 +
                   '詳細は次のファイルに記録しています:' + #13#10 + LogPath);

  { 2. 辞書とモデルの取得（元のユーザーの権限。そのユーザーの %APPDATA%\Nagi\data に入れる）。
      進み具合が分かるよう、PowerShell の窓を表示する。失敗しても導入は完了とし、完了画面で案内する。
      一時フォルダは昇格した側のものなので、元のユーザーが読める ProgramData にスクリプトを写して実行する }
  if WizardIsTaskSelected('getdata') then
  begin
    WizardForm.StatusLabel.Caption := '辞書とモデルをダウンロードしています（約 150MB）。別に開いた窓に進み具合が表示されます...';
    DataScript := SetupWorkDir + '\install-data.ps1';
    if FileCopy(Payload + '\install.ps1', DataScript, False) then
    begin
      Params := '-NoProfile -ExecutionPolicy Bypass -File "' + DataScript + '" -DataOnly -InstallDir "' + ExpandConstant('{app}') + '"';
      if (not ExecAsOriginalUser(PowerShellExe, Params, '', SW_SHOWNORMAL, ewWaitUntilTerminated, ResultCode)) or (ResultCode <> 0) then
        DataFailed := True;
      DeleteFile(DataScript);
    end
    else
      DataFailed := True;
  end;
end;

procedure CurPageChanged(CurPageID: Integer);
var
  Extra: String;
begin
  if CurPageID <> wpFinished then
    Exit;
  Extra := #13#10#13#10 +
    '次の手順: 設定 > 時刻と言語 > 言語と地域 > 日本語 >「言語のオプション」>「キーボードを追加する」で「Nagi」を選び、' +
    'Win+Space で切り替えます。すでに開いているアプリは開き直すと反映されます（更新の場合は、サインアウトか再起動で新しい版にそろいます）。';
  if DataFailed then
    Extra := Extra + #13#10#13#10 +
      '※ 辞書とモデルを取得できませんでした（ネットワークを確認してください）。辞書が無い間は、入力した読みがそのまま出ます。' +
      '後から取得するには、PowerShell で次を実行するか、このセットアップをもう一度実行してください:' + #13#10 +
      '& "$env:ProgramFiles\Nagi\imserver.exe" --install-data --data-dir "$env:APPDATA\Nagi\data" --model small'
  else if not WizardIsTaskSelected('getdata') then
    Extra := Extra + #13#10#13#10 +
      '※ 辞書とモデルは取得していません。このセットアップをもう一度実行して「辞書とモデルをダウンロードする」を選ぶと取得できます。';
  WizardForm.FinishedLabel.Caption := WizardForm.FinishedLabel.Caption + Extra;
end;

{ アンインストール: ユーザーデータ（設定・学習・ユーザー辞書・取得した辞書とモデル）は既定で残す。
  確認の既定は「いいえ」。サイレント実行では /REMOVEUSERDATA=1 を付けたときだけ削除する }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  UserDir: String;
  Remove: Boolean;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  UserDir := ExpandConstant('{userappdata}\Nagi');
  if not DirExists(UserDir) then
    Exit;
  if UninstallSilent then
    Remove := ExpandConstant('{param:REMOVEUSERDATA|0}') = '1'
  else
    Remove := MsgBox('Nagi を削除しました。' + #13#10#13#10 +
      '設定・学習データ・ユーザー辞書・取得した辞書とモデルを、次のフォルダに残しています:' + #13#10 + UserDir + #13#10#13#10 +
      'これらも削除しますか?（再インストールしたときに引き継ぐ場合は「いいえ」）',
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
  if Remove then
    DelTree(UserDir, True, True, True);
end;
