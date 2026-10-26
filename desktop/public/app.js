let latest=null,dirty=false,recordingAt=null,voicePreviewError=false;const form=document.querySelector('#design');
const names={working:'执行中',waiting:'等你确认',done:'做完了',idle:'空闲',interrupted:'已中断',unknown:'状态待确认'};
const colors={working:'#6ab2ff',waiting:'#ffc36a',done:'#6ce0ac',idle:'#8b9199',interrupted:'#e29494',unknown:'#8b9199'};
const strips={...colors,idle:'#333333',unknown:'#333333'};
const dots={...colors,idle:'#555555',unknown:'#555555'};
const el=(tag,text,cls)=>{const e=document.createElement(tag);if(text!==undefined)e.textContent=text;if(cls)e.className=cls;return e;};
async function post(url,value){const r=await fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(value)});const j=await r.json();if(!r.ok)throw Error(j.error||'请求失败');return j;}
function config(){return Object.fromEntries([...form.elements].filter(e=>e.name).map(e=>[e.name,e.type==='checkbox'?e.checked:['split','brightness'].includes(e.name)?Number(e.value):e.value]));}
function fill(c){for(const e of form.elements){if(!e.name)continue;if(e.type==='checkbox')e.checked=c[e.name];else e.value=c[e.name];}}
function render(){if(!latest)return;if(latest.voice)document.querySelector('#voiceBackendStatus').textContent=(latest.voice.backend==='wetype'?'微信输入法 · ':'本地模型 · ')+latest.voice.message;const c=config();document.querySelector('#splitValue').textContent=c.split+'%';const screen=document.querySelector('#screen');const previousVoice=screen.querySelector('.voice-column');screen.replaceChildren();const cards=c.swap?[...latest.cards].reverse():latest.cards;
 const clock=(from,to)=>{const t=from&&to>=from?Math.floor((to-from)/1000):0;return `${String(Math.floor(t/60)).padStart(2,'0')}:${String(t%60).padStart(2,'0')}`;};
 cards.forEach((s,i)=>{const card=el('div',undefined,'card');card.style.width=((i?100-c.split:c.split)*512/100)+'px';const strip=el('div',undefined,'strip'+(s.state==='waiting'?' breathe':''));strip.style.background=strips[s.state];strip.style.animationDelay=-(Date.now()%2600)+'ms';
 const brand=el('div',s.provider==='claude'?'Claude':'Codex','brand');brand.style.color=c[s.provider+'Color'];const status=el('div',names[s.state],'state');status.style.color=colors[s.state];
 let detail='';if(s.state==='working'){const tool=c.showTool?s.tool:'';const t=c.showElapsed?clock(s.startedAt,latest.now):'';detail=[tool,t].filter(Boolean).join(' · ');}else if(s.state==='waiting')detail='已等 '+clock(s.updatedAt,latest.now);else if(s.state==='done'&&c.showElapsed&&s.startedAt)detail='用时 '+clock(s.startedAt,s.endedAt);else if(s.state==='unknown')detail='5 分钟没有新动静';
 const list=el('div',undefined,'list');for(const r of (s.rows||[]).slice(0,3)){const row=el('div',undefined,'row'+(['working','waiting'].includes(r.state)?' active':''));const dot=el('span','●');dot.style.color=dots[r.state]||'#555';row.append(dot,el('span',' '+(r.project||'-')));list.append(row);}
 card.append(strip,brand,status,el('div',detail,'detail'),list);if(s.state==='working'){const runner=el('div',undefined,'runner');runner.style.animationDelay=-(Date.now()%2400)+'ms';card.append(runner);}screen.append(card);});
 const voiceColumn=previousVoice||el('div',undefined,'voice-column');
 if(!previousVoice){const preview=el('div',undefined,'voice-preview');const wave=el('div',undefined,'voice-wave');for(let i=0;i<5;i++){const bar=el('span');bar.style.setProperty('--i',i);wave.append(bar);}preview.append(wave,el('div','开始录音','voice-title'),el('div','00:00','voice-timer'),el('div','点击说话','voice-hint'));voiceColumn.append(preview);}
 screen.append(voiceColumn);updateVoicePreview();
 document.querySelector('#device').textContent=latest.device.connected?'● 屏幕已连接 · 设置可实时应用':'○ 屏幕未连接 · 可先预览和配网';
 const list=document.querySelector('#sessions');list.replaceChildren();for(const s of latest.sessions.sort((a,b)=>b.updatedAt-a.updatedAt).slice(0,30)){const row=el('div',undefined,'session');row.append(el('span',`${s.provider} · ${s.title||s.project} · ${names[s.state]}${s.unread?' · 提示保留':''}`));const b=el('button','显示此会话');b.onclick=()=>post('/api/select',{provider:s.provider,id:s.id}).catch(showError);row.append(b);if(s.state==='done'&&s.unread){const ack=el('button','清除完成提示');ack.onclick=()=>post('/api/ack',{provider:s.provider,id:s.id}).catch(showError);row.append(ack);}list.append(row);}
}
function updateVoicePreview(){
 const preview=document.querySelector('.voice-preview');if(!preview)return;
 const recording=recordingAt!==null;preview.classList.toggle('recording',recording);preview.classList.toggle('voice-error',voicePreviewError);
 preview.querySelector('.voice-title').textContent=recording?'识别中···':voicePreviewError?'出错了':'开始录音';
 preview.querySelector('.voice-hint').textContent=recording?'点击结束':voicePreviewError?'查看电脑':'点击说话';
 const sec=recording?Math.max(0,Math.floor((Date.now()-recordingAt)/1000)):0;
 preview.querySelector('.voice-timer').textContent=String(Math.floor(sec/60)).padStart(2,'0')+':'+String(sec%60).padStart(2,'0');
}
setInterval(updateVoicePreview,250);
function showError(e){document.querySelector('#saveStatus').textContent=e.message;}
form.oninput=()=>{dirty=true;render();};form.onsubmit=async e=>{e.preventDefault();try{await post('/api/config',config());dirty=false;document.querySelector('#saveStatus').textContent='已保存';}catch(e){showError(e);}};
function connect(){const ws=new WebSocket(`ws://${location.host}/live`);ws.onmessage=e=>{const msg=JSON.parse(e.data);if(msg.type==='debug'){renderDebug(msg.entries);return;}latest=msg;if(!dirty)fill(latest.config);render();};ws.onopen=()=>{lastDebugId=0;document.querySelector('#voiceDebug').replaceChildren();document.querySelector('#connection').textContent='● 本地服务在线';};ws.onclose=()=>{document.querySelector('#connection').textContent='○ 服务断开，正在重连';setTimeout(connect,2000);};}
async function setup(){const r=await fetch('/api/setup');const j=await r.json();const w=document.querySelector('#wifi');for(const p of j.ports)w.elements.port.add(new Option(p.path+' · '+p.label,p.path));for(const a of j.addresses)w.elements.host.add(new Option(a,a));}
document.querySelector('#wifi').onsubmit=async e=>{e.preventDefault();const f=e.target,b=f.querySelector('button'),status=document.querySelector('#wifiStatus');b.disabled=true;status.textContent='正在写入，请保持 USB 连接…';try{await post('/api/provision',Object.fromEntries(new FormData(f)));f.elements.password.value='';status.textContent='已写入！等待板子连接 Wi-Fi，通常需要几秒。';}catch(e){status.textContent=e.message;}finally{b.disabled=false;}};
let debugPaused=false,lastDebugId=0;
document.querySelector('#voiceCheck').onclick=async()=>{try{const r=await fetch('/api/voice-status');const s=await r.json();document.querySelector('#voiceBackendStatus').textContent=s.message;}catch(e){showError(e);}};
const debugNames={'wetype-external-stop':'电脑结束语音','wetype-capture-active':'微信开始采音','wetype-monitor-error':'状态检测异常','virtual-mic-ready':'虚拟麦克风就绪','virtual-mic-drained':'音频已送达','wetype-started':'微信语音启动','wetype-stopped':'微信语音结束','device-performance':'设备耗时','button':'按钮请求','recording-start':'开始录音','audio-level':'接收音频','recognition-start':'开始识别','recognized':'识别文字','voice-segment-timing':'粘贴完成','voice-capture-timing':'结束录音','voice-error':'错误','device-connected':'屏幕连接','device-disconnected':'屏幕断开'};
function renderDebug(entries){const box=document.querySelector('#voiceDebug');for(const entry of entries){if(entry.id<=lastDebugId)continue;lastDebugId=entry.id;
 if(entry.event==='recording-start'){recordingAt=entry.time;voicePreviewError=false;}
 if(entry.event==='button'&&recordingAt!==null)recordingAt=null;
 if(entry.event==='device-performance'){if(!entry.recording)recordingAt=null;else if(recordingAt===null)recordingAt=entry.time;}
 if(['wetype-external-stop','wetype-stopped','voice-capture-timing','device-disconnected','voice-error'].includes(entry.event))recordingAt=null;
 if(entry.event==='voice-error')voicePreviewError=true;
const row=el('div',undefined,'debug-row');row.dataset.event=entry.event;const date=new Date(entry.time);const time=date.toLocaleTimeString('zh-CN',{hour12:false})+'.'+String(date.getMilliseconds()).padStart(3,'0');const fields=[];
 if(entry.event==='device-performance')fields.push((entry.screenLocked?'屏幕已锁定':entry.recording?'录音中':'待机')+' · 循环最大 '+entry.loopMs+' ms · 采集 '+entry.readMs+' ms · 发送 '+entry.sendMs+' ms · 读取/发送错误 '+entry.readErrors+'/'+entry.sendErrors+' · Wi-Fi '+entry.rssi+' dBm');
 if(entry.source)fields.push(entry.source);if(entry.deviceMs!==undefined)fields.push('设备运行 '+entry.deviceMs+' ms');
 if(entry.signal!==undefined)fields.push(entry.signal?'检测到声音信号':'安静／低音量');if(entry.rms!==undefined)fields.push('音量 RMS '+entry.rms+' / 峰值 '+entry.peak);
 if(entry.audioSeconds!==undefined)fields.push('音频 '+entry.audioSeconds.toFixed(2)+' s');if(entry.wallSeconds!==undefined)fields.push('实际经过 '+entry.wallSeconds.toFixed(2)+' s');
 for(const [key,label] of [['queueMs','排队'],['recognizeMs','识别'],['pasteMs','粘贴'],['maxPacketGapMs','最大音频间隔']])if(entry[key]!==undefined)fields.push(label+' '+entry[key]+' ms');
 if(entry.text)fields.push(entry.text);if(entry.message)fields.push(entry.message);if(entry.stage)fields.push('阶段 '+entry.stage);
 row.append(el('span',time,'debug-time'),el('strong',debugNames[entry.event]||entry.event),el('span',fields.join(' · ')));box.append(row);while(box.children.length>300)box.firstChild.remove();}updateVoicePreview();if(!debugPaused)box.scrollTop=box.scrollHeight;}
 document.querySelector('#debugPause').onclick=e=>{debugPaused=!debugPaused;e.target.textContent=debugPaused?'恢复滚动':'暂停滚动';};
 document.querySelector('#debugClear').onclick=()=>document.querySelector('#voiceDebug').replaceChildren();
connect();setup().catch(showError);
new ResizeObserver(entries=>{const frame=entries[0].target;const width=frame.clientWidth-parseFloat(getComputedStyle(frame).paddingLeft)-parseFloat(getComputedStyle(frame).paddingRight);const scale=Math.min(1,width/640);const screen=document.querySelector('#screen');screen.style.transformOrigin='top left';screen.style.transform=`scale(${scale})`;screen.style.marginBottom=`${172*(scale-1)}px`;frame.style.overflow='hidden';}).observe(document.querySelector('.frame'));
