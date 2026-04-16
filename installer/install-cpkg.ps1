$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$cpkgExeUrl = "https://github.com/callenflynn/cpkg/releases/latest/download/cpkg.exe"
$installerExeUrl = "https://github.com/callenflynn/cpkg/releases/latest/download/installer.exe"
$thirdPartyUrl = "https://raw.githubusercontent.com/callenflynn/cpkg/refs/heads/main/THIRD_PARTY_LICENSES.txt"
$noticeUrl = "https://raw.githubusercontent.com/callenflynn/cpkg/refs/heads/main/NOTICE"
$licenseUrl = "https://raw.githubusercontent.com/callenflynn/cpkg/refs/heads/main/LICENSE"

function Test-CpkgAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Start-CpkgElevatedCopy {
    $powershellPath = Join-Path $PSHOME "powershell.exe"
    $arguments = @(
        "-NoProfile"
        "-ExecutionPolicy Bypass"
        "-STA"
        "-WindowStyle Hidden"
        "-File `"$PSCommandPath`""
    ) -join " "

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershellPath
    $startInfo.Arguments = $arguments
    $startInfo.UseShellExecute = $true
    $startInfo.Verb = "runas"

    try {
        [System.Diagnostics.Process]::Start($startInfo) | Out-Null
        return $true
    } catch [System.ComponentModel.Win32Exception] {
        return $false
    }
}

if (-not (Test-CpkgAdministrator)) {
    [void](Start-CpkgElevatedCopy)
    exit 0
}

$script:InstallDir = Join-Path $env:ProgramFiles "cpkg"
$script:CpkgExePath = Join-Path $script:InstallDir "cpkg.exe"

function Get-CpkgDownloadSet {
    return @(
        @{ Url = $cpkgExeUrl; Name = "cpkg.exe" },
        @{ Url = $installerExeUrl; Name = "installer.exe" },
        @{ Url = $thirdPartyUrl; Name = "THIRD_PARTY_LICENSES.txt" },
        @{ Url = $noticeUrl; Name = "NOTICE" },
        @{ Url = $licenseUrl; Name = "LICENSE" }
    )
}

function Send-CpkgStatus {
    param(
        [System.ComponentModel.BackgroundWorker]$Worker,
        [string]$Message
    )

    if ($Worker) {
        $Worker.ReportProgress(0, $Message)
    }
}

function Install-CpkgFiles {
    param([System.ComponentModel.BackgroundWorker]$Worker)

    Send-CpkgStatus $Worker "Preparing install folder..."
    New-Item -Path $script:InstallDir -ItemType Directory -Force | Out-Null

    foreach ($file in Get-CpkgDownloadSet) {
        $target = Join-Path $script:InstallDir $file.Name
        Send-CpkgStatus $Worker "Downloading $($file.Name)..."
        Invoke-WebRequest -Uri $file.Url -OutFile $target
    }

    Send-CpkgStatus $Worker "Finishing installation..."
}

function Uninstall-CpkgFiles {
    param([System.ComponentModel.BackgroundWorker]$Worker)

    if (-not (Test-Path $script:InstallDir)) {
        Send-CpkgStatus $Worker "cpkg is not installed."
        return
    }

    Send-CpkgStatus $Worker "Removing cpkg..."
    Remove-Item -Path $script:InstallDir -Recurse -Force
}

function Show-CpkgInstaller {
    [System.Windows.Forms.Application]::EnableVisualStyles()

    $isInstalled = Test-Path $script:CpkgExePath

    $form = [System.Windows.Forms.Form]::new()
    $form.Text = "cpkg Installer"
    $form.StartPosition = "CenterScreen"
    $form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedDialog
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.ClientSize = [System.Drawing.Size]::new(520, 250)
    $form.Font = [System.Drawing.Font]::new("Segoe UI", 9)

    $titleLabel = [System.Windows.Forms.Label]::new()
    $titleLabel.Text = "cpkg Installer"
    $titleLabel.Font = [System.Drawing.Font]::new("Segoe UI", 18, [System.Drawing.FontStyle]::Bold)
    $titleLabel.AutoSize = $true
    $titleLabel.Location = [System.Drawing.Point]::new(24, 20)

    $descriptionLabel = [System.Windows.Forms.Label]::new()
    $descriptionLabel.Text = "Install cpkg to Program Files. Windows will prompt for administrator permission if needed."
    $descriptionLabel.AutoSize = $false
    $descriptionLabel.Size = [System.Drawing.Size]::new(470, 42)
    $descriptionLabel.Location = [System.Drawing.Point]::new(24, 60)

    $statusLabel = [System.Windows.Forms.Label]::new()
    $statusLabel.AutoSize = $false
    $statusLabel.Size = [System.Drawing.Size]::new(470, 36)
    $statusLabel.Location = [System.Drawing.Point]::new(24, 188)
    $statusLabel.Text = if ($isInstalled) { "cpkg is already installed." } else { "Ready to install cpkg." }

    $progressBar = [System.Windows.Forms.ProgressBar]::new()
    $progressBar.Location = [System.Drawing.Point]::new(24, 160)
    $progressBar.Size = [System.Drawing.Size]::new(470, 18)
    $progressBar.Style = [System.Windows.Forms.ProgressBarStyle]::Blocks
    $progressBar.Value = 0

    $installButton = [System.Windows.Forms.Button]::new()
    $installButton.Text = if ($isInstalled) { "Repair / Update" } else { "Install cpkg" }
    $installButton.Size = [System.Drawing.Size]::new(140, 34)
    $installButton.Location = [System.Drawing.Point]::new(24, 112)

    $uninstallButton = [System.Windows.Forms.Button]::new()
    $uninstallButton.Text = "Uninstall"
    $uninstallButton.Size = [System.Drawing.Size]::new(110, 34)
    $uninstallButton.Location = [System.Drawing.Point]::new(172, 112)
    $uninstallButton.Enabled = $isInstalled

    $closeButton = [System.Windows.Forms.Button]::new()
    $closeButton.Text = "Close"
    $closeButton.Size = [System.Drawing.Size]::new(110, 34)
    $closeButton.Location = [System.Drawing.Point]::new(384, 112)

    $script:IsBusy = $false

    $setBusy = {
        param([bool]$Busy)
        $script:IsBusy = $Busy
        $installButton.Enabled = -not $Busy
        $uninstallButton.Enabled = (-not $Busy) -and (Test-Path $script:CpkgExePath)
        $closeButton.Enabled = -not $Busy
        $progressBar.Style = if ($Busy) { [System.Windows.Forms.ProgressBarStyle]::Marquee } else { [System.Windows.Forms.ProgressBarStyle]::Blocks }
        $progressBar.MarqueeAnimationSpeed = if ($Busy) { 30 } else { 0 }
        if (-not $Busy) {
            $progressBar.Value = 0
        }
    }

    $worker = [System.ComponentModel.BackgroundWorker]::new()
    $worker.WorkerReportsProgress = $true

    $worker.add_DoWork({
        param($sender, $e)
        switch ($e.Argument) {
            "install" {
                Install-CpkgFiles -Worker $sender
                $e.Result = "install"
            }
            "uninstall" {
                Uninstall-CpkgFiles -Worker $sender
                $e.Result = "uninstall"
            }
        }
    })

    $worker.add_ProgressChanged({
        param($sender, $e)
        $statusLabel.Text = [string]$e.UserState
    })

    $worker.add_RunWorkerCompleted({
        param($sender, $e)
        & $setBusy $false

        if ($e.Error) {
            $statusLabel.Text = "Installer failed."
            [System.Windows.Forms.MessageBox]::Show(
                "Installer failed:`n$($e.Error.Exception.Message)",
                "cpkg Installer",
                [System.Windows.Forms.MessageBoxButtons]::OK,
                [System.Windows.Forms.MessageBoxIcon]::Error
            ) | Out-Null
            return
        }

        switch ($e.Result) {
            "install" {
                $statusLabel.Text = "cpkg installed"
                $uninstallButton.Enabled = $true
                $installButton.Text = "Repair / Update"
                [System.Windows.Forms.MessageBox]::Show(
                    "cpkg installed.",
                    "cpkg Installer",
                    [System.Windows.Forms.MessageBoxButtons]::OK,
                    [System.Windows.Forms.MessageBoxIcon]::Information
                ) | Out-Null
            }
            "uninstall" {
                $statusLabel.Text = "cpkg removed."
                $uninstallButton.Enabled = $false
                $installButton.Text = "Install cpkg"
                [System.Windows.Forms.MessageBox]::Show(
                    "cpkg removed.",
                    "cpkg Installer",
                    [System.Windows.Forms.MessageBoxButtons]::OK,
                    [System.Windows.Forms.MessageBoxIcon]::Information
                ) | Out-Null
            }
        }
    })

    $startAction = {
        param([string]$Action)

        if ($script:IsBusy) {
            return
        }

        & $setBusy $true

        switch ($Action) {
            "install" {
                $statusLabel.Text = "Installing cpkg..."
            }
            "uninstall" {
                $statusLabel.Text = "Removing cpkg..."
            }
        }

        $worker.RunWorkerAsync($Action)
    }

    $installButton.Add_Click({ & $startAction "install" })
    $uninstallButton.Add_Click({ & $startAction "uninstall" })
    $closeButton.Add_Click({ if (-not $script:IsBusy) { $form.Close() } })
    $form.Add_FormClosing({ param($sender, $e) if ($script:IsBusy) { $e.Cancel = $true } })

    $form.Controls.AddRange(@(
        $titleLabel,
        $descriptionLabel,
        $installButton,
        $uninstallButton,
        $closeButton,
        $progressBar,
        $statusLabel
    ))

    $form.AcceptButton = $installButton
    $form.CancelButton = $closeButton

    & $setBusy $false
    [void]$form.ShowDialog()
}

Show-CpkgInstaller
exit 0
