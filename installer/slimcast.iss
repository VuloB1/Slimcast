; Instalador de Slimcast. Se compila con installer\build.ps1 (pasa AppVersion y deja ffmpeg en installer\ffmpeg).
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{8724878D-1425-4E40-81E4-66A536E8EFFB}
AppName=Slimcast
AppVersion={#AppVersion}
AppVerName=Slimcast {#AppVersion}
AppPublisher=VuloB1
AppPublisherURL=https://github.com/VuloB1/Slimcast
AppSupportURL=https://github.com/VuloB1/Slimcast/issues
DefaultDirName={autopf}\Slimcast
DisableProgramGroupPage=yes
DisableWelcomePage=no
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Slimcast-{#AppVersion}-setup
SetupIconFile=..\assets\slimcast.ico
UninstallDisplayIcon={app}\Slimcast.exe
UninstallDisplayName=Slimcast
WizardStyle=modern dark
WizardImageFile=art\wizard-100.png,art\wizard-125.png,art\wizard-150.png,art\wizard-175.png,art\wizard-200.png
WizardSmallImageFile=art\small-100.png,art\small-125.png,art\small-150.png,art\small-175.png,art\small-200.png
Compression=lzma2/ultra64
SolidCompression=yes
; ffmpeg y ffprobe comparten casi todo su código: un diccionario mayor que un .exe los deduplica
LZMADictionarySize=262144
LZMAUseSeparateProcess=yes
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\slimcast.exe"; DestDir: "{app}"; DestName: "Slimcast.exe"; Flags: ignoreversion
Source: "ffmpeg\ffmpeg.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "ffmpeg\ffprobe.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "ffmpeg\LICENSE"; DestDir: "{app}"; DestName: "ffmpeg-LICENSE.txt"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Slimcast"; Filename: "{app}\Slimcast.exe"
Name: "{autodesktop}\Slimcast"; Filename: "{app}\Slimcast.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Slimcast.exe"; Description: "{cm:LaunchProgram,Slimcast}"; Flags: nowait postinstall skipifsilent
; la actualización desde la app instala en silencio: al acabar, vuelve a abrir Slimcast
Filename: "{app}\Slimcast.exe"; Flags: nowait; Check: WizardSilent
