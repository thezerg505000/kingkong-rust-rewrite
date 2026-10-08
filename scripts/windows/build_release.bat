@echo off
setlocal
rem Builds dist\KingKongRecompiled.exe (launcher) + dist\bin\kk-fps.exe (game) and copies the play shortcuts.
rem Usage: scripts\windows\build_release.bat [dev <folder with already rebuilt assets>]
cd /d "%~dp0\..\.."
cargo build --release -p kk-fps -p kk-launcher
if errorlevel 1 (echo BUILD FAILED & exit /b 1)
if not exist dist\bin mkdir dist\bin
copy /y target\release\KingKongRecompiled.exe dist\ >nul || goto fail
copy /y target\release\kk-fps.exe dist\bin\ >nul || goto fail
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
