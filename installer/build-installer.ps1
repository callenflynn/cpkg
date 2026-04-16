$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Join-Path $scriptRoot ".."
$distDir = Join-Path $scriptRoot "dist"
$releaseDir = Join-Path $repoRoot "target\release"
$installerExe = Join-Path $releaseDir "installer.exe"

Push-Location $repoRoot
try {
    & cargo build --release --bin installer
} finally {
    Pop-Location
}

if (-not (Test-Path $installerExe)) {
    throw "Installer binary was not created at $installerExe"
}

New-Item -Path $distDir -ItemType Directory -Force | Out-Null
Copy-Item -Path $installerExe -Destination (Join-Path $distDir "installer.exe") -Force

Write-Host "Created installer: $(Join-Path $distDir 'installer.exe')"