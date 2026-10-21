; Windows installer for VERA20k. The Release workflow (.github/workflows/release.yml)
; compiles it with the Inno Setup 6 compiler on GitHub's Windows runner:
;
;   ISCC /DAppVersion=<x.y.z> /DReleaseTag=<tag> /DPackageDir=<unpacked archive>
;        /DOutputDir=<dir> /DOutputBaseFilename=<name> vera20k.iss
;
; It installs the files of the Windows archive for the current user, without an
; administrator prompt, and asks for the Yuri's Revenge folder. It writes
; config.toml beside the program with that folder as ra2_dir and the install
; folder as settings_dir. Shortcuts start the program in its install folder, so
; logs, saves, replays and its RA2MD.INI stay there and never touch the game
; folder, which may be read-only or shared with the original game.
;
; A silent install takes the game folder from /GAMEDIR=<folder>.
;
; ChineseSimplified.isl is Inno Setup's own translation, from jrsoftware/issrc at
; 6ef32198e (Files/Languages); the Inno Setup 6 compiler does not ship it.

#ifndef AppVersion
  #error Define AppVersion, ReleaseTag, PackageDir, OutputDir and OutputBaseFilename
#endif

[Setup]
; Fixed for the life of the program: upgrades find the previous install by it.
AppId={{9C9BFC32-A7B8-48ED-A428-72333354BDDB}
AppName=VERA20k
AppVersion={#AppVersion}
AppVerName=VERA20k {#ReleaseTag}
AppPublisher=VERA20k
AppPublisherURL=https://github.com/YuriPlanet/vera20k
AppSupportURL=https://github.com/YuriPlanet/vera20k/issues
AppUpdatesURL=https://github.com/YuriPlanet/vera20k/releases
VersionInfoVersion={#AppVersion}
; Per user: {autopf} is %LOCALAPPDATA%\Programs, which the program can write to.
PrivilegesRequired=lowest
DefaultDirName={autopf}\VERA20k
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
ShowLanguageDialog=auto
UninstallDisplayIcon={app}\vera20k.exe
UninstallDisplayName=VERA20k
SourceDir={#PackageDir}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "{#SourcePath}ChineseSimplified.isl"

[CustomMessages]
english.GameDirCaption=Yuri's Revenge folder
english.GameDirDescription=Where is Yuri's Revenge installed?
english.GameDirSubCaption=VERA20k plays with the files of your own copy of Yuri's Revenge 1.001. Choose the folder that has ra2md.mix and ra2.mix, usually the one with gamemd.exe.
english.GameDirPrompt=Yuri's Revenge folder:
english.GameDirMissing=This folder does not contain Yuri's Revenge:%n%n%1%n%nChoose the folder that has ra2md.mix and ra2.mix, usually the one with gamemd.exe.
english.ConfigNotWritten=Setup could not write %1.
chinesesimplified.GameDirCaption=《尤里的复仇》文件夹
chinesesimplified.GameDirDescription=《尤里的复仇》安装在哪里？
chinesesimplified.GameDirSubCaption=VERA20k 使用你自己的《尤里的复仇》1.001 版游戏文件。请选择包含 ra2md.mix 和 ra2.mix 的文件夹，通常就是 gamemd.exe 所在的文件夹。
chinesesimplified.GameDirPrompt=《尤里的复仇》文件夹：
chinesesimplified.GameDirMissing=这个文件夹里没有《尤里的复仇》：%n%n%1%n%n请选择包含 ra2md.mix 和 ra2.mix 的文件夹，通常就是 gamemd.exe 所在的文件夹。
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
Name: "{autoprograms}\VERA20k"; Filename: "{app}\vera20k.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\VERA20k"; Filename: "{app}\vera20k.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\vera20k.exe"; WorkingDir: "{app}"; Description: "{cm:LaunchProgram,VERA20k}"; Flags: nowait postinstall skipifsilent

[Code]
var
  GameDirPage: TInputDirWizardPage;

{ VERA20k mounts both masters; without either it cannot start (AssetManager::new). }
function IsGameDir(Dir: String): Boolean;
begin
  Result := (Dir <> '') and FileExists(AddBackslash(Dir) + 'ra2md.mix')
    and FileExists(AddBackslash(Dir) + 'ra2.mix');
end;

function RegistryDir(SubKey, ValueName: String; IsExecutable: Boolean): String;
var
  Value: String;
begin
  Result := '';
  if RegQueryStringValue(HKLM32, SubKey, ValueName, Value) then
  begin
    if IsExecutable then
      Value := ExtractFileDir(Value);
    Result := RemoveBackslashUnlessRoot(Value);
  end;
end;

{ The places CnCNet's Yuri's Revenge installer looks (CnCNet/cncnet-yr-client-package,
  tools/build-installer/inno/installer.twig), in its order: the Westwood, First
  Decade and Origin/EA registry entries, then their default folders under both
  Program Files folders. Empty when none holds the game. }
function DetectGameDir(): String;
var
  Candidates: TArrayOfString;
  I: Integer;
begin
  SetArrayLength(Candidates, 11);
  Candidates[0] := RegistryDir('SOFTWARE\Westwood\Red Alert 2', 'InstallPath', True);
  Candidates[1] := RegistryDir('SOFTWARE\Westwood\Yuri''s Revenge', 'InstallPath', True);
  Candidates[2] := RegistryDir('Software\Electronic Arts\EA Games\Command and Conquer The First Decade', 'r2_folder', False);
  Candidates[3] := RegistryDir('Software\EA Games\Command and Conquer Red Alert II', 'Install Dir', False);
  Candidates[4] := ExpandConstant('{commonpf32}\EA Games\Command & Conquer The First Decade\Command & Conquer Red Alert(tm) II\RA2');
  Candidates[5] := ExpandConstant('{commonpf64}\EA Games\Command & Conquer The First Decade\Command & Conquer Red Alert(tm) II\RA2');
  Candidates[6] := ExpandConstant('{commonpf32}\EA Games\Command & Conquer Die ersten 10 Jahre\Command & Conquer Red Alert(tm) II\RA2');
  Candidates[7] := ExpandConstant('{commonpf64}\EA Games\Command & Conquer Die ersten 10 Jahre\Command & Conquer Red Alert(tm) II\RA2');
  Candidates[8] := ExpandConstant('{commonpf32}\Origin Games\Command and Conquer Red Alert II');
  Candidates[9] := ExpandConstant('{commonpf64}\Origin Games\Command and Conquer Red Alert II');
  Candidates[10] := 'C:\Westwood\RA2';
  Result := '';
  for I := 0 to GetArrayLength(Candidates) - 1 do
    if IsGameDir(Candidates[I]) then
    begin
      Result := Candidates[I];
      Exit;
    end;
end;

{ /GAMEDIR= first, then the folder chosen last time, then a detected one. }
function InitialGameDir(): String;
begin
  Result := ExpandConstant('{param:GAMEDIR|}');
  if Result = '' then
    Result := GetPreviousData('GameDir', '');
  if (Result = '') or not IsGameDir(Result) then
    if ExpandConstant('{param:GAMEDIR|}') = '' then
      Result := DetectGameDir();
end;

function GameDir(): String;
begin
  Result := RemoveBackslashUnlessRoot(Trim(GameDirPage.Values[0]));
end;

procedure InitializeWizard();
begin
  GameDirPage := CreateInputDirPage(wpSelectDir, CustomMessage('GameDirCaption'),
    CustomMessage('GameDirDescription'), CustomMessage('GameDirSubCaption'), False, '');
  GameDirPage.Add(CustomMessage('GameDirPrompt'));
  GameDirPage.Values[0] := InitialGameDir();
end;

{ A silent install calls this too, for every page, and exits when it returns
  False. /SUPPRESSMSGBOXES silences only SuppressibleMsgBox, not MsgBox. }
function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = GameDirPage.ID) and not IsGameDir(GameDir()) then
  begin
    SuppressibleMsgBox(FmtMessage(CustomMessage('GameDirMissing'), [GameDir()]),
      mbError, MB_OK, IDOK);
    Result := False;
  end;
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
