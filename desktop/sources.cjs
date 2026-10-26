const fs=require('node:fs');const path=require('node:path');const os=require('node:os');
// Local rollout format is an observed adapter, not a stable public API.
function codexEvent(row,meta){
  const p=row.payload||{};const base={provider:'codex',session:meta.id||meta.session_id,cwd:meta.cwd,time:row.timestamp};
  if(row.type==='event_msg'){
    const names={task_started:'UserPromptSubmit',task_complete:'Stop',turn_aborted:'Interrupt'};
    if(names[p.type])return {...base,event:names[p.type]};
  }
  if(row.type==='response_item'&&['function_call','custom_tool_call'].includes(p.type))return {...base,event:'PreToolUse',tool:p.name};
  return null;
}
class Sources {
 constructor(state,root,{codexHome=process.env.CODEX_HOME||path.join(os.homedir(),'.codex')}={}){
  this.state=state;this.root=root;this.home=codexHome;this.files=new Map();this.lastScan=0;this.errors=[];this.titles=new Map();
  this.trackedFile=path.join(root,'data','tracked-codex.json');
  try{this.tracked=new Set(JSON.parse(fs.readFileSync(this.trackedFile,'utf8')));}catch{this.tracked=new Set();}
 }
 scan(){
  // Rollouts remain in their creation-date directory even when resumed months later.
  // Discover all directories, but read only recently modified or previously tracked files.
  let changed=false;
  const visit=dir=>{
   let entries;try{entries=fs.readdirSync(dir,{withFileTypes:true});}catch(e){if(e.code!=='ENOENT')this.errors=[e.code||'scan_error'];return;}
   for(const entry of entries){const name=path.join(dir,entry.name);
    if(entry.isDirectory()){visit(name);continue;}
    if(!entry.isFile()||!entry.name.endsWith('.jsonl'))continue;
    try{
     if(!this.tracked.has(name)&&Date.now()-fs.statSync(name).mtimeMs>86400000)continue;
     if(!this.tracked.has(name)){this.tracked.add(name);changed=true;}
     if(!this.files.has(name))this.files.set(name,{offset:0,carry:'',meta:{},kind:'codex'});
    }catch(e){if(e.code!=='ENOENT')this.errors=[e.code||'scan_error'];}
   }
  };
  visit(path.join(this.home,'sessions'));
  if(changed){try{fs.mkdirSync(path.dirname(this.trackedFile),{recursive:true});fs.writeFileSync(this.trackedFile+'.tmp',JSON.stringify([...this.tracked]));fs.renameSync(this.trackedFile+'.tmp',this.trackedFile);}catch(e){this.errors=[e.code||'save_error'];}}
  // Names only; this index does not provide the app's real read/unread state.
  try{for(const line of fs.readFileSync(path.join(this.home,'session_index.jsonl'),'utf8').split('\n')){try{const row=JSON.parse(line);if(row.id&&row.thread_name)this.titles.set(row.id,String(row.thread_name).slice(0,90));}catch{}}}catch{}
  for(const session of this.state.sessions.values())if(session.provider==='codex'&&this.titles.has(session.id))session.title=this.titles.get(session.id);
  const name=path.resolve(this.root,'cc-bridge/events.jsonl');if(!this.files.has(name))this.files.set(name,{offset:0,carry:'',kind:'claude'});
 }
 tick(){
  if(Date.now()-this.lastScan>10000){this.scan();this.lastScan=Date.now();}
  for(const [name,f] of this.files){
   try{
    if(f.ignore)continue;
    const stat=fs.statSync(name);if(stat.size<f.offset){f.offset=0;f.carry='';}
    if(stat.size===f.offset)continue;
    const fd=fs.openSync(name,'r');
    try{
     // Read metadata, then only the recent tail of old/large rollouts on startup.
     if(!f.offset&&f.kind==='codex'){
      const head=Buffer.alloc(Math.min(stat.size,2097152));fs.readSync(fd,head,0,head.length,0);
      const first=JSON.parse(head.toString('utf8').split('\n')[0]);f.meta=first.payload||{};
      if(typeof f.meta.source==='object'&&f.meta.source.subagent){f.ignore=true;f.offset=stat.size;continue;}
      if(stat.size>524288){f.offset=stat.size-524288;f.skipFirst=true;}
     }
     const bytes=Buffer.alloc(Math.min(stat.size-f.offset,2097152));const n=fs.readSync(fd,bytes,0,bytes.length,f.offset);f.offset+=n;
     const text=f.carry+bytes.subarray(0,n).toString('utf8');const lines=text.split('\n');f.carry=lines.pop();
     if(f.skipFirst){lines.shift();f.skipFirst=false;}
     for(const line of lines){try{
      const row=JSON.parse(line);if(f.kind==='claude'){this.state.apply({...row,provider:'claude'});continue;}
      if(row.type==='session_meta'){f.meta=row.payload;continue;}
      const e=codexEvent(row,f.meta);if(e)this.state.apply({...e,title:this.titles.get(e.session)});
     }catch{}}
    }finally{fs.closeSync(fd);}
   }catch(e){if(e.code!=='ENOENT')this.errors=[e.code||'read_error'];}
  }
 }
}
module.exports={Sources,codexEvent};
