const {execFile,spawn}=require('node:child_process');
const path=require('node:path');
const run=(file,args)=>new Promise((resolve,reject)=>execFile(file,args,{timeout:8000,windowsHide:true},(err,out)=>err?reject(err):resolve(out)));
const shortcutLabel=process.platform==='darwin'?'Fn＋空格':'F8';
// Match the configured global shortcut on each platform.
// WeType documents that another key ends a running session. No Return is sent.
async function shortcut(){
 if(process.platform==='darwin') return run(path.join(__dirname,'..','build','wetype-key'),[]);
 if(process.platform==='win32') return run('powershell.exe',['-NoProfile','-NonInteractive','-Command',`Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class WeTypeKey { [DllImport("user32.dll")] public static extern void keybd_event(byte v,byte s,uint f,UIntPtr e); }'; [WeTypeKey]::keybd_event(0x77,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [WeTypeKey]::keybd_event(0x77,0,2,[UIntPtr]::Zero)`]);
 throw Error('微信输入法桥接仅支持 macOS / Windows');
}
function watchVoice(onState,onError){
 if(process.platform!=='darwin')return ()=>{};
 const child=spawn(path.join(__dirname,'..','build','wetype-status'),['--watch'],{stdio:['ignore','pipe','ignore']});
 let buffer='',closed=false;
 child.stdout.on('data',chunk=>{buffer+=chunk;const rows=buffer.split('\n');buffer=rows.pop();for(const row of rows){try{const state=JSON.parse(row);if(state.available)onState(state.active);else onError(Error('系统暂时无法读取微信音频状态'));}catch{}}});
 child.on('error',e=>{if(!closed)onError(e);});
 child.on('close',()=>{if(!closed)onError(Error('微信音频状态检测已退出'));});
 return ()=>{closed=true;child.kill();};
}
module.exports={shortcut,shortcutLabel,watchVoice};
