#requires -Version 5.1
<#
    toolchain-env.ps1 - single source of truth for dev-toolchain locations.

    The dev environment lives in one folder (default: D:\Tools) and can be
    mirrored / moved to another drive, so nothing here hardcodes a drive
    letter that cannot be overridden.

    Usage from any build script under scripts\huawei:

        . (Join-Path $PSScriptRoot 'toolchain-env.ps1')
        $tc = Get-VrToolchain -Validate
        $tc.Apply()

    Override anything, in this priority order (low -> high):
        1. scripts\huawei\toolchain-paths.json   (optional, checked in)
        2. environment variables
        3. explicit parameters

        $env:TOOLS_ROOT        -> root of the dev environment
        $env:CARGO_HOME        -> cargo home
        $env:RUSTUP_HOME       -> rustup home
        $env:VC_TOOLS_ROOT     -> ...\VC\Tools\MSVC\<ver>
        $env:WIN_SDK_ROOT      -> ...\Windows Kits\10
        $env:LIBCLANG_PATH     -> folder holding libclang.dll
        $env:ANDROID_HOME      -> Android SDK root
        $env:ANDROID_NDK_HOME  -> NDK root
        $env:JAVA_HOME         -> JDK root
#>

# Preferred root first. F:\Tools is the full mirror of D:\Tools and is the one
# currently in use; the others are fallbacks after a drive change.
$script:VrDefaultToolsRoots = @('F:\Tools', 'D:\Tools', 'C:\Tools')

# Optional overrides file. Copy toolchain-paths.example.json and edit if the
# environment lives somewhere else.
$script:VrConfigFileName = 'toolchain-paths.json'

function Get-VrConfig {
    $path = Join-Path $PSScriptRoot $script:VrConfigFileName
    if (-not (Test-Path -LiteralPath $path)) { return @{} }
    try {
        $raw = Get-Content -LiteralPath $path -Raw -ErrorAction Stop
        $obj = $raw | ConvertFrom-Json -ErrorAction Stop
        $map = @{}
        foreach ($p in $obj.PSObject.Properties) {
            if ($null -ne $p.Value -and "$($p.Value)".Length -gt 0) { $map[$p.Name] = "$($p.Value)" }
        }
        return $map
    } catch {
        Write-Warning "toolchain-env: cannot parse $path ($($_.Exception.Message)); using defaults"
        return @{}
    }
}

function Get-VrFirstExisting {
    param([string[]]$Candidate)
    foreach ($c in $Candidate) {
        if ($c -and (Test-Path -LiteralPath $c)) { return $c }
    }
    return $null
}

# Newest child directory of $Parent (version folders sort correctly as strings
# for MSVC / Windows SDK / Android NDK naming).
function Get-VrLatestChild {
    param(
        [string]$Parent,
        [string]$Pattern = '*'
    )
    if (-not $Parent -or -not (Test-Path -LiteralPath $Parent)) { return $null }
    $children = @(Get-ChildItem -LiteralPath $Parent -Directory -Filter $Pattern -ErrorAction SilentlyContinue)
    if (-not $children.Count) { return $null }
    # Prefer real version folders (18.1.8, 14.44.35207, 10.0.26100.0) over
    # helper folders such as "clang-partial".
    $numeric = @($children | Where-Object { $_.Name -match '^\d' } | Sort-Object Name -Descending)
    if ($numeric.Count) { return $numeric[0].FullName }
    $rest = @($children | Sort-Object Name -Descending)
    return $rest[0].FullName
}

function Resolve-VrToolsRoot {
    param([string]$ToolsRoot)

    $cfg = Get-VrConfig

    $candidates = @()
    if ($ToolsRoot) { $candidates += $ToolsRoot }
    if ($cfg.ContainsKey('toolsRoot')) { $candidates += $cfg['toolsRoot'] }
    if ($env:TOOLS_ROOT) { $candidates += $env:TOOLS_ROOT }
    $candidates += $script:VrDefaultToolsRoots

    # A real dev environment root must contain the rust toolchain.
    foreach ($c in $candidates) {
        if ($c -and (Test-Path -LiteralPath (Join-Path $c 'rust'))) { return $c }
    }
    return $null
}

function Get-VrToolchain {
    <#
        Locate every toolchain path the build scripts need.
        -Validate        : throw when a required component is missing
        -RequireAndroid  : also validate the Android SDK / NDK / JDK
    #>
    [CmdletBinding()]
    param(
        [string]$ToolsRoot,
        [switch]$Validate,
        [switch]$RequireAndroid
    )

    $cfg = Get-VrConfig
    $root = Resolve-VrToolsRoot -ToolsRoot $ToolsRoot

    function cfg-or-env {
        param([string]$Key, [string]$EnvName)
        if ($cfg.ContainsKey($Key)) { return $cfg[$Key] }
        if ($EnvName) {
            # 某些机器上 Get-Item "Env:$name" 会抛出 ArgumentException，改用 .NET 接口
            $envValue = [System.Environment]::GetEnvironmentVariable($EnvName)
            if ($envValue) { return $envValue }
        }
        return $null
    }

    # --- Rust -----------------------------------------------------------
    $cargoHome = cfg-or-env 'cargoHome' 'CARGO_HOME'
    if (-not $cargoHome -and $root) {
        $cargoHome = Get-VrFirstExisting @((Join-Path $root 'rust\cargo'))
    }
    $rustupHome = cfg-or-env 'rustupHome' 'RUSTUP_HOME'
    if (-not $rustupHome -and $root) {
        $rustupHome = Get-VrFirstExisting @((Join-Path $root 'rust\rustup'))
    }
    $cargoBin = if ($cargoHome) { Join-Path $cargoHome 'bin' } else { $null }
    $rustBin = $null
    if ($rustupHome) {
        $tcRoot = Join-Path $rustupHome 'toolchains'
        if (Test-Path -LiteralPath $tcRoot) {
            foreach ($t in (Get-ChildItem -LiteralPath $tcRoot -Directory | Sort-Object Name -Descending)) {
                $b = Join-Path $t.FullName 'bin'
                if (Test-Path -LiteralPath (Join-Path $b 'cargo.exe')) { $rustBin = $b; break }
            }
        }
    }
    if (-not $rustBin -and $root) {
        $rustBin = Get-VrFirstExisting @(
            (Join-Path $root 'rust\rust-toolchain\bin'),
            (Join-Path $root 'rust\bin')
        )
    }
    if (-not $rustBin -and $cargoBin -and (Test-Path -LiteralPath (Join-Path $cargoBin 'rustc.exe'))) {
        $rustBin = $cargoBin
    }
    $cargoExe = Get-VrFirstExisting @(
        $(if ($rustBin) { Join-Path $rustBin 'cargo.exe' }),
        $(if ($cargoBin) { Join-Path $cargoBin 'cargo.exe' })
    )

    # --- MSVC / Windows SDK --------------------------------------------
    $vcRoot = cfg-or-env 'vcToolsRoot' 'VC_TOOLS_ROOT'
    if (-not $vcRoot -and $root) {
        $vcRoot = Get-VrLatestChild (Join-Path $root 'msvc\VC\Tools\MSVC')
    }
    if (-not $vcRoot) {
        $vcRoot = Get-VrLatestChild 'F:\VS2022\BuildTools\VC\Tools\MSVC'
    }
    $vcVersion = if ($vcRoot) { Split-Path $vcRoot -Leaf } else { $null }
    $vsAuxInclude = $null
    if ($vcRoot) {
        # ...\VC\Tools\MSVC\<ver>  ->  ...\VC\Auxiliary\VS\include
        $vcDir = Split-Path (Split-Path (Split-Path $vcRoot -Parent) -Parent) -Parent
        $vsAuxInclude = Get-VrFirstExisting @((Join-Path $vcDir 'Auxiliary\VS\include'))
    }

    $sdkRoot = cfg-or-env 'winSdkRoot' 'WIN_SDK_ROOT'
    if (-not $sdkRoot -and $root) {
        $sdkRoot = Get-VrFirstExisting @(
            (Join-Path $root 'Windows Kits\10'),
            (Join-Path $root 'msvc\Windows Kits\10')
        )
    }
    if (-not $sdkRoot) {
        $sdkRoot = Get-VrFirstExisting @('C:\Program Files (x86)\Windows Kits\10')
    }
    $sdkVersion = if ($sdkRoot) { Split-Path (Get-VrLatestChild (Join-Path $sdkRoot 'Include')) -Leaf } else { $null }

    # --- LLVM / libclang ------------------------------------------------
    $libclangPath = cfg-or-env 'libclangPath' 'LIBCLANG_PATH'
    if (-not $libclangPath -and $root) {
        $llvmRoot = Join-Path $root 'LLVM'
        $candidate = Get-VrLatestChild $llvmRoot
        if (-not $candidate) { $candidate = $llvmRoot }
        $libclangPath = Get-VrFirstExisting @(
            (Join-Path $candidate 'bin'),
            (Join-Path $llvmRoot 'bin')
        )
    }

    # --- Android --------------------------------------------------------
    $androidRoot = cfg-or-env 'androidHome' 'ANDROID_HOME'
    if (-not $androidRoot) { $androidRoot = cfg-or-env 'androidHome' 'ANDROID_SDK_ROOT' }
    if (-not $androidRoot) {
        $androidRoot = cfg-or-env 'androidHome' 'ANDROID_HOME'
    }
    if (-not $androidRoot -and $root) {
        $androidRoot = Get-VrFirstExisting @((Join-Path $root 'AndroidSDK'))
    }
    $ndkRoot = cfg-or-env 'androidNdkHome' 'ANDROID_NDK_HOME'
    if (-not $ndkRoot -and $androidRoot) {
        $ndkRoot = Get-VrLatestChild (Join-Path $androidRoot 'ndk')
    }
    $ndkVersion = if ($ndkRoot) { Split-Path $ndkRoot -Leaf } else { $null }
    $buildTools = if ($androidRoot) { Get-VrLatestChild (Join-Path $androidRoot 'build-tools') } else { $null }
    $platform = if ($androidRoot) { Get-VrLatestChild (Join-Path $androidRoot 'platforms') } else { $null }
    $adbExe = if ($androidRoot) { Get-VrFirstExisting @((Join-Path $androidRoot 'platform-tools\adb.exe')) } else { $null }
    $zipalignExe = if ($buildTools) { Get-VrFirstExisting @((Join-Path $buildTools 'zipalign.exe')) } else { $null }
    $apksigner = if ($buildTools) {
        Get-VrFirstExisting @((Join-Path $buildTools 'apksigner.bat'), (Join-Path $buildTools 'apksigner.jar'))
    } else { $null }

    # --- Java -----------------------------------------------------------
    $javaHome = cfg-or-env 'javaHome' 'JAVA_HOME'
    if (-not $javaHome) {
        $javaHome = Get-VrFirstExisting @(
            $(if ($root) { Get-VrLatestChild (Join-Path $root 'jdk') }),
            $(if ($root) { Get-VrLatestChild (Join-Path $root 'java') }),
            'F:\Download\jdk17\jdk-17.0.17+10'
        )
    }
    $javaExe = if ($javaHome) { Get-VrFirstExisting @((Join-Path $javaHome 'bin\java.exe')) } else { $null }

    # --- Derived MSVC env ----------------------------------------------
    $includeDirs = @()
    $libDirs = @()
    $clExe = $null
    $vcBinDir = $null
    if ($vcRoot) {
        $clExe = Get-VrFirstExisting @((Join-Path $vcRoot 'bin\HostX64\x64\cl.exe'))
        $vcBinDir = Split-Path $clExe -Parent
        $includeDirs += (Join-Path $vcRoot 'include')
        $libDirs += (Join-Path $vcRoot 'lib\x64')
    }
    if ($vsAuxInclude) { $includeDirs += $vsAuxInclude }
    if ($sdkRoot -and $sdkVersion) {
        $includeDirs += @(
            (Join-Path $sdkRoot "Include\$sdkVersion\ucrt"),
            (Join-Path $sdkRoot "Include\$sdkVersion\um"),
            (Join-Path $sdkRoot "Include\$sdkVersion\shared"),
            (Join-Path $sdkRoot "Include\$sdkVersion\winrt"),
            (Join-Path $sdkRoot "Include\$sdkVersion\cppwinrt")
        )
        $libDirs += @(
            (Join-Path $sdkRoot "Lib\$sdkVersion\ucrt\x64"),
            (Join-Path $sdkRoot "Lib\$sdkVersion\um\x64")
        )
    }
    $includeDirs = @($includeDirs | Where-Object { $_ -and (Test-Path -LiteralPath $_) })
    $libDirs = @($libDirs | Where-Object { $_ -and (Test-Path -LiteralPath $_) })

    $sdkBinDir = if ($sdkRoot -and $sdkVersion) { Join-Path $sdkRoot "bin\$sdkVersion\x64" } else { $null }

    $tc = [pscustomobject]@{
        ToolsRoot     = $root
        CargoHome     = $cargoHome
        RustupHome    = $rustupHome
        CargoBin      = $cargoBin
        RustBin       = $rustBin
        CargoExe      = $cargoExe
        VcRoot        = $vcRoot
        VcVersion     = $vcVersion
        VcBinDir      = $vcBinDir
        ClExe         = $clExe
        VsAuxInclude  = $vsAuxInclude
        SdkRoot       = $sdkRoot
        SdkVersion    = $sdkVersion
        SdkBinDir     = $sdkBinDir
        LibclangPath  = $libclangPath
        IncludeDirs   = $includeDirs
        LibDirs       = $libDirs
        AndroidRoot   = $androidRoot
        AndroidNdk    = $ndkRoot
        NdkVersion    = $ndkVersion
        BuildTools    = $buildTools
        AndroidPlatform = $platform
        AdbExe        = $adbExe
        ZipalignExe   = $zipalignExe
        Apksigner     = $apksigner
        JavaHome      = $javaHome
        JavaExe       = $javaExe
    }

    # --- Apply(): export the compiler environment for the current process --
    $tc | Add-Member -MemberType ScriptMethod -Name 'Apply' -Value {
        param([switch]$Offline)

        if ($this.CargoHome) { $env:CARGO_HOME = $this.CargoHome }
        if ($this.RustupHome) { $env:RUSTUP_HOME = $this.RustupHome }
        if ($this.LibclangPath) { $env:LIBCLANG_PATH = $this.LibclangPath }
        if ($this.IncludeDirs.Count) { $env:INCLUDE = ($this.IncludeDirs -join ';') }
        if ($this.LibDirs.Count) { $env:LIB = ($this.LibDirs -join ';') }
        if ($this.ClExe) { $env:CC = $this.ClExe; $env:CXX = $this.ClExe }

        $pathParts = @()
        if ($this.VcBinDir) { $pathParts += $this.VcBinDir }
        if ($this.SdkBinDir -and (Test-Path -LiteralPath $this.SdkBinDir)) { $pathParts += $this.SdkBinDir }
        if ($this.RustBin) { $pathParts += $this.RustBin }
        if ($this.CargoBin) { $pathParts += $this.CargoBin }
        if ($pathParts.Count) { $env:PATH = (($pathParts -join ';') + ';' + $env:PATH) }

        # Note: do not use Remove-Item on Env: - the safe-delete hook fails closed
        # when the variable is not set.
        if ($Offline) { $env:CARGO_NET_OFFLINE = 'true' }
        else { [Environment]::SetEnvironmentVariable('CARGO_NET_OFFLINE', $null, 'Process') }
        $env:CARGO_TERM_COLOR = 'never'
    }

    # --- Summary() -------------------------------------------------------
    $tc | Add-Member -MemberType ScriptMethod -Name 'Summary' -Value {
        $lines = @(
            "ToolsRoot   = $($this.ToolsRoot)",
            "cargo       = $($this.CargoExe)",
            "MSVC        = $($this.VcRoot)",
            "WinSDK      = $($this.SdkRoot) [$($this.SdkVersion)]",
            "libclang    = $($this.LibclangPath)",
            "Android SDK = $($this.AndroidRoot)",
            "NDK         = $($this.AndroidNdk)",
            "build-tools = $($this.BuildTools)",
            "platform    = $($this.AndroidPlatform)",
            "adb         = $($this.AdbExe)",
            "JAVA_HOME   = $($this.JavaHome)"
        )
        return ($lines -join [Environment]::NewLine)
    }

    if ($Validate) {
        $required = @{
            'cargo.exe'      = $tc.CargoExe
            'cl.exe'         = $tc.ClExe
            'Windows SDK'    = $tc.SdkRoot
            'libclang.dll'   = $(if ($tc.LibclangPath) { Join-Path $tc.LibclangPath 'libclang.dll' } else { $null })
        }
        foreach ($k in $required.Keys) {
            $v = $required[$k]
            if (-not $v -or -not (Test-Path -LiteralPath $v)) {
                throw "toolchain-env: missing $k (looked at '$v'). Set TOOLS_ROOT or edit $script:VrConfigFileName."
            }
        }
        if (-not $tc.IncludeDirs.Count) { throw 'toolchain-env: no MSVC/SDK include directories resolved' }
        if (-not $tc.LibDirs.Count) { throw 'toolchain-env: no MSVC/SDK library directories resolved' }
    }

    if ($RequireAndroid) {
        $androidRequired = @{
            'Android SDK'  = $tc.AndroidRoot
            'NDK'          = $tc.AndroidNdk
            'build-tools'  = $tc.BuildTools
            'platform'     = $tc.AndroidPlatform
            'adb'          = $tc.AdbExe
            'zipalign'     = $tc.ZipalignExe
            'apksigner'    = $tc.Apksigner
            'java'         = $tc.JavaExe
        }
        foreach ($k in $androidRequired.Keys) {
            $v = $androidRequired[$k]
            if (-not $v -or -not (Test-Path -LiteralPath $v)) {
                throw "toolchain-env: missing $k (looked at '$v'). Set ANDROID_HOME / JAVA_HOME or edit $script:VrConfigFileName."
            }
        }
    }

    return $tc
}
