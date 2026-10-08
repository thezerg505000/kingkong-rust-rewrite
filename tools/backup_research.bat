@echo off
rem Zips the important research (no decompiled kb, no game data) to KongPS2\backups\research_<date>.zip
setlocal
set ROOT=Z:\ClaudeCode\KongPS2
for /f %%d in ('powershell -nologo -command "Get-Date -Format yyyy-MM-dd"') do set D=%%d
if not exist "%ROOT%\backups" mkdir "%ROOT%\backups"
powershell -nologo -noprofile -command ^
 "$r='%ROOT%'; $o=\"$r\backups\research_%D%.zip\"; Remove-Item $o -ea 0;" ^
 "$inc=@('research\pc\*.md','research\pc\code\*.json','research\pc\code\*.py','research\pc\code\ova','research\pc\atmos','research\pc\fx\*.md','research\pc\reviews','research\pc\tools','research\pc\keys\tools','kingkong-ps2-rs\docs','kingkong-ps2-rs\spec','kingkong-ps2-rs\tools','kingkong-ps2-rs\crates');" ^
 "$ex='\\(kb|textures|meshes|game_assets|batches|target|vendor|__pycache__)\\|\.dec$|\.raw$';" ^
 "$files=foreach($i in $inc){Get-ChildItem -Path (Join-Path $r $i) -Recurse -File -ea 0};" ^
 "$files=$files | Where-Object {$_.FullName -notmatch $ex} | Sort-Object FullName -Unique;" ^
 "Add-Type -A System.IO.Compression.FileSystem; $z=[IO.Compression.ZipFile]::Open($o,'Create');" ^
 "foreach($f in $files){[void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($z,$f.FullName,$f.FullName.Substring($r.Length+1))}; $z.Dispose(); Write-Host wrote $o"
