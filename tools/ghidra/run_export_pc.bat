@echo off
rem Rebuild the PC knowledge base on Windows. Needs a JDK 21 on PATH and the Ghidra project
rem Z:\ClaudeCode\KongPS2\GhidraFiles\KK8 (import KingKong8.exe once with the Ghidra GUI or
rem analyzeHeadless ... -import ... -processor x86:LE:32:default -cspec windows).
set GHIDRA=Z:\ClaudeCode\KongPS2\ghidra_12.1.4_PUBLIC
set PROJ=Z:\ClaudeCode\KongPS2\GhidraFiles
set RESEARCH=Z:\ClaudeCode\KongPS2\research\pc
call "%GHIDRA%\support\analyzeHeadless.bat" "%PROJ%" KK8 -process KingKong8.exe -noanalysis ^
  -scriptPath "%~dp0" -postScript KKExport.java "%RESEARCH%\code" "%RESEARCH%\kb" > "%RESEARCH%\kb_export.log" 2>&1
