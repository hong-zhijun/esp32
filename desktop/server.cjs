const http=require('node:http'),fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),os=require('node:os');
const {WebSocketServer}=require('ws');const {SerialPort}=require('serialport');
const {State,defaults,validateConfig}=require('./state.cjs');const {Sources}=require('./sources.cjs');const {Voice}=require('./voice.cjs');const {recognize,load:loadRecognizer}=require('./recognizer.cjs');const {typeText}=require('./typer.cjs');
const root=__dirname,port=Number(process.env.PORT||8787);const data=path.join(root,'data');fs.mkdirSync(data,{recursive:true});
const read=(name,fallback)=>{try{return JSON.parse(fs.readFileSync(path.join(data,name),'utf8'));}catch{return fallback;}};
const save=(name,value)=>{const p=path.join(data,name);fs.writeFileSync(p+'.tmp',JSON.stringify(value,null,2));fs.renameSync(p+'.tmp',p);};
let config=read('config.json',defaults);try{config=validateConfig(config);}catch{config=defaults;}
const token=read('device.json',null)?.token||crypto.randomBytes(24).toString('hex');save('device.json',{token});
const state=new State(),sources=new Sources(state,root);
const voice=new Voice(path.join(data,'voice'),async pcm=>{const text=recognize(pcm);if(text)await typeText(text);return text;});const devices=new Set();let lastDeviceAt=0;let serialBusy=false;
const acknowledged=read('acknowledged.json',{});
function acknowledge(provider,id){const s=state.sessions.get(provider+':'+id);if(s){acknowledged[provider+':'+id]=s.updatedAt;save('acknowledged.json',acknowledged);state.acknowledge(provider,id);}}
const isLocal=req=>['127.0.0.1','::1','::ffff:127.0.0.1'].includes(req.socket.remoteAddress);
const addresses=()=>Object.values(os.networkInterfaces()).flat().filter(x=>x.family==='IPv4'&&!x.internal&&!x.address.startsWith('198.18.')).map(x=>x.address).sort((a,b)=>Number(/^(192\.168\.|10\.)/.test(b))-Number(/^(192\.168\.|10\.)/.test(a)));
function snapshot(){return {type:'state',now:Date.now(),config,...state.snapshot(),device:{connected:devices.size>0,lastSeen:lastDeviceAt},sources:{claude:'hooks',codex:'local-rollout',errors:sources.errors}};}
function broadcast(){const payload=JSON.stringify(snapshot());for(const c of wss.clients){if(c.readyState===1)c.send(payload);}}
async function body(req){let s='';for await(const c of req){s+=c;if(s.length>8192)throw Error('Request too large');}return JSON.parse(s||'{}');}
async function provision(input){
 if(serialBusy)throw Error('USB 配网正在进行');
 if(typeof input.ssid!=='string'||!input.ssid||Buffer.byteLength(input.ssid)>32)throw Error('SSID 不正确');
 if(typeof input.password!=='string'||Buffer.byteLength(input.password)>63)throw Error('密码过长');
 if(!addresses().includes(input.host))throw Error('请选择电脑局域网地址');
 const ports=await SerialPort.list();if(!ports.some(p=>p.path===input.port))throw Error('串口不存在');
 serialBusy=true;
 try{await new Promise((resolve,reject)=>{
   const sp=new SerialPort({path:input.port,baudRate:115200,autoOpen:false});let buffer='',finished=false;
   const done=(err)=>{if(finished)return;finished=true;clearTimeout(timer);if(sp.isOpen)sp.close(()=>err?reject(err):resolve());else err?reject(err):resolve();};
   const timer=setTimeout(()=>done(Error('设备未确认，请确认固件已烧录、串口未被占用')),15000);
   sp.on('error',()=>done(Error('串口连接失败')));sp.on('data',chunk=>{buffer+=chunk.toString();if(buffer.includes('CONFIG_SAVED'))done();});
   sp.open(err=>{if(err)return done(Error('无法打开串口，请关闭 Arduino 串口监视器'));setTimeout(()=>sp.write(JSON.stringify({type:'configure',ssid:input.ssid,password:input.password,host:input.host,port,token})+'\n'),1500);});
 });}finally{serialBusy=false;}
}
const server=http.createServer(async(req,res)=>{
 try{
  if(!isLocal(req)){res.writeHead(403);return res.end('Editor is localhost only');}
  if(req.method==='POST'&&req.headers.origin&&req.headers.origin!==`http://localhost:${port}`&&req.headers.origin!==`http://127.0.0.1:${port}`){res.writeHead(403);return res.end();}
  res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');
  if(req.url==='/api/state'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(snapshot()));}
  if(req.url==='/api/setup'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify({addresses:addresses(),ports:(await SerialPort.list()).map(p=>({path:p.path,label:p.friendlyName||p.manufacturer||p.path}))}));}
  if(req.method==='POST'){
   const b=await body(req);
   if(req.url==='/api/config'){config=validateConfig(b);save('config.json',config);}
   else if(req.url==='/api/ack'){acknowledge(b.provider,b.id);}
   else if(req.url==='/api/select'){if(['claude','codex'].includes(b.provider))state.selected[b.provider]=String(b.id);}
   else if(req.url==='/api/provision'){await provision(b);}
   else{res.writeHead(404);return res.end();}
   broadcast();res.setHeader('Content-Type','application/json');return res.end('{"ok":true}');
  }
  const assets={'/':'index.html','/app.js':'app.js','/style.css':'style.css'};const file=assets[req.url];
  if(!file){res.writeHead(404);return res.end();}
  res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'text/html; charset=utf-8');res.end(fs.readFileSync(path.join(root,'public',file)));
 }catch(e){res.writeHead(400,{'Content-Type':'application/json'});res.end(JSON.stringify({error:e.message}));}
});
const wss=new WebSocketServer({noServer:true,maxPayload:8192});
server.on('connection',socket=>{if(!['127.0.0.1','::1','::ffff:127.0.0.1'].includes(socket.remoteAddress))console.log(JSON.stringify({event:'tcp-connect',remote:socket.remoteAddress,time:new Date().toISOString()}));});
server.on('clientError',(err,socket)=>{console.log(JSON.stringify({event:'http-error',code:err.code,remote:socket.remoteAddress}));socket.destroy();});
server.on('upgrade',(req,socket,head)=>{
 const url=new URL(req.url,'http://localhost');const device=url.pathname==='/device'&&url.searchParams.get('token')===token;
 const browser=url.pathname==='/live'&&isLocal(req)&&[`http://localhost:${port}`,`http://127.0.0.1:${port}`].includes(req.headers.origin);
 if(url.pathname==='/device')console.log(JSON.stringify({event:'device-handshake',time:new Date().toISOString(),remote:socket.remoteAddress,authorized:device}));
 if(!device&&!browser){socket.destroy();return;}
 wss.handleUpgrade(req,socket,head,ws=>{ws.isDevice=device;ws.alive=true;ws.on('pong',()=>{ws.alive=true;if(device)lastDeviceAt=Date.now();});
  if(device){devices.add(ws);lastDeviceAt=Date.now();}
  ws.on('close',(code)=>{devices.delete(ws);if(device)console.log(JSON.stringify({event:'device-close',code,time:new Date().toISOString()}));});ws.on('error',()=>{});
  ws.on('message',(raw,binary)=>{if(device&&binary)return voice.audio(raw);try{const m=JSON.parse(raw);if(device&&m.type==='ack'){acknowledge(m.provider,m.id);broadcast();}
   if(device&&m.type==='voice'){if(m.action==='start')voice.start();if(m.action==='stop')voice.stop(r=>{if(ws.readyState===1)ws.send(JSON.stringify(r));});}}catch{}});
  ws.send(JSON.stringify(snapshot()));
 });
});
setInterval(()=>{sources.tick();for(const [key,s] of state.sessions){if(acknowledged[key]>=s.updatedAt)s.unread=false;}broadcast();},1000);
setInterval(()=>{for(const c of wss.clients){if(!c.alive){c.terminate();continue;}c.alive=false;c.ping();}},30000);
server.listen(port,'0.0.0.0',()=>{console.log(`Status editor: http://localhost:${port}`);setTimeout(()=>{try{loadRecognizer();}catch(e){console.log(JSON.stringify({event:'voice-model',message:e.message}));}},100);});
