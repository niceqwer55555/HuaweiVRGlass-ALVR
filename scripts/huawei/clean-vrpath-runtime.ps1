param([switch]$DryRun)

$vrpathFile = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'
Copy-Item $vrpathFile "$vrpathFile.bak2.$(Get-Date -Format yyyyMMdd_HHmmss)" -Force

$lines = Get-Content -LiteralPath $vrpathFile
$out = New-Object System.Collections.ArrayList
$kept = New-Object System.Collections.ArrayList
$inRuntime = $false

foreach ($line in $lines) {
    if ($line -match '"runtime"\s*:') {
        $inRuntime = $true
        [void]$out.Add($line)
        continue
    }
    if ($inRuntime -and ($line.Trim() -eq '[')) {
        [void]$out.Add($line)
        continue
    }
    if ($inRuntime -and ($line -match '^\s*\],?\s*$')) {
        for ($i = 0; $i -lt $kept.Count; $i++) {
            $l = $kept[$i]
            if ($l -match ',\s*$') { $l = $l -replace ',\s*$', '' }
            if ($i -lt ($kept.Count - 1)) { $l = $l + ',' }
            [void]$out.Add($l)
        }
        $inRuntime = $false
        [void]$out.Add($line)
        continue
    }
    if ($inRuntime) {
        $m = [regex]::Match($line.Trim(), '^"(.*?)"(,?)$')
        if ($m.Success) {
            $p = ($m.Groups[1].Value) -replace '\\\\', '\'
            if (Test-Path -LiteralPath $p) {
                $escaped = $p.Replace('\', '\\')
                [void]$kept.Add('"' + $escaped + '"')
            } else {
                Write-Host "dropped dead runtime: $p"
            }
        } else {
            [void]$kept.Add($line.Trim())
        }
        continue
    }
    [void]$out.Add($line)
}

if ($DryRun) { Write-Host "dry run, nothing written"; exit 0 }

Set-Content -LiteralPath $vrpathFile -Value ($out -join "`r`n") -NoNewline -Encoding UTF8

$json = (Get-Content -LiteralPath $vrpathFile -Raw) | ConvertFrom-Json
Write-Host "remaining runtime:"
foreach ($r in $json.runtime) { Write-Host ("  {0,-6} {1}" -f (Test-Path -LiteralPath $r), $r) }
Write-Host "drivers:"
foreach ($d in $json.external_drivers) { Write-Host ("  {0,-6} {1}" -f (Test-Path -LiteralPath $d), $d) }
Write-Host "json parse: OK"
