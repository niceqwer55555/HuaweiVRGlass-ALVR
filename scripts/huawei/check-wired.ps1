#requires -Version 5.1
<#
Check-Wired.ps1 - health check for the official wired mode (glasses on PC).

Reports, in dependency order, everything that must be true before the HUAWEI
VR Glass can come up over the official wired path:

  1. glasses visible to Windows    (PnP device + monitor EDID over DP alt-mode)
  2. Huawei PC VR Assistant running (it owns the USB link to the glasses)
  3. SteamVR runtime registered, and which huaweiVR copy will actually load
  4. driver_huaweiVR / driver_alvr_server enable flags
  5. SteamVR client running (required for the library panel)

Read-only: nothing is modified.

Usage:
    .\check-wired.ps1
    .\check-wired.ps1 -StartAssistant   # also launch the Huawei assistant
#>

param([switch]$StartAssistant)

$ErrorActionPreference = 'Continue'

function Section([string]$t) { Write-Host ''; Write-Host "== $t ==" -ForegroundColor Cyan }
function Good([string]$t) { Write-Host "  [OK]   $t" -ForegroundColor Green }
function Bad([string]$t) { Write-Host "  [FAIL] $t" -ForegroundColor Red }
function Warn([string]$t) { Write-Host "  [WARN] $t" -ForegroundColor Yellow }
function Info([string]$t) { Write-Host "         $t" -ForegroundColor Gray }

# ------------------------------------------------------------------ paths
$vrpathFile = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'
if (-not (Test-Path -LiteralPath $vrpathFile)) {
    Bad "openvrpaths.vrpath not found: $vrpathFile"
    exit 1
}
$vrpath = Get-Content -LiteralPath $vrpathFile -Raw | ConvertFrom-Json
$runtime = $vrpath.runtime | Select-Object -First 1
$configDir = $vrpath.config | Select-Object -First 1
$settingsFile = Join-Path $configDir 'steamvr.vrsettings'

# ---------------------------------------------------------- 1. the glasses
Section '1. Glasses visible to Windows'
# The HUAWEI VR Glass is a split design: the headset is just a DisplayPort
# panel. It does NOT show up as an HMD/HID device - it shows up as a MONITOR.
# So the check is: did the monitor count go up after plugging it in?
$hwDev = Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match 'Huawei|HUAWEI|VR Glass|VRGlass|Ceres' }
if ($hwDev) {
    foreach ($d in $hwDev) { Good "$($d.Name)  [Status=$($d.Status)]"; Info $d.DeviceID }
} else {
    Info 'no Huawei HID device - that is normal for this headset (display only)'
}

$monitors = @(Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBasicDisplayParams -ErrorAction SilentlyContinue)
Write-Host ("         monitors currently attached: {0}" -f $monitors.Count) -ForegroundColor Gray
foreach ($m in $monitors) { Info "EDID: $($m.InstanceName)" }
if ($monitors.Count -ge 2) {
    Good 'more than one monitor - the glasses are probably among them'
    Info 'Compare this list before/after plugging the cable to identify it.'
} else {
    Bad 'only one monitor - the glasses are not getting a DP signal'
    Info 'Re-seat the USB-C cable; the port must support DP alt-mode.'
}

# --------------------------------------------------------- 2. the assistant
Section '2. Huawei PC VR Assistant'
$assistantExe = Get-ChildItem -Path 'C:\Program Files (x86)', 'D:\Program Files (x86)', 'D:\Program Files', 'C:\Program Files' `
    -Filter 'HwPCVRAssistant.exe' -Recurse -Depth 3 -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($assistantExe) { Info "found: $($assistantExe.FullName)" } else { Info 'HwPCVRAssistant.exe not found' }

$assistProc = Get-Process -Name 'HwPCVRAssistant', 'PCCastScreen' -ErrorAction SilentlyContinue
if ($assistProc) {
    Good "assistant running (pid $($assistProc.Id -join ','))"
} else {
    Bad 'assistant not running'
    Info 'Official wired mode needs it: it owns the USB link and wakes the glasses.'
    if ($StartAssistant -and $assistantExe) {
        Start-Process -FilePath $assistantExe.FullName
        Write-Host '         assistant launched' -ForegroundColor Yellow
    } elseif ($assistantExe) {
        Info 're-run with -StartAssistant to launch it.'
    }
}

# ------------------------------------------------------------ 3. runtime
Section '3. SteamVR runtime'
Good "runtime: $runtime"
$manifest = Get-ChildItem -Path (Split-Path $runtime -Parent) -Filter 'appmanifest_250820.acf' -ErrorAction SilentlyContinue |
    Select-Object -First 1
if (-not $manifest) {
    $manifest = Get-ChildItem -Path 'D:\Program Files\Steam\steamapps', 'C:\Program Files\Steam\steamapps' `
        -Filter 'appmanifest_250820.acf' -ErrorAction SilentlyContinue | Select-Object -First 1
}
if ($manifest) {
    $txt = Get-Content -LiteralPath $manifest.FullName -Raw
    if ($txt -match '"buildid"\s+"(\d+)"') { Info "buildid: $($Matches[1])" }
}
Info 'Huawei driver files are dated 2021; a much newer runtime may refuse them.'
Info 'An older runtime can be registered with:'
Info '   <steamvr>\bin\win64\vrpathreg.exe setruntime <path-to-older-SteamVR>'

# ------------------------------------------------------------ 4. drivers
Section '4. Driver registration and enable flags'
$drivers = @()
if ($vrpath.external_drivers) { $drivers += $vrpath.external_drivers }
$drivers = $drivers | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') }

$hwCopies = @()
foreach ($d in $drivers) {
    if (-not (Test-Path -LiteralPath $d)) { Warn "missing driver dir: $d"; continue }
    $mf = Join-Path $d 'driver.vrdrivermanifest'
    $nm = $null
    if (Test-Path -LiteralPath $mf) {
        $t = Get-Content -LiteralPath $mf -Raw
        if ($t -match '"name"\s*:\s*"([^"]+)"') { $nm = $Matches[1] }
    }
    $bin = Join-Path $d 'bin\win64'
    $dll = $null
    if (Test-Path -LiteralPath $bin) {
        $dll = Get-ChildItem -LiteralPath $bin -Filter 'driver_*.dll' -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -notlike '*_real*' } | Select-Object -First 1
    }
    $size = if ($dll) { [math]::Round($dll.Length / 1KB) } else { 0 }
    $tag = ''
    if ($dll -and $dll.Length -lt 300KB) { $tag = ' [SHIM]' }
    Info ("{0,-14} {1,6} KB{2}  {3}" -f $nm, $size, $tag, $d)
    if ($nm -eq 'huaweiVR' -and $dll) {
        $hwCopies += [pscustomobject]@{ Dir = $d; Size = $dll.Length }
    }
}
if ($hwCopies.Count -gt 1) {
    Warn "$($hwCopies.Count) copies share the name 'huaweiVR' - SteamVR loads the first one it finds"
    Info 'and skips the rest (see vrserver.txt "Skipping duplicate external driver").'
    Info 'Run .\switch-vr-driver.ps1 status -Dedupe to keep only the largest real one.'
}

$raw = if (Test-Path -LiteralPath $settingsFile) { Get-Content -LiteralPath $settingsFile -Raw } else { $null }
foreach ($n in 'huaweiVR', 'alvr_server') {
    $v = $null
    if ($raw) {
        $m = [regex]::Match($raw, '"driver_' + [regex]::Escape($n) + '"\s*:\s*\{\s*"enable"\s*:\s*(true|false)', 'IgnoreCase')
        if ($m.Success) { $v = ($m.Groups[1].Value -eq 'true') }
    }
    $shown = if ($v -eq $null) { 'not set (defaults to enabled)' } elseif ($v) { 'enabled' } else { 'DISABLED' }
    if ($n -eq 'huaweiVR') {
        if ($v -eq $false) { Bad "driver_$n = $shown" } else { Good "driver_$n = $shown" }
    } else {
        if ($v -eq $true) { Warn "driver_$n = $shown (should be false in wired mode)" } else { Good "driver_$n = $shown" }
    }
}

# ------------------------------------------------------------- 5. SteamVR
Section '5. Steam processes'
$steam = Get-Process -Name 'steam' -ErrorAction SilentlyContinue
if ($steam) { Good 'Steam client running (needed for the VR library panel)' } else { Bad 'Steam client NOT running - the SteamVR library will be empty' }
$vr = Get-Process -Name 'vrserver', 'vrmonitor' -ErrorAction SilentlyContinue
if ($vr) { Good "SteamVR running ($(($vr.Name | Select-Object -Unique) -join ','))" } else { Info 'SteamVR not running' }

Write-Host ''
Write-Host 'Order that works: plug glasses -> start assistant -> assistant starts SteamVR.' -ForegroundColor Cyan
