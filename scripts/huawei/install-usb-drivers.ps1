#requires -Version 5.1
<#
Install the Windows device drivers for the Huawei VR Glass USB hardware:

  * CDC-ACM serial interfaces (VID_12D1 & SUBCLASS_13 & PROT_06/12/13)
    -> COM ports used by the glasses' IMU / control channel
  * VirtualDisplayDriver for the glasses' virtual display device

Uses the driver copies shipped next to this script in `hw-usb-driver/`
(copied from the official Huawei PC VR assistant). Requires Administrator:
pnputil refuses to add a driver package without an elevated token.

Run:  right-click -> Run as Administrator
      or:  powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList '-File','install-usb-drivers.ps1'"
#>
[CmdletBinding()]
param([switch]$Status)

$ErrorActionPreference = 'Continue'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Resolve driver root: script dir, or its parent if scripts/huawei layout.
$candidates = @(
    (Join-Path $scriptDir 'hw-usb-driver'),
    (Join-Path $scriptDir '..\..\hw-usb-driver'),
    'D:\PY\1\artifacts\HuaweiVRGlass-ALVR-Windows-v20.14.1\hw-usb-driver'
)
$driverRoot = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Container) } | Select-Object -First 1
if (-not $driverRoot) { throw "hw-usb-driver folder not found near $scriptDir" }
$driverRoot = (Resolve-Path -LiteralPath $driverRoot).Path

$cdcDir = Join-Path $driverRoot 'IMU\Driver\X64'
$displayDir = Join-Path $driverRoot 'driverX64'

Write-Host "driver root: $driverRoot" -ForegroundColor Cyan

function Show-Devices([string]$label) {
    Write-Host ""
    Write-Host "== $label ==" -ForegroundColor Cyan
    $bad = @(Get-PnpDevice -ErrorAction SilentlyContinue |
        Where-Object { $_.Status -eq 'Error' -and ($_.InstanceId -match 'VID_12D1' -or $_.InstanceId -match 'PRP0001|VLV0100') })
    if ($bad.Count -eq 0) {
        Write-Host "  all Huawei VR Glass devices OK (no Error state)" -ForegroundColor Green
    } else {
        $bad | Select-Object Status,FriendlyName,InstanceId | Format-Table -AutoSize -Wrap | Out-String -Width 180 | Write-Host
    }
}

if ($Status) { Show-Devices 'current device status'; return }

# Elevation check (pnputil needs admin to add a driver package)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "NOT ADMINISTRATOR. This script must run elevated (pnputil /add-driver needs admin)." -ForegroundColor Red
    Write-Host "Right-click the .ps1 -> Run with PowerShell (as Administrator)." -ForegroundColor Yellow
    Show-Devices 'current device status (not changed)'
    exit 1
}

$pnputil = Join-Path $env:WINDIR 'System32\pnputil.exe'
if (-not (Test-Path -LiteralPath $pnputil -PathType Leaf)) { throw "pnputil.exe not found" }

$infs = @()
foreach ($d in @($cdcDir, $displayDir)) {
    if (-not (Test-Path -LiteralPath $d -PathType Container)) { continue }
    Get-ChildItem -LiteralPath $d -Filter '*.inf' -ErrorAction SilentlyContinue | ForEach-Object { $infs += $_ }
}
Write-Host "INF packages: $($infs.Count)" -ForegroundColor Cyan

foreach ($inf in $infs) {
    Write-Host ""
    Write-Host "--- add $($inf.Name)" -ForegroundColor Cyan
    & $pnputil /add-driver $inf.FullName /install 2>&1 | ForEach-Object { Write-Host "  $_" }
}

# --- Configure the official OpenVR driver's own data path -------------------
# driver_huaweiVR.dll reads HKCU\SOFTWARE\HuaweiPCVRAssistant\Path and appends
# \steamVRDriver\huaweiVR\bin\win64 to find VR6DOF.dll / SettingsPara. Without
# it the driver loads but reports pNumDisplay=0 / HmdNotFound. We ship a copy
# of the driver at <artifacts>\huawei-driver; stage it into that layout here.
Write-Host ""
Write-Host "=== configure official driver data path ===" -ForegroundColor Cyan
$artifactDir = Split-Path -Parent $driverRoot
if (-not (Test-Path -LiteralPath (Join-Path $artifactDir 'huawei-driver\driver.vrdrivermanifest') -PathType Leaf)) {
    # script may live in scripts/huawei of the repo; find artifacts by manifest
    $artifactDir = 'D:\PY\1\artifacts\HuaweiVRGlass-ALVR-Windows-v20.14.1'
}
$hdrRoot = Join-Path $artifactDir 'hw-assistant'
$hdrExpected = Join-Path $hdrRoot 'steamVRDriver\huaweiVR'
if (Test-Path -LiteralPath (Join-Path $artifactDir 'huawei-driver') -PathType Container) {
    if (-not (Test-Path -LiteralPath (Join-Path $hdrExpected 'bin\win64\VR6DOF.dll') -PathType Leaf)) {
        New-Item -ItemType Directory -Path (Split-Path -Parent $hdrExpected) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $artifactDir 'huawei-driver') -Destination $hdrExpected -Recurse -Force -ErrorAction SilentlyContinue | Out-Null
    }
    $regKey = 'HKCU:\SOFTWARE\HuaweiPCVRAssistant'
    if (-not (Test-Path $regKey)) { New-Item -Path $regKey -Force | Out-Null }
    $steamvrPath = 'D:\Program Files\Steam\steamapps\common\SteamVR'
    foreach ($pair in @(
        @('Path', $hdrRoot),
        @('SteamVRInstalledPath', $steamvrPath),
        @('SteamVrFirstRunningStatus', 'TRUE'),
        @('XmlStatus', 'true'),
        @('GetXmlData', 'true'),
        @('DirectMode', 'true')
    )) {
        New-ItemProperty -Path $regKey -Name $pair[0] -Value $pair[1] -PropertyType String -Force | Out-Null
    }
    Write-Host "  registered driver Path = $hdrRoot" -ForegroundColor Green
} else {
    Write-Host "  huawei-driver not found under $artifactDir; skipped registry setup" -ForegroundColor Yellow
}

Write-Host ""
Show-Devices 'after install'
Write-Host ""
Write-Host "If a device still shows Error, unplug/replug the glasses USB-C once." -ForegroundColor Yellow
