const test=require('node:test'),assert=require('node:assert/strict');const {State,defaults,validateConfig}=require('./state.cjs');const {codexEvent}=require('./sources.cjs');
test('voice distinguishes recognition failures from paste failures and preserves recognized text', async () => {
 const fs=require('node:fs'),os=require('node:os'),path=require('node:path');const {Voice}=require('./voice.cjs');
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'voice-test-'));
 try {
  for (const mode of ['missing','paste','success','empty']) {
   let pasted=null;const replies=[];
   const voice=new Voice(dir,async()=>{if(mode==='missing')throw Error('语音模型缺失');return mode==='empty'?'':'你好，Mac';},async text=>{pasted=text;if(mode==='paste')throw Error('permission denied');});
   voice.start();voice.audio(Buffer.alloc(32000));await voice.stop(r=>replies.push(r));
   assert.equal(replies[0].state,'recognizing');const last=replies.at(-1);
   if(mode==='missing'){assert.equal(pasted,null);assert.match(last.text,/语音模型/);}
   if(mode==='paste'){assert.equal(last.state,'error');assert.match(last.text,/自动输入失败：你好，Mac/);}
   if(mode==='success'){assert.equal(pasted,'你好，Mac');assert.equal(last.state,'done');}
   if(mode==='empty'){assert.equal(pasted,null);assert.match(last.text,/没听清/);}
  }
 } finally {fs.rmSync(dir,{recursive:true,force:true});}
});
test('parallel sessions remain separate; acknowledgements do not complete active work',()=>{const s=new State(),time=new Date().toISOString();s.apply({provider:'claude',session:'a',event:'UserPromptSubmit',time});s.apply({provider:'claude',session:'b',event:'Stop',time});s.acknowledge('claude','b');assert.equal(s.sessions.get('claude:a').state,'working');assert.equal(s.sessions.get('claude:b').unread,false);});
test('unknown notifications do not imply approval; old events cannot overwrite completion',()=>{const s=new State(),n=Date.now();const event={provider:'claude',session:'x'};s.apply({...event,event:'Stop',time:new Date(n).toISOString()});s.apply({...event,event:'PreToolUse',time:new Date(n-1).toISOString()});s.apply({...event,event:'Notification',notification:'auth_success',time:new Date(n+1).toISOString()});assert.equal(s.snapshot(n+2).cards[0].state,'done');});
test('stale work is unknown, never automatically completed',()=>{const s=new State(),n=Date.now();s.apply({provider:'codex',session:'x',event:'UserPromptSubmit',time:new Date(n).toISOString()});assert.equal(s.snapshot(n+300001).cards[1].state,'unknown');});
test('completion survives time passing until acknowledged or restarted',()=>{
 const s=new State(),n=Date.now();s.apply({provider:'codex',session:'x',event:'Stop',time:new Date(n).toISOString()});
 for(const elapsed of [600001,2*86400000]){const card=s.snapshot(n+elapsed).cards[1];assert.equal(card.state,'done');assert.equal(card.unread,true);}
 s.acknowledge('codex','x');assert.equal(s.snapshot(n+600001).cards[1].state,'idle');
 assert.equal(s.snapshot(n+2*86400000).sessions.length,0);
 s.apply({provider:'codex',session:'x',event:'UserPromptSubmit',time:new Date(n+2*86400000).toISOString()});assert.equal(s.snapshot(n+2*86400000).cards[1].state,'working');
});
test('each card lists at most three sessions, most urgent first',()=>{const s=new State(),n=Date.now();for(const [id,ev,i] of [['a','Stop',0],['b','UserPromptSubmit',1],['c','Stop',2],['d','Stop',3]])s.apply({provider:'claude',session:id,cwd:'D:/w/'+id,event:ev,time:new Date(n+i).toISOString()});const rows=s.snapshot(n+10).cards[0].rows;assert.equal(rows.length,3);assert.deepEqual(rows[0],{id:'b',project:'b',state:'working'});});
test('Codex rollout maps explicit boundaries only',()=>{const meta={id:'x',cwd:'D:\\workspace\\esp32'};assert.equal(codexEvent({type:'event_msg',payload:{type:'task_complete'},timestamp:'now'},meta).event,'Stop');assert.equal(codexEvent({type:'response_item',payload:{type:'message',role:'assistant'}},meta),null);});
test('configuration rejects out-of-range and injection values',()=>{assert.deepEqual(validateConfig(defaults),defaults);assert.throws(()=>validateConfig({...defaults,brightness:999}));assert.throws(()=>validateConfig({...defaults,codexColor:'red;display:none'}));});
test('Codex internal subagents stay excluded after new lines are appended',()=>{
 const fs=require('node:fs'),os=require('node:os'),path=require('node:path');const {Sources}=require('./sources.cjs');
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'panel-test-'));const file=path.join(dir,'rollout.jsonl');
 try{fs.writeFileSync(file,JSON.stringify({type:'session_meta',payload:{id:'internal',source:{subagent:{other:'guardian'}}}})+'\n');const s=new State(),src=new Sources(s,dir);src.lastScan=Date.now();src.files.set(file,{offset:0,carry:'',meta:{},kind:'codex'});src.tick();fs.appendFileSync(file,JSON.stringify({type:'event_msg',timestamp:new Date().toISOString(),payload:{type:'task_started'}})+'\n');src.tick();assert.equal(s.sessions.size,0);}finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('voice previews do not paste and stale previews are discarded after stop or restart',async()=>{
 const fs=require('node:fs'),os=require('node:os'),path=require('node:path');const {Voice}=require('./voice.cjs');
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'voice-preview-'));let resolvePreview,pasted=0,calls=0;const replies=[];
 const voice=new Voice(dir,()=>{calls++;if(calls===1)return new Promise(r=>resolvePreview=r);return Promise.resolve('最终文字');},async()=>{pasted++;});
 try{
  voice.start(r=>replies.push(r));voice.audio(Buffer.alloc(48000));assert.equal(calls,1);
  voice.audio(Buffer.alloc(48000));assert.equal(calls,1);
  await voice.stop(r=>replies.push(r));resolvePreview('过时草稿');await new Promise(r=>setImmediate(r));
  assert.equal(pasted,1);assert.equal(replies.some(r=>r.state==='partial'),false);
  voice.start(r=>replies.push(r));voice.audio(Buffer.alloc(48000));await new Promise(r=>setImmediate(r));
  assert.equal(replies.at(-1).state,'partial');assert.equal(pasted,1);
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('live dictation pastes paused phrases before stop and never pastes the full recording again',async()=>{
 const fs=require('fs'),os=require('os'),path=require('path'),{Voice}=require('./voice.cjs');const dir=fs.mkdtempSync(path.join(os.tmpdir(),'live-paste-'));const pasted=[];let calls=0;
 const speech=Buffer.alloc(32000);for(let i=0;i<speech.length;i+=2)speech.writeInt16LE(1000,i);
 const v=new Voice(dir,async()=>['第一句','第二句'][calls++],async t=>pasted.push(t),{live:true});
 try{v.start(()=>{});v.audio(speech);v.audio(Buffer.alloc(19200));await v.segment.queue;assert.deepEqual(pasted,['第一句']);
 v.audio(speech);const replies=[];await v.stop(r=>replies.push(r));assert.deepEqual(pasted,['第一句','第二句']);assert.equal(replies.at(-1).text,'第一句第二句');assert.equal(calls,2);
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('live dictation bounds continuous phrases and stops pasting after an input failure',async()=>{
 const fs=require('fs'),os=require('os'),path=require('path'),{Voice}=require('./voice.cjs');const dir=fs.mkdtempSync(path.join(os.tmpdir(),'live-error-'));let calls=0;
 const speech=Buffer.alloc(128000);for(let i=0;i<speech.length;i+=2)speech.writeInt16LE(1000,i);
 const v=new Voice(dir,async()=> '文字',async()=>{calls++;throw Error('permission');},{live:true});
 try{v.start();v.audio(speech);await v.segment.queue;assert.equal(calls,1);v.audio(speech);const replies=[];await v.stop(r=>replies.push(r));assert.equal(calls,1);assert.equal(replies.at(-1).state,'error');assert.match(replies.at(-1).text,/自动输入失败/);
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('debug log keeps bounded chronological events including recognized text',()=>{const {DebugLog}=require('./debug.cjs');const log=new DebugLog(2);const seen=[];log.listeners.add(e=>seen.push(e));log.add({event:'button'});log.add({event:'recognized',text:'<script>测试</script>'});log.add({event:'done'});assert.deepEqual(log.entries.map(e=>e.id),[2,3]);assert.equal(seen.length,3);assert.equal(log.entries[0].text,'<script>测试</script>');assert.ok(log.entries[0].time>0);});

test('three tasks in one project keep separate rows and titles',()=>{
 const s=new State(),time=new Date().toISOString();
 for(const id of ['a','b','c'])s.apply({provider:'codex',session:id,cwd:'/work/shared',title:'任务 '+id,event:id==='a'?'UserPromptSubmit':'Stop',time});
 const rows=s.snapshot().cards[1].rows;assert.equal(rows.length,3);assert.equal(new Set(rows.map(r=>r.id)).size,3);assert.equal(rows[0].project,'任务 a');
});
test('recently modified rollouts in old directories are discovered and retained across restarts',()=>{
 const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),{Sources}=require('./sources.cjs');
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'panel-discovery-')),home=path.join(root,'home'),dir=path.join(home,'sessions','2020','01','01');
 try{
  fs.mkdirSync(dir,{recursive:true});const file=path.join(dir,'old-task.jsonl'),time=new Date().toISOString();
  fs.writeFileSync(file,[{type:'session_meta',payload:{id:'old',cwd:'/work/shared'}},{type:'event_msg',timestamp:time,payload:{type:'task_complete'}}].map(JSON.stringify).join('\n')+'\n');
  fs.writeFileSync(path.join(home,'session_index.jsonl'),JSON.stringify({id:'old',thread_name:'恢复旧任务'})+'\n');
  const s=new State(),src=new Sources(s,root,{codexHome:home});src.tick();assert.equal(s.snapshot().cards[1].rows[0].project,'恢复旧任务');
  const past=new Date(Date.now()-5*86400000);fs.utimesSync(file,past,past);
  const after=new State(),again=new Sources(after,root,{codexHome:home});again.tick();assert.equal(after.snapshot(Date.now()+2*86400000).cards[1].state,'done');
  fs.appendFileSync(file,JSON.stringify({type:'event_msg',timestamp:new Date(Date.now()+1000).toISOString(),payload:{type:'task_started'}})+'\n');again.tick();assert.equal(after.sessions.get('codex:old').state,'working');
 }finally{fs.rmSync(root,{recursive:true,force:true});}
});
