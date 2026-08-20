; SPDX-License-Identifier: MIT
;
; Clicker - an unofficial client for Channels DVR Server
; Copyright (c) 2026 David Brustein

; Clicker installer.
;
; Packages whatever build.ps1 staged into dist\Clicker. It deliberately does
; not know how that directory was assembled: the license vetting of the media
; plugins happens during staging, so anything that reaches this point has
; already been checked.
;
; Per-user by default. A DVR client is a personal application and there is no
; reason to demand an administrator prompt to install one.

#define AppName        "Clicker"
#define AppExeName     "clicker.exe"
#define StageDir       "..\dist\Clicker"
; Overridable from the command line: ISCC /DAppVersion=0.0.1
; build.ps1 passes whatever Cargo.toml says, so the installer, its filename and
; the executable's own version resource cannot drift apart.
#ifndef AppVersion
  #define AppVersion   "0.0.1"
#endif
#define AppPublisher   "David Brustein"

; Which processor the staged build is for. build.ps1 reads this out of the PE
; header of the executable it just compiled rather than being told, so it
; cannot disagree with what is in the [Files] section below.
#ifndef AppArch
  #define AppArch      "x64"
#endif

; The arm64 installer is the one that says so in its name. x64 keeps the plain
; filename it has always had: it is what almost everybody downloads, it is
; what the README and every existing link point at, and renaming it to make a
; matched pair would break all of that to no one's benefit.
;
; x64compatible rather than x64 is also deliberate and stays that way. It
; matches ARM64 machines too, so somebody on one who takes the obvious
; download gets a working emulated install instead of "this app can't run on
; your PC". The arm64 build is an upgrade for those users, not a rescue.
; Spelled out rather than "arm64 or else", because "or else" meant a typo in
; the value compiled a perfectly good x64 installer under whatever name was
; asked for. There are two architectures and anything that is not one of them
; is a mistake, so it stops here rather than shipping.
#if AppArch == "arm64"
  #define ArchSuffix   "-arm64"
  #define ArchAllowed  "arm64"
#elif AppArch == "x64"
  #define ArchSuffix   ""
  #define ArchAllowed  "x64compatible"
#else
  #error AppArch must be x64 or arm64
#endif

[Setup]
; Clicker's own identity, and it must never change: this GUID is what Windows
; matches an upgrade against, and a new one turns every future version into a
; second entry in Add or Remove Programs beside the first.
;
; Shared by both architectures on purpose. An ARM user who has been running
; the emulated x64 build and moves to the native one is upgrading Clicker, not
; installing a second copy of it, and both land in the same directory with the
; same settings behind them.
AppId={{3F9A6C41-8E27-4B5D-A0F3-6D14C97B2E85}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=no
OutputBaseFilename={#AppName}-Setup-{#AppVersion}{#ArchSuffix}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

; Per-user, so no elevation prompt.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed={#ArchAllowed}
ArchitecturesInstallIn64BitMode={#ArchAllowed}
; Windows 10 1809 and up, Windows 11 included. The floor is that release
; because it is the one that added the dark-mode window attribute, below which
; a light system border is drawn around an application that is entirely dark.
; Nothing above the floor is required: the interface draws its own backdrop
; rather than asking the compositor for one, which is what used to make this
; Windows 11 only.
MinVersion=10.0.17763

LicenseFile={#StageDir}\LICENSE.md
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExeName}

SetupIconFile=..\assets\clicker.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#StageDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE.md";    DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\NOTICE.md";     DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\clicker.ico";   DestDir: "{app}"; Flags: ignoreversion
; The LGPL text and third-party notices, next to the libraries they cover.
Source: "{#StageDir}\licenses\*";    DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs

; FFmpeg's libraries, shipped as ordinary DLLs beside the executable,
; unmodified and individually replaceable. This is a license requirement, not a
; packaging preference: LGPL-2.1 section 6 obliges us to leave the recipient
; able to substitute their own build of these, which is only true while they
; stay separate files loaded at runtime. They must never be folded into the
; executable or renamed.
Source: "{#StageDir}\*.dll";         DestDir: "{app}"; Flags: ignoreversion

; A software OpenGL, in a directory of its own, and only if the build staged
; one — see build.ps1, which makes it optional. The application asks every
; machine what OpenGL it has before it opens a window and loads this by full
; path only when the answer is that the machine cannot draw, or when somebody
; sets Draw with to Software. That is why it is here rather than beside the
; executable, where the loader would prefer it to the real driver on every
; machine that installs this.
;
; The directory always holds at least the note that says what belongs in it, so
; this never matches nothing; the flag is there in case a stage is ever
; assembled by hand.
Source: "{#StageDir}\mesa\*";        DestDir: "{app}\mesa"; Flags: ignoreversion skipifsourcedoesntexist

[UninstallDelete]
; Whatever was put here by hand on a machine that needed a software OpenGL and
; did not get one from the installer. Uninstalling removes what it installed;
; this is the rest of that directory, so nothing is left behind but the folder
; the user chose to fill.
Type: filesandordirs; Name: "{app}\mesa"

[Icons]
; IconFilename is given explicitly rather than left to inherit from the
; executable. The exe now carries the icon as a Win32 resource, so inheriting
; would work — but a shortcut that names its icon keeps the right one even if
; the resource is ever stripped, and costs nothing.
Name: "{group}\{#AppName}";        Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\clicker.ico"
Name: "{autodesktop}\{#AppName}";  Filename: "{app}\{#AppExeName}"; IconFilename: "{app}\clicker.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

; Settings are written beside the user's other application data rather than
; into the program directory, so uninstalling leaves the machine as it was and
; there is nothing extra to sweep up here.
