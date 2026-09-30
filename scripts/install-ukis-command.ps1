param(
    [string]$CommandDirectory = (Join-Path $env:USERPROFILE '.local\bin'),
    [string]$LegacyExecutable
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$launcher = Join-Path $repo 'scripts\ukis-codex.mjs'
$commandPath = Join-Path $CommandDirectory 'ukis.cmd'
$legacyPath = Join-Path $CommandDirectory 'ukis-legacy.cmd'
$lines = @('@echo off', ('node "' + $launcher + '" %*'))
$command = ($lines -join "`r`n") + "`r`n"
if (Test-Path -LiteralPath $commandPath) {
    if ([IO.File]::ReadAllText($commandPath) -ne $command) {
        throw "An unrelated command already exists at $commandPath"
    }
}
if ($LegacyExecutable) {
    $LegacyExecutable = (Resolve-Path -LiteralPath $LegacyExecutable).Path
    $legacy = (@('@echo off', ('"' + $LegacyExecutable + '" %*')) -join "`r`n") + "`r`n"
    if ((Test-Path -LiteralPath $legacyPath) -and [IO.File]::ReadAllText($legacyPath) -ne $legacy) {
        throw "An unrelated command already exists at $legacyPath"
    }
}
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
$pathEntries = ($userPath + ';' + $machinePath).Split(';') | ForEach-Object {
    [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\')
}
if ($CommandDirectory.TrimEnd('\') -notin $pathEntries) {
    throw "Add $CommandDirectory to your user PATH before installing these commands."
}
New-Item -ItemType Directory -Path $CommandDirectory -Force | Out-Null
if ($LegacyExecutable) { [IO.File]::WriteAllText($legacyPath, $legacy, [Text.Encoding]::ASCII) }
[IO.File]::WriteAllText($commandPath, $command, [Text.Encoding]::ASCII)
Write-Host "Installed ukis at $commandPath"
if ($LegacyExecutable) { Write-Host "Previous app remains available as ukis-legacy ($LegacyExecutable)" }
