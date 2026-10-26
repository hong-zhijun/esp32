const test=require('node:test'),assert=require('node:assert/strict'),{EventEmitter}=require('node:events'),{PassThrough}=require('node:stream');
const {WeTypeVoice}=require('./wetype-voice.cjs');
const delay=ms=>new Promise(r=>setTimeout(r,ms));
function fakeChild(ready=true){const c=new EventEmitter();c.stdin=new PassThrough();c.stdout=new PassThrough();c.stderr=new PassThrough();c.received=[];c.stdin.on('data',b=>c.received.push(Buffer.from(b)));c.stdin.on('finish',()=>setImmediate(()=>c.emit('close',0)));c.kill=()=>setImmediate(()=>c.emit('close',1));if(ready)setImmediate(()=>c.stdout.write(JSON.stringify({event:'ready',device:{name:'Test cable'}})+'\n'));return c;}
test('wireless bridge buffers initial audio, streams exact PCM, drains then stops WeType',async()=>{const c=fakeChild(),keys=[],logs=[],replies=[];const v=new WeTypeVoice({watch:()=>()=>{},spawnProcess:()=>c,trigger:async()=>keys.push(Date.now()),log:e=>logs.push(e)});v.start(r=>replies.push(r));const a=Buffer.alloc(640,7),b=Buffer.alloc(640,8);v.audio(a);await v.session.readyPromise;assert.equal(keys.length,1);v.audio(b);await v.stop(r=>replies.push(r));assert.deepEqual(Buffer.concat(c.received),Buffer.concat([a,b]));assert.equal(keys.length,2);assert.equal(replies.at(-1).state,'finished');assert.equal(v.session,null);assert.equal(logs.filter(x=>x.event==='wetype-started').length,1);});
test('missing virtual cable never triggers voice shortcut or local recognition',async()=>{const c=fakeChild(false),keys=[],replies=[];const v=new WeTypeVoice({watch:()=>()=>{},spawnProcess:()=>c,trigger:async()=>keys.push(1)});v.start(r=>replies.push(r));const pending=v.session.readyPromise;c.stdout.write(JSON.stringify({event:'error',message:'no cable'})+'\n');await pending;assert.equal(keys.length,0);assert.equal(replies.at(-1).state,'error');assert.equal(v.session,null);});
test('disconnect stops forwarding and ends WeType once',async()=>{const c=fakeChild(),keys=[],replies=[];const v=new WeTypeVoice({watch:()=>()=>{},spawnProcess:()=>c,trigger:async()=>keys.push(1)});v.start(r=>replies.push(r));await v.session.readyPromise;v.cancel();v.audio(Buffer.alloc(640));await new Promise(r=>setImmediate(r));assert.equal(keys.length,2);assert.equal(c.received.length,0);assert.equal(replies.at(-1).state,'error');});
test('startup buffering is bounded while WeType is unavailable',async()=>{const c=fakeChild(false),keys=[],replies=[];const v=new WeTypeVoice({watch:()=>()=>{},spawnProcess:()=>c,trigger:async()=>keys.push(1)});v.start(r=>replies.push(r));const pending=v.session.readyPromise;v.audio(Buffer.alloc(96002));await pending;assert.equal(keys.length,0);assert.equal(v.session,null);assert.match(replies.at(-1).text,/过慢/);});
test('disconnect during shortcut startup completes cleanup before allowing another session',async()=>{
 const c=fakeChild(),replies=[],keys=[];let finishStart;
 const v=new WeTypeVoice({watch:()=>()=>{},spawnProcess:()=>c,trigger:()=>{keys.push(keys.length+1);return keys.length===1?new Promise(r=>finishStart=r):Promise.resolve();}});
 v.start(r=>replies.push(r));const session=v.session;
 while(!finishStart)await new Promise(r=>setImmediate(r));
 v.cancel();assert.equal(v.session,session);
 const second=[];v.start(r=>second.push(r));assert.equal(second[0].state,'error');assert.equal(keys.length,1);
 finishStart();await session.readyPromise;await session.cleanup;
 assert.deepEqual(keys,[1,2]);assert.equal(v.session,null);assert.equal(c.received.length,0);
});
test('computer stop stops PCM and finishes device without reopening WeType',async()=>{
 const c=fakeChild(),keys=[],replies=[];let state;
 const v=new WeTypeVoice({watch:cb=>{state=cb;return ()=>{};},spawnProcess:()=>c,trigger:async()=>keys.push(1)});
 v.start(r=>replies.push(r));const s=v.session;await s.readyPromise;
 state(false);await delay(280);assert.equal(v.session,s); // Startup idle is not a stop.
 state(true);state(false);await delay(280);await s.cleanup;
 v.audio(Buffer.alloc(640));assert.equal(c.received.length,0);
 assert.equal(replies.at(-1).state,'finished');assert.equal(keys.length,1);assert.equal(v.session,null);
});
test('brief input transition does not end recording; close during tail does not toggle again',async()=>{
 const c=fakeChild(),keys=[],replies=[];let state;
 const v=new WeTypeVoice({watch:cb=>{state=cb;return ()=>{};},spawnProcess:()=>c,trigger:async()=>keys.push(1)});
 v.start(r=>replies.push(r));const s=v.session;await s.readyPromise;
 state(true);state(false);state(true);await delay(280);assert.equal(v.session,s);
 const stopped=v.stop(r=>replies.push(r));state(false);await stopped;await s.cleanup;
 assert.equal(keys.length,1);assert.equal(replies.filter(r=>r.state==='finished').length,1);assert.equal(v.session,null);
});
