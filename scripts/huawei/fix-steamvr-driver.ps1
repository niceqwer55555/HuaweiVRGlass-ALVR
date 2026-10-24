# Fix SteamVR config broken by third party installers:
#   1. driver_alvr_server.enable was set to false -> ALVR driver is skipped
#   2. driver_huaweiVR.enable is true and recorded as LastKnown HMD -> Huawei stock
#      driver grabs the headset before ALVR
#   3. openvrpaths.vrpath keeps runtime entries pointing to deleted directories
#
# Both files are backed up next to the originals first. Refuses to run while SteamVR is up.
# Revert: powershell -ExecutionPolicy Bypass -File <this file> -Revert
param(
    [switch]$Revert
)

$ErrorActionPreference = 'Stop'

$vrsettings = 'D:\Program Files\Steam\config\steamvr.vrsettings'
$vrpathFile = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'

$procs = Get-Process -Name vrmonitor, vrserver, vrdashboard, vrwebhelper, vrcompositor -ErrorAction SilentlyContinue
if ($procs) {
    Write-Host "SteamVR is still running: $($procs.Name -join ', '). Close SteamVR and retry."
    exit 1
}

function Backup-File($path) {
    if (-not (Test-Path -LiteralPath $path)) { return }
    $bak = "$path.bak.$(Get-Date -Format yyyyMMdd_HHmmss)"
    Copy-Item -LiteralPath $path -Destination $bak -Force
    Write-Host "backup: $bak"
}

if ($Revert) {
    $latest = Get-ChildItem (Split-Path $vrsettings) -Filter 'steamvr.vrsettings.bak.*' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) {
        Copy-Item $latest.FullName -Destination $vrsettings -Force
        Write-Host "restored vrsettings from $($latest.FullName)"
    }
    $latestP = Get-ChildItem (Split-Path $vrpathFile) -Filter 'openvrpaths.vrpath.bak.*' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latestP) {
        Copy-Item $latestP.FullName -Destination $vrpathFile -Force
        Write-Host "restored vrpath from $($latestP.FullName)"
    }
    exit 0
}

Backup-File $vrsettings
Backup-File $vrpathFile

# ---- 1. steamvr.vrsettings: enable ALVR, disable the Huawei stock driver ----
$text = Get-Content -LiteralPath $vrsettings -Raw
$changes = @()

$patternFalse = '("driver_alvr_server"\s*:\s*\{\s*"enable"\s*:\s*)false'
if (([regex]::Matches($text, $patternFalse)).Count -gt 0) {
    $text = [regex]::Replace($text, $patternFalse, '${1}true')
    $changes += "driver_alvr_server: false -> true"
} elseif ($text -match '"driver_alvr_server"') {
    Write-Host "driver_alvr_server already enabled"
} else {
    $text = [regex]::Replace($text, '(\{\s*)', "`${1}`r`n   `"driver_alvr_server`" : {`r`n      `"enable`" : true`r`n   },", 1)
    $changes += "driver_alvr_server: added -> true"
}

$patternTrue = '("driver_huaweiVR"\s*:\s*\{\s*"enable"\s*:\s*)true'
if (([regex]::Matches($text, $patternTrue)).Count -gt 0) {
    $text = [regex]::Replace($text, $patternTrue, '${1}false')
    $changes += "driver_huaweiVR: true -> false"
}

Set-Content -LiteralPath $vrsettings -Value $text -NoNewline -Encoding UTF8
Write-Host "vrsettings updated: $($changes -join '; ')"

# ---- 2. openvrpaths.vrpath: drop dead runtime entries ----
if (Test-Path -LiteralPath $vrpathFile) {
    $json = (Get-Content -LiteralPath $vrpathFile -Raw) | ConvertFrom-Json
    $dead = @($json.runtime | Where-Object { -not (Test-Path -LiteralPath $_) })
    if ($dead.Count -gt 0) {
        $lines = Get-Content -LiteralPath $vrpathFile
        $newLines = New-Object System.Collections.ArrayList
        $inRuntime = $false
        foreach ($line in $lines) {
            if ($line -match '"runtime"\s*:') {
                $inRuntime = $true
                [void]$newLines.Add($line)
                continue
            }
            if ($line -match '^\s*\]') {
                $inRuntime = $false
                [void]$newLines.Add($line)
                continue
            }
            $skip = $false
            if ($inRuntime) {
                foreach ($d in $dead) {
                    if ($line -match [regex]::Escape($d)) { $skip = $true }
                }
            }
            if ($skip) {
                Write-Host "removed dead runtime entry: $($line.Trim())"
                continue
            }
            [void]$newLines.Add($line)
        }
        Set-Content -LiteralPath $vrpathFile -Value ($newLines -join "`r`n") -NoNewline -Encoding UTF8
        $keep = @($json.runtime | Where-Object { Test-Path -LiteralPath $_ })
        Write-Host "remaining runtime: $($keep -join ' | ')"
    } else {
        Write-Host "no dead runtime entry in vrpath"
    }
}

Write-Host ""
Write-Host "done. Run ALVR Dashboard -> register driver -> connect headset -> start SteamVR."
