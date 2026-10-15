param([string]$Archive = "")
$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $Archive) {
    $Archive = (Get-ChildItem -LiteralPath (Join-Path $projectRoot "artifacts") -Filter "*.zip" -File |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
}
if (-not $Archive) { throw "No ZIP found; run package-windows.ps1 first." }
$Archive = (Resolve-Path -LiteralPath $Archive).Path
$expected = (Get-Content -LiteralPath "$Archive.sha256" -Raw).Trim().Split(' ')[0]
$actual = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($expected -ne $actual) { throw "ZIP SHA-256 mismatch" }
$extractRoot = Join-Path $projectRoot (".scratch/verify-package-" + [guid]::NewGuid().ToString("N"))
Expand-Archive -LiteralPath $Archive -DestinationPath $extractRoot
$roots = @(Get-ChildItem -LiteralPath $extractRoot -Directory)
if ($roots.Count -ne 1) { throw "ZIP must have one package root" }
& node (Join-Path $PSScriptRoot "verify-portable.mjs") $roots[0].FullName
if ($LASTEXITCODE -ne 0) { throw "Extracted package startup failed" }
Write-Output "ZIP hash, binary manifest and isolated MCP startup verified."
