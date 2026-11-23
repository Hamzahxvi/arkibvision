@echo off
setlocal
cd /d "%~dp0"

set "PYTHON_EXE=%~dp0.venv\Scripts\python.exe"
if not exist "%PYTHON_EXE%" (
    echo [ERROR] Virtual environment not found at "%PYTHON_EXE%".
    echo Create it first or install dependencies from pyproject.toml.
    pause
    exit /b 1
)

for %%I in ("%~dp0..\LibreChat") do set "LIBRECHAT_DIR=%%~fI"

echo Starting RojakFace MCP ^& LibreChat workspace...

REM 1. Ensure Ollama is running
powershell -NoProfile -Command "try { $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', 11434); $c.Close(); exit 0 } catch { exit 1 }"
if errorlevel 1 (
    where ollama >nul 2>nul
    if not errorlevel 1 (
        echo Starting Ollama server...
        powershell -NoProfile -Command "Start-Process -FilePath 'ollama' -ArgumentList 'serve' -WindowStyle Minimized"
        timeout /t 3 /nobreak >nul
    ) else (
        echo [WARNING] Ollama CLI not found on PATH.
    )
) else (
    echo Ollama is already running on port 11434.
)

REM 2. Start RojakFace MCP Service (port 8005)
powershell -NoProfile -Command "try { $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', 8005); $c.Close(); exit 0 } catch { exit 1 }"
if errorlevel 1 (
    echo Starting RojakFace MCP Service ^(port 8005^)...
    powershell -NoProfile -Command "Start-Process -FilePath '%~dp0.venv\Scripts\python.exe' -ArgumentList '\"%~dp0src\models\rojakface_mcp.py\"' -WorkingDirectory '%~dp0.' -WindowStyle Minimized"
) else (
    echo RojakFace MCP Service is already running on port 8005.
)

REM 3. Wait for RojakFace MCP (port 8005) before starting LibreChat
echo Waiting for RojakFace MCP ^(port 8005^)...
powershell -NoProfile -Command ^
  "for ($i = 0; $i -lt 30; $i++) {" ^
  "  try { $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', 8005); $c.Close(); break } catch { Start-Sleep -Seconds 1 }" ^
  "}"

REM 4. Ensure LibreChat (port 3080) is running
powershell -NoProfile -Command "try { $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', 3080); $c.Close(); exit 0 } catch { exit 1 }"
if errorlevel 1 (
    if exist "%LIBRECHAT_DIR%\docker-compose.yml" (
        echo Starting LibreChat ^(port 3080^)...
        pushd "%LIBRECHAT_DIR%"
        docker compose up -d
        popd
    ) else (
        echo [WARNING] LibreChat not running on port 3080 and "%LIBRECHAT_DIR%\docker-compose.yml" not found.
    )
) else (
    echo LibreChat is already running on port 3080.
)

REM 5. Wait for LibreChat (port 3080) and open in browser
echo Waiting for LibreChat to become ready...
powershell -NoProfile -Command ^
  "$ready = $false;" ^
  "for ($i = 0; $i -lt 60; $i++) {" ^
  "  try { $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', 3080); $c.Close(); $ready = $true; break } catch {}" ^
  "  Start-Sleep -Seconds 1" ^
  "}" ^
  "if ($ready) { Start-Process 'http://localhost:3080' } else { Write-Host '[ERROR] LibreChat did not respond on port 3080 within 60s.' }"

echo Done. Use stop.bat to close RojakFace MCP.
exit /b 0
