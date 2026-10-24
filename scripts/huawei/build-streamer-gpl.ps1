#requires -Version 5.1
# Build ALVR streamer for Windows with the GPL (FFmpeg/x264) software encoder enabled.
# Output: build/alvr_streamer_windows (driver DLL + bundled FFmpeg DLLs)

[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
$repoRoot = 'F:\PY\HuaweiVRGlass-ALVR'
$vsDevCmd = 'F:\VS2022\BuildTools\Common7\Tools\VsDevCmd.bat'
$libclangPath = 'F:\LLVM\bin'
$cargoBin = 'D:\PY\1\toolchain-setup\rust-toolchain\bin'
$ffmpegDir = Join-Path $repoRoot 'deps\windows\ffmpeg'
$logPath = Join-Path $repoRoot 'build\gpl-build.log'

foreach ($p in @($vsDevCmd, (Join-Path $libclangPath 'libclang.dll'), (Join-Path $cargoBin 'cargo.exe'))) {
    if (-not (Test-Path -LiteralPath $p)) { throw "missing prerequisite: $p" }
}
foreach ($p in @((Join-Path $ffmpegDir 'include\libavcodec\avcodec.h'), (Join-Path $ffmpegDir 'lib\avcodec.lib'))) {
    if (-not (Test-Path -LiteralPath $p)) { throw "FFmpeg dev files not found: $p" }
}

# Import the MSVC environment (cl.exe, INCLUDE, LIB, PATH)
$vsEnvironment = & $env:ComSpec /d /s /c "`"$vsDevCmd`" -no_logo -arch=x64 -host_arch=x64 && set"
foreach ($line in $vsEnvironment) {
    if ($line -match '^([^=]+)=(.*)$') {
        [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
    }
}
$env:LIBCLANG_PATH = $libclangPath
$env:PATH = "$cargoBin;$env:PATH"
$env:CARGO_TERM_COLOR = 'never'

# Windows SDK is installed separately; make sure its include/lib dirs are present
$winSdkRoot = 'C:\Program Files (x86)\Windows Kits\10'
if (Test-Path -LiteralPath (Join-Path $winSdkRoot 'Include')) {
    $winSdkVersion = Get-ChildItem -LiteralPath (Join-Path $winSdkRoot 'Include') -Directory |
        Where-Object { $_.Name -match '^\d' } |
        Sort-Object Name -Descending |
        Select-Object -First 1 -ExpandProperty Name
    if ($env:INCLUDE -notlike "*Windows Kits*") {
        $sdkInclude = @('ucrt', 'um', 'shared', 'winrt', 'cppwinrt') | ForEach-Object {
            Join-Path (Join-Path (Join-Path $winSdkRoot 'Include') $winSdkVersion) $_
        }
        $env:INCLUDE = (@($env:INCLUDE) + $sdkInclude) -join ';'
    }
    if ($env:LIB -notlike "*Windows Kits*") {
        $sdkLib = @('ucrt\x64', 'um\x64') | ForEach-Object {
            Join-Path (Join-Path (Join-Path $winSdkRoot 'Lib') $winSdkVersion) $_
        }
        $env:LIB = (@($env:LIB) + $sdkLib) -join ';'
    }
}

Push-Location $repoRoot
try {
    $arguments = @('xtask', 'build-streamer')
    if ($Configuration -eq 'Release') { $arguments += '--release' }
    $arguments += @('--gpl', '--keep-config')
    Write-Host ("cargo " + ($arguments -join ' '))
    & cargo @arguments 2>&1 | Tee-Object -FilePath $logPath
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
