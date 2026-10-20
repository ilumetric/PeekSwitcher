# Cuts a release: bumps the workspace version, commits, tags vX.Y.Z and pushes.
# The pushed tag triggers .github/workflows/release.yml, which builds and publishes the zip.
# Run from the repository root:  powershell -File tools/release.ps1 0.2.0 [-NoPush]
param(
    [Parameter(Mandatory)][string]$Version,
    [switch]$NoPush
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') { throw "Version must look like 1.2.3 or 1.2.3-beta.1" }
$tag = "v$Version"
if (git status --porcelain) { throw "Working tree is not clean" }
if ((git branch --show-current) -ne "master") { throw "Releases are cut from master" }
if (git tag --list $tag) { throw "Tag $tag already exists" }

# Only the [workspace.package] version: the first top-level `version = ...` line.
$toml = Get-Content Cargo.toml -Raw
$re = [regex]'(?m)^version\s*=\s*"[^"]+"'
if (-not $re.IsMatch($toml)) { throw "No version line in Cargo.toml" }
$toml = $re.Replace($toml, "version = `"$Version`"", 1)
[IO.File]::WriteAllText((Join-Path $root "Cargo.toml"), $toml)

# Refresh Cargo.lock for the workspace crates only, then make sure it still builds.
cargo update --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --workspace --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

git add Cargo.toml Cargo.lock
git commit -m "Release $tag"
git tag -a $tag -m "PeekSwitcher $Version"

if ($NoPush) {
    "Tagged $tag. Publish with:  git push --follow-tags"
} else {
    git push --follow-tags origin master
    "Pushed ${tag}: https://github.com/ilumetric/PeekSwitcher/actions"
}
