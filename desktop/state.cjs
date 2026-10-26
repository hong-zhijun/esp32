const path = require('node:path');
const defaults = {split:50,brightness:160,claudeColor:'#df9877',codexColor:'#63d9ca',showTool:true,showElapsed:true,swap:false};
function validateConfig(input) {
  const result={};
  for(const [k,lo,hi] of [['split',35,65],['brightness',25,255]]) {
    if(!Number.isInteger(input[k]) || input[k]<lo || input[k]>hi) throw Error('Invalid '+k);
    result[k]=input[k];
  }
  for(const k of ['claudeColor','codexColor']) {if(!/^#[0-9a-f]{6}$/i.test(input[k]))throw Error('Invalid color');result[k]=input[k];}
  for(const k of ['showTool','showElapsed','swap']) {if(typeof input[k]!=='boolean')throw Error('Invalid toggle');result[k]=input[k];}
  return result;
}
class State {
  constructor(){this.sessions=new Map();this.selected={};}
  apply(e){
    if(!['claude','codex'].includes(e.provider)||!e.session)return;
    const time=Date.parse(e.time);if(!Number.isFinite(time))return;
    const key=e.provider+':'+e.session;
    let s=this.sessions.get(key);
    if(s && time<s.updatedAt)return;
    s=s||{id:e.session,provider:e.provider,state:'idle',project:'',tool:'',startedAt:0,unread:false};
    s.updatedAt=time;
    if(e.cwd)s.project=path.win32.basename(e.cwd)||path.basename(e.cwd);
    if(e.title)s.title=String(e.title).slice(0,90);
    if(e.event==='UserPromptSubmit'){s.state='working';s.startedAt=time;s.endedAt=0;s.unread=false;s.tool='';}
    if(e.event==='PreToolUse'||e.event==='PostToolUse'){s.state='working';s.tool=String(e.tool||'').slice(0,70);if(!s.startedAt)s.startedAt=time;}
    if(e.event==='PermissionRequest'||(e.event==='Notification'&&['permission_prompt','idle_prompt','elicitation_dialog'].includes(e.notification))){s.state='waiting';}
    if(e.event==='Stop'){s.state='done';s.endedAt=time;s.unread=true;s.tool='';}
    if(e.event==='Interrupt'){s.state='interrupted';s.endedAt=time;s.tool='';}
    if(e.event==='SessionEnd'){s.state=s.unread?'done':'idle';s.endedAt=s.endedAt||time;}
    this.sessions.set(key,s);
  }
  acknowledge(provider,id){const s=this.sessions.get(provider+':'+id);if(s)s.unread=false;}
  snapshot(now=Date.now()){
    const view=s=>['working','waiting'].includes(s.state)&&now-s.updatedAt>300000?{...s,state:'unknown'}:s.state==='done'&&!s.unread?{...s,state:'idle'}:{...s};
    const all=[...this.sessions.values()].filter(s=>(s.state==='done'&&s.unread)||now-s.updatedAt<24*3600000).map(view);
    const cards=['claude','codex'].map(provider=>{
      const rows=all.filter(s=>s.provider===provider).sort((a,b)=>{
        const p=s=>s.state==='waiting'?4:s.state==='working'?3:s.unread?2:1;
        return p(b)-p(a)||b.updatedAt-a.updatedAt;
      });
      const chosen=rows.find(s=>s.id===this.selected[provider])||rows[0];
      // Existing firmware renders rows.project; use a short task title in that display field.
      const list=rows.slice(0,3).map(s=>({id:s.id,project:[...(s.title||s.project||s.id)].slice(0,18).join(''),state:s.state}));
      return chosen?{...chosen,count:rows.length,rows:list}:{provider,id:'',state:'idle',project:'',count:0,unread:false,rows:[]};
    });return {cards,sessions:all};
  }
}
module.exports={State,defaults,validateConfig};
