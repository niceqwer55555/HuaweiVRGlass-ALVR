#requires -Version 5.1
# Build ALVR streamer for Windows with the GPL (FFmpeg/x264) software encoder enabled.
# Output: build/alvr_streamer_windows (driver DLL + bundled FFmpeg DLLs)

[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [string]$ToolsRoot,
    [switch]$Online
)

$ErrorActionPreference = 'Stop'
# Toolchain locations are resolved at runtime (default root: D:\Tools, see toolchain-env.ps1).
# The MSVC environment is set manually instead of calling VsDevCmd.bat, which shells out to
# reg.exe and is blocked in the build sandbox.
. (Join-Path $PSScriptRoot 'toolchain-env.ps1')
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$tc = Get-VrToolchain -ToolsRoot $ToolsRoot -Validate
$tc.Apply()
if ($Online) { [Environment]::SetEnvironmentVariable('CARGO_NET_OFFLINE', $null, 'Process') }
else { $env:CARGO_NET_OFFLINE = 'true' }
$ffmpegDir = Join-Path $repoRoot 'deps\windows\ffmpeg'
$logPath = Join-Path $repoRoot 'build\gpl-build.log'

foreach ($p in @((Join-Path $ffmpegDir 'include\libavcodec\avcodec.h'), (Join-Path $ffmpegDir 'lib\avcodec.lib'))) {
    if (-not (Test-Path -LiteralPath $p)) { throw "FFmpeg dev files not found: $p" }
}

Write-Host ($tc.Summary())
Write-Host "INCLUDE = $env:INCLUDE"
Write-Host "LIB     = $env:LIB"
Write-Host "cargo   = $($tc.CargoExe)"

Push-Location $repoRoot
try {
    $arguments = @('xtask', 'build-streamer')
    if ($Configuration -eq 'Release') { $arguments += '--release' }
    $arguments += @('--gpl', '--keep-config')
    Write-Host ("cargo " + ($arguments -join ' '))
    # cargo writes its progress to stderr; merging it into the pipeline under
    # $ErrorActionPreference='Stop' would abort the build immediately.
    $previousEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $tc.CargoExe @arguments 2>&1 | Tee-Object -FilePath $logPath
    $ErrorActionPreference = $previousEap
    if ($LASTEXITCODE -ne 0) { throw "cargo xtask build-streamer --gpl failed with exit code $LASTEXITCODE" }

    $driverDir = Join-Path $repoRoot 'build\alvr_streamer_windows\bin\win64'
    Get-ChildItem -LiteralPath $driverDir -Filter *.dll |
        Select-Object Name, @{n = 'SizeMB'; e = { [math]::Round($_.Length / 1MB, 2) } } |
        Format-Table -AutoSize | Out-String | Write-Host
    Write-Host 'BUILD_OK'
}
finally {
    Pop-Location
}
