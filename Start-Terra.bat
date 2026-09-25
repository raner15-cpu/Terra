@echo off
setlocal EnableExtensions
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "TEMP=%LOCALAPPDATA%\Temp"
set "TMP=%TEMP%"
if not exist "%TEMP%" mkdir "%TEMP%"

where node >nul 2>&1 || goto :need_node
where npm.cmd >nul 2>&1 || goto :need_node
where cargo >nul 2>&1 || goto :need_rust

if exist "frontend\node_modules\.bin\vite.cmd" goto :frontend_ready
echo Installing Terra frontend dependencies...
pushd "frontend"
call npm.cmd ci
set "STEP_RESULT=%ERRORLEVEL%"
popd
if not "%STEP_RESULT%"=="0" goto :failed
:frontend_ready

cargo tauri --version >nul 2>&1
if errorlevel 1 (
  echo Installing Tauri CLI. The first-time setup may take several minutes...
  cargo install tauri-cli --version "^2.0.0" --locked
  if errorlevel 1 goto :failed
)

echo Generating Terra application icon...
pushd "crates\jarvis-gui"
cargo tauri icon "..\..\resources\icons\terra-source.svg" --output "..\..\resources\icons"
set "STEP_RESULT=%ERRORLEVEL%"
popd
if not "%STEP_RESULT%"=="0" goto :failed

copy /Y "resources\icons\128x128.png" "frontend\public\media\128x128.png" >nul
if errorlevel 1 goto :failed

echo Building the voice-assistant runtime...
cargo build -p jarvis-app
if errorlevel 1 goto :failed

echo Starting Terra. Keep this window open while using the app.
pushd "crates\jarvis-gui"
cargo tauri dev
set "STEP_RESULT=%ERRORLEVEL%"
popd
echo.
echo Terra has stopped (exit code %STEP_RESULT%).
pause
exit /b %STEP_RESULT%

:need_node
echo Node.js LTS and npm.cmd are required. Install Node.js, then reopen Windows and retry.
goto :failed

:need_rust
echo Rust stable with the MSVC toolchain is required. Install Rust and Visual Studio C++ Build Tools, then retry.
goto :failed

:failed
echo.
echo Setup or launch failed. Review the error above. Downloaded dependencies remain cached.
pause
exit /b 1
