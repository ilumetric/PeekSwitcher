# Builds the release (full optimization) and assembles a portable folder:
#   dist/PekSwitcher/PekSwitcher.exe + resources/ (dictionaries, sounds)
# Settings are created next to the exe in data/ on first run.
# With -Zip also writes dist/PekSwitcher-<version>-windows-x64.zip and its .sha256
# (what the release workflow uploads).
# Run from the repository root:  powershell -File tools/package.ps1 [-Zip]
param([switch]$Zip)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

cargo build --profile dist -p pekswitcher
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$out = Join-Path $root "dist\PekSwitcher"
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item "target\dist\PekSwitcher.exe" $out -Force
# Resources are replaced as a whole; data/ (the user's settings) is left alone.
$res = Join-Path $out "resources"
if (Test-Path $res) { Remove-Item $res -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $res "dict"), (Join-Path $res "sounds") | Out-Null
Copy-Item "resources\dict\*.txt" (Join-Path $res "dict")
Copy-Item "resources\sounds\*.wav" (Join-Path $res "sounds")

$size = (Get-ChildItem $out -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
"Packaged {0} ({1:N1} MB)" -f $out, $size

if ($Zip) {
    $version = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
    $zipPath = Join-Path $root "dist\PekSwitcher-$version-windows-x64.zip"
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
    # Only the exe and resources: a local data/ must never end up in a release.
    $stage = Join-Path $root "dist\stage"
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory -Force (Join-Path $stage "PekSwitcher") | Out-Null
    Copy-Item (Join-Path $out "PekSwitcher.exe"), $res (Join-Path $stage "PekSwitcher") -Recurse
    Compress-Archive -Path (Join-Path $stage "PekSwitcher") -DestinationPath $zipPath
    Remove-Item $stage -Recurse -Force
    $hash = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()
    "$hash  $(Split-Path -Leaf $zipPath)" | Out-File "$zipPath.sha256" -Encoding ascii -NoNewline
    "Zipped {0}" -f $zipPath
}
