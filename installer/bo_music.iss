#ifndef APP_VERSION
  #error APP_VERSION must be provided
#endif
#ifndef APP_VERSION_CORE
  #error APP_VERSION_CORE must be provided
#endif
#ifndef SOURCE_DIR
  #error SOURCE_DIR must be provided
#endif
#ifndef OUTPUT_DIR
  #error OUTPUT_DIR must be provided
#endif

[Setup]
AppId={{E17D463A-233B-482B-93C8-7BAC3C832E97}
AppName=bo_music
AppVersion={#APP_VERSION}
VersionInfoVersion={#APP_VERSION_CORE}
AppPublisher=bo_music
DefaultDirName={localappdata}\Programs\bo_music
DefaultGroupName=bo_music
UninstallDisplayIcon={app}\bo_music.exe
SetupIconFile=..\assets\bo_music.ico
OutputDir={#OUTPUT_DIR}
OutputBaseFilename=bo_music_{#APP_VERSION}_windows_x64_setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes
DisableWelcomePage=no
UsePreviousLanguage=no
ShowLanguageDialog=no
CloseApplications=yes
RestartApplications=no
LicenseFile=..\LICENSE

[Languages]
Name: "chinesesimp"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "快捷方式："; Flags: unchecked

[Files]
Source: "{#SOURCE_DIR}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\bo_music"; Filename: "{app}\bo_music.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\bo_music"; Filename: "{app}\bo_music.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\bo_music.exe"; WorkingDir: "{app}"; Description: "启动 bo_music"; Flags: nowait postinstall skipifsilent
