# Builds the release (full optimization) and assembles a portable folder:
#   dist/PekSwitcher/PekSwitcher.exe + resources/ (dictionaries, sounds)
# Settings are created next to the exe in data/ on first run.
# Run from the repository root:  powershell -File tools/package.ps1
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
