const { execFile } = require('node:child_process');
// Pastes text into whatever window has focus, then restores the previous clipboard text.
const script = `
Add-Type -AssemblyName System.Windows.Forms
$t=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:VOICE_TEXT_B64))
$old=$null;try{if([Windows.Forms.Clipboard]::ContainsText()){$old=[Windows.Forms.Clipboard]::GetText()}}catch{}
[Windows.Forms.Clipboard]::SetText($t)
[Windows.Forms.SendKeys]::SendWait('^v')
Start-Sleep -Milliseconds 100
if($old -ne $null){[Windows.Forms.Clipboard]::SetText($old)}
`;
const macScript = `
on run argv
  set spokenText to item 1 of argv
  set oldClipboard to missing value
  try
    set oldClipboard to the clipboard as record
  end try
  set the clipboard to spokenText
  tell application "System Events"
    key code 9 using command down
  end tell
  delay 0.1
  try
    if oldClipboard is not missing value and (the clipboard as text) is spokenText then set the clipboard to oldClipboard
  end try
end run
`;
function typeText(text) {
  return new Promise((resolve, reject) => {
    if (process.platform === 'darwin') {
      execFile('/usr/bin/osascript', ['-e', macScript, text], { timeout: 10000 },
        err => err ? reject(new Error('Mac 自动粘贴失败，请检查辅助功能和自动化权限：' + err.message)) : resolve());
      return;
    }
    if (process.platform !== 'win32') return reject(new Error('当前系统尚不支持自动输入'));
    execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-STA', '-Command', script],
      { env: { ...process.env, VOICE_TEXT_B64: Buffer.from(text, 'utf8').toString('base64') }, windowsHide: true, timeout: 10000 },
      err => err ? reject(err) : resolve());
  });
}
module.exports = { typeText };
