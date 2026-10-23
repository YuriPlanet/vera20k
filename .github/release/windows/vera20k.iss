; Windows installer for VERA. The Release workflow (.github/workflows/release.yml)
; compiles it with the Inno Setup compiler pinned in the workflow:
;
;   ISCC /DAppVersion=<x.y.z> /DBuild=<YYYYMMDD-HHMMSS> /DPackageDir=<package folder>
;        /DOutputDir=<dir> /DOutputBaseFilename=<name> vera20k.iss
;
; It installs the files of the package folder for the current user, without an
; administrator prompt, and asks for the Yuri's Revenge folder, offering the one
; it finds (DetectGameDirs). It writes config.toml beside the program with that
; folder as ra2_dir and the install folder as settings_dir. Shortcuts start the
; program in its install folder, so logs, saves, replays and its VERA.INI stay
; there and never touch the game folder, which may be read-only or shared with
; the original game.
;
; A silent install takes the game folder from /GAMEDIR=<folder>, or else uses
; the one it would offer.
;
#ifndef AppVersion
  #error Define AppVersion, Build, PackageDir, OutputDir and OutputBaseFilename
#endif
#if Ver < EncodeVer(7, 1, 0)
  #error Compile with Inno Setup 7.1.0 or later
#endif

[Setup]
; Fixed for the life of the program: upgrades find the previous install by it.
AppId={{9C9BFC32-A7B8-48ED-A428-72333354BDDB}
AppName=VERA
AppVersion={#AppVersion}
; The version and build, which stay true when a release republishes this
; installer under a plain name.
AppVerName=VERA {#AppVersion} (build {#Build})
AppPublisher=VERA
AppPublisherURL=https://github.com/YuriPlanet/vera20k
AppSupportURL=https://github.com/YuriPlanet/vera20k/issues
AppUpdatesURL=https://github.com/YuriPlanet/vera20k/releases
VersionInfoVersion={#AppVersion}
; Per user: {autopf} is %LOCALAPPDATA%\Programs, which the program can write to.
PrivilegesRequired=lowest
DefaultDirName={autopf}\VERA
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
DisableWelcomePage=no
ShowLanguageDialog=auto
; Use Inno Setup's built-in setup icon and wizard images.
UninstallDisplayIcon={app}\vera20k.exe
UninstallDisplayName=VERA
SourceDir={#PackageDir}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Messages]
; Credit the actual compiler in the title bar; keep the standard footer clear.
english.SetupWindowTitle=Setup - %1 | Inno Setup {#DecodeVer(Ver)}
chinesesimplified.SetupWindowTitle=安装 - %1 | Inno Setup {#DecodeVer(Ver)}
english.WelcomeLabel2=VERA is a community-built engine for Yuri's Revenge.%n%nRequires game files from your own copy of Yuri's Revenge 1.001.
chinesesimplified.WelcomeLabel2=VERA 是由社区开发的《尤里的复仇》游戏引擎。%n%n需要您自己拥有的《尤里的复仇》1.001 版游戏文件。

[CustomMessages]
english.GameDirCaption=Yuri's Revenge folder
english.GameDirDescription=Where is Yuri's Revenge installed?
english.GameDirSubCaption=Choose an installation or browse for another folder.
english.GameDirPrompt=Yuri's Revenge folder:
english.GameInstallReady=Ready
english.GameInstallOptional=Ready; optional files missing
english.GameInstallMissing=Required files unavailable
english.GameInstallRecommended=Recommended
english.GameInstallLanguage=Language: %1
english.GameInstallSource=Source: %1
english.GameSourceSteam=Steam
english.GameSourceEAApp=EA App
english.GameSourceManual=Manually selected
english.GameSourceDetected=Detected installation
english.GameInstallUntouched=Your original game files stay untouched.
english.GameInstallChecking=Checking detected installations...
english.GameInstallChoose=Select an installation to see any missing content.
english.GameInstallManual=Click Next to check the chosen folder.
english.GameInstallNone=No installations found. Browse for your Yuri's Revenge folder.
english.GameLanguage0=English (US)
english.GameLanguage1=English (UK)
english.GameLanguage2=German
english.GameLanguage3=French
english.GameLanguage4=Spanish
english.GameLanguage5=Italian
english.GameLanguage6=Japanese
english.GameLanguage7=Jabberwockie (test language)
english.GameLanguage8=Korean
english.GameLanguage9=Chinese
english.GameLanguageUnknown=Unknown
english.GameLanguageOther=Unknown (CSF language %1)
english.GameCheckCaption=Game files
english.GameCheckDescription=Review the files in the selected game folder.
english.GameCheckRequired=Required files are missing or unreadable. Click Back to choose another folder.
english.GameCheckOptional=Optional content is unavailable. You can continue, or click Back to choose another folder.
english.GameCheckReady=The required game files were found. Click Next to continue.
english.GameCheckRequiredHeading=Required files - must be fixed:
english.GameCheckOptionalHeading=Optional content - installation can continue:
english.GameCheckFailed=Setup could not check this folder. Choose another folder or try again.%n%1
english.GameCheckNoFolder=No game folder was selected.
english.ConfigNotWritten=Setup could not write %1.
chinesesimplified.GameDirCaption=《尤里的复仇》文件夹
chinesesimplified.GameDirDescription=《尤里的复仇》安装在哪里？
chinesesimplified.GameDirSubCaption=请选择一个安装，或浏览其他文件夹。
chinesesimplified.GameDirPrompt=《尤里的复仇》文件夹：
chinesesimplified.GameInstallReady=可以使用
chinesesimplified.GameInstallOptional=可以使用；缺少可选文件
chinesesimplified.GameInstallMissing=必需文件不可用
chinesesimplified.GameInstallRecommended=推荐
chinesesimplified.GameInstallLanguage=语言：%1
chinesesimplified.GameInstallSource=来源：%1
chinesesimplified.GameSourceSteam=Steam
chinesesimplified.GameSourceEAApp=EA App
chinesesimplified.GameSourceManual=手动选择
chinesesimplified.GameSourceDetected=检测到的安装
chinesesimplified.GameInstallUntouched=不会改动您的原版游戏文件。
chinesesimplified.GameInstallChecking=正在检查检测到的游戏安装……
chinesesimplified.GameInstallChoose=选择一个安装可查看缺失内容。
chinesesimplified.GameInstallManual=点击“下一步”检查所选文件夹。
chinesesimplified.GameInstallNone=未找到游戏安装。请浏览选择《尤里的复仇》文件夹。
chinesesimplified.GameLanguage0=英语（美国）
chinesesimplified.GameLanguage1=英语（英国）
chinesesimplified.GameLanguage2=德语
chinesesimplified.GameLanguage3=法语
chinesesimplified.GameLanguage4=西班牙语
chinesesimplified.GameLanguage5=意大利语
chinesesimplified.GameLanguage6=日语
chinesesimplified.GameLanguage7=Jabberwockie（测试语言）
chinesesimplified.GameLanguage8=韩语
chinesesimplified.GameLanguage9=中文
chinesesimplified.GameLanguageUnknown=未知
chinesesimplified.GameLanguageOther=未知（CSF 语言编号 %1）
chinesesimplified.GameCheckCaption=游戏文件
chinesesimplified.GameCheckDescription=检查所选游戏文件夹中的文件。
chinesesimplified.GameCheckRequired=必需文件缺失或无法读取。请点击“上一步”选择其他文件夹。
chinesesimplified.GameCheckOptional=部分可选内容不可用。您可以继续安装，或点击“上一步”选择其他文件夹。
chinesesimplified.GameCheckReady=已找到必需的游戏文件。点击“下一步”继续。
chinesesimplified.GameCheckRequiredHeading=必需文件 - 必须修复：
chinesesimplified.GameCheckOptionalHeading=可选内容 - 可以继续安装：
chinesesimplified.GameCheckFailed=安装程序无法检查此文件夹。请选择其他文件夹或重试。%n%1
chinesesimplified.GameCheckNoFolder=尚未选择游戏文件夹。
chinesesimplified.ConfigNotWritten=安装程序无法写入 %1。

[Files]
Source: "vera20k.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "config.toml.example"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE-GPL"; DestDir: "{app}"; Flags: ignoreversion
Source: "THIRD_PARTY_LICENSES.html"; DestDir: "{app}"; Flags: ignoreversion

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Icons]
Name: "{autoprograms}\VERA"; Filename: "{app}\vera20k.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\VERA"; Filename: "{app}\vera20k.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[InstallDelete]
; AppId and the previous install directory still identify upgrades. Replace
; our old shortcuts so an upgrade has one visible product name.
Type: files; Name: "{autoprograms}\VERA20K.lnk"
Type: files; Name: "{autodesktop}\VERA20K.lnk"

[Run]
Filename: "{app}\vera20k.exe"; WorkingDir: "{app}"; Description: "{cm:LaunchProgram,VERA}"; Flags: nowait postinstall skipifsilent

[Code]
type
  TGameSource = (gsManual, gsDetected, gsSteam, gsEAApp);
  TGameInstallation = record
    Folder, Language, RequiredIssues, OptionalIssues: String;
    Source: TGameSource;
  end;
  TGameInstallations = array of TGameInstallation;

var
  GameDirPage: TInputDirWizardPage;
  GameCheckPage: TOutputMsgMemoWizardPage;
  GameInstallList: TNewCheckListBox;
  GameInstallDetails: TNewMemo;
  GameInstallNote: TNewStaticText;
  Installations: TGameInstallations;
  RecommendedInstall: Integer;
  InstallationsScanned, UpdatingSelection: Boolean;
  RequiredIssues, OptionalIssues: String;

function NormalizeGameDir(Dir: String): String;
begin
  Result := RemoveQuotes(Trim(Dir));
  if Result <> '' then
  begin
    StringChangeEx(Result, '/', '\', True);
    { The edit box also calls this while a path is still being typed. Leave
      incomplete paths to the reader's normal error report on Next. }
    try
      Result := ExpandFileName(Result);
    except
    end;
    Result := RemoveBackslashUnlessRoot(Result);
  end;
end;

{ The folder a registry value names, or empty without the value. A value naming
  a program gives that program's folder. }
function RegistryDir(RootKey: Integer; SubKey, ValueName: String; IsExecutable: Boolean): String;
var
  Value: String;
begin
  Result := '';
  if RegQueryStringValue(RootKey, SubKey, ValueName, Value) then
  begin
    Value := RemoveQuotes(Trim(Value));
    if IsExecutable and not DirExists(Value) and
       (CompareText(ExtractFileExt(Value), '.exe') = 0) then
      Value := ExtractFileDir(Value);
    Result := RemoveBackslashUnlessRoot(Value);
  end;
end;

procedure AddCandidate(var Candidates: TGameInstallations; Dir: String; Source: TGameSource);
var
  I, Count: Integer;
begin
  Dir := NormalizeGameDir(Dir);
  { A known installation may be damaged: keep it even without both masters.
    Only known registry/library/default paths are probed, never whole drives. }
  if (Dir = '') or not DirExists(Dir) then
    Exit;
  Count := GetArrayLength(Candidates);
  for I := 0 to Count - 1 do
    if CompareText(Candidates[I].Folder, Dir) = 0 then
    begin
      { Keep a known source when an explicit/previous path names the same copy.
        Only Steam/EA discovery identifies a storefront; never guess by name. }
      if ((Candidates[I].Source = gsManual) and (Source <> gsManual)) or
        ((Candidates[I].Source = gsDetected) and (Source in [gsSteam, gsEAApp])) then
        Candidates[I].Source := Source;
      Exit;
    end;
  SetArrayLength(Candidates, Count + 1);
  Candidates[Count].Folder := Dir;
  Candidates[Count].Source := Source;
end;

{ A machine-wide entry, in the registry view of 32-bit programs, then 64-bit ones. }
procedure AddMachineEntry(var Candidates: TGameInstallations; SubKey, ValueName: String;
  IsExecutable: Boolean; Source: TGameSource);
begin
  AddCandidate(Candidates, RegistryDir(HKLM32, SubKey, ValueName, IsExecutable), Source);
  AddCandidate(Candidates, RegistryDir(HKLM64, SubKey, ValueName, IsExecutable), Source);
end;

{ The folder of a `"path"  "<folder>"` line of Steam's libraryfolders.vdf, with
  its doubled backslashes undone, or empty for any other line. The folder only
  passes through Inno's String functions: through Pascal Script's Copy and
  Trim, the characters the system code page lacks became question marks. }
function SteamLibraryPath(Line: String): String;
var
  Fields: TArrayOfString;
  Folder: String;
begin
  Result := '';
  Fields := StringSplitEx(Line, [#9, ' '], '"', stExcludeEmpty);
  if (GetArrayLength(Fields) = 2) and (CompareText(Fields[0], '"path"') = 0) then
  begin
    Folder := RemoveQuotes(Fields[1]);
    StringChangeEx(Folder, '\\', '\', True);
    Result := Folder;
  end;
end;

{ Steam installs Red Alert 2 and Yuri's Revenge, app 2229850, in
  steamapps\common\Command & Conquer Red Alert II (its installdir) of a library.
  The libraries are Steam's own folder and each "path" of its
  steamapps\libraryfolders.vdf, the files OpenRA's SteamSourceResolver reads.
  Steam names its folder with forward slashes in HKCU and backslashes in HKLM. }
procedure AddSteamLibraries(var Candidates: TGameInstallations; Steam: String);
var
  Lines: TArrayOfString;
  I: Integer;
  Folder: String;
begin
  if Steam = '' then
    Exit;
  StringChangeEx(Steam, '/', '\', True);
  Log('Steam folder: ' + Steam);
  AddCandidate(Candidates, AddBackslash(Steam) + 'steamapps\common\Command & Conquer Red Alert II', gsSteam);
  if LoadStringsFromFile(AddBackslash(Steam) + 'steamapps\libraryfolders.vdf', Lines) then
    for I := 0 to GetArrayLength(Lines) - 1 do
    begin
      Folder := SteamLibraryPath(Lines[I]);
      if Folder <> '' then
      begin
        Log('Steam library: ' + Folder);
        AddCandidate(Candidates, AddBackslash(Folder) + 'steamapps\common\Command & Conquer Red Alert II', gsSteam);
      end;
    end;
end;

{ Collect every existing known installation, including incomplete copies.
  Normalize and deduplicate registry views, Steam libraries and default paths. }
function DetectGameDirs(): TGameInstallations;
var
  Candidates: TGameInstallations;
begin
  AddCandidate(Candidates, ExpandConstant('{param:GAMEDIR|}'), gsManual);
  AddCandidate(Candidates, GetPreviousData('GameDir', ''), gsManual);
  AddMachineEntry(Candidates, 'SOFTWARE\Westwood\Red Alert 2', 'InstallPath', True, gsDetected);
  AddMachineEntry(Candidates, 'SOFTWARE\Westwood\Yuri''s Revenge', 'InstallPath', True, gsDetected);
  AddMachineEntry(Candidates, 'Software\Electronic Arts\EA Games\Command and Conquer The First Decade', 'r2_folder', False, gsDetected);
  AddMachineEntry(Candidates, 'Software\EA Games\Command and Conquer Red Alert II', 'Install Dir', False, gsEAApp);
  AddSteamLibraries(Candidates, RegistryDir(HKCU, 'Software\Valve\Steam', 'SteamPath', False));
  AddSteamLibraries(Candidates, RegistryDir(HKLM32, 'SOFTWARE\Valve\Steam', 'InstallPath', False));
  AddSteamLibraries(Candidates, RegistryDir(HKLM64, 'SOFTWARE\Valve\Steam', 'InstallPath', False));
  AddCandidate(Candidates, ExpandConstant('{commonpf32}\EA Games\Command & Conquer The First Decade\Command & Conquer Red Alert(tm) II\RA2'), gsDetected);
  AddCandidate(Candidates, ExpandConstant('{commonpf64}\EA Games\Command & Conquer The First Decade\Command & Conquer Red Alert(tm) II\RA2'), gsDetected);
  AddCandidate(Candidates, ExpandConstant('{commonpf32}\EA Games\Command & Conquer Die ersten 10 Jahre\Command & Conquer Red Alert(tm) II\RA2'), gsDetected);
  AddCandidate(Candidates, ExpandConstant('{commonpf64}\EA Games\Command & Conquer Die ersten 10 Jahre\Command & Conquer Red Alert(tm) II\RA2'), gsDetected);
  AddCandidate(Candidates, ExpandConstant('{commonpf32}\Origin Games\Command and Conquer Red Alert II'), gsDetected);
  AddCandidate(Candidates, ExpandConstant('{commonpf64}\Origin Games\Command and Conquer Red Alert II'), gsDetected);
  { The EA app's default games folder, with the folder name HasanForge/
    redalert2-recomp's README gives an EA app install. }
  AddCandidate(Candidates, ExpandConstant('{commonpf64}\EA Games\Command and Conquer Red Alert II'), gsEAApp);
  AddCandidate(Candidates, 'C:\Westwood\RA2', gsDetected);
  Result := Candidates;
end;

function GameDir(): String;
begin
  Result := NormalizeGameDir(GameDirPage.Values[0]);
end;

function ReadReport(FileName: String): String;
var
  Lines: TArrayOfString;
  I: Integer;
begin
  if not LoadStringsFromFile(FileName, Lines) then
    RaiseException('Cannot read ' + FileName);
  Result := '';
  for I := 0 to GetArrayLength(Lines) - 1 do
    Result := Result + Lines[I] + #13#10;
  Result := Trim(Result);
end;

{ Run our bundled reader, never a program from the selected game folder.
  Refresh the report fields while keeping the source recorded at discovery.
  Required/optional policy and nested MIX resolution belong to the game. }
procedure InspectGameFolder(Dir: String; var Installation: TGameInstallation);
var
  ReportDir, Params: String;
  ExitCode: Integer;
begin
  Installation.Folder := NormalizeGameDir(Dir);
  Installation.RequiredIssues := '';
  Installation.OptionalIssues := '';
  Installation.Language := '';
  try
    if Installation.Folder = '' then
      RaiseException(CustomMessage('GameCheckNoFolder'));
    if not FileExists(ExpandConstant('{tmp}\vera20k.exe')) then
      ExtractTemporaryFile('vera20k.exe');
    ReportDir := ExpandConstant('{tmp}\game-data-check');
    if not ForceDirectories(ReportDir) then
      RaiseException('Cannot create ' + ReportDir);
    { A retry must never read an older folder's results. }
    if FileExists(ReportDir + '\required.txt') and
      not DeleteFile(ReportDir + '\required.txt') then
      RaiseException('Cannot clear required.txt');
    if FileExists(ReportDir + '\optional.txt') and
      not DeleteFile(ReportDir + '\optional.txt') then
      RaiseException('Cannot clear optional.txt');
    if FileExists(ReportDir + '\language.txt') and
      not DeleteFile(ReportDir + '\language.txt') then
      RaiseException('Cannot clear language.txt');
    { The final dot also quotes drive roots safely: no trailing backslash
      immediately before the Windows command line's closing quote. }
    Params := '--check-game-data "' + AddBackslash(Installation.Folder) + '." "' + ReportDir + '"';
    if not Exec(ExpandConstant('{tmp}\vera20k.exe'), Params,
      ExpandConstant('{tmp}'), SW_HIDE, ewWaitUntilTerminated, ExitCode) then
      RaiseException(SysErrorMessage(ExitCode));
    if (ExitCode <> 0) and (ExitCode <> 1) then
      RaiseException('Game-data checker exit code ' + IntToStr(ExitCode));
    Installation.RequiredIssues := ReadReport(ReportDir + '\required.txt');
    Installation.OptionalIssues := ReadReport(ReportDir + '\optional.txt');
    Installation.Language := ReadReport(ReportDir + '\language.txt');
    if ((ExitCode = 0) and (Installation.RequiredIssues <> '')) or
      ((ExitCode = 1) and (Installation.RequiredIssues = '')) then
      RaiseException('Game-data checker returned an inconsistent report');
  except
    Installation.RequiredIssues := FmtMessage(CustomMessage('GameCheckFailed'), [GetExceptionMessage]);
  end;
end;

{ The CSF header, not the operating system, identifies the game text language.
  Language enum: github.com/Ares-Developers/YRpp/blob/master/StringTable.h. }
function GameLanguageName(Language: String): String;
begin
  case Language of
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9':
      Result := CustomMessage('GameLanguage' + Language);
    '', '10': Result := CustomMessage('GameLanguageUnknown');
  else
    Result := FmtMessage(CustomMessage('GameLanguageOther'), [Language]);
  end;
end;

function GameSourceName(Source: TGameSource): String;
begin
  case Source of
    gsManual: Result := CustomMessage('GameSourceManual');
    gsSteam: Result := CustomMessage('GameSourceSteam');
    gsEAApp: Result := CustomMessage('GameSourceEAApp');
  else
    Result := CustomMessage('GameSourceDetected');
  end;
end;

function InstallationStatus(Installation: TGameInstallation): String;
begin
  if Installation.RequiredIssues <> '' then
    Result := CustomMessage('GameInstallMissing')
  else if Installation.OptionalIssues <> '' then
    Result := CustomMessage('GameInstallOptional')
  else
    Result := CustomMessage('GameInstallReady');
end;

function InstallationIssues(Installation: TGameInstallation): String;
begin
  Result := '';
  if Installation.RequiredIssues <> '' then
    Result := CustomMessage('GameCheckRequiredHeading') + #13#10 +
      Installation.RequiredIssues;
  if Installation.OptionalIssues <> '' then
  begin
    if Result <> '' then
      Result := Result + #13#10#13#10;
    Result := Result + CustomMessage('GameCheckOptionalHeading') + #13#10 +
      Installation.OptionalIssues;
  end;
end;

function InstallationReport(Installation: TGameInstallation): String;
var
  Issues: String;
begin
  Result := Installation.Folder + #13#10 +
    FmtMessage(CustomMessage('GameInstallSource'), [GameSourceName(Installation.Source)]) + #13#10 +
    FmtMessage(CustomMessage('GameInstallLanguage'), [GameLanguageName(Installation.Language)]) + #13#10 +
    InstallationStatus(Installation);
  Issues := InstallationIssues(Installation);
  if Issues <> '' then
    Result := Result + #13#10#13#10 + Issues;
end;

function FindInstallation(Dir: String): Integer;
var
  I: Integer;
begin
  Dir := NormalizeGameDir(Dir);
  Result := -1;
  for I := 0 to GetArrayLength(Installations) - 1 do
    if CompareText(Installations[I].Folder, Dir) = 0 then
    begin
      Result := I;
      Exit;
    end;
end;

procedure RecommendInstallation();
var
  I: Integer;
begin
  RecommendedInstall := -1;
  for I := 0 to GetArrayLength(Installations) - 1 do
    if Installations[I].RequiredIssues = '' then
    begin
      if RecommendedInstall < 0 then
        RecommendedInstall := I
      else if (Installations[RecommendedInstall].OptionalIssues <> '') and
        (Installations[I].OptionalIssues = '') then
        RecommendedInstall := I;
    end;
end;

function PreferredGameDir(Requested: String): String;
begin
  Result := NormalizeGameDir(Requested);
  if Result <> '' then
    Exit;
  if RecommendedInstall >= 0 then
    Result := Installations[RecommendedInstall].Folder
  else if GetArrayLength(Installations) > 0 then
    Result := Installations[0].Folder;
end;

procedure RefreshInstallationList();
var
  I, Selected, Space: Integer;
  Summary: String;
begin
  UpdatingSelection := True;
  try
    Selected := FindInstallation(GameDir());
    GameInstallList.Items.Clear();
    for I := 0 to GetArrayLength(Installations) - 1 do
    begin
      Summary := GameSourceName(Installations[I].Source) + ' | ' +
        GameLanguageName(Installations[I].Language) + ' | ' +
        InstallationStatus(Installations[I]);
      if I = RecommendedInstall then
        Summary := CustomMessage('GameInstallRecommended') + ' | ' + Summary;
      GameInstallList.AddRadioButton(
        MinimizePathName(Installations[I].Folder, GameInstallList.Font,
          GameInstallList.Width - ScaleX(32)), Summary, 0, I = Selected, True, nil);
    end;
    GameInstallList.ItemIndex := Selected;
    if Selected >= 0 then
      GameInstallDetails.Text := InstallationIssues(Installations[Selected])
    else if (GetArrayLength(Installations) = 0) and (GameDir() = '') then
      GameInstallDetails.Text := CustomMessage('GameInstallNone')
    else
      GameInstallDetails.Text := GameSourceName(gsManual) + #13#10 +
        CustomMessage('GameInstallManual');
    { Healthy copies need no second box repeating their row. Keep the space
      for detected installations; show exact content issues when needed. }
    Space := GameInstallNote.Top - GameInstallList.Top - ScaleY(8);
    GameInstallDetails.Visible := GameInstallDetails.Text <> '';
    if GameInstallDetails.Visible then
    begin
      GameInstallList.Height := (Space - ScaleY(8)) div 2;
      GameInstallDetails.Top := GameInstallList.Top + GameInstallList.Height + ScaleY(8);
      GameInstallDetails.Height := GameInstallNote.Top - GameInstallDetails.Top - ScaleY(8);
    end
    else
      GameInstallList.Height := Space;
  finally
    UpdatingSelection := False;
  end;
end;

procedure GameInstallationSelected(Sender: TObject);
var
  I: Integer;
begin
  if UpdatingSelection then
    Exit;
  for I := 0 to GetArrayLength(Installations) - 1 do
    if GameInstallList.Checked[I] then
    begin
      UpdatingSelection := True;
      try
        GameDirPage.Values[0] := Installations[I].Folder;
      finally
        UpdatingSelection := False;
      end;
      RefreshInstallationList();
      Exit;
    end;
end;

procedure GameFolderEdited(Sender: TObject);
begin
  if InstallationsScanned and not UpdatingSelection then
    RefreshInstallationList();
end;

procedure ScanGameInstallations();
var
  I: Integer;
begin
  if InstallationsScanned then
    Exit;
  WizardForm.NextButton.Enabled := False;
  WizardForm.Cursor := crHourGlass;
  GameInstallDetails.Text := CustomMessage('GameInstallChecking');
  GameInstallDetails.Repaint();
  try
    Installations := DetectGameDirs();
    for I := 0 to GetArrayLength(Installations) - 1 do
    begin
      InspectGameFolder(Installations[I].Folder, Installations[I]);
      Log('Detected game installation: ' + InstallationReport(Installations[I]));
    end;
    Log('Detected game installations: ' + IntToStr(GetArrayLength(Installations)));
    RecommendInstallation();
    UpdatingSelection := True;
    try
      GameDirPage.Values[0] := PreferredGameDir(ExpandConstant('{param:GAMEDIR|}'));
    finally
      UpdatingSelection := False;
    end;
    if RecommendedInstall >= 0 then
      Log('Recommended game folder: ' + Installations[RecommendedInstall].Folder)
    else
      Log('No detected game folder passed required-file validation.');
    InstallationsScanned := True;
    RefreshInstallationList();
  finally
    WizardForm.Cursor := crDefault;
    WizardForm.NextButton.Enabled := True;
  end;
end;

procedure InitializeWizard();
var
  HeaderTop, ListTop, Space: Integer;
begin
  { Align the standard header lines and leave a small gap below the title.
    Keep the subtitle's bottom edge inside the existing header panel. }
  HeaderTop := WizardForm.PageNameLabel.Top + WizardForm.PageNameLabel.Height + ScaleY(4);
  WizardForm.PageDescriptionLabel.SetBounds(WizardForm.PageNameLabel.Left, HeaderTop,
    WizardForm.PageNameLabel.Width, WizardForm.PageDescriptionLabel.Top +
    WizardForm.PageDescriptionLabel.Height - HeaderTop);
  RecommendedInstall := -1;
  GameDirPage := CreateInputDirPage(wpSelectDir, CustomMessage('GameDirCaption'),
    CustomMessage('GameDirDescription'), CustomMessage('GameDirSubCaption'), False, '');
  GameDirPage.Add(CustomMessage('GameDirPrompt'));
  GameDirPage.Values[0] := NormalizeGameDir(ExpandConstant('{param:GAMEDIR|}'));
  GameDirPage.Edits[0].OnChange := @GameFolderEdited;
  ListTop := GameDirPage.Edits[0].Top + GameDirPage.Edits[0].Height + ScaleY(8);
  GameInstallNote := TNewStaticText.Create(GameDirPage);
  GameInstallNote.Parent := GameDirPage.Surface;
  GameInstallNote.AutoSize := False;
  GameInstallNote.WordWrap := True;
  GameInstallNote.Width := GameDirPage.SurfaceWidth;
  GameInstallNote.Caption := CustomMessage('GameInstallUntouched');
  GameInstallNote.AdjustHeight();
  GameInstallNote.Top := GameDirPage.SurfaceHeight - GameInstallNote.Height;
  Space := GameInstallNote.Top - ListTop - ScaleY(16);
  GameInstallList := TNewCheckListBox.Create(GameDirPage);
  GameInstallList.Parent := GameDirPage.Surface;
  GameInstallList.SetBounds(0, ListTop, GameDirPage.SurfaceWidth, Space div 2);
  GameInstallList.RequireRadioSelection := False;
  GameInstallList.OnClickCheck := @GameInstallationSelected;
  GameInstallDetails := TNewMemo.Create(GameDirPage);
  GameInstallDetails.Parent := GameDirPage.Surface;
  GameInstallDetails.SetBounds(0, ListTop + GameInstallList.Height + ScaleY(8),
    GameDirPage.SurfaceWidth, Space - GameInstallList.Height);
  GameInstallDetails.ReadOnly := True;
  GameInstallDetails.ScrollBars := ssVertical;
  GameInstallDetails.WordWrap := True;
  GameInstallDetails.Text := CustomMessage('GameInstallChoose');
  { Inno omits SubCaptionLabel for an empty initial caption, but the report
    updates it below. Inno 7.1.0: Setup.WizardForm.CustomPages.pas:754-770. }
  GameCheckPage := CreateOutputMsgMemoPage(GameDirPage.ID,
    CustomMessage('GameCheckCaption'), CustomMessage('GameCheckDescription'),
    CustomMessage('GameCheckRequired'), '');
end;

procedure CheckGameFiles();
var
  Selected: Integer;
  Checked: TGameInstallation;
begin
  ScanGameInstallations();
  WizardForm.NextButton.Enabled := False;
  WizardForm.Cursor := crHourGlass;
  try
    { Recheck the chosen folder, including manual paths, before installation. }
    Selected := FindInstallation(GameDir());
    if Selected >= 0 then
      Checked := Installations[Selected]
    else
      Checked.Source := gsManual;
    InspectGameFolder(GameDir(), Checked);
    if Checked.Folder <> '' then
    begin
      if Selected < 0 then
      begin
        Selected := GetArrayLength(Installations);
        SetArrayLength(Installations, Selected + 1);
      end;
      Installations[Selected] := Checked;
    end;
    RequiredIssues := Checked.RequiredIssues;
    OptionalIssues := Checked.OptionalIssues;
    RecommendInstallation();
    RefreshInstallationList();
    GameCheckPage.RichEditViewer.Lines.Text := InstallationReport(Checked);
    Log('Game-data check: ' + InstallationReport(Checked));
    if RequiredIssues <> '' then
      GameCheckPage.SubCaptionLabel.Caption := CustomMessage('GameCheckRequired')
    else if OptionalIssues <> '' then
      GameCheckPage.SubCaptionLabel.Caption := CustomMessage('GameCheckOptional')
    else
      GameCheckPage.SubCaptionLabel.Caption := CustomMessage('GameCheckReady');
    GameCheckPage.SubCaptionLabel.AdjustHeight();
    GameCheckPage.RichEditViewer.Top := GameCheckPage.SubCaptionLabel.Top +
      GameCheckPage.SubCaptionLabel.Height + ScaleY(10);
    GameCheckPage.RichEditViewer.Height := GameCheckPage.SurfaceHeight -
      GameCheckPage.RichEditViewer.Top;
  finally
    WizardForm.Cursor := crDefault;
    WizardForm.NextButton.Enabled := True;
  end;
end;

{ A silent install calls this too, for every page, and exits when it returns
  False. /SUPPRESSMSGBOXES silences only SuppressibleMsgBox, not MsgBox. }
function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = GameDirPage.ID then
  begin
    CheckGameFiles();
    { Interactive installs show the full report with Back available. Silent
      installs record it in /LOG and stop before installing required failures. }
    Result := not WizardSilent() or (RequiredIssues = '');
  end
  else if CurPageID = GameCheckPage.ID then
    Result := RequiredIssues = '';
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = GameDirPage.ID then
    ScanGameInstallations()
  else if CurPageID = GameCheckPage.ID then
    WizardForm.NextButton.Enabled := RequiredIssues = '';
end;

procedure RegisterPreviousData(PreviousDataKey: Integer);
begin
  SetPreviousData(PreviousDataKey, 'GameDir', GameDir());
end;

{ Whether Line assigns Key: `key = ...`, or with Commented `# key = ...`. }
function AssignedKey(Line: String; Key: String; Commented: Boolean): Boolean;
var
  Rest: String;
begin
  Rest := Trim(Line);
  if Commented then
  begin
    if Copy(Rest, 1, 1) <> '#' then
    begin
      Result := False;
      Exit;
    end;
    Rest := Trim(Copy(Rest, 2, Length(Rest)));
  end;
  Result := (Copy(Rest, 1, Length(Key)) = Key)
    and (Copy(Trim(Copy(Rest, Length(Key) + 1, Length(Rest))), 1, 1) = '=');
end;

{ Set [paths] Key to the TOML string Value: on its line, else on a commented-out
  example of it, else on a new line after [paths]. config.toml rejects a key
  given twice, so an existing line is always reused. }
procedure SetPathsKey(var Lines: TArrayOfString; Key, Value: String);
var
  I, Target, PathsHeader, Count: Integer;
  Assignment: String;
begin
  Assignment := Key + ' = "' + Value + '"';
  Count := GetArrayLength(Lines);
  Target := -1;
  for I := 0 to Count - 1 do
    if (Target < 0) and AssignedKey(Lines[I], Key, False) then
      Target := I;
  for I := 0 to Count - 1 do
    if (Target < 0) and AssignedKey(Lines[I], Key, True) then
      Target := I;
  if Target >= 0 then
  begin
    Lines[Target] := Assignment;
    Exit;
  end;
  PathsHeader := -1;
  for I := 0 to Count - 1 do
    if (PathsHeader < 0) and (Trim(Lines[I]) = '[paths]') then
      PathsHeader := I;
  if PathsHeader < 0 then
  begin
    SetArrayLength(Lines, Count + 2);
    Lines[Count] := '[paths]';
    Lines[Count + 1] := Assignment;
    Exit;
  end;
  SetArrayLength(Lines, Count + 1);
  for I := Count downto PathsHeader + 2 do
    Lines[I] := Lines[I - 1];
  Lines[PathsHeader + 1] := Assignment;
end;

{ Point config.toml at the game folder. A new config starts from the example
  and keeps settings in the install folder; an existing one keeps everything
  but ra2_dir. Forward slashes need no escaping in a TOML string. }
procedure WriteConfig();
var
  ConfigPath, Folder: String;
  Lines: TArrayOfString;
begin
  ConfigPath := ExpandConstant('{app}\config.toml');
  Folder := GameDir();
  StringChangeEx(Folder, '\', '/', True);
  if FileExists(ConfigPath) then
  begin
    if not LoadStringsFromFile(ConfigPath, Lines) then
      RaiseException(FmtMessage(CustomMessage('ConfigNotWritten'), [ConfigPath]));
    SetPathsKey(Lines, 'ra2_dir', Folder);
  end
  else
  begin
    if not LoadStringsFromFile(ExpandConstant('{app}\config.toml.example'), Lines) then
      SetArrayLength(Lines, 0);
    SetPathsKey(Lines, 'ra2_dir', Folder);
    SetPathsKey(Lines, 'settings_dir', '.');
  end;
  if not SaveStringsToUTF8FileWithoutBOM(ConfigPath, Lines, False) then
    RaiseException(FmtMessage(CustomMessage('ConfigNotWritten'), [ConfigPath]));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    WriteConfig();
end;
