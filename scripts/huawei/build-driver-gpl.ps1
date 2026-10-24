#requires -Version 5.1
# Build only the OpenVR driver DLL with the GPL (FFmpeg/x264) software encoder enabled.
# No VsDevCmd.bat (it calls reg.exe, which the sandbox blocks) - MSVC env is set manually.
# Output: build\gpl_driver_stage (driver DLL + FFmpeg runtime DLLs)

[CmdletBinding()]
param(
    [string]$StageDir = 'F:\PY\HuaweiVRGlass-ALVR\build\gpl_driver_stage',
    [switch]$Clean,
    # Allow cargo to refresh the crate index. Needed when the local index cache loses an entry,
    # otherwise cargo fails with "no matching package named ..." even though the crate is cached.
    [switch]$Online
)

$ErrorActionPreference = 'Stop'
$repoRoot = 'F:\PY\HuaweiVRGlass-ALVR'
$msvcVer = '14.44.35207'
$sdkVer = '10.0.26100.0'
$vcRoot = "F:\VS2022\BuildTools\VC\Tools\MSVC\$msvcVer"
$sdkRoot = 'C:\Program Files (x86)\Windows Kits\10'
$libclangPath = 'F:\LLVM\bin'
$cargoHome = 'D:\PY\1\toolchain-setup\cargo'
$cargoBin = Join-Path $cargoHome 'bin'
$rustBin = 'D:\PY\1\toolchain-setup\rust-toolchain\bin'
$ffmpegDir = Join-Path $repoRoot 'deps\windows\ffmpeg'
$logPath = Join-Path $repoRoot 'build\gpl-driver-build.log'

$checks = @(
    (Join-Path $vcRoot 'bin\HostX64\x64\cl.exe'),
    (Join-Path $vcRoot 'lib\x64'),
    (Join-Path $sdkRoot "Include\$sdkVer\ucrt"),
    (Join-Path $libclangPath 'libclang.dll'),
    (Join-Path $rustBin 'cargo.exe'),
    (Join-Path $ffmpegDir 'include\libavcodec\avcodec.h'),
    (Join-Path $ffmpegDir 'lib\avcodec.lib')
)
foreach ($p in $checks) {
    if (-not (Test-Path -LiteralPath $p)) { throw "missing prerequisite: $p" }
}

# --- MSVC / Windows SDK environment (manual, no VsDevCmd) ---
$includeDirs = @(
    (Join-Path $vcRoot 'include'),
    (Join-Path 'F:\VS2022\BuildTools\VC\Auxiliary\VS' 'include'),
    (Join-Path $sdkRoot "Include\$sdkVer\ucrt"),
    (Join-Path $sdkRoot "Include\$sdkVer\um"),
    (Join-Path $sdkRoot "Include\$sdkVer\shared"),
    (Join-Path $sdkRoot "Include\$sdkVer\winrt"),
    (Join-Path $sdkRoot "Include\$sdkVer\cppwinrt")
)
$libDirs = @(
    (Join-Path $vcRoot 'lib\x64'),
    (Join-Path $sdkRoot "Lib\$sdkVer\ucrt\x64"),
    (Join-Path $sdkRoot "Lib\$sdkVer\um\x64")
)
$env:INCLUDE = ($includeDirs | Where-Object { Test-Path -LiteralPath $_ }) -join ';'
$env:LIB = ($libDirs | Where-Object { Test-Path -LiteralPath $_ }) -join ';'
$env:CC = Join-Path $vcRoot 'bin\HostX64\x64\cl.exe'
$env:CXX = $env:CC
$env:PATH = @(
    (Join-Path $vcRoot 'bin\HostX64\x64'),
    (Join-Path $sdkRoot "bin\$sdkVer\x64"),
    $rustBin,
    $cargoBin,
    $env:PATH
) -join ';'

# --- Rust / cargo ---
$env:CARGO_HOME = $cargoHome
if ($Online) {
    Remove-Item Env:CARGO_NET_OFFLINE -ErrorAction SilentlyContinue
    Write-Host 'online: cargo may refresh the crate index'
} else {
    $env:CARGO_NET_OFFLINE = 'true'
}
$env:CARGO_TERM_COLOR = 'never'
$env:LIBCLANG_PATH = $libclangPath

Write-Host "INCLUDE = $env:INCLUDE"
Write-Host "LIB     = $env:LIB"
Write-Host "CC      = $env:CC"

Push-Location $repoRoot
try {
    $previousEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    if ($Clean) {
        & (Join-Path $rustBin 'cargo.exe') clean -p alvr_filesystem -p alvr_server_openvr 2>&1 |
            Out-String | Write-Host
    }
    $cargoArgs = @('build', '--release', '-p', 'alvr_server_openvr', '--features', 'gpl')
    if (-not $Online) { $cargoArgs += '--offline' }
    & (Join-Path $rustBin 'cargo.exe') @cargoArgs 2>&1 |
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
