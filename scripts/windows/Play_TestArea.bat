@echo off
rem Flat test area with the creatures and Kong.
cd /d "%~dp0"
KingKongRecompiled.exe --scene testarea %*
