# Run against the desktop layout of the release build, with Search enabled.
[CmdletBinding()]
param([ValidateRange(25, 1000)][int]$Rounds = 100)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$output = Join-Path $root 'target/bern/windows'
$player = (Get-Item (Join-Path $output 'Enji/enji.exe')).FullName
$cli = (Get-Item (Join-Path $output 'bin/enji.exe')).FullName
$process = Get-Process enji | Where-Object Path -eq $player
if (!$process -or @($process).Count -ne 1) { throw "Open the release build at $player first." }

function Get-Page {
    $controls = & $cli ui inspect
    if ($LASTEXITCODE) { throw 'Could not inspect the running Enji window.' }
    ($controls | ConvertFrom-Json | Where-Object target -eq 'desktop_shell.pages').value
}
function Set-Page([string]$page) {
    & $cli ui select --target desktop_shell.pages --option $page | Out-Null
    if ($LASTEXITCODE) { throw "Could not select $page." }
    Start-Sleep -Milliseconds 80
}
function Get-Sample([string]$phase) {
    $process.Refresh()
    [pscustomobject]@{
        Phase = $phase
        PrivateMiB = [math]::Round($process.PrivateMemorySize64 / 1MB, 2)
        WorkingSetMiB = [math]::Round($process.WorkingSet64 / 1MB, 2)
        Handles = $process.HandleCount
    }
}

$original = Get-Page
if (!$original) { throw 'The desktop sidebar must be visible.' }
try {
    for ($i = 0; $i -lt 10; $i++) {
        Set-Page 'Settings'
        Set-Page 'Search'
    }
    if ((Get-Page) -ne 'Search') { throw 'Search must be enabled for this test.' }
    Start-Sleep -Seconds 2
    $warm = Get-Sample 'Warm'
    $warm
    for ($i = 1; $i -le $Rounds; $i++) {
        Set-Page 'Settings'
        Set-Page 'Search'
        if ($i % 25 -eq 0) { Get-Sample $i }
    }
    Start-Sleep -Seconds 5
    $final = Get-Sample 'Settled'
    $final
    # Allow a fixed amount for WinUI initialization and allocator variation.
    # The regression retained roughly 9.7 MiB on every round trip.
    if ($final.PrivateMiB - $warm.PrivateMiB -gt 24 + $Rounds * 0.5) {
        throw "Private memory kept growing across $Rounds round trips."
    }
} finally {
    Set-Page $original
}
