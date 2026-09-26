# Downloads whisper.cpp (Windows x64) and the multilingual Whisper Small model
# into resources/whisper. Terra uses them only in conversation mode.
#
#   powershell -ExecutionPolicy Bypass -File scripts\setup-whisper.ps1
#
# Optional: -Model ggml-small-q5_1.bin   (smaller / faster, slightly worse)

param(
    [string]$Model = "ggml-small.bin"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "resources\whisper"
New-Item -ItemType Directory -Force -Path $target | Out-Null

$modelPath = Join-Path $target $Model
if (Test-Path $modelPath) {
    Write-Host "Model already present: $modelPath"
} else {
    $modelUrl = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$Model"
    Write-Host "Downloading $Model (about 500 MB for small)..."
    Invoke-WebRequest -Uri $modelUrl -OutFile $modelPath
}

$exe = Get-ChildItem -Path $target -Filter "*.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -in @("whisper-cli.exe", "main.exe") } |
    Select-Object -First 1

if ($exe) {
    Write-Host "whisper.cpp binary already present: $($exe.FullName)"
} else {
    Write-Host "Downloading whisper.cpp Windows binaries..."
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/ggml-org/whisper.cpp/releases/latest" -Headers @{ "User-Agent" = "terra-setup" }
    $asset = $release.assets | Where-Object { $_.name -like "*bin-x64*.zip" } | Select-Object -First 1
    if (-not $asset) {
        throw "Could not find a Windows x64 archive in the latest whisper.cpp release. Download whisper-cli.exe manually into $target"
    }

    $zip = Join-Path $env:TEMP $asset.name
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip
    $unpacked = Join-Path $env:TEMP "terra-whisper-bin"
    if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked }
    Expand-Archive -Path $zip -DestinationPath $unpacked

    # flatten: Terra expects the binary and its DLLs directly in resources\whisper
    Get-ChildItem -Path $unpacked -Recurse -Include *.exe, *.dll | ForEach-Object {
        Copy-Item $_.FullName -Destination $target -Force
    }
    Remove-Item -Recurse -Force $unpacked
    Remove-Item -Force $zip
}

Write-Host ""
Write-Host "Whisper is ready in $target"
Write-Host "Terra loads it the first time you say: Terra -> Da -> Razgovor"
