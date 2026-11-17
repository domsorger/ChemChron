@echo off
rem Builds ChemChron on Windows into the folder ChemChron-Windows and puts a
rem shortcut with the ChemChron icon on the desktop. Double-click this file,
rem or run it from the ChemChron_program folder:  windows\make_windows.bat
rem Needs Rust (https://rustup.rs, rustup-init.exe) and the Visual Studio
rem "Build Tools" with "Desktop development with C++" (rustup offers them).
cd /d "%~dp0\.."
set "CARGO_TARGET_DIR=%CD%\target"
cargo build --release || (echo Build failed & pause & exit /b 1)
if not exist ChemChron-Windows mkdir ChemChron-Windows
copy /y target\release\chemchron.exe ChemChron-Windows\ChemChron.exe >nul
if exist assets\icon.png (copy /y assets\icon.png ChemChron-Windows\icon.png >nul)
ChemChron-Windows\ChemChron.exe --write-icon ChemChron-Windows\ChemChron.ico
powershell -NoProfile -Command "$s=(New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop')+'\ChemChron.lnk'); $s.TargetPath='%CD%\ChemChron-Windows\ChemChron.exe'; $s.IconLocation='%CD%\ChemChron-Windows\ChemChron.ico'; $s.WorkingDirectory='%CD%\ChemChron-Windows'; $s.Save()"
echo Done: ChemChron-Windows\ChemChron.exe, and a ChemChron shortcut on the desktop.
echo The ChemChron-Windows folder can be copied to other Windows computers.
pause
