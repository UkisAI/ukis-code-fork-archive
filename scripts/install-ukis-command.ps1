param(
    [string]$CommandDirectory = (Join-Path $env:USERPROFILE '.local\bin')
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$launcher = Join-Path $repo 'scripts\ukis-code.mjs'
$commandPath = Join-Path $CommandDirectory 'ukis.cmd'
$lines = @('@echo off', ('node "' + $launcher + '" %*'))
$command = ($lines -join "`r`n") + "`r`n"
if (Test-Path -LiteralPath $commandPath) {
    if ([IO.File]::ReadAllText($commandPath) -ne $command) {
        throw "An unrelated command already exists at $commandPath"
    }
}
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
$pathEntries = ($userPath + ';' + $machinePath).Split(';') | ForEach-Object {
    [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\')
}
if ($CommandDirectory.TrimEnd('\') -notin $pathEntries) {
    throw "Add $CommandDirectory to your user PATH before installing this command."
}
New-Item -ItemType Directory -Path $CommandDirectory -Force | Out-Null
[IO.File]::WriteAllText($commandPath, $command, [Text.Encoding]::ASCII)
Write-Host "Installed Ukis Code: $commandPath"
