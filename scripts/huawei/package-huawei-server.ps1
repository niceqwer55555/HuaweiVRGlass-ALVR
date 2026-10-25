#requires -Version 5.1
<#
    Refresh the Windows server package (build\end\server) with a freshly built
    dashboard and re-pack the 7z archive.

    The template directory is the deployment folder, because that is where the
    session.json with the wired-client settings lives. Only the dashboard
    executable is replaced on top of it - the driver DLL and the FFmpeg runtime
    are only rebuilt when the driver sources change.

    Usage:
        .\package-huawei-server.ps1
        .\package-huawei-server.ps1 -Deploy          # also update the live folder
        .\package-huawei-server.ps1 -DashboardExe X:\path\alvr_dashboard.exe

    ASCII-only on purpose: PowerShell 5.1 mis-reads UTF-8 without BOM.
#>
[CmdletBinding()]
param(
    [string]$DashboardExe = (Join-Path $PSScriptRoot '..\..\target\release\alvr_dashboard.exe'),
    [string]$TemplateDir = 'D:\HuaweiVRGlass_ALVR_Windows_v0.5.0',
    [string]$OutputDir = (Join-Path $PSScriptRoot '..\..\build\end\server'),
    [string]$Archive = (Join-Path $PSScriptRoot '..\..\build\end\server.7z'),
    [string]$SevenZip,
    [switch]$Deploy
)

$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

# Resolve a path that may be absolute already, or relative to the repo root.
function Resolve-RepoPath {
    param([string]$Path)
    if ([System.IO.Path]::IsPathRooted($Path)) { return [System.IO.Path]::GetFullPath($Path) }
    return [System.IO.Path]::GetFullPath((Join-Path $repoRoot $Path))
}

$dashboardExe = Resolve-RepoPath $DashboardExe
$outputDir = Resolve-RepoPath $OutputDir
$archive = Resolve-RepoPath $Archive

if (-not (Test-Path -LiteralPath $dashboardExe -PathType Leaf)) {
    throw "dashboard executable not found: $dashboardExe"
}
if (-not (Test-Path -LiteralPath $TemplateDir -PathType Container)) {
    throw "template (deployment) folder not found: $TemplateDir"
}

# --- 7-Zip -----------------------------------------------------------------
if (-not $SevenZip) {
    $SevenZip = @(
        'D:\Program Files\7-Zip\7z.exe',
        'C:\Program Files\7-Zip\7z.exe',
        'F:\Program Files\7-Zip\7z.exe'
    ) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
}
if (-not $SevenZip) {
    throw '7-Zip not found; pass -SevenZip <path to 7z.exe>'
}

# --- Copy the template, then drop in the new dashboard ---------------------
Write-Host "template : $TemplateDir"
Write-Host "output   : $outputDir"
Write-Host "dashboard: $dashboardExe"

New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
Get-ChildItem -LiteralPath $TemplateDir -Force | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $outputDir -Recurse -Force
}
Copy-Item -LiteralPath $dashboardExe -Destination (Join-Path $outputDir 'ALVR Dashboard.exe') -Force

# --- Repack ----------------------------------------------------------------
if (Test-Path -LiteralPath $archive) {
    Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $archive) { throw "cannot overwrite $archive" }
}
$previousEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
& $SevenZip a -t7z -mx=5 -r -- $archive (Join-Path $outputDir '*') 2>&1 | Out-String | Write-Host
$ErrorActionPreference = $previousEap
if (-not (Test-Path -LiteralPath $archive)) { throw "archive was not produced: $archive" }

# --- Optional: update the live deployment folder ---------------------------
if ($Deploy) {
    Copy-Item -LiteralPath $dashboardExe -Destination (Join-Path $TemplateDir 'ALVR Dashboard.exe') -Force
    Write-Host "deployed to $TemplateDir"
}

Get-ChildItem -LiteralPath $outputDir -File |
    Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } } |
    Format-Table -AutoSize | Out-String | Write-Host
Get-Item -LiteralPath $archive | Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } } |
    Format-Table -AutoSize | Out-String | Write-Host
Write-Host 'PACKAGE_OK'
