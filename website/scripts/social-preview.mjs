import {chromium} from '../../web/node_modules/@playwright/test/index.mjs';
const browser=await chromium.launch();
try {
  const page=await browser.newPage({viewport:{width:1200,height:630},deviceScaleFactor:1});
  await page.goto('http://127.0.0.1:4886/');
  await page.setContent(`<!doctype html><html lang="en"><head><link rel="stylesheet" href="http://127.0.0.1:4886/styles.css"><style>body{width:1200px;height:630px;overflow:hidden;padding:55px 65px;background:#101a1b}.brand{font-size:31px}.brand img{width:48px}.social{display:grid;grid-template-columns:1fr 1fr;gap:30px;align-items:center;margin-top:50px}h1{font-size:70px;line-height:1.05;margin-bottom:24px;color:#ecf0e8}h1 span{color:#82d6bf}p{font-size:20px;line-height:1.6}.preview{border:1px solid #517763;border-radius:12px;overflow:hidden;box-shadow:0 22px 60px #0005}.preview img{width:100%;height:340px;object-fit:cover;object-position:top}.site-name{font:13px PlexMono;margin-top:34px;color:#8eab9a;letter-spacing:.08em}</style></head><body><div class="brand"><img src="http://127.0.0.1:4886/assets/owl.svg" alt=""><span>metric<span class="brand-heavy">owl</span><b>AI</b></span></div><div class="social"><div><h1>Less watching.<br><span>More knowing.</span></h1><p>A watchful companion for your server.<br>Live metrics. Clear alerts. Optional AI.</p><div class="site-name">METRICOWL.CLOUD · EARLY PREVIEW</div></div><div class="preview"><img src="http://127.0.0.1:4886/assets/dashboard.jpg" alt="Application preview with example data"></div></div></body></html>`);
  await page.evaluate(()=>document.fonts.ready);
  await page.locator('img').evaluateAll(imgs=>Promise.all(imgs.map(i=>i.decode())));
  await page.screenshot({path:'website/public/assets/social-preview.jpg',type:'jpeg',quality:90});
}finally{await browser.close()}
