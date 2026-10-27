#Requires -RunAsAdministrator
param(
    [string]$ProgramPath = ''
)

$ErrorActionPreference = 'Stop'
if (-not $ProgramPath) {
    $panelPaths = @(Get-Process -Name 'ai-status-panel' -ErrorAction SilentlyContinue |
        ForEach-Object { $_.Path } | Where-Object { $_ } | Sort-Object -Unique)
    if ($panelPaths.Count -ne 1) {
        throw 'Start AI Status Panel first, or pass -ProgramPath with its executable path.'
    }
    $ProgramPath = $panelPaths[0]
}
$ProgramPath = (Resolve-Path -LiteralPath $ProgramPath).ProviderPath
if (-not (Test-Path -LiteralPath $ProgramPath -PathType Leaf) -or
    [System.IO.Path]::GetFileName($ProgramPath) -ne 'ai-status-panel.exe') {
    throw 'ProgramPath must point to ai-status-panel.exe.'
}

$panelProgramHasher = [Security.Cryptography.SHA256]::Create()
try {
    $panelProgramHash = [BitConverter]::ToString($panelProgramHasher.ComputeHash(
        [Text.Encoding]::UTF8.GetBytes($ProgramPath.ToLowerInvariant())
    )).Replace('-', '').Substring(0, 12)
} finally {
    $panelProgramHasher.Dispose()
}
# Keep each executable path's rule, so opening an update beside the installed
# version does not revoke the installed version's access on its next launch.
$panelRuleName = 'AIStatusPanel-Device-Private-8787-' + $panelProgramHash
$panelRule = Get-NetFirewallRule -Name $panelRuleName -ErrorAction SilentlyContinue
$panelSettings = @{
    Direction = 'Inbound'
    Action = 'Allow'
    Enabled = 'True'
    Profile = 'Private'
    Program = $ProgramPath
    Protocol = 'TCP'
    LocalPort = 8787
    RemoteAddress = 'LocalSubnet'
}
if ($panelRule) {
    Set-NetFirewallRule -Name $panelRuleName @panelSettings | Out-Null
} else {
    New-NetFirewallRule -Name $panelRuleName @panelSettings `
        -DisplayName 'AI Status Panel device TCP 8787 (Private LAN)' `
        -Description 'Allow ESP32 status panels on the local subnet to connect to the Rust desktop service.' | Out-Null
}
Write-Host "Allowed private LAN TCP 8787 for $ProgramPath"
