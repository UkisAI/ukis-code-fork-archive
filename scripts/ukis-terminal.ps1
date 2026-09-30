$env:UKIS_TERMINAL_PROFILE = '{4fc3ef90-34ce-5ce0-adf3-7d124d958fb8}'
$host.UI.RawUI.WindowTitle = 'Ukis Code'
& node (Join-Path $PSScriptRoot 'ukis-code.mjs')
