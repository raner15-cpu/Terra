# Installs whisper.cpp (Windows x64, CPU build) and a multilingual Whisper model
# into a PERSISTENT folder outside the project, so updating or re-downloading the
# project never deletes the model.
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

# whisper.cpp publishes Windows binaries on rolling build tags (b5130, ...), NOT on
# the tagged stable releases - those have no assets at all. So the archive has to be
# picked from the newest release that actually carries a Windows x64 CPU build.
$apiUrl = "https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=30"
$pinnedUrl = "https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-x64.zip"
$preferredNames = @("whisper-bin-x64.zip", "whisper-blas-bin-x64.zip")

New-Item -ItemType Directory -Force -Path $Target | Out-Null
Write-Host "Whisper folder: $Target"

function Get-InstalledExe {
    Get-ChildItem -Path $Target -Filter "*.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -in @("whisper-cli.exe", "main.exe") } |
        Select-Object -First 1
}

function Install-Archive {
    param([string]$Url, [string]$Name)

    Write-Host "  trying $Name"
    $zip = Join-Path $env:TEMP $Name
    $unpacked = Join-Path $env:TEMP "terra-whisper-bin"

    try {
        Invoke-WebRequest -Uri $Url -OutFile $zip
        if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked }
        Expand-Archive -Path $zip -DestinationPath $unpacked

        $cli = Get-ChildItem -Path $unpacked -Recurse -Include "whisper-cli.exe", "main.exe" -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if (-not $cli) {
            Write-Host "  archive has no whisper-cli.exe, skipping"
            return $false
        }

        # flatten: Terra expects the binary and its DLLs directly in the target folder
        Get-ChildItem -Path $unpacked -Recurse -Include *.exe, *.dll | ForEach-Object {
            Copy-Item $_.FullName -Destination $Target -Force
        }
        return $true
    } catch {
        Write-Host "  failed: $($_.Exception.Message)"
        return $false
    } finally {
        if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked -ErrorAction SilentlyContinue }
        if (Test-Path $zip) { Remove-Item -Force $zip -ErrorAction SilentlyContinue }
    }
}

# ### 1. model

$modelPath = Join-Path $Target $Model
if ((Test-Path $modelPath) -and ((Get-Item $modelPath).Length -gt 100MB)) {
    Write-Host "Model already installed: $Model"
} else {
    Write-Host "Downloading $Model (about 500 MB, one time only)..."
    $partPath = "$modelPath.part"
    Invoke-WebRequest -Uri "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$Model" -OutFile $partPath
    Move-Item -Force $partPath $modelPath
}

# ### 2. binary

if (Get-InstalledExe) {
    Write-Host "whisper.cpp already installed: $((Get-InstalledExe).Name)"
    Write-Host "Whisper is ready. Terra loads it when you say: Terra -> Da -> Razgovor"
    exit 0
}

Write-Host "Looking for whisper.cpp Windows x64 binaries..."

$candidates = @()
try {
    $releases = Invoke-RestMethod -Uri $apiUrl -Headers @{ "User-Agent" = "terra-setup" }
    $assets = $releases | ForEach-Object { $_.assets } | Where-Object { $_ }

    # exact CPU/BLAS builds first, newest release first
    foreach ($wanted in $preferredNames) {
        $candidates += $assets | Where-Object { $_.name -eq $wanted }
    }

    # then anything that looks like a Windows x64 build, without GPU runtimes
    $candidates += $assets | Where-Object {
        $_.name -match '^whisper.*x64\.zip$' -and
        $_.name -notmatch 'cublas|cuda|opencl|vulkan|sycl|hip|arm64|Win32'
    }
} catch {
    Write-Host "GitHub API unavailable ($($_.Exception.Message)). Using the pinned build."
}

$installed = $false
foreach ($asset in ($candidates | Select-Object -First 4)) {
    if (Install-Archive -Url $asset.browser_download_url -Name $asset.name) {
        $installed = $true
        break
    }
}

if (-not $installed) {
    Write-Host "Falling back to the pinned whisper.cpp build."
    $installed = Install-Archive -Url $pinnedUrl -Name "whisper-bin-x64.zip"
}

if (-not $installed) {
    Write-Host ""
    Write-Host "Could not install whisper.cpp automatically."
    Write-Host "Download a Windows x64 build manually and unpack whisper-cli.exe plus its DLLs into:"
    Write-Host "  $Target"
    Write-Host "Builds: https://github.com/ggml-org/whisper.cpp/releases (look for whisper-bin-x64.zip)"
    exit 1
}

Write-Host "Whisper is ready. Terra loads it when you say: Terra -> Da -> Razgovor"
exit 0
