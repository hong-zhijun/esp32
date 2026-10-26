$ErrorActionPreference = 'Stop'
$nodePath = & 'D:\soft\nodejs\node.exe' -p "require('fs').realpathSync(process.execPath)"
if (!(Get-NetFirewallRule -DisplayName 'ESP32 AI Status Local' -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -DisplayName 'ESP32 AI Status Local' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8787 -RemoteAddress LocalSubnet -Profile Private -Program $nodePath | Out-Null
}
else { Set-NetFirewallRule -DisplayName 'ESP32 AI Status Local' -Program $nodePath | Out-Null }
