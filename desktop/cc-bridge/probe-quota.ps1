param([string]$Proxy = 'http://127.0.0.1:7897')
$ErrorActionPreference = 'Stop'
$credentialPath = Join-Path $env:USERPROFILE '.claude\.credentials.json'
if (!(Test-Path -LiteralPath $credentialPath)) {
    Write-Output '{"status":"credentials_not_found"}'
    exit 2
}
$credentials = Get-Content -Raw -LiteralPath $credentialPath | ConvertFrom-Json
$oauth = if ($credentials.claudeAiOauth) { $credentials.claudeAiOauth } elseif ($credentials.oauth) { $credentials.oauth } else { $credentials }
if (!$oauth.accessToken) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ClaudeCredentialReader {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  public struct Credential {
    public uint Flags, Type;
    public string TargetName, Comment;
    public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
    public uint BlobSize;
    public IntPtr Blob;
    public uint Persist, AttributeCount;
    public IntPtr Attributes;
    public string TargetAlias, UserName;
  }
  [DllImport("advapi32.dll", EntryPoint="CredReadW", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern bool Read(string target, uint type, uint flags, out IntPtr credential);
  [DllImport("advapi32.dll")] public static extern void CredFree(IntPtr buffer);
  public static string Get(string target) {
    IntPtr pointer;
    if (!Read(target, 1, 0, out pointer)) return null;
    try {
      var value=(Credential)Marshal.PtrToStructure(pointer, typeof(Credential));
      byte[] bytes=new byte[value.BlobSize];
      Marshal.Copy(value.Blob,bytes,0,bytes.Length);
      string text=System.Text.Encoding.UTF8.GetString(bytes).TrimEnd('\0');
      if (text.Contains("\0")) text=System.Text.Encoding.Unicode.GetString(bytes).TrimEnd('\0');
      return text;
    } finally { CredFree(pointer); }
  }
}
'@
    foreach ($target in @('Claude Code-credentials', "Claude Code-credentials:$env:USERNAME", "Claude Code-credentials/$env:USERNAME")) {
        $secret = [ClaudeCredentialReader]::Get($target)
        if (!$secret) { continue }
        try {
            $parsed = $secret | ConvertFrom-Json
            $candidate = if ($parsed.claudeAiOauth) { $parsed.claudeAiOauth } elseif ($parsed.oauth) { $parsed.oauth } else { $parsed }
            if ($candidate.accessToken) { $oauth = $candidate; break }
        } catch { }
    }
}
if (!$oauth.accessToken) { Write-Output '{"status":"access_token_not_found_in_file_or_windows_credential_manager"}'; exit 2 }
$headers = @{
    Authorization = 'Bearer ' + $oauth.accessToken
    'anthropic-beta' = 'oauth-2025-04-20'
    Accept = 'application/json'
}
$result = [ordered]@{ checkedAt = [DateTime]::UtcNow.ToString('o'); source = 'claude-code-oauth'; status = 'unknown' }
try {
    $usage = Invoke-RestMethod -Uri 'https://api.anthropic.com/api/oauth/usage' -Headers $headers -UserAgent 'claude-cli/2.1.229 (external, cli)' -Proxy $Proxy -TimeoutSec 25
    $result.status = 'ok'
    foreach ($name in @('five_hour', 'seven_day')) {
        $window = $usage.$name
        $result[$name] = if ($null -eq $window) { $null } else { @{ usedPercent = $window.utilization; resetsAt = $window.resets_at } }
    }
    try {
        $profile = Invoke-RestMethod -Uri 'https://api.anthropic.com/api/oauth/profile' -Headers $headers -UserAgent 'claude-cli/2.1.229 (external, cli)' -Proxy $Proxy -TimeoutSec 20
        $email = [string]$profile.account.email
        $result.account = if ($email -match '^(.).*(@.*)$') { $Matches[1] + '***' + $Matches[2] } else { 'profile_received' }
    } catch { $result.account = 'profile_unavailable' }
} catch {
    $result.status = 'request_failed'
    $response = $_.Exception.Response
    if ($response) { $result.httpStatus = [int]$response.StatusCode }
    $result.errorType = $_.Exception.GetType().Name
}
# Only explicitly selected, non-secret fields leave this process.
$result | ConvertTo-Json -Depth 5
if ($result.status -ne 'ok') { exit 1 }
