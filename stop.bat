@echo off
setlocal
cd /d "%~dp0"

echo Stopping RojakFace Pure-Rust MCP service...

powershell -NoProfile -Command ^
  "$procs = Get-Process -Name 'rojakface-server' -ErrorAction SilentlyContinue;" ^
  "if (-not $procs) { Write-Host 'No running rojakface-server processes found.' }" ^
  "foreach ($p in $procs) {" ^
  "  Write-Host ('Stopping PID ' + $p.Id + ': ' + $p.ProcessName);" ^
  "  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue" ^
  "}"

echo Project services stopped.
exit /b 0
