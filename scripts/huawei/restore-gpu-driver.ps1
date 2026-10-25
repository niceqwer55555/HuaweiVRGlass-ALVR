#Requires -Version 5.1
<#
    Restore the Valve OEM GPU driver for the Van Gogh APU.

    Why this exists
    ---------------
    The Amernime "Release AMD SoC" modified driver (ati2mtag_VanGogh / DEV_163F) installs
    and even clears Code 43 after a cold reboot, but it never brings up the VCN media
    engine: AMF init fails, D3D11VA/DXVA2 fail, the AMDh265Encoder hardware MFT cannot be
    activated. Hardware encoding on Windows is therefore a dead end for this APU, and the
    machine should go back to the Valve driver (u0388790.inf, 31.0.14033.2008).

    This script finds that driver in the DriverStore and rebinds the display adapter to it,
    so no local backup folder is required.

    ASCII only on purpose
    ---------------------
    PowerShell 5.1 parses a UTF-8 file without a BOM as ANSI, so Chinese comments turn into
    mojibake and the parser dies with "Missing closing '}'" / "The string is missing the
    terminator". Keep this file ASCII.

    Usage
    -----
        .\restore-gpu-driver.ps1                 # restore from the DriverStore copy
        .\restore-gpu-driver.ps1 -ListOnly       # just report what is installed
        .\restore-gpu-driver.ps1 -InfPath D:\bak\u0388790.inf
        .\restore-gpu-driver.ps1 -SkipProcessStop

    Exit codes: 0 ok, 1 precondition failed or adapter still unhealthy, 2 driver not found.
#>
[CmdletBinding()]
param(
    [string]$InfPath,
    [switch]$ListOnly,
    [switch]$SkipProcessStop
)

$ErrorActionPreference = 'Continue'

$ValveInfName = 'u0388790.inf'
$DisplayClass = 'Display'

function Test-Administrator {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($id)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Write-Section {
    param([string]$Text)
    Write-Host ''
    Write-Host "=== $Text ===" -ForegroundColor Cyan
}

function Get-DisplayDriverPackages {
    # pnputil output is localized, so parse defensively: look for the oem/u INF names
    # anywhere in each block instead of relying on the English field labels.
    $raw = & pnputil.exe /enum-drivers 2>&1
    $text = ($raw | Out-String)
    $blocks = $text -split "(\r?\n){2,}"

    $result = @()
    foreach ($block in $blocks) {
        if ($block -notmatch 'oem\d+\.inf') { continue }

        $published = $null
        $original = $null
        $provider = $null

        foreach ($line in ($block -split "\r?\n")) {
            if (-not $published -and $line -match ':\s*(oem\d+\.inf)') { $published = $Matches[1] }
            if (-not $original -and $line -match ':\s*(u\d+\.inf)') { $original = $Matches[1] }
            if (-not $provider -and $line -match '(Advanced Micro Devices|AMD|NVIDIA|Intel|Microsoft)') { $provider = $Matches[1] }
        }

        # Only Display class packages carry a u####.inf AMD style name or match the known set.
        if ($published) {
            $isDisplay = $false
            if ($original -and $original -match '^u\d+\.inf$') { $isDisplay = $true }
            if ($block -match 'Display|u0388790|u0394729|u0397') { $isDisplay = $true }
            if (-not $isDisplay) { continue }

            $result += [pscustomobject]@{
                Published = $published
                Original  = $original
                Provider  = $provider
            }
        }
    }

    $seen = @{}
    $unique = @()
    foreach ($item in $result) {
        $key = "$($item.Published)|$($item.Original)"
        if (-not $seen.ContainsKey($key)) {
            $seen[$key] = $true
            $unique += $item
        }
    }
    return $unique
}

function Show-CurrentAdapter {
    Write-Section 'Current display adapter'
    Get-CimInstance Win32_VideoController |
        Select-Object Name, DriverVersion, InfFilename, Status |
        Format-List

    Write-Section 'Display class devices'
    Get-PnpDevice -Class $DisplayClass -PresentOnly -ErrorAction SilentlyContinue |
        Select-Object Status, FriendlyName, @{ n = 'ProblemCode'; e = { $_.ProblemCode } } |
        Format-Table -AutoSize
}

function Find-ValveInf {
    $repository = 'C:\Windows\System32\DriverStore\FileRepository'
    $dirs = @()
    if (Test-Path -LiteralPath $repository) {
        $dirs = @(Get-ChildItem -LiteralPath $repository -Directory -Filter 'u0388790.inf_amd64_*' -ErrorAction SilentlyContinue)
    }

    # Prefer a directory that still holds the INF itself.
    foreach ($dir in $dirs) {
        $candidate = Join-Path $dir.FullName $ValveInfName
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }

    # Some DriverStore folders keep the INF under a pack subdirectory.
    foreach ($dir in $dirs) {
        $hit = Get-ChildItem -LiteralPath $dir.FullName -Recurse -Filter $ValveInfName -File -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($hit) { return $hit.FullName }
    }

    return $null
}

# ---------------------------------------------------------------------------
# Preconditions
# ---------------------------------------------------------------------------
if (-not (Test-Administrator)) {
    Write-Host 'This script must run elevated (Run as administrator).' -ForegroundColor Red
    exit 1
}

if ($ListOnly) {
    Show-CurrentAdapter
    Write-Section 'Installed display driver packages'
    Get-DisplayDriverPackages | Format-Table -AutoSize
    Write-Section 'Valve INF in DriverStore'
    $found = Find-ValveInf
    if ($found) { Write-Host $found } else { Write-Host 'not found' -ForegroundColor Yellow }
    exit 0
}

# ---------------------------------------------------------------------------
# Locate the INF to install
# ---------------------------------------------------------------------------
if ($InfPath) {
    if (-not (Test-Path -LiteralPath $InfPath)) {
        Write-Host "INF not found: $InfPath" -ForegroundColor Red
        exit 2
    }
    $targetInf = (Resolve-Path -LiteralPath $InfPath).Path
}
else {
    $targetInf = Find-ValveInf
    if (-not $targetInf) {
        Write-Host "Could not find $ValveInfName in the DriverStore." -ForegroundColor Red
        Write-Host 'Pass the path of a copy with -InfPath. The package needs both the INF and its CAT.' -ForegroundColor Yellow
        exit 2
    }
}

Write-Section 'Target INF'
Write-Host $targetInf

$targetCat = $null
$catCandidate = [IO.Path]::ChangeExtension($targetInf, '.cat')
if (Test-Path -LiteralPath $catCandidate) { $targetCat = $catCandidate }
if (-not $targetCat) {
    $catDir = Split-Path -Parent $targetInf
    $anyCat = Get-ChildItem -LiteralPath $catDir -Filter '*.cat' -File -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($anyCat) { $targetCat = $anyCat.FullName }
}
if ($targetCat) {
    Write-Host "Catalog: $targetCat"
}
else {
    Write-Host 'Warning: no catalog file next to the INF, signature verification will likely fail.' -ForegroundColor Yellow
}

# ---------------------------------------------------------------------------
# Stop the VR stack so the display driver is not held in use
# ---------------------------------------------------------------------------
if (-not $SkipProcessStop) {
    Write-Section 'Stopping VR processes'
    $names = @(
        'ALVR Dashboard', 'vrmonitor', 'vrserver', 'vrcompositor', 'vrdashboard',
        'vrwebhelper', 'vrdriver', 'vrserverhelper', 'vrmonitorhelper', 'vrstartup'
    )
    foreach ($name in $names) {
        $procs = Get-Process -Name $name -ErrorAction SilentlyContinue
        if ($procs) {
            $procs | Stop-Process -Force -ErrorAction SilentlyContinue
            Write-Host "stopped: $name"
        }
    }
    Start-Sleep -Seconds 3
}

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------
Write-Section 'Installing driver'
$installOut = & pnputil.exe /add-driver $targetInf /install 2>&1
$installOut | Out-String -Width 200 | Write-Host
$installRc = $LASTEXITCODE
Write-Host "pnputil exit code: $installRc"

Write-Host ''
Write-Host 'Rescanning devices...'
& pnputil.exe /scan-devices 2>&1 | Out-String -Width 200 | Write-Host
Start-Sleep -Seconds 5

# ---------------------------------------------------------------------------
# Verify
# ---------------------------------------------------------------------------
Show-CurrentAdapter

$adapter = Get-CimInstance Win32_VideoController |
    Where-Object { $_.Name -notmatch 'Basic Display|Remote' } |
    Select-Object -First 1

$problem = $null
if ($adapter) {
    $pnp = Get-PnpDevice -Class $DisplayClass -PresentOnly -ErrorAction SilentlyContinue |
        Where-Object { $_.FriendlyName -eq $adapter.Name } |
        Select-Object -First 1
    if ($pnp) { $problem = $pnp.ProblemCode }
}

Write-Section 'Result'
if ($adapter -and $adapter.Status -eq 'OK' -and (-not $problem -or $problem -eq 0)) {
    Write-Host "OK: $($adapter.Name) on $($adapter.DriverVersion) ($($adapter.InfFilename))" -ForegroundColor Green
    exit 0
}

Write-Host 'Adapter is not healthy yet.' -ForegroundColor Yellow
if ($problem -eq 43) {
    Write-Host 'Code 43 right after a driver package swap is expected: the previous kernel' -ForegroundColor Yellow
    Write-Host 'mode driver is still resident in memory. Do a full shutdown/restart, then' -ForegroundColor Yellow
    Write-Host 're-run with -ListOnly to confirm.' -ForegroundColor Yellow
}
else {
    Write-Host "ProblemCode: $problem" -ForegroundColor Yellow
    Write-Host 'A restart is usually required before the new driver takes over.' -ForegroundColor Yellow
}
exit 1
