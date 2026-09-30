# Install this user's font files and a dedicated Windows Terminal profile.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$fontSource = Join-Path $repo 'branding\fonts\ibm-plex-mono'
$fontDirectory = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\Fonts'
$fragmentDirectory = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows Terminal\Fragments\Ukis'
$fragmentPath = Join-Path $fragmentDirectory 'ukis.json'
$registryPath = 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Fonts'
$profileId = '{4fc3ef90-34ce-5ce0-adf3-7d124d958fb8}'
$fontNames = [ordered]@{
    'IBMPlexMono-Regular.ttf' = 'IBM Plex Mono Regular (TrueType)'
    'IBMPlexMono-Bold.ttf' = 'IBM Plex Mono Bold (TrueType)'
    'IBMPlexMono-Italic.ttf' = 'IBM Plex Mono Italic (TrueType)'
    'IBMPlexMono-BoldItalic.ttf' = 'IBM Plex Mono Bold Italic (TrueType)'
}
Get-Command wt.exe -ErrorAction Stop | Out-Null
if (Test-Path -LiteralPath $fragmentPath) {
    $existing = [IO.File]::ReadAllText($fragmentPath) | ConvertFrom-Json
    if (@($existing.profiles).Count -ne 1 -or $existing.profiles[0].guid -ne $profileId) {
        throw "An unrelated Terminal fragment exists at $fragmentPath"
    }
}
# Validate all destinations before installing anything.
foreach ($fileName in $fontNames.Keys) {
    $source = Join-Path $fontSource $fileName
    $destination = Join-Path $fontDirectory $fileName
    if (!(Test-Path -LiteralPath $source)) { throw "Missing font: $source" }
    if ((Test-Path -LiteralPath $destination) -and
        (Get-FileHash -LiteralPath $destination).Hash -ne (Get-FileHash -LiteralPath $source).Hash) {
        throw "A different font version already exists at $destination"
    }
}
if (-not ('Ukis.FontInstaller' -as [type])) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;
namespace Ukis {
    public static class FontInstaller {
        [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
        public static extern int AddFontResource(string path);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        public static extern IntPtr SendMessageTimeout(IntPtr window, uint message,
            UIntPtr wparam, IntPtr lparam, uint flags, uint timeout, out UIntPtr result);
    }
}
"@
}
New-Item -ItemType Directory -Path $fontDirectory -Force | Out-Null
New-Item -ItemType Directory -Path $fragmentDirectory -Force | Out-Null
if (!(Test-Path -LiteralPath $registryPath)) { New-Item -Path $registryPath -Force | Out-Null }
foreach ($fileName in $fontNames.Keys) {
    $destination = Join-Path $fontDirectory $fileName
    if (!(Test-Path -LiteralPath $destination)) {
        Copy-Item -LiteralPath (Join-Path $fontSource $fileName) -Destination $destination
    }
    New-ItemProperty -LiteralPath $registryPath -Name $fontNames[$fileName] -Value $destination -PropertyType String -Force | Out-Null
    if ([Ukis.FontInstaller]::AddFontResource($destination) -eq 0) {
        throw "Windows could not load $destination"
    }
}
$result = [UIntPtr]::Zero
[Ukis.FontInstaller]::SendMessageTimeout([IntPtr]0xffff, 0x001D, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 1000, [ref]$result) | Out-Null
$fragment = @{
    profiles = @(@{
        guid = $profileId
        name = 'Ukis Code'
        commandline = 'powershell.exe -NoLogo -NoExit -ExecutionPolicy Bypass -File "' + (Join-Path $PSScriptRoot 'ukis-terminal.ps1') + '"'
        startingDirectory = $repo
        tabTitle = 'Ukis Code'
        font = @{ face = 'IBM Plex Mono' }
    })
} | ConvertTo-Json -Depth 8
[IO.File]::WriteAllText($fragmentPath, $fragment, (New-Object Text.UTF8Encoding($false)))
# Ask existing Terminal instances to discover the new fragment.
$terminalSettings = @(
    'Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json',
    'Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json',
    'Microsoft\Windows Terminal\settings.json'
)
foreach ($relativePath in $terminalSettings) {
    $settingsPath = Join-Path $env:LOCALAPPDATA $relativePath
    if (Test-Path -LiteralPath $settingsPath) {
        (Get-Item -LiteralPath $settingsPath).LastWriteTime = Get-Date
    }
}
Write-Host 'Installed IBM Plex Mono and the Ukis Windows Terminal profile.'
Write-Host 'Run ukis from any project folder to open it with this font.'
