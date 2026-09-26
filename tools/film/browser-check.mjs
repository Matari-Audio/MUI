import assert from 'node:assert/strict';import{mkdir,writeFile}from'node:fs/promises';
const{default:puppeteer}=await import(process.env.PUPPETEER_MODULE||'puppeteer-core');
const browser=await puppeteer.launch({executablePath:process.env.CHROME,headless:true,args:['--no-sandbox','--autoplay-policy=no-user-gesture-required']});
const shots=new URL('./review/',import.meta.url);await mkdir(shots,{recursive:true});
try{
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>{errors.push(e.message);console.error(e.message);});await page.setViewport({width:1440,height:1050});await page.goto(process.argv[2]||'http://localhost:3020');await page.click('#connect');
 await page.waitForFunction(()=>!!window.muiLive,{timeout:30000});
 const send=command=>page.evaluate(c=>window.muiLive.send(c),command);
 const wait=ms=>new Promise(r=>setTimeout(r,ms));
 const control=async(id,value)=>page.$eval('#'+id,(el,value)=>{el.value=value;el.dispatchEvent(new Event('input'));},String(value));
 const nextScene=async command=>{const old=await page.evaluate(()=>window.muiLive.state.revision);await send(command);await page.waitForFunction(old=>window.muiLive.state.revision>old,{timeout:30000},old);};
 await page.click('#record');await page.waitForFunction(()=>document.querySelector('#record').textContent==='Stop recording');
 await send({op:'note_on',note:69,velocity:96});await page.waitForFunction(()=>window.muiAudioPeak>.005);await wait(350);
 await control('explode',75);await control('angle',-25);await wait(350);await page.screenshot({path:new URL('desktop.png',shots).pathname,fullPage:true});
 await send({op:'note_off',note:69});await wait(180);await send({op:'note_on',note:71});await wait(650);await send({op:'note_off',note:71});await wait(180);await send({op:'note_on',note:72});await wait(650);await send({op:'note_off',note:72});
 const capability=await page.evaluate(()=>({options:[...document.querySelector('#kind').options].map(o=>o.value),id:window.muiLive.state.modules[0].id}));
 if(capability.options.includes('noise')){
  await nextScene({op:'add',kind:'noise'});const ids=await page.evaluate(()=>window.muiLive.state.modules.map(m=>m.id));assert.equal(ids.length,2);
  await send({op:'note_on',note:69});await wait(700);await nextScene({op:'move',id:ids[1],direction:-1});assert.deepEqual(await page.evaluate(()=>window.muiLive.state.modules.map(m=>m.id)),ids.toReversed());await wait(350);
  await nextScene({op:'delete',id:ids[1]});assert.equal(await page.evaluate(()=>window.muiLive.state.modules.length),1);
 }
 await nextScene({op:'set',id:capability.id,field:'level',value:0});await wait(500);const silent=await page.evaluate(()=>window.muiAudioPeak);assert(silent<.001,`actual DSP gain should silence output, got ${silent}`);
 await nextScene({op:'set',id:capability.id,field:'level',value:.3});await send({op:'note_on',note:69});await page.waitForFunction(()=>window.muiAudioPeak>.005);
 const part=await page.evaluate(()=>window.muiLive.state.modules[0].part);await page.select('#part',part);await control('stretch',125);await control('depth',180);await wait(700);
 await send({op:'panic'});await control('explode',0);await control('angle',0);await page.click('#reset-pose');await wait(650);
 await page.click('#record');await page.waitForFunction(()=>!!window.muiLastRecording,{timeout:30000});const take=await page.evaluate(()=>window.muiLastRecording);console.log(JSON.stringify(take));
 await writeFile(new URL('last-take.json',shots),JSON.stringify(take,null,2));
 if(process.env.MUI_RENDER_PROOF){await page.evaluate(()=>[...document.querySelectorAll('button')].find(b=>b.textContent==='Render MP4').click());await page.waitForFunction(()=>!!window.muiLastRender,{timeout:60000});console.log('render',await page.evaluate(()=>window.muiLastRender));}
 await page.setViewport({width:390,height:844});await page.screenshot({path:new URL('mobile.png',shots).pathname,fullPage:true});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
 const health=await page.evaluate(()=>window.muiAudioHealth);console.log('audio health',health);assert.equal(health.dropped,0,'jitter buffer should not resync under normal local playback');assert.deepEqual(errors,[]);
 await page.close();
 const film=await browser.newPage();await film.goto(new URL(take.url,process.argv[2]||'http://localhost:3020').href);await film.waitForFunction(()=>!!window.__timelines?.['mui-performance']);
 const seek=async t=>film.evaluate(t=>{window.__timelines['mui-performance'].seek(t,false);return {scope:document.querySelector('#scope').toDataURL(),notes:document.querySelector('#notes').textContent,poses:[...document.querySelectorAll('.mui-plane')].map(p=>p.style.transform)};},t);
 const middle=await seek(1);await seek(take.seconds-.1);await seek(0);assert.deepEqual(await seek(1),middle,'recorded visuals must be history-independent');await film.close();
 console.log('PASS native audio, silence after edit, notes, topology, recording, responsive client.');
}finally{await browser.close();}
