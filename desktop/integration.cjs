const assert=require('node:assert/strict'),fs=require('node:fs');const WebSocket=require('ws');
(async()=>{
 const origin='http://localhost:8787';const initial=await fetch(origin+'/api/state').then(r=>r.json());assert.equal(initial.cards.length,2);
 const token=JSON.parse(fs.readFileSync(__dirname+'/data/device.json')).token;
 const ws=new WebSocket(`ws://localhost:8787/device?token=${token}`);
 const first=await new Promise((resolve,reject)=>{ws.once('message',r=>resolve(JSON.parse(r)));ws.once('error',reject);setTimeout(()=>reject(Error('WebSocket timeout')),5000).unref();});
 assert.equal(first.type,'state');assert.equal(first.config.split,initial.config.split);
 const invalid=await fetch(origin+'/api/config',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...initial.config,split:100})});assert.equal(invalid.status,400);
 const forbidden=await fetch(origin+'/api/config',{method:'POST',headers:{Origin:'http://example.com','Content-Type':'application/json'},body:JSON.stringify(initial.config)});assert.equal(forbidden.status,403);
 ws.close();console.log('PASS: HTTP state, authenticated device snapshot, config validation, cross-origin rejection');
})().catch(e=>{console.error(e.message);process.exit(1);});
