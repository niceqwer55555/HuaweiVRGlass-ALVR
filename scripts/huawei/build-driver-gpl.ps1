#requires -Version 5.1
# Build only the OpenVR driver DLL with the GPL (FFmpeg/x264) software encoder enabled.
# No VsDevCmd.bat (it calls reg.exe, which the sandbox blocks) - MSVC env is set manually.
# Output: build\gpl_driver_stage (driver DLL + FFmpeg runtime DLLs)

[CmdletBinding()]
param(
    [string]$StageDir,
    [string]$ToolsRoot,
    [switch]$Clean,
    # Allow cargo to refresh the crate index. Needed when the local index cache loses an entry,
    # otherwise cargo fails with "no matching package named ..." even though the crate is cached.
    [switch]$Online
)

$ErrorActionPreference = 'Stop'
# Toolchain locations are resolved at runtime (default root: D:\Tools, see toolchain-env.ps1).
. (Join-Path $PSScriptRoot 'toolchain-env.ps1')
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $StageDir) { $StageDir = Join-Path $repoRoot 'build\gpl_driver_stage' }
$tc = Get-VrToolchain -ToolsRoot $ToolsRoot -Validate
$tc.Apply()
if ($Online) {
    [Environment]::SetEnvironmentVariable('CARGO_NET_OFFLINE', $null, 'Process')
    Write-Host 'online: cargo may refresh the crate index'
} else {
    $env:CARGO_NET_OFFLINE = 'true'
}
$ffmpegDir = Join-Path $repoRoot 'deps\windows\ffmpeg'
$logPath = Join-Path $repoRoot 'build\gpl-driver-build.log'

$checks = @(
    (Join-Path $ffmpegDir 'include\libavcodec\avcodec.h'),
    (Join-Path $ffmpegDir 'lib\avcodec.lib')
)
foreach ($p in $checks) {
    if (-not (Test-Path -LiteralPath $p)) { throw "missing prerequisite: $p" }
}

Write-Host ($tc.Summary())
Write-Host "INCLUDE = $env:INCLUDE"
Write-Host "LIB     = $env:LIB"
Write-Host "CC      = $env:CC"
Write-Host "cargo   = $($tc.CargoExe)"

Push-Location $repoRoot
try {
    $previousEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    if ($Clean) {
        & $tc.CargoExe clean -p alvr_filesystem -p alvr_server_openvr 2>&1 |
            Out-String | Write-Host
    }
    $cargoArgs = @('build', '--release', '-p', 'alvr_server_openvr', '--features', 'gpl')
    if (-not $Online) { $cargoArgs += '--offline' }
    & $tc.CargoExe @cargoArgs 2>&1 |
        Tee-Object -FilePath $logPath
    $cargoExit = $LASTEXITCODE
    $ErrorActionPreference = $previousEap
    if ($cargoExit -ne 0) { throw "cargo build failed with exit code $cargoExit" }

    $dll = Join-Path $repoRoot 'target\release\alvr_server_openvr.dll'
    $pdb = Join-Path $repoRoot 'target\release\alvr_server_openvr.pdb'
    if (-not (Test-Path -LiteralPath $dll)) { throw "driver DLL not produced: $dll" }

    if (Test-Path -LiteralPath $StageDir) { Remove-Item -LiteralPath $StageDir -Recurse -Force }
    New-Item -ItemType Directory -Path $StageDir -Force | Out-Null
    Copy-Item -LiteralPath $dll -Destination (Join-Path $StageDir 'driver_alvr_server.dll') -Force
    if (Test-Path -LiteralPath $pdb) {
        Copy-Item -LiteralPath $pdb -Destination (Join-Path $StageDir 'alvr_server_openvr.pdb') -Force
    }
    Get-ChildItem -LiteralPath (Join-Path $ffmpegDir 'bin') -Filter *.dll |
        Copy-Item -Destination $StageDir -Force

    Get-ChildItem -LiteralPath $StageDir |
        Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } } |
        Format-Table -AutoSize | Out-String | Write-Host
    Write-Host 'BUILD_OK'
}
finally {
    Pop-Location
}
