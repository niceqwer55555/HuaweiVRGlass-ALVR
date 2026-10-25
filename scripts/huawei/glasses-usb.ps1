#requires -Version 5.1
<#
    glasses-usb.ps1 - bring up the USB control link of the Huawei VR Glass
    so the official wired driver works without HwPCVRAssistant.exe.

    The glasses expose TWO things over their single USB-C cable:
      * a DisplayPort sink  -> shows up as monitor  DISPLAY\RTK2560
      * a USB composite dev -> VID_12D1&PID_107E (usbccgp), carrying the
        IMU / key / control endpoints the SteamVR driver reads with HID API.
    If only the monitor is present, the DP lanes are wired but the USB data
    pair is not: the driver will never leave "not ready".

    Modes:
      -Status   report only (default)
      -Install  also install the Huawei USB driver INF if the device showed up
      -Wait     poll until the USB device appears (seconds, default 120)

    ASCII only: PowerShell 5.1 parses BOM-less UTF-8 as ANSI.
#>
param(
    [switch]$Status,
    [switch]$Install,
    [switch]$Wait,
    [int]$Timeout = 120
)

$ErrorActionPreference = 'Continue'

$AssistantRoot = 'D:\Program Files (x86)\HwPCVR Assistant'
$InfDir = Join-Path $AssistantRoot 'pcCastScreen\DriverSetUp\IMU\Driver\X64'

$HuaweiVid = '12D1'
$GlassesPids = @('107E', '107e', '107D', '107F')
$MonitorEdid = 'RTK2560'

function Say-Ok  { param($m) Write-Host "  [OK]   $m" -ForegroundColor Green }
function Say-Bad { param($m) Write-Host "  [FAIL] $m" -ForegroundColor Red }
function Say-Inf { param($m) Write-Host "         $m" -ForegroundColor DarkGray }

function Get-GlassUsbDevice {
    Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue |
        Where-Object { $_.DeviceID -match "VID_$HuaweiVid" }
}

function Get-GlassMonitor {
    Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue |
        Where-Object { $_.DeviceID -match "^DISPLAY\\" -and $_.DeviceID -match $MonitorEdid }
}

Write-Host ''
Write-Host '=== Huawei VR Glass - USB control link ===' -ForegroundColor Cyan

# --- 1. DP side ------------------------------------------------------------
Write-Host ''
Write-Host '[1] DisplayPort side (video)'
$mon = Get-GlassMonitor
if ($mon) {
    Say-Ok "glasses present as monitor: $($mon[0].DeviceID)"
} else {
    Say-Bad 'monitor RTK2560 not found - glasses not plugged in / no DP link'
}

# --- 2. USB side -----------------------------------------------------------
Write-Host ''
Write-Host '[2] USB data side (control + IMU)'
$devs = @(Get-GlassUsbDevice)

if ($Wait -and $devs.Count -eq 0) {
    Write-Host "     waiting up to ${Timeout}s for VID_$HuaweiVid ..." -ForegroundColor Yellow
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($devs.Count -eq 0 -and $sw.Elapsed.TotalSeconds -lt $Timeout) {
        Start-Sleep -Seconds 3
        $devs = @(Get-GlassUsbDevice)
    }
    $sw.Stop()
}

if ($devs.Count -eq 0) {
    Say-Bad "no USB device with VID_$HuaweiVid - data lanes are NOT connected"
    Say-Inf 'The official cable splits into DP + USB-A. The USB-A tail carries'
    Say-Inf 'IMU/control data; without it only video works. Plug it in, or use'
    Say-Inf 'a full-featured (data-capable) USB-C cable, then re-run with -Wait.'
} else {
    foreach ($d in $devs) {
        $code = $d.ConfigManagerErrorCode
        if ($code -eq 0) { Say-Ok "$($d.Name)" } else { Say-Bad "$($d.Name) (error $code)" }
        Say-Inf $d.DeviceID
        if ($d.Service) { Say-Inf "service: $($d.Service)" }
    }
}

# --- 3. driver packages ----------------------------------------------------
Write-Host ''
Write-Host '[3] Huawei USB driver'
$infOk = Test-Path -LiteralPath $InfDir
if (-not $infOk) {
    Say-Bad "driver INF dir not found: $InfDir"
} else {
    Say-Ok "INF dir present"
    foreach ($f in @('ew_usbccgpfilter.inf', 'hw_cdcacm.inf', 'cdc-acm.inf')) {
        $p = Join-Path $InfDir $f
        if (Test-Path -LiteralPath $p) { Say-Inf "found   $f" }
    }
    $installed = & pnputil.exe /enum-drivers 2>$null |
        Select-String -Pattern 'hw_cdcacm|usbccgpfilter|cdc-acm' -SimpleMatch
    if ($installed) {
        Say-Ok 'driver already staged in the driver store'
    } else {
        Say-Inf 'not staged yet (run with -Install once the device is visible)'
    }
}

# --- 4. install ------------------------------------------------------------
if ($Install) {
    Write-Host ''
    Write-Host '[4] Installing driver' -ForegroundColor Cyan
    if ($devs.Count -eq 0) {
        Say-Bad 'nothing to install - device not present'
    } elseif (-not $infOk) {
        Say-Bad 'INF dir missing'
    } else {
        $id = New-Object System.Security.Principal.WindowsIdentity([System.Security.Principal.WindowsIdentity]::GetCurrent().Name)
        $pr = New-Object System.Security.Principal.WindowsPrincipal($id)
        if (-not $pr.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
            Say-Bad 'driver installation needs an elevated PowerShell'
        } else {
            foreach ($f in @('ew_usbccgpfilter.inf', 'hw_cdcacm.inf', 'cdc-acm.inf')) {
                $p = Join-Path $InfDir $f
                if (-not (Test-Path -LiteralPath $p)) { continue }
                Write-Host "     pnputil /add-driver $f"
                & pnputil.exe /add-driver $p /install 2>&1 | ForEach-Object { Say-Inf $_ }
            }
            Start-Sleep -Seconds 3
            $after = @(Get-GlassUsbDevice)
            Write-Host ''
            Write-Host "     USB devices now: $($after.Count)"
            foreach ($d in $after) {
                if ($d.ConfigManagerErrorCode -eq 0) { Say-Ok $d.Name } else { Say-Bad "$($d.Name) ($($d.ConfigManagerErrorCode))" }
                Say-Inf $d.DeviceID
            }
        }
    }
}

# --- 5. verdict ------------------------------------------------------------
Write-Host ''
Write-Host '=== verdict ===' -ForegroundColor Cyan
if ($mon -and $devs.Count -gt 0) {
    $ready = $true
    foreach ($d in $devs) { if ($d.ConfigManagerErrorCode -ne 0) { $ready = $false } }
    if ($ready) {
        Say-Ok 'video + USB data both up - the official wired driver can work'
        Say-Inf 'start SteamVR now (or: scripts\huawei\check-wired.ps1 -StartAssistant)'
    } else {
        Say-Bad 'device present but driver failed - re-run as admin with -Install'
    }
} elseif ($mon) {
    Say-Bad 'video only. Plug the USB-A tail of the official cable.'
} else {
    Say-Bad 'glasses not detected at all.'
}
Write-Host ''
