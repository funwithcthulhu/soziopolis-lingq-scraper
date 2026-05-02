#define MyAppName "Soziopolis Reader"
#define MyAppPublisher "Soziopolis Reader contributors"
#define MyAppURL "https://github.com/funwithcthulhu/soziopolis-lingq-scraper"
#define MyAppExeName "Soziopolis Reader.exe"
#ifndef AppVersion
  #define AppVersion "1.1.0"
#endif
#ifndef StageDir
  #define StageDir "."
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
AppId={{C4B6A42D-734E-4BF7-9C4A-6E1B54E12C9B}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=SoziopolisReaderSetup-{#AppVersion}
Compression=lzma
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#StageDir}\soziopolis-hires.ico
UninstallDisplayIcon={app}\soziopolis-hires.ico
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#StageDir}\Soziopolis Reader.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\soziopolis-hires.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\README.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\soziopolis-hires.ico"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\soziopolis-hires.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
