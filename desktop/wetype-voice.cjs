const {spawn,execFile}=require('node:child_process');
const fs=require('node:fs'),path=require('node:path');
const {level}=require('./voice.cjs');
const {shortcut,shortcutLabel,watchVoice}=require('./wetype-control.cjs');
const worker=path.join(__dirname,'bridge','audio-output.py');
function python(){return process.env.VOICE_PYTHON||path.join(__dirname,'.venv',process.platform==='win32'?'Scripts/python.exe':'bin/python');}
function outputDevice(){return process.env.VOICE_OUTPUT_DEVICE||(process.platform==='darwin'?'BlackHole 2ch':'CABLE Input');}
async function check(){
 if(!fs.existsSync(python()))throw Error('请先安装桥接音频依赖：运行 bridge/setup-mac.sh 或 setup-windows.ps1');
 return new Promise((resolve,reject)=>execFile(python(),[worker,'--check','--device',outputDevice()],{timeout:8000},(e,out)=>{
  try{const rows=out.trim().split('\n').filter(Boolean).map(JSON.parse);const error=rows.find(x=>x.event==='error');if(e||error)return reject(Error(error?.message||e.message));resolve(rows.at(-1));}catch(err){reject(err);}
 }));
}
class WeTypeVoice {
 constructor({log=()=>{},trigger=shortcut,spawnProcess=spawn,watch=watchVoice}={}){this.log=log;this.trigger=trigger;this.spawnProcess=spawnProcess;this.watch=watch;this.session=null;}
 start(reply){
  if(this.session){reply({type:'voice',state:'error',text:'上一段语音还未结束'});return;}
  const s={reply,process:null,ready:false,started:false,ended:false,buffers:[],bytes:0,lastLog:0,at:Date.now(),aborted:false};this.session=s;
  this.log({event:'recording-start',backend:'wetype'});
  s.readyPromise=this.open(s).catch(e=>this.fail(s,e));
 }
 async open(s){
  if(!fs.existsSync(python()))throw Error('虚拟麦克风依赖未安装');
  const child=this.spawnProcess(python(),[worker,'--device',outputDevice()],{stdio:['pipe','pipe','pipe'],windowsHide:true});s.process=child;
  child.stdin.on('error',e=>{if(!s.ended)this.fail(s,e);});
  let stdout='',stderr='';
  s.exitPromise=new Promise(resolve=>child.once('close',code=>{s.exitCode=code;s.exitResolve=true;resolve(code);if((code!==0||!s.ended)&&!s.aborted)this.fail(s,Error(stderr||s.workerError||'虚拟麦克风意外退出'));}));
  await new Promise((resolve,reject)=>{
   const timer=setTimeout(()=>reject(Error('虚拟声卡启动超时')),8000);s.openReject=reject;
   child.once('error',e=>{clearTimeout(timer);reject(e);});
   child.once('close',code=>{clearTimeout(timer);if(!s.ready)reject(Error(s.workerError||stderr||'虚拟声卡未启动'));});
   child.stderr.on('data',b=>{stderr=(stderr+b).slice(-2000);});
   child.stdout.on('data',b=>{stdout+=b;const lines=stdout.split('\n');stdout=lines.pop();for(const line of lines){try{const row=JSON.parse(line);if(row.event==='ready'){s.ready=true;clearTimeout(timer);resolve();this.log({event:'virtual-mic-ready',message:row.device.name});}if(row.event==='error'){s.workerError=row.message;clearTimeout(timer);reject(Error(row.message));}if(row.event==='drained')this.log({event:'virtual-mic-drained',...row});}catch{}}});
  });
  if(s.aborted)return;
  s.stopWatch=this.watch(active=>{
   if(s.aborted)return;
   clearTimeout(s.closedTimer);
   if(active){s.seenActive=true;this.log({event:'wetype-capture-active'});}
   else if(s.seenActive)s.closedTimer=setTimeout(()=>this.externalStop(s),250);
  },e=>{clearTimeout(s.closedTimer);this.log({event:'wetype-monitor-error',message:e.message});});
  s.activation=Promise.resolve().then(()=>this.trigger()).then(()=>{s.started=!s.externalClosed;});
  await s.activation;
  if(s.aborted)return;
  // Allow WeType to open the selected cable input, retaining the first syllable.
  await new Promise(r=>setTimeout(r,350));
  if(s.aborted)return;
  for(const b of s.buffers){if(s.aborted)return;this.write(s,b);}s.buffers=[];if(s.aborted)return;s.forwarding=true;
  s.reply({type:'voice',state:'partial',text:'微信输入法正在语音输入'});
  this.log({event:'wetype-started',message:'已发送 '+shortcutLabel+'，音频转发到 '+outputDevice()});
 }
 write(s,buf){if(s.process.stdin.destroyed)return;if(s.process.stdin.writableLength>64000){this.fail(s,Error('音频转发积压超过 2 秒'));return;}s.process.stdin.write(buf);}
 audio(buf){const s=this.session;if(!s||s.ended||s.aborted)return;s.bytes+=buf.length;
  if(Date.now()-s.lastLog>=500){this.log({event:'audio-level',...level(buf),signal:level(buf).rms>=120,audioSeconds:s.bytes/32000,wallSeconds:(Date.now()-s.at)/1000});s.lastLog=Date.now();}
  if(s.forwarding)this.write(s,buf);else{if(s.bytes>96000){this.fail(s,Error('微信输入法启动过慢，录音已停止'));return;}s.buffers.push(Buffer.from(buf));}
 }
 async stop(reply){const s=this.session;if(!s||s.ended)return;s.ended=true;s.reply=reply;
  await s.readyPromise;if(s.aborted)return;
  s.process.stdin.end();
  const code=await Promise.race([s.exitPromise,new Promise(r=>{const t=setTimeout(()=>r('timeout'),5000);t.unref();})]);
  if(s.aborted)return;
  if(code!==0){this.fail(s,Error('虚拟音频未完成输出'));return;}
  // Give WeType final silence for punctuation before stopping; never press Return.
  await new Promise(r=>setTimeout(r,500));
  if(s.aborted)return;
  try{if(s.started){await this.trigger();s.started=false;}}
  catch(e){this.fail(s,e);return;}
  this.clearWatch(s);
  if(s.aborted)return;
  if(this.session===s)this.session=null;
  this.log({event:'wetype-stopped',audioSeconds:s.bytes/32000,wallSeconds:(Date.now()-s.at)/1000});
  reply({type:'voice',state:'finished',text:'微信语音已结束，请查看输入框'});
 }
 clearWatch(s){clearTimeout(s.closedTimer);s.stopWatch?.();s.stopWatch=null;}
 externalStop(s){
  if(s.aborted||this.session!==s)return;
  s.externalClosed=true;s.aborted=true;s.ended=true;s.started=false;
  this.clearWatch(s);
  // The input method has already stopped: discard remaining PCM and never toggle it.
  s.process?.kill();
  s.reply({type:'voice',state:'finished',text:'电脑已结束语音'});
  this.log({event:'wetype-external-stop',audioSeconds:s.bytes/32000});
  s.cleanup=(async()=>{try{await s.activation;}catch{}await s.exitPromise;if(this.session===s)this.session=null;})();
 }
 fail(s,e){
  if(s.aborted)return s.cleanup;
  s.aborted=true;this.clearWatch(s);s.process?.kill();
  this.log({event:'voice-error',stage:'wetype',message:e.message});s.reply({type:'voice',state:'error',text:e.message});
  // Keep ownership until any in-flight start key has completed and been balanced
  // with a stop key. A subsequent recording must not receive the old stop key.
  s.cleanup=(async()=>{
   try{await s.activation;}catch{}
   if(s.started){s.started=false;try{await this.trigger();}catch(error){this.log({event:'voice-error',stage:'wetype-stop',message:error.message});}}
   if(this.session===s)this.session=null;
  })();
  return s.cleanup;
 }
 cancel(){if(this.session)this.fail(this.session,Error('屏幕断开，已停止微信语音'));
 }
}
module.exports={WeTypeVoice,check,python,outputDevice};
