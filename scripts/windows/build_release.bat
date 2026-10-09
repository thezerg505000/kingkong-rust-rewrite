@echo off
setlocal
rem Builds dist\KingKongRecompiled.exe (launcher) + dist\bin\kk-fps.exe (game) and copies the play shortcuts.
rem Usage: scripts\windows\build_release.bat [dev <folder with already rebuilt assets>]
cd /d "%~dp0\..\.."
rem Bevy 0.19 needs Rust 1.95 or newer
rustup update stable >nul 2>&1
rem ray tracing (Bevy Solari) is compiled in; it only runs on GPUs with ray queries and when switched on in F10
cargo build --release -p kk-fps --features raytracing
if errorlevel 1 (echo BUILD FAILED & exit /b 1)
cargo build --release -p kk-launcher
if errorlevel 1 (echo BUILD FAILED & exit /b 1)
if not exist dist\bin mkdir dist\bin
copy /y target\release\KingKongRecompiled.exe dist\ >nul || goto fail
copy /y target\release\kk-fps.exe dist\bin\ >nul || goto fail
if not exist dist\bin\mods mkdir dist\bin\mods
copy /y docs\MODDING.md dist\bin\mods\README.md >nul
for %%F in (Play_Jack_03E.bat Play_Kong_Marsh_05C.bat Play_TestArea.bat Watch_Kong_Fight_05C.bat) do copy /y scripts\windows\%%F dist\ >nul
if /i "%1"=="dev" (
  if "%~2"=="" (echo dev needs an asset folder & exit /b 1)
  xcopy /e /i /y "%~2" dist\assets_dev >nul
  echo Copied developer assets to dist\assets_dev - never commit or share that folder.
)
echo OK: dist\KingKongRecompiled.exe is ready. First start: dist\KingKongRecompiled.exe --game "^<your King Kong install folder^>"
exit /b 0
:fail
echo COPY FAILED
exit /b 1
