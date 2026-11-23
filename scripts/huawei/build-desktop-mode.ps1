#requires -Version 5.1
<#
Build the vendored Huawei desktop-mode renderer (deps/huawei-desktop) with
MinGW-w64. The renderer is a standalone C++ exe: DXGI desktop duplication ->
D3D11 curved-screen rendering -> fullscreen swapchain on the glasses monitor.

Everything is derived from the repo / toolchain root - no hardcoded drive or
MinGW path. Override with -ToolsRoot / -OutDir when needed.

Usage:
    .\build-desktop-mode.ps1
    .\build-desktop-mode.ps1 -OutDir D:\some\deploy\desktop-mode
#>
[CmdletBinding()]
param(
    [string]$ToolsRoot = '',
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

. (Join-Path $PSScriptRoot 'toolchain-env.ps1')
$tcArgs = @{}
if ($ToolsRoot) { $tcArgs['ToolsRoot'] = $ToolsRoot }
$tc = Get-VrToolchain @tcArgs
$root = $tc.ToolsRoot
if (-not $root) { throw 'No toolchain root found; pass -ToolsRoot or set TOOLS_ROOT' }

$gppCandidates = @(
    (Join-Path $root 'mingw64\bin\g++.exe'),
    (Join-Path $root 'mingw\bin\g++.exe')
)
$gpp = $gppCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $gpp) { throw "MinGW g++.exe not found under: $root" }

$srcDir = Join-Path $repoRoot 'deps\huawei-desktop\src'
if (-not (Test-Path -LiteralPath $srcDir -PathType Container)) {
    throw "desktop-mode sources not found: $srcDir"
}
if (-not $OutDir) { $OutDir = Join-Path $repoRoot 'build\end\server\desktop-mode' }
if (-not (Test-Path -LiteralPath $OutDir -PathType Container)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

$exe = Join-Path $OutDir 'HwVRDesktop.exe'
$sources = @(Get-ChildItem -LiteralPath $srcDir -Filter '*.cpp' | ForEach-Object { $_.FullName })

Write-Host "toolchain: $root"
Write-Host "g++      : $gpp"
Write-Host "sources  : $($sources.Count) cpp files"
Write-Host "output   : $exe"

# _WIN32_WINNT=0x0A00 is required: MinGW 8.1 guards QDC_ALL_PATHS,
# GetDisplayConfigBufferSizes, GetTickCount64 and MOD_NOREPEAT behind the
# Vista+ API level. Static linking keeps the exe dependency-free.
&$gpp -O2 -s -static -municode -mwindows -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00 `
    -o $exe $sources `
    "-I$srcDir" `
    -ld3d11 -ldxgi -ld3dcompiler -lwindowscodecs -lole32 -luuid -lsetupapi -lhid `
    -lshell32 -ladvapi32 -lcomctl32 -lwbemuuid -loleaut32 -lpdh `
    -static-libgcc -static-libstdc++ 2>&1 | Out-String -Width 200 | Write-Host

if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "desktop-mode build failed (exit=$LASTEXITCODE)"
}

# Ship the default config next to the exe so it works out of the box.
$ini = Join-Path $OutDir 'desktop_mode.ini'
if (-not (Test-Path -LiteralPath $ini -PathType Leaf)) {
    @'
[general]
enabled=0

[display]
monitor=0
distance=2.0
scale=4.29
curvature=0.6
opacity=1.0
tilt=0.0
ipd=40
fov_tan_x=1.0
fov_tan_y=1.0
# 画面方向（反了就调这几个，渲染器每 0.6s 自动重载）
flip180=1
eye_rot_left=270
eye_rot_right=90
roll_left=0
roll_right=0

[mode]
follow=1
deadzone=8
tau=0.6

[background]
mode=0

[s3d]
mode=0
swap_eyes=0
parallax=0

[imu]
enabled=1
source=1

[advanced]
vsync=1
exclusive=0
'@ | Set-Content -LiteralPath $ini -Encoding Ascii
}

$item = Get-Item -LiteralPath $exe
Write-Host ("BUILD_OK: {0} ({1} bytes)" -f $item.FullName, $item.Length)
