#requires -Version 5.1
<#
Install / remove the auto-VR watcher as a per-user Startup task. No admin needed.

  .\setup-auto-vr-watcher.ps1          # install (autostart + start now)
  .\setup-auto-vr-watcher.ps1 -Remove  # remove autostart
#>
[CmdletBinding()]
param([switch]$Remove)

$ErrorActionPreference = 'Stop'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$watcher = Join-Path $scriptDir 'auto-vr-watcher.ps1'
$startup = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup'
$launcher = Join-Path $startup 'AutoVrWatcher.vbs'

if ($Remove) {
    if (Test-Path -LiteralPath $launcher -PathType Leaf) {
        Remove-Item -LiteralPath $launcher -Force
        Write-Host "removed autostart: $launcher"
    } else {
        Write-Host "no autostart entry to remove"
    }
    Write-Host "To stop the running watcher: Stop-Process -Name powershell (the one running auto-vr-watcher.ps1)"
    return
}

if (-not (Test-Path -LiteralPath $watcher -PathType Leaf)) { throw "watcher not found: $watcher" }
if (-not (Test-Path -LiteralPath $startup -PathType Container)) {
    New-Item -ItemType Directory -Path $startup -Force | Out-Null
}

# VBScript launches PowerShell fully hidden so no console flashes on login.
$psExe = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$wArgs = "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$watcher`""
$vbs = @"
Set sh = CreateObject("WScript.Shell")
sh.Run """$psExe"" $wArgs", 0, False
"@
[IO.File]::WriteAllText($launcher, $vbs, (New-Object Text.UTF8Encoding($false)))
Write-Host "autostart installed: $launcher"
Write-Host "watcher script     : $watcher"

# Also start it now (hidden) so the user does not need to log out/in.
try {
    Start-Process -FilePath $psExe -ArgumentList $wArgs -WindowStyle Hidden
    Write-Host "watcher started now"
} catch {
    Write-Host "could not start watcher now; it will start on next login: $_" -ForegroundColor Yellow
}
