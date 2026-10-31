// Focused local/browser acceptance for the optional product video. Never publishes.
import {chromium} from '../../web/node_modules/@playwright/test/index.mjs';
import {mkdir, stat, writeFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
const base=process.env.WEBSITE_URL||'http://127.0.0.1:4886';
const output='artifacts/validation/website';
await mkdir(output,{recursive:true});
await mkdir('artifacts/screenshots/website',{recursive:true});
for(const path of ['website/public/assets/demo/metric-owl-overview-v3.mp4','website/public/assets/demo/poster-v2.jpg']){
 const info=await stat(path);assert.ok(info.size>0&&info.size<=20*1024*1024,`Missing/oversized video asset: ${path}`);
}
const browser=await chromium.launch();
const result={base,at:new Date().toISOString(),result:'passed',viewports:[]};
try{
 for(const [width,height] of [[1440,1000],[390,844]]){
  const page=await browser.newPage({viewport:{width,height},isMobile:width<500,hasTouch:width<500});
  const errors=[],requests=[];
  page.on('pageerror',error=>errors.push(error.message));
  page.on('request',request=>requests.push(request.url()));
  await page.goto(base+'/',{waitUntil:'networkidle'});
  const video=page.locator('#film video');
  assert.equal(await video.count(),1);
  assert.equal(await video.getAttribute('preload'),'none');
  assert.equal(await video.getAttribute('autoplay'),null);
  assert.notEqual(await video.getAttribute('controls'),null);
  assert.notEqual(await video.getAttribute('playsinline'),null);
  assert.equal(await video.evaluate(element=>element.paused),true);
  assert.equal(requests.some(url=>url.endsWith('/metric-owl-overview-v3.mp4')),false,'Video downloaded before a user play gesture');
  assert.equal(await page.locator('#film-caption').innerText(),'Illustrative motion preview · English voiceover · Music: Tech Live — Kevin MacLeod · CC BY 4.0 / credits.');
  await video.scrollIntoViewIfNeeded();
  const bounds=await video.boundingBox();assert.ok(bounds);
  // Click the browser's own play control. No autoplay or scripted play() is used.
  await video.click({position:{x:24,y:bounds.height-49}});
  await page.waitForFunction(()=>{const video=document.querySelector('#film video');return video&&!video.paused&&video.currentTime>1&&video.readyState>=2},{},{timeout:15000});
  const metadata=await video.evaluate(element=>({duration:element.duration,width:element.videoWidth,height:element.videoHeight,currentTime:element.currentTime,paused:element.paused,error:element.error?.code??null}));
  assert.equal(Number(metadata.duration.toFixed(3)),48);
  assert.equal(metadata.width,1920);assert.equal(metadata.height,1080);assert.equal(metadata.error,null);
  const before=metadata.currentTime;
  await page.waitForFunction(previous=>document.querySelector('#film video').currentTime>previous+.6,before);
  await video.evaluate(element=>{element.currentTime=15});
  await page.waitForFunction(()=>document.querySelector('#film video').currentTime>15.3);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
  const screenshot=`artifacts/screenshots/website/product-video-${width}.png`;
  await page.locator('#film').screenshot({path:screenshot,animations:'disabled'});
  await video.evaluate(element=>element.pause());
  assert.equal(await video.evaluate(element=>element.paused),true);
  assert.deepEqual(errors,[]);
  assert.equal(requests.every(url=>url.startsWith(base+'/')),true,'Unexpected external request');
  result.viewports.push({width,duration:metadata.duration,videoWidth:metadata.width,videoHeight:metadata.height,userInitiatedPlayback:true,timeAdvanced:true,noAutoplay:true,noOverflow:true,screenshot});
  await page.close();
 }
 await writeFile(`${output}/video-browser.json`,JSON.stringify(result,null,2));
 console.log(JSON.stringify(result,null,2));
}finally{await browser.close()}
