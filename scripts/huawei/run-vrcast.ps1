#requires -Version 5.1
<#
Run the clean-room caster (vrcast) for the wired Huawei VR Glass.

vrcast captures the Windows desktop with DDA and submits each eye to the
OpenVR compositor, which scans it out to the glasses via the official Huawei
driver. This is the piece that actually "lights up" the glasses in SteamVR.

Prerequisites (all in this server folder):
  * vrcast\  (vrcast.exe + openvr_api.dll)
  * official Huawei driver installed into SteamVR (install-usb-drivers.ps1)
  * glasses docked to PC, EDID visible, SteamVR runtime = the one with the
    Huawei built-in driver

Usage:
  .\run-vrcast.ps1              # list outputs, use output 1 (glasses) if present
  .\run-vrcast.ps1 -Output 0    # capture main screen instead
  .\run-vrcast.ps1 -NoSteamVR   # don't launch SteamVR (assume it's running)
#>
[CmdletBinding()]
param(
    [int]$Output = -1,
    [ValidateSet('world', 'follow')][string]$Mode = 'world',
    [switch]$NoSteamVR
)

$ErrorActionPreference = 'Continue'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$vrcastDir = Join-Path $scriptDir 'vrcast'
$vrcastExe = Join-Path $vrcastDir 'vrcast.exe'

if (-not (Test-Path -LiteralPath $vrcastExe -PathType Leaf)) { throw "vrcast.exe not found: $vrcastExe" }

# Find SteamVR: prefer the runtime that has our built-in huaweiVR driver.
function Find-SteamVrExe {
    $paths = @()
    $vpPath = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'
    if (Test-Path -LiteralPath $vpPath -PathType Leaf) {
        try {
            $vp = Get-Content $vpPath -Raw | ConvertFrom-Json
            foreach ($r in @($vp.runtime)) {
                if (-not $r) { continue }
                $paths += (Join-Path $r 'bin\win64\vrmonitor.exe')
                if (Test-Path -LiteralPath (Join-Path $r 'drivers\huaweivr\bin\win64\driver_huaweiVR.dll') -PathType Leaf) {
                    return $r   # this runtime has the Huawei driver -> use it
                }
            }
        } catch {}
    }
    foreach ($base in @($env:ProgramFiles, ${env:ProgramFiles(x86)}, "$env:LOCALAPPDATA\Programs")) {
        if ($base) { $paths += (Join-Path $base 'Steam\steamapps\common\SteamVR\bin\win64\vrmonitor.exe') }
    }
    foreach ($p in $paths) { if (Test-Path -LiteralPath $p -PathType Leaf) { return (Split-Path (Split-Path (Split-Path $p -Parent) -Parent) -Parent) } }
    return $null
}

$steamvrRoot = Find-SteamVrExe
Write-Host "vrcast dir   : $vrcastDir" -ForegroundColor Cyan
Write-Host "SteamVR root : $steamvrRoot" -ForegroundColor Cyan

if (-not $NoSteamVR) {
    $running = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match 'vrserver|vrmonitor' })
    if ($running.Count -eq 0 -and $steamvrRoot) {
        $mon = Join-Path $steamvrRoot 'bin\win64\vrmonitor.exe'
        Write-Host "starting SteamVR: $mon" -ForegroundColor Yellow
        & $mon | Out-Null
        Start-Sleep -Seconds 15
    }
}

Write-Host ""
Write-Host "=== available outputs ===" -ForegroundColor Cyan
& $vrcastExe --list-outputs 2>&1 | ForEach-Object { Write-Host $_ }

if ($Output -lt 0) {
    Write-Host ""
    Write-Host "Pick the glasses output index from the list above, then run:" -ForegroundColor Yellow
    Write-Host "  .\run-vrcast.ps1 -Output N" -ForegroundColor Yellow
    Write-Host "(usually the second output, index 1, at a negative Y offset)"
    return
}

Write-Host ""
Write-Host "=== casting output $Output ($Mode) - press Esc in the vrcast window to stop ===" -ForegroundColor Green
Push-Location $vrcastDir
try {
    & $vrcastExe --output $Output --mode $Mode --verbose 2>&1 | ForEach-Object { Write-Host $_ }
} finally {
    Pop-Location
}
