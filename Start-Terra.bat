@echo off
setlocal EnableExtensions
cd /d "%~dp0"
set "TERRA_NATIVE_LIB=%CD%\lib\windows\amd64"
set "PATH=%TERRA_NATIVE_LIB%;%USERPROFILE%\.cargo\bin;%PATH%"
set "TEMP=%LOCALAPPDATA%\Temp"
set "TMP=%TEMP%"
if not exist "%TEMP%" mkdir "%TEMP%"

rem Whisper lives outside the project so project updates never delete the model.
if not defined TERRA_WHISPER_DIR set "TERRA_WHISPER_DIR=%LOCALAPPDATA%\Terra\whisper"
if not defined TERRA_WHISPER_MODEL set "TERRA_WHISPER_MODEL=ggml-small.bin"
if not exist "%TERRA_WHISPER_DIR%" mkdir "%TERRA_WHISPER_DIR%" 2>nul

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

if "%TERRA_SKIP_WHISPER%"=="1" goto :whisper_ready
if exist "%TERRA_WHISPER_DIR%\%TERRA_WHISPER_MODEL%" if exist "%TERRA_WHISPER_DIR%\whisper-cli.exe" goto :whisper_ready
if exist "%TERRA_WHISPER_DIR%\%TERRA_WHISPER_MODEL%" if exist "%TERRA_WHISPER_DIR%\main.exe" goto :whisper_ready
echo Installing Whisper for conversation mode into "%TERRA_WHISPER_DIR%"...
echo This is a one-time download of about 500 MB and it is kept outside the project.
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\setup-whisper.ps1"
if errorlevel 1 (
  echo.
  echo Whisper setup failed. Terra will still start and use Vosk text in conversations.
  echo You can retry later, or set TERRA_SKIP_WHISPER=1 to skip this step.
  echo.
)
:whisper_ready

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

echo Preparing voice-assistant runtime libraries...
if not exist "target\debug" goto :failed
copy /Y "lib\windows\amd64\*.dll" "target\debug\" >nul
if errorlevel 1 goto :failed
if not exist "target\debug\libvosk.dll" goto :failed

echo Synchronizing Terra commands, voices and models...
if exist "target\debug\resources" rmdir /S /Q "target\debug\resources"
xcopy /E /I /Y "resources" "target\debug\resources" >nul
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
