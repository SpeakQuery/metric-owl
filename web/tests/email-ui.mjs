// Local UI acceptance only: synthetic settings, intercepted APIs, no email/network delivery.
import {chromium, expect} from '@playwright/test';
import {createServer} from 'node:http';
import {readFile, mkdir} from 'node:fs/promises';
import {resolve, extname} from 'node:path';
import assert from 'node:assert/strict';

const root=resolve('web/dist');
const mime={'.js':'text/javascript','.css':'text/css','.woff2':'font/woff2','.svg':'image/svg+xml','.png':'image/png','.html':'text/html'};
const server=createServer(async(request,response)=>{
 const path=new URL(request.url,'http://localhost').pathname;
 const asset=path.startsWith('/admin/assets/')?path.slice('/admin'.length):path;
 const file=path==='/admin'?resolve(root,'index.html'):resolve(root,'.'+asset);
 if(!file.startsWith(root+'/')){response.writeHead(404).end();return}
 try{response.setHeader('Content-Type',mime[extname(file)]||'application/octet-stream');response.end(await readFile(file))}catch{response.writeHead(404).end()}
});
await new Promise(done=>server.listen(0,'127.0.0.1',done));
const base=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch();
let settings={alias:'Example server',email_transport:'disabled',smtp_host:'',smtp_port:465,smtp_security:'tls',smtp_username:'',smtp_from:'',recipient:'',recipient_verified:false,relay_url:'',timezone:'UTC',report_times:['09:00'],instant:true,reports:true,recovery:true,ai_provider:'openai',ai_base_url:'',ai_model:'',webhook_url:''};
let savedPassword=false, rejectConnection=true, requireReauth=false, testEmails=0;
const writes=[],checks=[],errors=[],external=[];
try{
 const page=await browser.newPage({viewport:{width:1440,height:1050}});
 page.on('pageerror',error=>errors.push(error.message));
 page.on('request',request=>{if(!request.url().startsWith(base))external.push(request.url())});
 await page.addInitScript(()=>{localStorage.setItem('guard-guide-dismissed','1');window.EventSource=class extends EventTarget{close(){}}});
 await page.route('**/api/v1/**',async route=>{
  const path=new URL(route.request().url()).pathname.replace('/api/v1/','');
  const respond=(body,status=200)=>route.fulfill({status,contentType:'application/json',body:JSON.stringify(body)});
  if(path==='settings'){
   if(route.request().method()==='PUT'){
    const body=route.request().postDataJSON();writes.push(body);
    if(requireReauth)return respond({error:'Please re-authenticate before changing sensitive settings.'},403);
    if(savedPassword&&(settings.smtp_host!==body.settings.smtp_host||settings.smtp_username!==body.settings.smtp_username)&&!body.keys.smtp)return respond({error:'Re-enter the SMTP password when changing the server or username.'},400);
    settings={...body.settings};if(body.keys.smtp)savedPassword=true;
    return respond({ok:true});
   }
   return respond({settings,keys:{smtp:savedPassword,relay:false,ai:false,webhook:false}});
  }
  if(path==='connections/test'){
   checks.push(route.request().postDataJSON().kind);
   return rejectConnection?respond({error:'SMTP TLS verification failed. Check the provider hostname and connection security.'},400):respond({message:'SMTP connection verified. TLS and authentication succeeded. No email was sent.'});
  }
  if(path==='recipients/start')return respond({message:'Verification message accepted by your SMTP provider. Check your mailbox.'});
  if(path==='recipients/verify'){settings.recipient_verified=true;return respond({ok:true})}
  if(path==='notifications/test'){assert.equal(settings.recipient_verified,true);testEmails++;return respond({message:'Test email queued. Provider acceptance does not confirm inbox delivery.'})}
  if(path==='auth/reauth'){requireReauth=false;assert.equal(route.request().postDataJSON().code,'');return respond({ok:true})}
  const data={
   'auth/status':{configured:true,mfa_required:false,session:{csrf:'fixture'}},
   host:{name:'Example server',os:'Ubuntu 24.04',cores:2,memory_total:8*1024**3,version:'preview'},
   incidents:{items:[]},notifications:{items:[]},rules:[],'mcp/clients':[],
  }[path];
  return respond(data??{error:'Unexpected fixture endpoint'},data===undefined?404:200);
 });
 await page.goto(base+'/admin');
 await page.getByRole('button',{name:'Notifications & connections',exact:true}).click();
 await expect(page.getByText('Email is off.',{exact:false})).toBeVisible();
 await expect(page.getByRole('button',{name:'Send verification code',exact:true})).toBeDisabled();
 await page.getByLabel('Email delivery',{exact:true}).selectOption('smtp');
 await expect(page.getByLabel('Service URL',{exact:true})).toHaveCount(0);
 await page.getByLabel('SMTP server',{exact:true}).fill('smtp.example.com');
 await page.getByLabel('Connection security',{exact:true}).selectOption('starttls');
 await expect(page.getByLabel('SMTP port',{exact:true})).toHaveValue('587');
 await page.getByLabel('Connection security',{exact:true}).selectOption('tls');
 await expect(page.getByLabel('SMTP port',{exact:true})).toHaveValue('465');
 await page.getByLabel('SMTP username',{exact:true}).fill('notifications@example.com');
 await page.getByLabel('SMTP password',{exact:true}).fill('Synthetic-test-only-secret');
 await page.getByLabel('Sender address',{exact:true}).fill('notifications@example.com');
 await page.getByLabel('Notification email address',{exact:true}).fill('recipient@example.com');
 await page.getByRole('button',{name:'Test SMTP connection',exact:true}).click();
 await expect(page.getByRole('alert')).toContainText('TLS verification failed');
 assert.equal(writes[0].keys.smtp,'Synthetic-test-only-secret');
 await expect(page.getByLabel('SMTP password',{exact:true})).toHaveValue('');
 rejectConnection=false;
 await page.getByRole('button',{name:'Test SMTP connection',exact:true}).click();
 await expect(page.getByRole('status')).toContainText('No email was sent');
 assert.deepEqual(writes.at(-1).keys,{});assert.equal(testEmails,0);assert.deepEqual(checks,['smtp','smtp']);
 await expect(page.getByRole('button',{name:'Send test email',exact:true})).toBeDisabled();
 await page.getByRole('button',{name:'Send verification code',exact:true}).click();
 await expect(page.getByRole('status')).toContainText('Check your mailbox');
 await page.getByLabel('Verification code',{exact:true}).fill('123456');
 await page.getByRole('button',{name:'Verify',exact:true}).click();
 await expect(page.getByRole('button',{name:'Send test email',exact:true})).toBeEnabled();
 await page.getByRole('button',{name:'Send test email',exact:true}).click();
 await expect(page.getByRole('status')).toContainText('does not confirm inbox delivery');
 assert.equal(testEmails,1);
 await page.getByLabel('SMTP server',{exact:true}).fill('smtp2.example.com');
 await expect(page.getByRole('button',{name:'Send test email',exact:true})).toBeDisabled();
 await page.getByRole('button',{name:'Save SMTP settings',exact:true}).click();
 await expect(page.getByRole('alert')).toContainText('Re-enter the SMTP password');
 await page.getByLabel('SMTP password',{exact:true}).fill('Synthetic-test-only-secret');
 requireReauth=true;
 await page.getByRole('button',{name:'Save SMTP settings',exact:true}).click();
 await expect(page.getByRole('heading',{name:'Re-authenticate for sensitive settings',exact:true})).toBeVisible();
 await expect(page.getByLabel('New verification code',{exact:true})).toHaveCount(0);
 await page.getByLabel('Username',{exact:true}).fill('admin');
 await page.getByLabel('Password',{exact:true}).fill('Synthetic-test-only-password');
 await page.getByRole('button',{name:'Verify',exact:true}).last().click();
 await page.getByRole('button',{name:'Save SMTP settings',exact:true}).click();
 await expect(page.getByRole('status')).toContainText('SMTP settings saved');
 await mkdir('artifacts/screenshots',{recursive:true});
 await page.screenshot({path:'artifacts/screenshots/email-smtp-desktop.png',fullPage:true});
 await page.getByRole('button',{name:'Switch theme',exact:true}).click();
 await page.screenshot({path:'artifacts/screenshots/email-smtp-light.png',fullPage:true});
 await page.setViewportSize({width:390,height:844});
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
 await page.screenshot({path:'artifacts/screenshots/email-smtp-mobile.png',fullPage:true});
 assert.deepEqual(errors,[]);assert.deepEqual(external,[]);
 console.log('Email UI passed: disabled default, SMTP TLS/port fields, encrypted-secret preservation, TLS failure/success feedback, verified-recipient gate, explicit test send, host-change re-entry, password-only reauth, light/dark/mobile. No external email was sent.');
}finally{await browser.close();await new Promise(done=>server.close(done))}
