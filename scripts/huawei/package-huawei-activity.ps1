[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$NativeLibrary,
    [Parameter(Mandatory)]
    [string]$LoaderLibrary,
    [Parameter(Mandatory)]
    [string]$BridgeJar,
    [Parameter(Mandatory)]
    [string]$OutputApk,
    [Parameter(Mandatory)]
    [string]$AndroidSdk,
    [Parameter(Mandatory)]
    [string]$JavaHome,
    [Parameter(Mandatory)]
    [string]$Keystore
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$clientRoot = Join-Path $repoRoot 'alvr\client_openxr'
$buildRoot = Join-Path $repoRoot 'build'
$workRoot = Join-Path $buildRoot 'huawei_activity_package'
$resolvedBuildRoot = [System.IO.Path]::GetFullPath($buildRoot).TrimEnd('\') + '\'
$resolvedWorkRoot = [System.IO.Path]::GetFullPath($workRoot).TrimEnd('\') + '\'

if (-not $resolvedWorkRoot.StartsWith($resolvedBuildRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "拒绝清理仓库 build 目录以外的路径：$resolvedWorkRoot"
}

foreach ($requiredFile in @($NativeLibrary, $LoaderLibrary, $BridgeJar, $Keystore)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "缺少 Huawei Activity 打包输入：$requiredFile"
    }
}

$buildTools = Join-Path $AndroidSdk 'build-tools\34.0.0'
$androidJar = Join-Path $AndroidSdk 'platforms\android-34\android.jar'
$manifest = Join-Path $clientRoot 'android_huawei\AndroidManifest.xml'
$javaSource = Join-Path $clientRoot 'android_huawei\src\com\lht\huaweivr\alvr\HuaweiVrActivity.java'
$resources = Join-Path $clientRoot 'resources'

if (Test-Path -LiteralPath $workRoot) {
    Remove-Item -LiteralPath $workRoot -Recurse -Force
}
$classesRoot = Join-Path $workRoot 'classes'
$dexRoot = Join-Path $workRoot 'dex'
$packageRoot = Join-Path $workRoot 'package'
$nativeRoot = Join-Path $packageRoot 'lib\armeabi-v7a'
New-Item -ItemType Directory -Path $classesRoot, $dexRoot, $nativeRoot -Force | Out-Null

& (Join-Path $JavaHome 'bin\javac.exe') `
    -encoding UTF-8 `
    -source 8 `
    -target 8 `
    -classpath "$androidJar;$BridgeJar" `
    -d $classesRoot `
    $javaSource

$activityJar = Join-Path $workRoot 'huawei-activity.jar'
& (Join-Path $JavaHome 'bin\jar.exe') cf $activityJar -C $classesRoot .
& (Join-Path $buildTools 'd8.bat') --min-api 26 --output $dexRoot $activityJar $BridgeJar

$unsignedApk = Join-Path $workRoot 'alvr-huawei-activity-unsigned.apk'
$alignedApk = Join-Path $workRoot 'alvr-huawei-activity-aligned.apk'
& (Join-Path $buildTools 'aapt.exe') package `
    -f `
    -M $manifest `
    -S $resources `
    -I $androidJar `
    -F $unsignedApk

Copy-Item -LiteralPath $NativeLibrary -Destination (Join-Path $nativeRoot 'libalvr_client_openxr.so')
Copy-Item -LiteralPath $LoaderLibrary -Destination (Join-Path $nativeRoot 'libxr_loader.so')
& (Join-Path $JavaHome 'bin\jar.exe') uf $unsignedApk -C $dexRoot classes.dex -C $packageRoot lib
& (Join-Path $buildTools 'zipalign.exe') -f 4 $unsignedApk $alignedApk
& (Join-Path $buildTools 'apksigner.bat') sign `
    --ks $Keystore `
    --ks-key-alias androiddebugkey `
    --ks-pass pass:alvrclient `
    --key-pass pass:alvrclient `
    $alignedApk

$outputDirectory = Split-Path -Parent $OutputApk
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
Copy-Item -LiteralPath $alignedApk -Destination $OutputApk -Force

& (Join-Path $buildTools 'zipalign.exe') -c -v 4 $OutputApk
& (Join-Path $buildTools 'apksigner.bat') verify --verbose $OutputApk
