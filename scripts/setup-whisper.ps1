# Installs whisper.cpp (Windows x64) and a multilingual Whisper model into a
# PERSISTENT folder outside the project, so updating/re-downloading the project
# never deletes the model.
#
# Called automatically by Start-Terra.bat. Manual use:
#   powershell -ExecutionPolicy Bypass -File scripts\setup-whisper.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\setup-whisper.ps1 -Target "D:\TerraModels\whisper"

param(
    [string]$Target = "",
    [string]$Model = ""
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

if (-not $Target) { $Target = $env:TERRA_WHISPER_DIR }
if (-not $Target) { $Target = Join-Path $env:LOCALAPPDATA "Terra\whisper" }
if (-not $Model)  { $Model  = $env:TERRA_WHISPER_MODEL }
if (-not $Model)  { $Model  = "ggml-small.bin" }

New-Item -ItemType Directory -Force -Path $Target | Out-Null
Write-Host "Whisper folder: $Target"

$modelPath = Join-Path $Target $Model
if (Test-Path $modelPath) {
    Write-Host "Model already installed: $Model"
} else {
    Write-Host "Downloading $Model (about 500 MB, one time only)..."
    Invoke-WebRequest -Uri "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$Model" -OutFile "$modelPath.part"
    Move-Item -Force "$modelPath.part" $modelPath
}

$exe = Get-ChildItem -Path $Target -Filter "*.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -in @("whisper-cli.exe", "main.exe") } |
    Select-Object -First 1

if ($exe) {
    Write-Host "whisper.cpp already installed: $($exe.Name)"
} else {
    Write-Host "Downloading whisper.cpp binaries..."
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/ggml-org/whisper.cpp/releases/latest" -Headers @{ "User-Agent" = "terra-setup" }
    $asset = $release.assets | Where-Object { $_.name -like "*bin-x64*.zip" } | Select-Object -First 1
    if (-not $asset) {
        throw "No Windows x64 archive in the latest whisper.cpp release. Put whisper-cli.exe into $Target manually."
    }

    $zip = Join-Path $env:TEMP $asset.name
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip
    $unpacked = Join-Path $env:TEMP "terra-whisper-bin"
    if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked }
    Expand-Archive -Path $zip -DestinationPath $unpacked

    # flatten: Terra expects the binary and its DLLs directly in the target folder
    Get-ChildItem -Path $unpacked -Recurse -Include *.exe, *.dll | ForEach-Object {
        Copy-Item $_.FullName -Destination $Target -Force
    }
    Remove-Item -Recurse -Force $unpacked
    Remove-Item -Force $zip
}

Write-Host "Whisper is ready. Terra loads it when you say: Terra -> Da -> Razgovor"
