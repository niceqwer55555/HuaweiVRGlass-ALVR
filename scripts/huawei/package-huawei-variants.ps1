[CmdletBinding()]
param(
    [string]$NativeLibrary = (Join-Path $PSScriptRoot '..\..\target\armv7-linux-androideabi\release\libalvr_client_openxr.so'),
    [string]$BaseApk = (Join-Path $PSScriptRoot '..\..\build\HuaweiVRGlass_ALVR_Android_v0.5.0_6dof.apk'),
    [string]$OutputDirectory,
    [string]$AndroidSdkRoot,
    [string]$JavaHome,
    [string]$Keystore,
    [string]$KeystoreAlias = 'androiddebugkey',
    [string]$KeystorePassword = 'alvrclient',
    [string]$ToolsRoot
)

<#
    从同一份原生库打包出双自由度（3dof|6dof）APK。

    华为 VR 框架按应用自己声明的 meta-data 决定要起哪一种会话：

        <meta-data android:name="com.huawei.vr.application.freeDegree" android:value="3dof|6dof" />

    "6dof" = 视觉定位套件 + 两只手柄，"3dof" = 眼镜 IMU + 小遥控器，"3dof|6dof" = 两种都允许，
    由 runtime 按实际接入的手柄放行（官方文档：值为 "3dof|6dof" 时应用同时支持两种手柄）。
    客户端运行时会把自己声明的自由度读回来（alvr/client_openxr/src/huawei_session.rs），
    因此一个 APK 即可覆盖两种会话；manifest 的取值由 patch_apk_degree.py 改写（字符串池会重建，
    不能像过去那样等长覆盖）。

    用法：
        .\package-huawei-variants.ps1 -NativeLibrary target\armv7-linux-androideabi\release\libalvr_client_openxr.so
#>

$ErrorActionPreference = 'Stop'

function Resolve-FirstExistingPath {
    param([string[]]$Candidates)
    foreach ($candidate in $Candidates) {
        if (-not [string]::IsNullOrWhiteSpace($candidate) -and (Test-Path -LiteralPath $candidate -PathType Container)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    return $null
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

# 工具链位置统一由 toolchain-env.ps1 运行时探测（默认根目录 D:\Tools）。
. (Join-Path $PSScriptRoot 'toolchain-env.ps1')
if ($AndroidSdkRoot) { $env:ANDROID_HOME = $AndroidSdkRoot }
if ($JavaHome) { $env:JAVA_HOME = $JavaHome }
$tc = Get-VrToolchain -ToolsRoot $ToolsRoot -RequireAndroid
$androidSdk = $tc.AndroidRoot
$javaHome = $tc.JavaHome
# apksigner.bat / zipalign 依赖 JAVA_HOME，必须在调用前注入。
$env:JAVA_HOME = $javaHome
$env:Path = "$(Join-Path $javaHome 'bin');$env:Path"

$buildToolsVersion = $tc.BuildTools
$zipalign = $tc.ZipalignExe
$apksigner = $tc.Apksigner
Write-Host "Android SDK：$androidSdk"
Write-Host "build-tools：$buildToolsVersion"
Write-Host "JDK：$javaHome"

foreach ($requiredFile in @($NativeLibrary, $BaseApk)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) { throw "缺少打包输入：$requiredFile" }
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot 'build'
}
if (-not (Test-Path -LiteralPath $OutputDirectory)) {
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
}

if ([string]::IsNullOrWhiteSpace($Keystore)) {
    $Keystore = Join-Path $repoRoot 'build\alvr_client_android\debug.keystore'
}
if (-not (Test-Path -LiteralPath $Keystore -PathType Leaf)) {
    throw "找不到签名：$Keystore（可用 -Keystore / -KeystorePassword 指定）"
}

$nativeEntryName = 'lib/armeabi-v7a/libalvr_client_openxr.so'
$manifestEntryName = 'AndroidManifest.xml'
$workRoot = Join-Path $repoRoot 'build\huawei_variants'
if (-not (Test-Path -LiteralPath $workRoot)) {
    New-Item -ItemType Directory -Path $workRoot -Force | Out-Null
}

# 双自由度标记：runtime 按实际接入的手柄放行 3DoF / 6DoF，应用只需一个包。
$dualDegree = '3dof|6dof'

$stem = [System.IO.Path]::GetFileNameWithoutExtension($BaseApk) -replace '_(3dof6dof|3dof|6dof)$', ''

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$variant = '3dof6dof'
$unsignedApk = Join-Path $workRoot "$stem`_$variant`_stripped.apk"
$dualApk = Join-Path $workRoot "$stem`_$variant`_unsigned.apk"
$alignedApk = Join-Path $workRoot "$stem`_$variant`_aligned.apk"
$signedApk = Join-Path $OutputDirectory "$stem`_$variant.apk"

Write-Host "重打包（换 .so、丢弃旧签名）：$unsignedApk"

$source = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $BaseApk).Path)
try {
    # File::Create truncates, so a leftover file from a previous run is fine.
    $stream = [System.IO.File]::Create($unsignedApk)
    try {
        $target = New-Object System.IO.Compression.ZipArchive($stream, [System.IO.Compression.ZipArchiveMode]::Create)
        try {
            $nativeWritten = $false

            foreach ($entry in $source.Entries) {
                # 旧签名在重打包后失效，必须丢弃再签。
                if ($entry.FullName.StartsWith('META-INF/', [System.StringComparison]::OrdinalIgnoreCase)) { continue }

                if ($entry.FullName -eq $nativeEntryName) {
                    $created = $target.CreateEntry($nativeEntryName, [System.IO.Compression.CompressionLevel]::NoCompression)
                    $writeStream = $created.Open()
                    try {
                        $inputStream = [System.IO.File]::OpenRead((Resolve-Path -LiteralPath $NativeLibrary).Path)
                        try { $inputStream.CopyTo($writeStream) }
                        finally { $inputStream.Dispose() }
                    }
                    finally { $writeStream.Dispose() }
                    $nativeWritten = $true
                    continue
                }

                $created = $target.CreateEntry($entry.FullName, [System.IO.Compression.CompressionLevel]::NoCompression)
                $writeStream = $created.Open()
                try {
                    $readStream = $entry.Open()
                    try { $readStream.CopyTo($writeStream) }
                    finally { $readStream.Dispose() }
                }
                finally { $writeStream.Dispose() }
            }

            if (-not $nativeWritten) { throw "基础 APK 中没有 $nativeEntryName" }
        }
        finally { $target.Dispose() }
    }
    finally { $stream.Dispose() }
}
finally { $source.Dispose() }

# manifest 的 freeDegree 要改成 "3dof|6dof"：字符串变长，必须重建二进制 XML 字符串池，
# 不能再用等长字节覆盖（"3dof|6dof" 里本身就含有 "6dof" 子串）。
$python = (Get-Command python -ErrorAction SilentlyContinue)
if (-not $python) { throw 'Need python on PATH to rewrite AndroidManifest.xml (freeDegree)' }
$patcher = Join-Path $PSScriptRoot 'patch_apk_degree.py'
Write-Host "改写 freeDegree -> $dualDegree"
& $python.Source $patcher $unsignedApk $dualApk $dualDegree
if ($LASTEXITCODE -ne 0) { throw 'patch_apk_degree.py failed' }

Write-Host "对齐：$alignedApk"
& $zipalign -p -f 4 $dualApk $alignedApk
if ($LASTEXITCODE -ne 0) { throw 'zipalign failed' }

Write-Host "签名：$signedApk"
& $apksigner sign --ks $Keystore --ks-key-alias $KeystoreAlias `
    --ks-pass "pass:$KeystorePassword" --key-pass "pass:$KeystorePassword" `
    --out $signedApk $alignedApk
if ($LASTEXITCODE -ne 0) { throw 'apksigner sign failed' }

& $apksigner verify $signedApk
if ($LASTEXITCODE -ne 0) { throw 'apksigner verify failed' }

Write-Host "完成：$OutputDirectory"
Get-ChildItem -LiteralPath $OutputDirectory -Filter "$stem*.apk"
