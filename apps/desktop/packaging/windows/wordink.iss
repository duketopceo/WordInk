; Per-user Windows installer for WordInk (WordInk-Setup-windows-x64.exe on a
; release). No admin rights. It installs the same layout as the one-line
; installer, packaging/windows/install.ps1, so either can update or remove what
; the other installed:
;
;   %LOCALAPPDATA%\Programs\WordInk\wordink.exe (+ LICENSE, THIRD_PARTY_NOTICES.md)
;   %LOCALAPPDATA%\Programs\WordInk\install-receipt.txt  (the installed version)
;   Start Menu\Programs\WordInk.lnk
;   HKCU Run value "WordInk"  (the app's own Launch at login command)
;
; It writes no install.json receipt, so the app does not try to replace itself
; (Windows builds are unsigned for now) and only links to newer releases.
; The uninstaller lives in %LOCALAPPDATA%\WordInk\uninstall, outside the
; program folder that install.ps1 swaps on update. Uninstalling keeps
; settings, history and models (%APPDATA%\wordink), as install.ps1 -Uninstall
; does.
;
; Compile with scripts/build-windows-installer.ps1, which passes:
;   /DAppVersion=1.2.3 /DArch=x64|arm64 /DExecutable=<wordink.exe> /DOutputDir=<dir>

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
; Keep this AppId forever: it names the Apps & Features entry.
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
DefaultDirName={localappdata}\Programs\WordInk
DisableDirPage=yes
UsePreviousAppDir=no
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableWelcomePage=yes
UninstallFilesDir={localappdata}\WordInk\uninstall
UninstallDisplayName=WordInk
UninstallDisplayIcon={app}\wordink.exe
SetupIconFile=..\..\resources\windows\wordink.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=wordink-setup-{#AppVersion}-windows-{#Arch}
CloseApplications=yes
RestartApplications=no
MinVersion=10.0

[Tasks]
Name: "startup"; Description: "Start WordInk when I sign in"

[Files]
Source: "{#Executable}"; DestDir: "{app}"; DestName: "wordink.exe"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\WordInk"; Filename: "{app}\wordink.exe"; Parameters: "app"; WorkingDir: "{app}"; Comment: "WordInk"

[Registry]
; The same command the app writes for itself (src/windows/login_item.rs).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "WordInk"; ValueData: """{app}\wordink.exe"" app --hidden"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\wordink.exe"; Parameters: "app"; Description: "Open WordInk"; Flags: postinstall nowait skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\install-receipt.txt"
Type: dirifempty; Name: "{app}"

[Code]
#if Arch == "x64"
// The 64-bit vulkan-1.dll lives in the 64-bit System32 directory. Setup is a
// 32-bit process, so a check on the sys constant may be redirected by WOW64
// to SysWOW64, which holds only the 32-bit loader; the sysnative constant
// names the 64-bit directory from a 32-bit process. Either one found means
// the graphics driver installed the loader.
function VulkanLoaderInstalled(): Boolean;
begin
  Result := FileExists(ExpandConstant('{sys}\vulkan-1.dll')) or
    FileExists(ExpandConstant('{sysnative}\vulkan-1.dll'));
end;
#endif

function InitializeSetup(): Boolean;
begin
  Result := True;
  { Installs this version whatever is installed now: versions are never
    compared. }
#if Arch == "x64"
  { The x64 build decodes on the GPU through Vulkan. Its loader ships with
    NVIDIA, AMD, and Intel graphics drivers, not with Windows. }
  if (ExpandConstant('{param:SKIPVULKANCHECK|0}') <> '1') and
     not VulkanLoaderInstalled() then
  begin
    SuppressibleMsgBox('WordInk needs a graphics driver with Vulkan support, and vulkan-1.dll was not found.' + #13#10#13#10 +
      'Update your NVIDIA, AMD, or Intel graphics driver, then run this installer again.',
      mbError, MB_OK, IDOK);
    Result := False;
  end;
#endif
end;

{ Stops a WordInk running from the program folder, which would hold
  wordink.exe open. }
procedure StopInstalledWordInk();
var
  ResultCode: Integer;
  Folder: String;
begin
  Folder := ExpandConstant('{app}\');
  StringChangeEx(Folder, '''', '''''', True);
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -WindowStyle Hidden -Command "Get-Process wordink -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.Path -and $_.Path.StartsWith(''' + Folder + ''', [StringComparison]::OrdinalIgnoreCase) } | ' +
    'Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopInstalledWordInk();
  Result := '';
end;

{ install.ps1 reads the installed version from this file. }
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    if not SaveStringToFile(ExpandConstant('{app}\install-receipt.txt'), '{#AppVersion}' + #13#10, False) then
      RaiseException('Could not write ' + ExpandConstant('{app}\install-receipt.txt'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    StopInstalledWordInk();
    { The app's own Launch at login setting may have written the Run value
      even when the startup task was not chosen. }
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'WordInk', Command) and
       (Pos(LowerCase(ExpandConstant('{app}\wordink.exe')), LowerCase(Command)) > 0) then
      RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'WordInk');
  end;
end;
