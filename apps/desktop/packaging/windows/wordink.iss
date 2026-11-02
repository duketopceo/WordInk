; Per-user Windows installer for WordInk. It installs exactly the managed layout
; that .github/release/install-windows.ps1 creates and src/windows_updater.rs updates:
;
;   %APPDATA%\wordink\versions\<version>\wordink.exe
;   %APPDATA%\wordink\current-version
;   %APPDATA%\wordink\install.json  (the direct-install receipt)
;   Start Menu\Programs\WordInk.lnk  (description "WordInk managed install")
;
; so the app keeps updating itself from the release service afterwards. The
; uninstaller lives outside versions\ because the updater prunes old version
; directories. Settings, logs, history, and models stay on uninstall, as with
; install-windows.ps1 -Uninstall.
;
; Compile with scripts/build-windows-installer.ps1, which passes:
;   /DAppVersion=2.3.0 /DArch=x64|arm64 /DExecutable=<wordink.exe> /DOutputDir=<dir>

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef Arch
  #error Pass /DArch=x64 or /DArch=arm64
#endif
#ifndef Executable
  #error Pass /DExecutable=<path to wordink.exe>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

#if Arch == "x64"
  #define AllowedArchitectures "x64os"
#elif Arch == "arm64"
  #define AllowedArchitectures "arm64"
#else
  #error Arch must be x64 or arm64
#endif

[Setup]
; Keep this AppId forever: it names the Apps & Features entry that
; src/windows_updater.rs keeps current after self-updates.
AppId={{DCAC91D8-E7C0-4CEF-A61F-A42EC0FB6BC8}
AppName=WordInk
AppVersion={#AppVersion}
AppVerName=WordInk {#AppVersion}
AppPublisher=WordInk contributors
AppPublisherURL=https://github.com/duketopceo/WordInk
AppSupportURL=https://github.com/duketopceo/WordInk/issues
AppUpdatesURL=https://github.com/duketopceo/WordInk/releases
PrivilegesRequired=lowest
ArchitecturesAllowed={#AllowedArchitectures}
ArchitecturesInstallIn64BitMode={#AllowedArchitectures}
DefaultDirName={userappdata}\wordink
DisableDirPage=yes
UsePreviousAppDir=no
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableWelcomePage=yes
UninstallFilesDir={app}\uninstall
UninstallDisplayName=WordInk
UninstallDisplayIcon={app}\uninstall\wordink.ico
SetupIconFile=..\..\resources\windows\wordink.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=wordink-setup-{#AppVersion}-windows-{#Arch}
CloseApplications=yes
RestartApplications=no
MinVersion=10.0

[Files]
Source: "{#Executable}"; DestDir: "{app}\versions\{#AppVersion}"; DestName: "wordink.exe"; Flags: ignoreversion
Source: "..\..\resources\windows\wordink.ico"; DestDir: "{app}\uninstall"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\WordInk"; Filename: "{app}\versions\{#AppVersion}\wordink.exe"; Parameters: "app"; WorkingDir: "{app}\versions\{#AppVersion}"; Comment: "WordInk managed install"

[Run]
Filename: "{app}\versions\{#AppVersion}\wordink.exe"; Parameters: "app"; Description: "Open WordInk"; Flags: postinstall nowait skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}\versions"
Type: filesandordirs; Name: "{app}\updates"
Type: files; Name: "{app}\current-version"
Type: files; Name: "{app}\current-version.partial"
Type: files; Name: "{app}\install.json"

[Code]
function InitializeSetup(): Boolean;
begin
  Result := True;
  { Installs this version whatever is installed now: versions are never
    compared, and the self-updater follows the channel's current release. }
#if Arch == "x64"
  { The x64 build decodes on the GPU through Vulkan. Its loader ships with
    NVIDIA, AMD, and Intel graphics drivers, not with Windows. }
  if (ExpandConstant('{param:SKIPVULKANCHECK|0}') <> '1') and
     not FileExists(ExpandConstant('{sys}\vulkan-1.dll')) then
  begin
    SuppressibleMsgBox('WordInk needs a graphics driver with Vulkan support, and vulkan-1.dll was not found.' + #13#10#13#10 +
      'Update your NVIDIA, AMD, or Intel graphics driver, then run this installer again.',
      mbError, MB_OK, IDOK);
    Result := False;
  end;
#endif
end;

procedure StopManagedWordInk();
var
  ResultCode: Integer;
  Versions: String;
begin
  Versions := ExpandConstant('{app}\versions\');
  StringChangeEx(Versions, '''', '''''', True);
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -WindowStyle Hidden -Command "Get-Process wordink -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.Path -and $_.Path.StartsWith(''' + Versions + ''', [StringComparison]::OrdinalIgnoreCase) } | ' +
    'Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

{ An earlier version left running would hold the instance lock. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopManagedWordInk();
  Result := '';
end;

{ Replaces Path with Contents through a .partial file. }
procedure WriteAtomically(Path, Contents: String);
var
  Partial: String;
begin
  Partial := Path + '.partial';
  DeleteFile(Partial);
  if not SaveStringToFile(Partial, Contents, False) then
    RaiseException('Could not write ' + Partial);
  DeleteFile(Path);
  if not RenameFile(Partial, Path) then
    RaiseException('Could not write ' + Path);
end;

{ Point current-version at this version only once its files are in place,
  then mark the install as direct so the app updates itself
  (docs/plans/release-service.md). }
procedure CurStepChanged(CurStep: TSetupStep);
var
  Root: String;
begin
  if CurStep = ssPostInstall then
  begin
    WriteAtomically(ExpandConstant('{app}\current-version'), '{#AppVersion}' + #10);
    Root := ExpandConstant('{app}');
    StringChangeEx(Root, '\', '\\', True);
    WriteAtomically(ExpandConstant('{app}\install.json'),
      '{"app":"hex","artifact":"desktop","channel":"stable","method":"direct","root":"' + Root + '"}' + #10);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    { Self-updates run from newer version directories this uninstaller did
      not create, so stop any WordInk running from the managed layout. }
    StopManagedWordInk();
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'WordInk', Command) and
       (Pos(LowerCase(ExpandConstant('{app}\versions\')), LowerCase(Command)) > 0) then
      RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'WordInk');
  end;
end;
