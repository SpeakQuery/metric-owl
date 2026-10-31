import { chromium } from '../../web/node_modules/@playwright/test/index.mjs';
import { writeFile, mkdir } from 'node:fs/promises';
import assert from 'node:assert/strict';
const base=process.env.WEBSITE_URL||'http://127.0.0.1:4886';
const output='artifacts/validation/website';
await mkdir(output,{recursive:true});
const browser=await chromium.launch();
const result={base,at:new Date().toISOString(),viewports:[],pages:[],checks:[]};
try {
  const page=await browser.newPage({viewport:{width:1440,height:1000}});
  const errors=[]; const requests=[];
  page.on('pageerror',e=>errors.push(e.message));
  page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
  page.on('request',r=>requests.push(r.url()));
  for(const width of [1440,1024,768,390,320]) {
    await page.setViewportSize({width,height:900});
    await page.goto(base+'/',{waitUntil:'networkidle'});
    await page.evaluate(()=>document.fonts.ready);
    assert.equal(await page.locator('html').getAttribute('lang'),'en');
    assert.equal(await page.locator('h1').count(),1);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`Overflow at ${width}`);
    assert.equal(await page.locator('img').evaluateAll(imgs=>imgs.filter(i=>i.getAttribute('src')&&!i.loading).every(i=>i.complete&&i.naturalWidth>0)),true);
    if(width===390||width===1440)await page.screenshot({path:`artifacts/screenshots/website/home-${width}.png`,fullPage:true,animations:'disabled'});
    result.viewports.push({width,overflow:false});
  }
  await page.setViewportSize({width:390,height:844});
  await page.getByRole('button',{name:'Open navigation',exact:true}).click();
  assert.equal(await page.locator('.menu-toggle').getAttribute('aria-expanded'),'true');
  await page.locator('#main-nav').getByRole('link',{name:'Product',exact:true}).click();
  assert.equal(await page.locator('.menu-toggle').getAttribute('aria-expanded'),'false');
  for(const name of ['Alert rules','MCP connections','Light theme','Overview']) {
    await page.getByRole('tab',{name,exact:true}).click();
    await page.locator('#tour-image').evaluate(img=>img.decode());
    assert.equal(await page.getByRole('tab',{name,exact:true}).getAttribute('aria-selected'),'true');
  }
  await page.getByRole('tab',{name:'Overview',exact:true}).focus();
  await page.keyboard.press('ArrowRight');
  assert.equal(await page.getByRole('tab',{name:'Alert rules',exact:true}).getAttribute('aria-selected'),'true');
  await page.getByRole('button',{name:'Enlarge application screenshot'}).click();
  assert.equal(await page.getByRole('dialog').isVisible(),true);
  await page.keyboard.press('Escape');
  assert.equal(await page.getByRole('dialog').isVisible(),false);
  await page.getByText('Do I need an AI key to use Metric Owl?',{exact:true}).click();
  assert.equal(await page.locator('.faq-list details').first().getAttribute('open'),'');
  result.checks.push('mobile navigation','all screenshot tabs','keyboard tabs','image dialog and Escape','FAQ expand');
  await page.setViewportSize({width:1440,height:1000});
  await page.goto(base+'/');
  const links=await page.locator('a[href]').evaluateAll(links=>[...new Set(links.map(a=>a.getAttribute('href')).filter(h=>h.startsWith('/')))]);
  for(const href of links){
    const url=new URL(href,base);
    if(url.hash){
      if(new URL(page.url()).pathname!==url.pathname)await page.goto(url.origin+url.pathname);
      assert.equal(await page.locator(url.hash).count()>0,true,`Missing section ${url.pathname}${url.hash}`);continue;
    }
    const response=await page.request.get(url.href);assert.equal(response.status(),200,`Broken link ${url}`);
  }
  for(const path of ['/docs/quick-start.html','/docs/advanced.html','/docs/releases.html','/privacy.html']){
    for(const width of [390,1440]){
      await page.setViewportSize({width,height:900});await page.goto(base+path,{waitUntil:'networkidle'});
      assert.equal(await page.locator('h1').count(),1);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`Overflow ${path} ${width}`);
    }
    result.pages.push({path,ok:true});
  }
  await page.goto(base+'/docs/quick-start.html',{waitUntil:'networkidle'});
  assert.equal(await page.locator('.quick-steps > li').count(),3);
  assert.equal(await page.locator('input,select').count(),0);
  await page.context().grantPermissions(['clipboard-read','clipboard-write']);
  await page.getByRole('button',{name:'Copy command',exact:true}).click();
  assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),'curl -fsSL https://metricowl.cloud/install.sh | sudo bash');
  await page.screenshot({path:'artifacts/screenshots/website/quick-start-simple.png',fullPage:true,animations:'disabled'});
  await page.goto(base+'/docs/advanced.html',{waitUntil:'networkidle'});
  await page.locator('.ssh-fallback summary').click();
  assert.equal(await page.locator('[data-copy="ssh-tunnel"]').isDisabled(),true);
  await page.locator('#server-host').fill('203.0.113.44');
  await page.locator('#server-port').fill('2222');
  assert.equal(await page.locator('#ssh-tunnel').textContent(),'ssh -p 2222 -N -o ExitOnForwardFailure=yes -L 127.0.0.1:14877:127.0.0.1:4877 root@203.0.113.44');
  assert.equal(await page.locator('[data-copy="ssh-tunnel"]').isDisabled(),false);
  await page.locator('.mcp-advanced-doc summary').click();
  await page.locator('#mcp-client-id').fill('01234567-1234-4567-89ab-0123456789ab');
  assert.equal(await page.locator('[data-copy="mcp-config"]').isDisabled(),false);
  assert.match(await page.locator('#mcp-config').textContent(),/root@203\.0\.113\.44/);
  await page.locator('#mcp-format').selectOption('custom');
  const generated=JSON.parse(await page.locator('#mcp-config').textContent());
  assert.equal(generated.mcpServers.metric_owl.command,'ssh');
  assert.match(generated.mcpServers.metric_owl.args.at(-1),/--key-file .*01234567-1234-4567-89ab-0123456789ab\.key$/);
  for (const invalid of ['-oProxyCommand=id','host;id','https://example.com','999.1.2.3','$(id)']) {
    await page.locator('#server-host').fill(invalid);
    assert.equal(await page.locator('[data-copy="ssh-tunnel"]').isDisabled(),true);
    assert.equal(await page.locator('[data-copy="mcp-config"]').isDisabled(),true);
  }
  await page.locator('#server-host').fill('monitor.example.com');
  await page.locator('#server-user').fill('deploy');
  assert.match(await page.locator('#ssh-tunnel').textContent(),/deploy@monitor\.example\.com/);
  assert.equal(await page.locator('[data-copy="mcp-config"]').isDisabled(),false);
  await page.locator('#server-user').fill('root;id');
  assert.equal(await page.locator('[data-copy="ssh-tunnel"]').isDisabled(),true);
  await page.locator('#server-user').fill('root');
  await page.locator('#server-port').fill('65536');
  assert.equal(await page.locator('[data-copy="ssh-tunnel"]').isDisabled(),true);
  await page.locator('#server-port').fill('22');
  for (const width of [1440,390,320]) {
    await page.setViewportSize({width,height:900});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`Configured guide overflow at ${width}`);
  }
  await page.setViewportSize({width:1440,height:1000});
  await page.locator('.connection-builder').screenshot({path:'artifacts/screenshots/website/connection-builder.png'});
  result.checks.push('live server address command builder','MCP config only for existing UUID','shell injection inputs rejected','three-step guide with working copy and no connection forms','advanced SSH and MCP guide');
  const thirdParty=requests.filter(url=>!url.startsWith(base+'/'));
  assert.deepEqual(thirdParty,[],'Third-party browser requests');
  assert.deepEqual(errors,[],'Browser errors');
  result.checks.push('internal links','English pages','no third-party assets','no browser errors');
  await writeFile(`${output}/${base.startsWith('https')?'live':'local'}-browser.json`,JSON.stringify(result,null,2));
  console.log(JSON.stringify(result,null,2));
} finally {await browser.close();}
