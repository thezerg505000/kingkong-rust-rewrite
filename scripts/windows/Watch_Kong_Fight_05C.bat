@echo off
rem Scripted Kong-vs-rex fight in the 05C marsh; screenshots + check report go to out_b10\.
cd /d "%~dp0"
if not exist out_b10 mkdir out_b10
KingKongRecompiled.exe --batch b10_swamp_fight --out out_b10 %*
echo Results in %~dp0out_b10
pause
