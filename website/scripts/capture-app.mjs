// Render the actual compiled application with explicitly synthetic public demo data.
// No owner account, production API, credentials, or personal machine information is used.
import { chromium } from '../../web/node_modules/@playwright/test/index.mjs';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, extname } from 'node:path';

const root = resolve('web/dist');
const output = resolve('website/public/assets');
const mime = {'.js':'text/javascript','.css':'text/css','.woff2':'font/woff2','.svg':'image/svg+xml','.png':'image/png','.html':'text/html'};
const server = createServer(async (req,res) => {
  const path = new URL(req.url,'http://localhost').pathname;
  const assetPath = path.startsWith('/admin/assets/') ? path.slice('/admin'.length) : path;
  const file = path === '/admin' || path === '/admin/' ? resolve(root,'index.html') : resolve(root,'.'+assetPath);
  if (!file.startsWith(root+'/')) {res.writeHead(404).end(); return;}
  try { res.setHeader('Content-Type',mime[extname(file)]||'application/octet-stream');res.end(await readFile(file)); }
  catch {res.writeHead(404).end();}
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const base = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch();
try {
  const page = await browser.newPage({viewport:{width:1440,height:1120},deviceScaleFactor:1});
  await page.addInitScript(() => {
    localStorage.setItem('guard-guide-dismissed','1');
    const reading=(value,unit='%')=>({value,unit,quality:'good',reason:null});
    class DemoStream extends EventTarget {
      constructor() {super(); this.stopped=false; this.count=0; this.timer=setInterval(()=>{
        if(this.stopped) return;
        const n=this.count++;
        const cpu=32+10*Math.sin(n/13)+4*Math.sin(n/3.2)+(n>115&&n<155?28:0);
        const sample={source_id:'demo-host',at:Date.now()/1000-Math.max(0,299-n),uptime:1843200,
          cpu:reading(cpu),cores:[28,34,29,37],memory:reading(58+2*Math.sin(n/35)),memory_used:9.3*1024**3,memory_total:16*1024**3,
          network_in:reading(850000+250000*Math.sin(n/11),'B/s'),network_out:reading(180000,'B/s'),
          disks:[{id:'/',mount:'/',total:200*1024**3,available:124*1024**3,used_percent:reading(38)}],
          diagnostics:[],processes:[['nginx',2.4,48],['postgres',8.1,612],['node',5.2,184],['server-guard',.6,23]].map(([name,cpu,mem],i)=>({id:`demo-${i}`,pid:1200+i,started_at:1,name,cpu,memory:mem*1024**2})),agent_memory:23*1024**2,agent_cpu:.6};
        this.dispatchEvent(new MessageEvent('sample',{data:JSON.stringify(sample)}));
      },nDelay()); }
      close(){this.stopped=true;clearInterval(this.timer)}
    }
    function nDelay(){return 12}
    window.EventSource=DemoStream;
  });
  const rule=(id,name,metric,threshold,window_seconds=300,severity='warning')=>({id,name,source:'host',metric,operator:'>',threshold,window_seconds,fraction:.8,severity,recovery_threshold:threshold-5,recovery_seconds:120,enabled:true,notify:true});
  const balanced=[rule('cpu','Sustained high CPU','cpu',80),rule('cpu-critical','Critical CPU load','cpu',90,180,'critical'),rule('ram','Memory pressure','memory',90),rule('ram-critical','Critical memory pressure','memory',95,180,'critical')];
  const presets=[{id:'balanced',name:'Balanced monitoring',description:'Reliable 3–5 minute defaults for CPU and memory.',rules:balanced},{id:'sensitive',name:'Sensitive monitoring',description:'Earlier alerts: 2-minute windows and lower thresholds.',rules:[rule('s1','Early CPU','cpu',70,120),rule('s2','Early RAM','memory',80,120)]},{id:'sustained',name:'Sustained load',description:'Alert when CPU stays high for 1 hour or memory for 30 minutes.',rules:[rule('l1','Sustained CPU','cpu',70,3600),rule('l2','Sustained RAM','memory',85,1800)]}];
  const settings={settings:{alias:'production-01',recipient:'',recipient_verified:false,report_times:['09:00'],timezone:'UTC',instant:true,reports:true,recovery:true,relay_url:'',ai_provider:'openai',ai_base_url:'https://api.groq.com/openai/v1',ai_model:'',ai_auto:false,webhook_url:''},keys:{ai:false,relay:false,webhook:false}};
  await page.route('**/api/v1/**',async route=>{
    const path=new URL(route.request().url()).pathname.replace('/api/v1/','');
    const data={
      'auth/status':{configured:true,session:{csrf:'public-demo-not-a-real-token'}},
      host:{id:'demo-host',name:'production-01',os:'Ubuntu 24.04 LTS',cpu_model:'4-core virtual CPU',cores:4,memory_total:16*1024**3,version:'0.1.0'},
      incidents:{items:[]},rules:balanced,'rules/presets':presets,settings,
      'mcp/clients':[],'notifications':{items:[]},'mcp/connection':{command:'/usr/local/bin/server-guard',args:['--data-dir','/var/lib/server-guard','mcp'],key_variable:'GUARD_MCP_KEY',tools:['server_info','current_metrics','history','list_incidents','incident_detail','log_summary'],key_file_supported:true,suggested_transport:'ssh',ssh:{host:'203.0.113.10',user:'root',port:22}}
    }[path];
    await route.fulfill({status:data===undefined?404:200,contentType:'application/json',body:JSON.stringify(data??{error:'Not part of the screenshot fixture'})});
  });
  await mkdir(output,{recursive:true});
  await page.goto(base+'/admin');
  await page.getByRole('heading',{name:/^Overview/}).waitFor();
  await page.waitForTimeout(4300);
  await page.screenshot({path:output+'/dashboard.jpg',quality:90,type:'jpeg',animations:'disabled'});
  await page.getByRole('navigation').getByRole('button',{name:'Alert rules',exact:true}).click();
  await page.getByRole('heading',{name:'Ready-made rule packs',exact:true}).waitFor();
  await page.screenshot({path:output+'/rules.jpg',quality:90,type:'jpeg',animations:'disabled'});
  await page.getByRole('button',{name:'Notifications & connections',exact:true}).click();
  await page.getByRole('tab',{name:'MCP clients',exact:true}).click();
  await page.getByRole('heading',{name:'Connect your AI agent',exact:true}).waitFor();
  await page.getByRole('button',{name:'Create connection',exact:true}).waitFor();
  await page.screenshot({path:output+'/mcp.jpg',quality:90,type:'jpeg',fullPage:true,animations:'disabled'});
  await page.getByRole('button',{name:'Overview',exact:true}).click();
  await page.getByRole('button',{name:'Switch theme',exact:true}).click();
  await page.waitForTimeout(200);
  await page.screenshot({path:output+'/dashboard-light.jpg',quality:90,type:'jpeg',animations:'disabled'});
  console.log('Captured four real application views using synthetic, non-sensitive data.');
} finally {await browser.close();server.close();}
