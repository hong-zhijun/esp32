// Local browser diagnostics only. Bounded memory; never saved to disk.
class DebugLog {
 constructor(limit=300){this.limit=limit;this.entries=[];this.next=1;this.listeners=new Set();}
 add(event){const row={...event,id:this.next++,time:Date.now()};this.entries.push(row);if(this.entries.length>this.limit)this.entries.shift();for(const listener of this.listeners)listener(row);return row;}
}
module.exports={DebugLog};
