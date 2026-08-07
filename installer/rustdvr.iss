; RustDVR installer.
;
; Packages whatever build.bat staged into dist\RustDVR. It deliberately does
; not know how that directory was assembled: the licence vetting of the media
; plugins happens during staging, so anything that reaches this point has
; already been checked.
;
; Per-user by default. A DVR client is a personal application and there is no
; reason to demand an administrator prompt to install one.

#define AppName        "RustDVR"
; Overridable from the command line: ISCC /DAppVersion=0.0.1
; build.ps1 passes whatever Cargo.toml says, so the installer, its filename and
; the executable's own version resource cannot drift apart.
#ifndef AppVersion
  #define AppVersion   "0.0.1"
#endif
#define AppPublisher   "David Brustein"
#define AppExeName     "rustdvr.exe"
#define StageDir       "..\dist\RustDVR"

[Setup]
AppId={{7B1E4C2A-9D3F-4A18-9C6E-5F2A7D8B0E41}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=no
OutputBaseFilename=RustDVR-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

; Per-user, so no elevation prompt. Windows 11 only: the app draws its own
; Mica-backed chrome and there is no fallback path for older releases.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000

LicenseFile={#StageDir}\LICENSE.md
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExeName}

SetupIconFile=..\assets\rustdvr.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#StageDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE.md";    DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\rustdvr.ico";   DestDir: "{app}"; Flags: ignoreversion
; The LGPL text and third-party notices, next to the libraries they cover.
Source: "{#StageDir}\licenses\*";    DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs

; FFmpeg's libraries, shipped as ordinary DLLs beside the executable,
; unmodified and individually replaceable. This is a licence requirement, not a
; packaging preference: LGPL-2.1 section 6 obliges us to leave the recipient
; able to substitute their own build of these, which is only true while they
; stay separate files loaded at runtime. They must never be folded into the
; executable or renamed.
Source: "{#StageDir}\*.dll";         DestDir: "{app}"; Flags: ignoreversion

[Icons]
; IconFilename is given explicitly rather than left to inherit from the
; executable. The exe now carries the icon as a Win32 resource, so inheriting
; would work — but a shortcut that names its icon keeps the right one even if
; the resource is ever stripped, and costs nothing.
Name: "{group}\{#AppName}";        Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\rustdvr.ico"
Name: "{autodesktop}\{#AppName}";  Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\rustdvr.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

; Settings are written beside the user's other application data rather than
; into the program directory, so uninstalling leaves the machine as it was and
; there is nothing extra to sweep up here.
