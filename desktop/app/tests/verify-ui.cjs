// Headless verification of this project's UI, with a fake Tauri bridge. Never
// attaches to the user's browser or desktop app; no keys or microphone are used.
const { chromium } = require(process.env.PANEL_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

(async () => {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.PANEL_CHROME || chromium.executablePath() });
  try {
    const page = await browser.newPage({ viewport: { width: 1000, height: 700 } });
    const failures = [];
    page.on('pageerror', e => failures.push(e.message));
    const now = Date.now();
    const screen = { split: 50, brightness: 160, claudeColor: '#df9877', codexColor: '#63d9ca', deepseekColor: '#7c9fff', showTool: true, showElapsed: true, swap: false };
    const cards = ['claude', 'codex', 'deepseek'].map((provider, i) => ({ provider, id: 'task-'+i,
      state: ['working','done','waiting'][i], title: ['更新项目界面','接口回归测试','DeepSeek Harness 接入'][i],
      project: 'project', tool: 'pwsh', startedAt: now-90000, endedAt: now-10000, updatedAt: now-15000, unread: i===1, count:1, rows:[] }));
    const snapshot = { now, config: { screen, voice: { outputDevice:null, hotkey:{key:'F8',ctrl:true} } },
      cards, sessions:cards, pinned:{claude:null,codex:null,deepseek:null},
      device:{connected:true,ip:'192.168.1.186',ssid:'Wi-Fi',ssids:[],rssi:-55,fw:3,lastSeen:now},server:{port:8787},sourceErrors:[] };
    await page.route('http://panel.test/**', route => {
      const name = new URL(route.request().url()).pathname;
      const files = {'/':'index.html','/app.css':'app.css','/app.js':'app.js'};
      if(!files[name])return route.fulfill({status:404,body:''});
      return route.fulfill({status:200,contentType:name.endsWith('.js')?'text/javascript':name.endsWith('.css')?'text/css':'text/html',body:fs.readFileSync(path.join(__dirname,'../ui',files[name]))});
    });
    await page.addInitScript(({snapshot})=>{
      window.__calls=[];
      const listeners={};
      window.__TAURI__={event:{listen:async(name,fn)=>{listeners[name]=fn;return ()=>{};}},core:{invoke:async(name,args)=>{
        window.__calls.push({name,args});
        switch(name){
          case 'get_snapshot':return snapshot;
          case 'get_debug_log':return [];
          case 'voice_status':return {ready:true,message:'CABLE Input 已就绪'};
          case 'list_audio_devices':return ['CABLE Input'];
          case 'list_serial_ports':return [];
          case 'list_addresses':return ['192.168.0.64'];
          case 'get_autostart':return false;
          case 'hooks_status':return {installedEvents:[],missingEvents:[],path:'settings.json'};
        }
      }}};
    },{snapshot});
    await page.goto('http://panel.test/');
    await page.waitForFunction(()=>document.querySelector('.card[data-provider="deepseek"] .card-state').textContent==='等你确认');
    assert.deepEqual(await page.locator('.card .brand').allTextContents(),['Claude','Codex','DeepSeek']);
    const boxes=await page.locator('.card').evaluateAll(nodes=>nodes.map(n=>({x:n.getBoundingClientRect().x,y:n.getBoundingClientRect().y})));
    assert(boxes.every(b=>b.y===boxes[0].y),'three cards share one row at desktop width');
    assert(boxes[0].x<boxes[1].x&&boxes[1].x<boxes[2].x);
    await page.locator('.session[data-provider="deepseek"] [data-action="pin"]').click();
    assert(await page.evaluate(()=>window.__calls.some(c=>c.name==='select_session'&&c.args.provider==='deepseek')));
    const output = process.env.PANEL_SCREENSHOT || path.join(__dirname,'../../../build/deepseek-desktop.png');
    fs.mkdirSync(path.dirname(output),{recursive:true});
    await page.screenshot({path:output,fullPage:true});
    await page.getByRole('button',{name:'设置',exact:true}).click();
    await page.locator('input[name="deepseekColor"]').fill('#829cff');
    await page.waitForTimeout(450);
    const save=await page.evaluate(()=>window.__calls.findLast(c=>c.name==='save_screen_config'));
    assert.equal(save.args.config.deepseekColor,'#829cff');
    assert.equal(save.args.config.split,50);
    await page.setViewportSize({width:420,height:700});
    await page.getByRole('button',{name:'状态',exact:true}).click();
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal overflow');
    assert.deepEqual(failures,[]);
    console.log(JSON.stringify({passed:true,screenshot:output,checks:['three cards','DeepSeek waiting state','pin command','color save','420px layout','no script errors']}));
  } finally { await browser.close(); }
})().catch(e=>{console.error(e);process.exitCode=1;});
