# Install the pinned Bern CLI with Enji's Windows UI lifetime fixes.
$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$manifest = Get-Content (Join-Path $project 'Cargo.toml') -Raw
$pin = [regex]::Match($manifest, '(?m)^bern = \{ git = "([^"]+)", rev = "([0-9a-f]{40})"')
if (!$pin.Success) { throw 'Could not find the pinned Bern revision in Cargo.toml.' }
$repository = $pin.Groups[1].Value
$revision = $pin.Groups[2].Value
$patch = Join-Path $PSScriptRoot 'bern-windows-lifetime.patch'
$patchId = (Get-FileHash -LiteralPath $patch).Hash.Substring(0, 12)
$source = Join-Path $project "target/bern-toolchain/$revision-$patchId"

if (!(Test-Path "$source/.git")) {
    git init --quiet $source
    if ($LASTEXITCODE) { throw 'Could not initialize the Bern build directory.' }
}
git -C $source rev-parse --verify --quiet "$revision^{commit}" | Out-Null
if ($LASTEXITCODE) {
    git -C $source fetch --depth 1 $repository $revision
    if ($LASTEXITCODE) { throw 'Could not fetch the pinned Bern revision.' }
}
git -C $source checkout --quiet --detach $revision
if ($LASTEXITCODE) { throw 'Could not check out the pinned Bern revision.' }
git -C $source apply --reverse --check --quiet $patch
if ($LASTEXITCODE) {
    git -C $source apply $patch
    if ($LASTEXITCODE) { throw 'Could not apply the Windows UI lifetime patch.' }
}
cargo install --locked --force --path (Join-Path $source 'crates/bern-cli')
if ($LASTEXITCODE) { throw 'Could not install the patched Bern CLI.' }
