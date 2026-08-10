; SPDX-License-Identifier: MIT
;
; Clicker - an unofficial, native Windows client for Channels DVR
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

[Setup]
; Clicker's own identity, and it must never change: this GUID is what Windows
; matches an upgrade against, and a new one turns every future version into a
; second entry in Add or Remove Programs beside the first.
AppId={{3F9A6C41-8E27-4B5D-A0F3-6D14C97B2E85}
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
