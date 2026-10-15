# Build the PointHand computer-use host (Rust).
# Copies release binaries to bin\windows\x64 and writes manifest.json with sha256 and size.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path -LiteralPath $cargo)) {
    throw "Rust cargo was not found at $cargo. Install rustup, then run this script again."
}

Push-Location (Join-Path $root "host")
try {
    & $cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
finally {
    Pop-Location
}

$out = Join-Path $root "bin\windows\x64"
New-Item -ItemType Directory -Force -Path $out | Out-Null
$hostExe = Join-Path $root "host\target\release\pointhand-computer-use.exe"
Copy-Item -LiteralPath $hostExe -Destination (Join-Path $out "pointhand-computer-use.exe") -Force

$manifest = @()
Get-ChildItem -LiteralPath $out -File | Where-Object { $_.Name -ne "manifest.json" } | ForEach-Object {
    $hash = Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256
    $manifest += [ordered]@{
        name = $_.Name
        sha256 = $hash.Hash.ToLowerInvariant()
        size = $_.Length
    }
}
$json = ConvertTo-Json -InputObject @($manifest)
[System.IO.File]::WriteAllText((Join-Path $out "manifest.json"), $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output "wrote $(Join-Path $out 'manifest.json')"
