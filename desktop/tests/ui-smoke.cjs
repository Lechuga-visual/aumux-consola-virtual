// Run with NODE_PATH pointing to an installed playwright-core, Chromium and a built Linux binary.
const {chromium}=require('playwright-core');
const {spawn}=require('node:child_process');
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const path=require('node:path');

(async()=>{
 const binary=process.env.MONITOR_BINARY||path.resolve('desktop/target/debug/monitor-mix');
 const app=spawn(binary,['--no-browser'],{stdio:['ignore','pipe','pipe']});
 let browser;
 try{
  const url=await new Promise((resolve,reject)=>{let out='';const timeout=setTimeout(()=>reject(Error('App startup timed out')),5000);app.stdout.on('data',b=>{out+=b;const m=out.match(/http:\/\/127\.0\.0\.1:\d+/);if(m){clearTimeout(timeout);resolve(m[0])}});app.on('exit',code=>{clearTimeout(timeout);reject(Error(`App exited: ${code}`))})});
  browser=await chromium.launch({executablePath:process.env.CHROMIUM||'/usr/bin/chromium',headless:true,args:['--no-sandbox','--disable-dev-shm-usage']});
  const page=await browser.newPage({viewport:{width:1500,height:1100},acceptDownloads:true});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(url);await page.waitForSelector('#mixer .strip');
  assert.equal(await page.locator('#mixer .strip').count(),8);assert.equal(await page.locator('#buses .strip').count(),5);
  await page.click('#demo');await page.waitForFunction(()=>document.getElementById('status').textContent==='DEMO');
  await page.selectOption('#view','0');
  await page.locator('#mixer .fader input').first().evaluate(e=>{e.value='-6';e.dispatchEvent(new Event('input',{bubbles:true}))});
  await page.waitForFunction(async()=>{const c=await(await fetch('/api/config')).json();return c.channels[0].sends_db[0]===-6});
  await page.waitForFunction(async()=>{const s=await(await fetch('/api/status')).json();return s.meters[1]>0});
  await page.click('#panic');await page.waitForFunction(()=>document.getElementById('status').textContent.includes('SILENCIO TOTAL'));
  await page.click('#stop');await page.waitForFunction(()=>document.getElementById('status').textContent.startsWith('DETENIDO'));
  if(process.env.MONITOR_VST3_FIXTURE){
   await page.locator('#mixer .strip').first().getByRole('button',{name:'VST3 +',exact:true}).click();
   await page.fill('#path',process.env.MONITOR_VST3_FIXTURE);await page.click('#inspect');
   await page.waitForFunction(()=>document.getElementById('classes').options.length>0);
   assert.match(await page.locator('#classes option').first().textContent(),/Fixture Gain/);
   const gain=page.locator('#params label').filter({hasText:'Gain'}).locator('input');
   await gain.fill('0.5');await gain.dispatchEvent('change');await page.click('#applyfx');
   await page.waitForFunction(async()=>{const c=await(await fetch('/api/config')).json();return c.channels[0].fx?.params['7']===0.5});
  }
  const downloadPromise=page.waitForEvent('download');await page.click('#save');const download=await downloadPromise;
  const scene=JSON.parse(await fs.readFile(await download.path(),'utf8'));
  assert.equal(scene.channels[0].sends_db[0],-6);assert.equal(scene.channels.length,8);
  const unauthorised=await page.request.post(url+'/api/panic',{data:{}});assert.equal(unauthorised.status(),403);
  await page.click('#demo');await page.waitForFunction(()=>document.getElementById('status').textContent==='DEMO');
  await page.screenshot({path:process.env.MONITOR_SCREENSHOT||'/workspace/scratch/monitor-mix.png',fullPage:true});
  assert.deepEqual(errors,[]);
  console.log('PASS: browser controls, independent monitor send, demo, emergency mute, VST3 inspection, settings download and POST authentication');
 }finally{if(browser)await browser.close();app.kill('SIGTERM');}
})().catch(e=>{console.error(e);process.exitCode=1});
