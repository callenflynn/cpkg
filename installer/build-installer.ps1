$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$stageDir = Join-Path $scriptRoot "stage"
$distDir = Join-Path $scriptRoot "dist"
$sedPath = Join-Path $scriptRoot "cpkg-installer.sed"
$outputExe = Join-Path $distDir "installer.exe"

New-Item -Path $distDir -ItemType Directory -Force | Out-Null
if (Test-Path $stageDir) {
    Remove-Item -Path $stageDir -Recurse -Force
}
New-Item -Path $stageDir -ItemType Directory | Out-Null

Copy-Item -Path (Join-Path $scriptRoot "install-cpkg.ps1") -Destination (Join-Path $stageDir "install-cpkg.ps1") -Force
Copy-Item -Path (Join-Path $scriptRoot "run-installer.cmd") -Destination (Join-Path $stageDir "run-installer.cmd") -Force

$stageDirEscaped = $stageDir -replace "\\", "\\\\"
$outputExeEscaped = $outputExe -replace "\\", "\\\\"

$sedContent = @"
[Version]
Class=IEXPRESS
SEDVersion=3

[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=0
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=
DisplayLicense=
FinishMessage=CPKG installer finished.
TargetName=$outputExeEscaped
FriendlyName=CPKG Installer
AppLaunched=cmd /c run-installer.cmd
PostInstallCmd=<None>
AdminQuietInstCmd=cmd /c run-installer.cmd
UserQuietInstCmd=cmd /c run-installer.cmd
SourceFiles=SourceFiles

[Strings]
FILE0="run-installer.cmd"
FILE1="install-cpkg.ps1"

[SourceFiles]
SourceFiles0=$stageDirEscaped

[SourceFiles0]
%FILE0%=
%FILE1%=
"@

Set-Content -Path $sedPath -Value $sedContent -Encoding ASCII

$iexpress = Join-Path $env:WINDIR "System32\iexpress.exe"
if (-not (Test-Path $iexpress)) {
    throw "IExpress not found at $iexpress"
}

& $iexpress /N /Q $sedPath | Out-Null

if (-not (Test-Path $outputExe)) {
    throw "Installer was not created at $outputExe"
}

if (Test-Path $stageDir) {
    Remove-Item -Path $stageDir -Recurse -Force
}
if (Test-Path $sedPath) {
    Remove-Item -Path $sedPath -Force
}

Write-Host "Created installer: $outputExe"
