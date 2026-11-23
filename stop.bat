@echo off
setlocal
cd /d "%~dp0"

echo Stopping RojakFace MCP service...

powershell -NoProfile -Command ^
  "$root = [Regex]::Escape((Resolve-Path '%~dp0').Path.TrimEnd('\'));" ^
  "$procs = Get-CimInstance Win32_Process | Where-Object {" ^
  "  $_.CommandLine -and ($_.CommandLine -match $root) -and ($_.CommandLine -match 'rojakface_mcp\.py')" ^
  "};" ^
  "if (-not $procs) { Write-Host 'No running project processes found.' }" ^
  "foreach ($p in $procs) {" ^
  "  Write-Host ('Stopping PID ' + $p.ProcessId + ': ' + $p.Name);" ^
  "  Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue" ^
  "}"

echo Project services stopped.
exit /b 0
