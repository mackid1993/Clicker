; RustDVR / RustVCR installer.
;
; Packages whatever build.ps1 staged into dist. It deliberately does not know
; how that directory was assembled: the licence vetting of the media plugins
; happens during staging, so anything that reaches this point has already been
; checked.
;
; One script, two editions. Compiled plain it produces the RustDVR installer
; for Windows 11; compiled with ISCC /DWin10 it produces the RustVCR installer
; for Windows 10 — the same application built without Mica, named for the
; recording technology its operating system deserves. Separate AppIds, so the
; two register as distinct products and neither hijacks the other's uninstall
; entry.
;
; Per-user by default. A DVR client is a personal application and there is no
; reason to demand an administrator prompt to install one.

#ifdef Win10
  #define AppName      "RustVCR"
  #define AppExeName   "rustvcr.exe"
  #define AppIdValue   "{{0267B8DD-4FC2-4738-9E97-BF0D51FC1620}"
  #define StageDir     "..\dist\RustVCR"
#else
  #define AppName      "RustDVR"
  #define AppExeName   "rustdvr.exe"
  #define AppIdValue   "{{7B1E4C2A-9D3F-4A18-9C6E-5F2A7D8B0E41}"
  #define StageDir     "..\dist\RustDVR"
#endif
; Overridable from the command line: ISCC /DAppVersion=0.0.1
; build.ps1 passes whatever Cargo.toml says, so the installer, its filename and
; the executable's own version resource cannot drift apart.
#ifndef AppVersion
  #define AppVersion   "0.0.1"
#endif
#define AppPublisher   "David Brustein"

[Setup]
AppId={#AppIdValue}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=no
OutputBaseFilename={#AppName}-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

; Per-user, so no elevation prompt.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#ifdef Win10
; RustVCR is built for Windows 10. The floor is 1809: current enough for the
; dark-mode DWM attribute and a working D3D12, old enough to cover anything
; still receiving updates. There is deliberately no ceiling: an edition that
; needs nothing Windows 11 has also runs fine on it, and whoever prefers the
; opaque look — or wants to test this edition without a second machine — is
; allowed to have it.
MinVersion=10.0.17763
#else
; Windows 11 only: this edition draws its own Mica-backed chrome, and Mica
; does not exist before build 22000. Windows 10 machines get RustVCR instead.
MinVersion=10.0.22000
#endif

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
