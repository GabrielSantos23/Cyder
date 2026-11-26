; Instalador do Cyder (Inno Setup 6). A Release do GitHub compila com:  iscc /DVersion=1.2.3 installer\cyder.iss
; Instala só para o usuário, sem pedir administrador, em %LOCALAPPDATA%\Programs\Cyder: a pasta fica gravável e a
; atualização do próprio app (crates/core/src/update.rs, que troca o cyder.exe no lugar) continua funcionando.

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
; Não mudar o AppId: é por ele que o Windows reconhece atualizações e a desinstalação.
AppId={{6F1B2C4E-8A3D-4F7B-9C21-5D8E0A7B3C19}
AppName=Cyder
AppVersion={#Version}
AppVerName=Cyder {#Version}
AppPublisher=Cyder
AppPublisherURL=https://github.com/GabrielSantos23/Cyder
AppSupportURL=https://github.com/GabrielSantos23/Cyder/issues
AppUpdatesURL=https://github.com/GabrielSantos23/Cyder/releases
DefaultDirName={localappdata}\Programs\Cyder
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\target\release
OutputBaseFilename=CyderSetup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\cyder.exe
UninstallDisplayName=Cyder
; O Cyder aberto (ou escondido na bandeja) é fechado antes de trocar o executável.
CloseApplications=force
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\cyder.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Cyder"; Filename: "{app}\cyder.exe"
Name: "{autodesktop}\Cyder"; Filename: "{app}\cyder.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\cyder.exe"; Description: "{cm:LaunchProgram,Cyder}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Tira o Cyder de "Abrir com o Windows" e devolve os links spotify: ao app de antes (main.rs, --uninstall).
Filename: "{app}\cyder.exe"; Parameters: "--uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "CyderUninstall"

[UninstallDelete]
; A cópia que a atualização deixa para trás (crates/core/src/update.rs).
Type: files; Name: "{app}\cyder.old.exe"
