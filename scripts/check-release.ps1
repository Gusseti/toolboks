param(
    [Parameter(Mandatory)][ValidateSet('branch', 'tag')][string]$RefType,
    [Parameter(Mandatory)][string]$RefName,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$SourceSha
)
$ErrorActionPreference = 'Stop'
$toml = git show "${SourceSha}:Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw 'Cannot read the release commit' }
$match = [regex]::Match(($toml -join "`n"), '(?m)^version\s*=\s*"([^"]+)"')
$version = $match.Groups[1].Value
if (!$match.Success -or $version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$') {
    throw 'The release version is invalid'
}
$tag = "v$version"
if ($RefType -eq 'tag') {
    if ($RefName -cne $tag) { throw 'The tag does not match Cargo.toml' }
} elseif ($RefName -cne 'main' -or $version.Contains('-')) {
    throw 'Only main may publish a stable branch release'
}
$branch = if ($version.Contains('-')) { 'origin/staging' } else { 'origin/main' }
git merge-base --is-ancestor $SourceSha $branch
if ($LASTEXITCODE -ne 0) { throw "The release commit must belong to $branch" }
$tag
