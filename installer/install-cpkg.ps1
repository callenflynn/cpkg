$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$cpkgExeUrl = "https://github.com/callenflynn/cpkg/releases/latest/download/cpkg.exe"
$installerExeUrl = "https://github.com/callenflynn/cpkg/releases/latest/download/installer.exe"
$thirdPartyUrl = "https://raw.githubusercontent.com/callenflynn/cpkg/refs/heads/main/THIRD_PARTY_LICENSES.txt"
$noticeUrl = "https://raw.githubusercontent.com/callenflynn/cpkg/refs/heads/main/NOTICE"
$licenseUrl = "https://raw.githubusercontent.com/callenflynn/cpkg/refs/heads/main/LICENSE"

$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator
)

if (-not $isAdmin) {
    [System.Windows.Forms.MessageBox]::Show(
        "Please run the installer as Administrator.",
        "cpkg Installer",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Error
    ) | Out-Null
    throw "Please run the installer as Administrator."
}

$installDir = Join-Path $env:ProgramFiles "cpkg"

function Get-DownloadSet {
    return @(
        @{ Url = $cpkgExeUrl; Name = "cpkg.exe" },
        @{ Url = $installerExeUrl; Name = "installer.exe" },
        @{ Url = $thirdPartyUrl; Name = "THIRD_PARTY_LICENSES.txt" },
        @{ Url = $noticeUrl; Name = "NOTICE" },
        @{ Url = $licenseUrl; Name = "LICENSE" }
    )
}

function Install-Or-Update {
    New-Item -Path $installDir -ItemType Directory -Force | Out-Null

    foreach ($file in Get-DownloadSet) {
        $target = Join-Path $installDir $file.Name
        Invoke-WebRequest -Uri $file.Url -OutFile $target
    }

    [System.Windows.Forms.MessageBox]::Show(
        "cpkg is ready in $installDir`nDownloaded: cpkg.exe, installer.exe, LICENSE, NOTICE, THIRD_PARTY_LICENSES.txt",
        "cpkg Installer",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Information
    ) | Out-Null
}

function Uninstall-Cpkg {
    if (Test-Path $installDir) {
        Remove-Item -Path $installDir -Recurse -Force
        [System.Windows.Forms.MessageBox]::Show(
            "cpkg has been removed from $installDir",
            "cpkg Installer",
            [System.Windows.Forms.MessageBoxButtons]::OK,
            [System.Windows.Forms.MessageBoxIcon]::Information
        ) | Out-Null
    } else {
        [System.Windows.Forms.MessageBox]::Show(
            "cpkg is not installed.",
            "cpkg Installer",
            [System.Windows.Forms.MessageBoxButtons]::OK,
            [System.Windows.Forms.MessageBoxIcon]::Information
        ) | Out-Null
    }
}

function Show-InstalledMenu {
    $result = [System.Windows.Forms.MessageBox]::Show(
        "cpkg is already installed.`n`nYes = Update`nNo = Uninstall`nCancel = Exit",
        "cpkg Installer",
        [System.Windows.Forms.MessageBoxButtons]::YesNoCancel,
        [System.Windows.Forms.MessageBoxIcon]::Question
    )

    switch ($result) {
        ([System.Windows.Forms.DialogResult]::Yes) { Install-Or-Update; break }
        ([System.Windows.Forms.DialogResult]::No) { Uninstall-Cpkg; break }
        default { break }
    }
}

function Show-InstallPrompt {
    $result = [System.Windows.Forms.MessageBox]::Show(
        "cpkg is not installed. Install now?",
        "cpkg Installer",
        [System.Windows.Forms.MessageBoxButtons]::OKCancel,
        [System.Windows.Forms.MessageBoxIcon]::Question
    )

    if ($result -eq [System.Windows.Forms.DialogResult]::OK) {
        Install-Or-Update
    }
}

if (Test-Path (Join-Path $installDir "cpkg.exe")) {
    Show-InstalledMenu
} else {
    Show-InstallPrompt
}
