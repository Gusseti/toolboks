# Cut a release: bump version, commit, tag and push. GitHub Actions does the rest.
#   .\scripts\release.ps1 0.2.0          stable release (run from main)
#   .\scripts\release.ps1 0.2.0-rc.1     pre-release   (run from staging)
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$')][string]$Version
)
$ErrorActionPreference = 'Stop'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Set-Location (Split-Path $PSScriptRoot)

$branch = git branch --show-current
$expected = if ($Version -match '-') { 'staging' } else { 'main' }
if ($branch -ne $expected) { throw "Release $Version must be cut from '$expected' (on '$branch')" }
if (git status --porcelain) { throw 'Working tree is not clean' }
if (git tag -l "v$Version") { throw "Tag v$Version already exists" }

$toml = Get-Content Cargo.toml -Raw
$toml = $toml -replace '(?m)^version\s*=\s*".+"', "version = `"$Version`""
Set-Content Cargo.toml $toml -NoNewline
cargo update --workspace --quiet

git add Cargo.toml Cargo.lock
git commit -m "chore: release v$Version"
git tag -a "v$Version" -m "v$Version"
git push origin $branch "v$Version"
Write-Host "Pushed v$Version - watch the Release workflow on GitHub."
