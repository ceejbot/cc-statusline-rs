# Install cc-statusline-rs on Windows: download the release build for this
# architecture, verify it, drop it in ~\.claude\, and point Claude Code at it.
# Works in both Windows PowerShell 5.1 and pwsh:
#     irm https://raw.githubusercontent.com/ceejbot/cc-statusline-rs/main/scripts/install.ps1 | iex
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'  # makes Invoke-WebRequest fast on PS 5.1

$Repo = 'ceejbot/cc-statusline-rs'
$Binary = 'cc-statusline-rs'
$BaseUrl = if ($env:CC_STATUSLINE_BASE_URL) { $env:CC_STATUSLINE_BASE_URL }
           else { "https://github.com/$Repo/releases/latest/download" }

# OSArchitecture reports the machine, not the process; PROCESSOR_ARCHITECTURE
# lies when an x64 PowerShell runs emulated on an ARM machine.
$osArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ("$osArch") {
    'X64'   { $target = 'x86_64-pc-windows-msvc' }
    'Arm64' { $target = 'aarch64-pc-windows-msvc' }
    default { throw "unsupported architecture: $osArch" }
}
$zipName = "$Binary-$target.zip"

$workDir = Join-Path $env:TEMP "cc-statusline-install-$PID"
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
try {
    $zipPath = Join-Path $workDir $zipName
    Write-Host "downloading $zipName ..."
    Invoke-WebRequest -Uri "$BaseUrl/$zipName" -OutFile $zipPath
    Invoke-WebRequest -Uri "$BaseUrl/$zipName.sha256" -OutFile "$zipPath.sha256"

    $expected = (Get-Content "$zipPath.sha256" -Raw).Trim()
    $actual = (Get-FileHash -Algorithm SHA256 $zipPath).Hash
    if ($actual -ne $expected) {
        throw "sha256 mismatch for ${zipName}: expected $expected, got $actual"
    }

    Expand-Archive -Force -Path $zipPath -DestinationPath $workDir

    $claudeDir = Join-Path $env:USERPROFILE '.claude'
    New-Item -ItemType Directory -Force -Path $claudeDir | Out-Null
    $dest = Join-Path $claudeDir "$Binary.exe"

    # Windows locks a running executable against overwrite but allows renaming
    # it, so move a live statusline aside before copying the new one in.
    if (Test-Path $dest) {
        Move-Item -Force $dest "$dest.old"
    }
    Copy-Item (Join-Path $workDir "$Binary.exe") $dest
    Remove-Item "$dest.old" -ErrorAction SilentlyContinue

    & $dest setup
    if ($LASTEXITCODE -ne 0) { throw "$Binary setup failed with exit code $LASTEXITCODE" }
    Write-Host 'done. Claude Code picks up the statusline on its next session.'
}
finally {
    Remove-Item -Recurse -Force $workDir -ErrorAction SilentlyContinue
}
