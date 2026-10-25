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
    从同一份原生库打包出两个 only-freeDegree 变体 APK。

    华为 VR 框架按应用自己声明的 meta-data 决定要起哪一种会话：

        <meta-data android:name="com.huawei.vr.application.freeDegree" android:value="6dof" />

    6dof = 视觉定位套件 + 两只手柄，3dof = 眼镜 IMU + 小遥控器。runtime 只接受与当前会话
    相匹配的 interaction profile（同时建议两个会在 libvrsdk.so 里崩溃），而二进制 XML 里的
    "6dof" 与 "3dof" 长度一致，可以就地替换并保持 manifest 有效。客户端运行时会把自己声明
    的自由度读回来（alvr/client_openxr/src/huawei_session.rs），因此两个变体共用同一个 .so。

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

# 二进制 XML 的字符串池按 UTF-16LE 存放取值，等长替换不会移动任何偏移。
function Replace-Bytes {
    param([byte[]]$Source, [byte[]]$Pattern, [byte[]]$Replacement)

    if ($Pattern.Length -ne $Replacement.Length) { throw '替换串长度必须与被替换串一致' }

    $count = 0
    $limit = $Source.Length - $Pattern.Length
    for ($i = 0; $i -le $limit; $i++) {
        $matched = $true
        for ($j = 0; $j -lt $Pattern.Length; $j++) {
            if ($Source[$i + $j] -ne $Pattern[$j]) { $matched = $false; break }
        }
        if (-not $matched) { continue }
        for ($j = 0; $j -lt $Replacement.Length; $j++) { $Source[$i + $j] = $Replacement[$j] }
        $i += $Pattern.Length - 1
        $count++
    }

    return $count
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

# com.huawei.vr.application.freeDegree 的取值，按二进制 XML 里的 UTF-16LE 存储形式。
$sixDofBytes = [System.Text.Encoding]::Unicode.GetBytes('6dof')
$threeDofBytes = [System.Text.Encoding]::Unicode.GetBytes('3dof')

$stem = [System.IO.Path]::GetFileNameWithoutExtension($BaseApk) -replace '_(3|6)dof$', ''

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

foreach ($variant in @('6dof', '3dof')) {
    $unsignedApk = Join-Path $workRoot "$stem`_$variant`_unsigned.apk"
    $alignedApk = Join-Path $workRoot "$stem`_$variant`_aligned.apk"
    $signedApk = Join-Path $OutputDirectory "$stem`_$variant.apk"

    Write-Host "生成 $variant 变体：$unsignedApk"

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
                        try {
                            if ($entry.FullName -eq $manifestEntryName -and $variant -eq '3dof') {
                                $memory = New-Object System.IO.MemoryStream
                                try {
                                    $readStream.CopyTo($memory)
                                    $manifest = $memory.ToArray()
                                    $patched = Replace-Bytes $manifest $sixDofBytes $threeDofBytes
                                    if ($patched -eq 0) { throw 'manifest 中找不到 6dof 取值' }
                                    Write-Host "  manifest 中替换 $patched 处 6dof -> 3dof"
                                    $writeStream.Write($manifest, 0, $manifest.Length)
                                }
                                finally { $memory.Dispose() }
                            }
                            else {
                                $readStream.CopyTo($writeStream)
                            }
                        }
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

    Write-Host "对齐：$alignedApk"
    & $zipalign -p -f 4 $unsignedApk $alignedApk

    Write-Host "签名：$signedApk"
    & $apksigner sign --ks $Keystore --ks-key-alias $KeystoreAlias `
        --ks-pass "pass:$KeystorePassword" --key-pass "pass:$KeystorePassword" `
        --out $signedApk $alignedApk

    & $apksigner verify $signedApk
}

Write-Host "完成：$OutputDirectory"
Get-ChildItem -LiteralPath $OutputDirectory -Filter "$stem*.apk"
