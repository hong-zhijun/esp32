$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$node = (Get-Command node.exe).Source
try { Invoke-RestMethod 'http://localhost:8787/api/state' -TimeoutSec 2 | Out-Null }
catch { Start-Process -FilePath $node -ArgumentList ('"' + (Join-Path $root 'server.cjs') + '"') -WorkingDirectory $root -WindowStyle Hidden -RedirectStandardOutput (Join-Path $root 'server.log') -RedirectStandardError (Join-Path $root 'server-error.log') }
Start-Process 'http://localhost:8787'
