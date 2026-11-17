; Windows installer for ChemChron (Inno Setup 6). Built by the GitHub workflow:
;   ISCC.exe /DAppVersion=1.2.0 packaging\windows\chemchron.iss
; It expects dist\ChemChron.exe and dist\ChemChron.ico next to it (made by the workflow).

#ifndef AppVersion
  #define AppVersion "1.2.0"
#endif

[Setup]
AppId={{6C2F2A3E-6B71-4E0B-9C1E-2A6E0C9B1A20}
AppName=ChemChron
AppVersion={#AppVersion}
AppPublisher=Dominik Sorger, Department of Earth Sciences, University of Graz
AppCopyright=Copyright 2026 Dominik Sorger, Department of Earth Sciences, University of Graz
DefaultDirName={autopf}\ChemChron
DefaultGroupName=ChemChron
; installs for the user without administrator rights; asks when started as administrator
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\..
OutputBaseFilename=ChemChron-{#AppVersion}-Windows-Setup
SetupIconFile=..\..\dist\ChemChron.ico
UninstallDisplayIcon={app}\ChemChron.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "desktopicon"; Description: "Create a desktop icon"; GroupDescription: "Additional icons:"

[Files]
Source: "..\..\dist\ChemChron.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\dist\ChemChron.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\ChemChron"; Filename: "{app}\ChemChron.exe"; IconFilename: "{app}\ChemChron.ico"
Name: "{group}\Uninstall ChemChron"; Filename: "{uninstallexe}"
Name: "{autodesktop}\ChemChron"; Filename: "{app}\ChemChron.exe"; IconFilename: "{app}\ChemChron.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\ChemChron.exe"; Description: "Start ChemChron"; Flags: nowait postinstall skipifsilent
