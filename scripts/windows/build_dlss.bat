@echo off
setlocal
rem Builds the game with NVIDIA DLSS (+ ray tracing). Needs, from NVIDIA / LunarG (not shipped with this project):
rem   1. DLSS SDK v310.5.3:  git clone --branch v310.5.3 https://github.com/NVIDIA/DLSS   and set DLSS_SDK to that folder
rem   2. Vulkan SDK (sets VULKAN_SDK)      3. LLVM / clang (for bindgen), on PATH or LIBCLANG_PATH set
rem See docs\REMASTER.md.  Usage: scripts\windows\build_dlss.bat
cd /d "%~dp0\..\.."
if "%DLSS_SDK%"=="" (echo Set DLSS_SDK to the NVIDIA DLSS SDK v310.5.3 folder first. & exit /b 1)
if "%VULKAN_SDK%"=="" (echo Install the Vulkan SDK first (it sets VULKAN_SDK^). & exit /b 1)
rustup update stable >nul 2>&1
cargo build --release -p kk-fps --features raytracing,dlss
if errorlevel 1 (echo BUILD FAILED & exit /b 1)
cargo build --release -p kk-launcher
if errorlevel 1 (echo BUILD FAILED & exit /b 1)
if not exist dist\bin mkdir dist\bin
copy /y target\release\KingKongRecompiled.exe dist\ >nul || goto fail
copy /y target\release\kk-fps.exe dist\bin\ >nul || goto fail
rem DLSS runtime DLLs + licence text must sit next to kk-fps.exe (DLSS SDK licence)
copy /y "%DLSS_SDK%\lib\Windows_x86_64\rel\nvngx_dlss.dll" dist\bin\ >nul || goto fail
if exist "%DLSS_SDK%\lib\Windows_x86_64\rel\nvngx_dlssd.dll" copy /y "%DLSS_SDK%\lib\Windows_x86_64\rel\nvngx_dlssd.dll" dist\bin\ >nul
copy /y "%DLSS_SDK%\LICENSE.txt" dist\bin\DLSS_LICENSE.txt >nul
if not exist dist\bin\mods mkdir dist\bin\mods
for %%F in (Play_Jack_03E.bat Play_Kong_Marsh_05C.bat Play_TestArea.bat Watch_Kong_Fight_05C.bat) do copy /y scripts\windows\%%F dist\ >nul
echo OK: DLSS build in dist\. Pick DLSS in the F10 menu (Upscaling).
exit /b 0
:fail
echo COPY FAILED
exit /b 1
