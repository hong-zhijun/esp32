const { execFile } = require('node:child_process');
// Pastes text into whatever window has focus, then restores the previous clipboard text.
const script = `
Add-Type -AssemblyName System.Windows.Forms
$t=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:VOICE_TEXT_B64))
$old=$null;try{if([Windows.Forms.Clipboard]::ContainsText()){$old=[Windows.Forms.Clipboard]::GetText()}}catch{}
[Windows.Forms.Clipboard]::SetText($t)
[Windows.Forms.SendKeys]::SendWait('^v')
Start-Sleep -Milliseconds 400
if($old -ne $null){[Windows.Forms.Clipboard]::SetText($old)}
`;
function typeText(text) {
  return new Promise((resolve, reject) => {
    execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-STA', '-Command', script],
      { env: { ...process.env, VOICE_TEXT_B64: Buffer.from(text, 'utf8').toString('base64') }, windowsHide: true, timeout: 10000 },
      err => err ? reject(err) : resolve());
  });
}
module.exports = { typeText };
