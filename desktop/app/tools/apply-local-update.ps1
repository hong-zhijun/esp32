param(
    [string]$Port = 'COM3',
    [switch]$DesktopOnly,
    [switch]$FirmwareOnly,
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
if ($DesktopOnly -and $FirmwareOnly) { throw 'Choose either DesktopOnly or FirmwareOnly.' }
$panelManifestPath = Join-Path $PSScriptRoot 'manifest.json'
$panelManifest = Get-Content -LiteralPath $panelManifestPath -Raw | ConvertFrom-Json
$panelTarget = [IO.Path]::GetFullPath($panelManifest.targetExecutable)
$panelData = [IO.Path]::GetFullPath($panelManifest.dataDirectory)
$panelIdentityPath = Join-Path $panelData 'device.json'

if ([IO.Path]::GetFileName($panelTarget) -ne 'ai-status-panel.exe' -or
    -not (Test-Path -LiteralPath $panelTarget -PathType Leaf)) {
    throw 'The original executable is missing. Do not launch the update executable from another folder.'
}
if (-not $DesktopOnly -and -not (Test-Path -LiteralPath $panelManifest.arduinoCli -PathType Leaf)) {
    throw 'The configured Arduino CLI is missing.'
}
$panelIdentity = Get-Content -LiteralPath $panelIdentityPath -Raw | ConvertFrom-Json
if ($panelIdentity.token -notmatch '^[0-9a-fA-F]{48}$' -or
    $panelIdentity.computerId -notmatch '^[0-9a-fA-F]{12}$') {
    throw 'The existing pairing identity is invalid. No new identity will be generated.'
}
$panelHasher = [Security.Cryptography.SHA256]::Create()
try {
    $panelFingerprint = [BitConverter]::ToString($panelHasher.ComputeHash(
        [Text.Encoding]::UTF8.GetBytes($panelIdentity.token)
    )).Replace('-', '').Substring(0, 12).ToLowerInvariant()
} finally { $panelHasher.Dispose() }
if ($panelFingerprint -cne $panelManifest.pairingFingerprint) {
    throw 'The pairing identity changed after this update was prepared. Stop and verify it first.'
}

foreach ($panelArtifact in $panelManifest.files) {
    if ($DesktopOnly -and $panelArtifact.path -ne 'ai-status-panel.exe') { continue }
    if ($FirmwareOnly -and $panelArtifact.path -eq 'ai-status-panel.exe') { continue }
    $panelArtifactPath = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $panelArtifact.path))
    $panelPackageRoot = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\') + '\'
    if (-not $panelArtifactPath.StartsWith($panelPackageRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'An artifact path escaped the update folder.'
    }
    if ((Get-FileHash -LiteralPath $panelArtifactPath -Algorithm SHA256).Hash -cne $panelArtifact.sha256) {
        throw "An update artifact failed its integrity check: $($panelArtifact.path)"
    }
}

if ($ValidateOnly) {
    [pscustomobject]@{
        Valid = $true
        TargetExecutable = $panelTarget
        DataDirectory = $panelData
        Port = $Port
        Firmware = $(if ($DesktopOnly) { 'Unchanged (desktop only)' } else { 'Three columns: Claude / ChatGPT / DeepSeek + voice' })
        FirmwareVersion = $panelManifest.firmwareVersion
        DesktopUnchanged = [bool]$FirmwareOnly
        PairingMatches = $true
        WritesPerformed = $false
    } | ConvertTo-Json
    exit 0
}

if (-not $FirmwareOnly -and (Get-Process -Name 'ai-status-panel' -ErrorAction SilentlyContinue)) {
    throw 'Exit AI Status Panel from its system tray menu first. Closing the window only hides it.'
}
if (-not $DesktopOnly -and -not ([IO.Ports.SerialPort]::GetPortNames() -contains $Port)) {
    throw "Serial port $Port is unavailable. Connect the board by USB first."
}

$panelStamp = Get-Date -Format 'yyyyMMdd-HHmmssfff'
$panelBackupDirectory = Join-Path $panelData ('before-deepseek-update-' + $panelStamp)
New-Item -ItemType Directory -Path $panelBackupDirectory -ErrorAction Stop | Out-Null
foreach ($panelFile in @('device.json', 'config.json', 'acknowledged.json', 'tracked-codex.json', 'tracked-deepseek.json')) {
    $panelSourceFile = Join-Path $panelData $panelFile
    if (Test-Path -LiteralPath $panelSourceFile -PathType Leaf) {
        Copy-Item -LiteralPath $panelSourceFile -Destination (Join-Path $panelBackupDirectory $panelFile) -ErrorAction Stop
    }
}
$panelOldExecutable = $panelTarget + '.backup-' + $panelStamp
if (-not $FirmwareOnly) { Copy-Item -LiteralPath $panelTarget -Destination $panelOldExecutable -ErrorAction Stop }

if (-not $DesktopOnly) {
Write-Host "Uploading the compiled firmware on $Port. Keep USB connected."
# Use the unchanged partition scheme and the individual upload images. This
# writes bootloader/partition/application ranges, never erase-flash or NVS.
& $panelManifest.arduinoCli upload --port $Port --fqbn $panelManifest.fqbn `
    --input-dir (Join-Path $PSScriptRoot 'firmware') `
    --upload-property 'build.project_name=ai-status-screen.ino' --upload-property 'upload.erase_cmd='
if ($LASTEXITCODE -ne 0) {
    throw "Firmware upload failed. Desktop executable and pairing were left intact. Settings backup: $panelBackupDirectory"
}
}

if ($FirmwareOnly) {
    Write-Host 'Screen firmware updated. The desktop service was left running.'
    Write-Host "Settings backup: $panelBackupDirectory"
    Write-Host 'Allow about 20 seconds for the screen to reconnect.'
    exit 0
}

Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'ai-status-panel.exe') -Destination $panelTarget -Force -ErrorAction Stop
if ((Get-FileHash -LiteralPath $panelTarget).Hash -cne (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'ai-status-panel.exe')).Hash) {
    Copy-Item -LiteralPath $panelOldExecutable -Destination $panelTarget -Force
    throw 'Desktop replacement verification failed; restored the previous executable.'
}
$env:AI_PANEL_DATA_DIR = $panelData
Start-Process -FilePath $panelTarget -WorkingDirectory (Split-Path -Parent $panelTarget) -WindowStyle Hidden
Write-Host 'Updated and started the same executable path. Pairing/configuration were preserved.'
Write-Host "Desktop backup: $panelOldExecutable"
Write-Host "Settings backup: $panelBackupDirectory"
Write-Host 'Allow about 20 seconds for the screen to reconnect.'
