// Browser rendering acceptance for the actual Rust email renderer, never SMTP.
import { chromium } from '../node_modules/@playwright/test/index.mjs';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, extname } from 'node:path';
import assert from 'node:assert/strict';
const output=resolve('artifacts/validation/email-templates');
const generated=spawnSync('cargo',['run','--quiet','-p','guard-core','--example','email-preview','--',output],{encoding:'utf8'});
assert.equal(generated.status,0,generated.stderr);
await mkdir('artifacts/screenshots/email-templates',{recursive:true});
const server=createServer(async(req,res)=>{
  const file=resolve(output,'.'+new URL(req.url,'http://localhost').pathname);
  if(!file.startsWith(output+'/')){res.writeHead(404).end();return;}
  try{res.setHeader('Content-Type',extname(file)==='.html'?'text/html':'text/plain');res.end(await readFile(file));}
  catch{res.writeHead(404).end();}
});
await new Promise(done=>server.listen(0,'127.0.0.1',done));
const base=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch();
const checks=[];
try{
 const page=await browser.newPage();const requests=[];const errors=[];
 page.on('request',r=>requests.push(r.url()));page.on('pageerror',e=>errors.push(e.message));
 for(const name of ['warning','critical','recovery','summary','verification','test']){
  for(const width of [600,390,320]){
   await page.setViewportSize({width,height:1000});
   await page.goto(`${base}/${name}.html`,{waitUntil:'networkidle'});
   assert.equal(await page.locator('h1').count(),1);
   assert.equal(await page.locator('script,form,input,img,link').count(),0);
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`${name} overflow ${width}`);
   const links=await page.locator('a').evaluateAll(a=>a.map(x=>x.getAttribute('href')));
   assert.deepEqual(links,['verification','test'].includes(name)?[]:['https://panel.example.com/admin']);
   if(width!==320)await page.screenshot({path:`artifacts/screenshots/email-templates/${name}-${width}.png`,fullPage:true});
   checks.push({name,width,overflow:false,links:links.length});
  }
 }
 assert.deepEqual(requests.filter(url=>!url.startsWith(base+'/')),[],'External assets/tracking must not load');
 assert.deepEqual(errors,[]);
 await writeFile(`${output}/browser-acceptance.json`,JSON.stringify({synthetic:true,email_sent:false,checks},null,2));
 console.log(JSON.stringify({templates:6,viewportChecks:checks.length,email_sent:false,external_requests:0}));
}finally{await browser.close();server.close();}
