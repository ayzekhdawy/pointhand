# Portable MCP distribution. Does not include development logs or user media originals.
param([string]$OutputDirectory = "")
$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $projectRoot "artifacts" }
$packageRoot = Join-Path $OutputDirectory ("pointhand-windows-x64-" + [guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
foreach ($entry in @("client/dist", "bin/windows/x64/pointhand-computer-use.exe", "bin/windows/x64/manifest.json")) {
    if (-not (Test-Path -LiteralPath (Join-Path $projectRoot $entry))) { throw "Build first: missing $entry" }
}
$binaryBytes = [IO.File]::ReadAllBytes((Join-Path $projectRoot "bin/windows/x64/pointhand-computer-use.exe"))
foreach ($encoding in @([Text.Encoding]::UTF8, [Text.Encoding]::Unicode)) {
    $binaryText = $encoding.GetString($binaryBytes)
    foreach ($privateRoot in @($env:USERPROFILE, $env:USERPROFILE.Replace('\', '/'))) {
        if ($privateRoot -and $binaryText.Contains($privateRoot)) {
            throw "Host contains a private builder path. Rebuild with scripts/build-windows.ps1."
        }
    }
}
New-Item -ItemType Directory -Path (Join-Path $packageRoot "client") -Force | Out-Null
foreach ($entry in @("package.json", "package-lock.json", "dist")) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "client/$entry") -Destination (Join-Path $packageRoot "client") -Recurse
}
foreach ($entry in @("bin", "docs", "examples", "README.md", "LICENSE", "SECURITY.md", "CONTRIBUTING.md", "CHANGELOG.md")) {
    Copy-Item -LiteralPath (Join-Path $projectRoot $entry) -Destination $packageRoot -Recurse
}
Push-Location (Join-Path $packageRoot "client")
try {
    & npm.cmd ci --omit=dev --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "Production dependency installation failed" }
} finally { Pop-Location }
New-Item -ItemType Directory -Path (Join-Path $packageRoot "scripts") -Force | Out-Null
foreach ($entry in @("paint-image.mjs", "image-stroke-plan.py")) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $entry) -Destination (Join-Path $packageRoot "scripts")
}
New-Item -ItemType Directory -Path (Join-Path $packageRoot "scripts/lib") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "lib/paint-ui.mjs") -Destination (Join-Path $packageRoot "scripts/lib")
$publicAssets = Join-Path $packageRoot "assets"
New-Item -ItemType Directory -Path $publicAssets -Force | Out-Null
foreach ($entry in @("logo.png", "logo-black.png", "logo-sheet.png", "glass-preview.png")) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "assets/$entry") -Destination $publicAssets
}
$zipPath = "$packageRoot.zip"
Compress-Archive -LiteralPath $packageRoot -DestinationPath $zipPath
$hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$zipPath.sha256", "$hash  $([IO.Path]::GetFileName($zipPath))`n")
Write-Output $zipPath
