/** Run against an already served project; uses HyperFrames' Puppeteer dependency.
 * PUPPETEER_MODULE=/absolute/path/to/puppeteer-core.js CHROME=/path/to/chrome \
 * node tools/kurv-motion/browser-check.mjs http://localhost:3018 http://localhost:3017
 */
import assert from 'node:assert/strict';
import {mkdir} from 'node:fs/promises';
const {default:puppeteer}=await import(process.env.PUPPETEER_MODULE || 'puppeteer-core');
const [base='http://localhost:3018',studio='http://localhost:3017']=process.argv.slice(2);
const shots=new URL('../../videos/kurv-unfold/.impeccable/review/',import.meta.url);
await mkdir(shots,{recursive:true});
const browser=await puppeteer.launch({executablePath:process.env.CHROME,headless:true,args:['--no-sandbox']});
try{
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>{errors.push(e.message);console.error(e.message);});
 await page.setViewport({width:1440,height:1000});await page.goto(base+'/tools/kurv-motion/lab.html');
 await page.waitForFunction(()=>!document.querySelector('#part').disabled);
 assert.equal(await page.$$eval('.mui-plane img',imgs=>imgs.every(i=>i.complete&&i.naturalWidth>0)),true);
 await page.select('#part','osc/0');
 await page.$eval('#scaleX',e=>{e.value='140';e.dispatchEvent(new Event('input'));});
 assert.equal(await page.$$eval('[data-mui-id="osc/0"]',els=>els.every(e=>/scale\(1\.4, ?1\)/.test(e.style.transform))),true);
 await page.click('#demo');await page.screenshot({path:new URL('desktop.png',shots).pathname,fullPage:true});
 await page.click('#reset');
 assert.equal(await page.$eval('#scaleX',e=>e.value),'100');
 await page.setViewport({width:390,height:844});await page.screenshot({path:new URL('mobile.png',shots).pathname,fullPage:true});
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'mobile horizontal overflow');
 await page.goto(base+'/videos/kurv-unfold/index.html');await page.waitForFunction(()=>!!window.__timelines?.['kurv-unfold']);
 const seek=async t=>page.evaluate(t=>{window.__timelines['kurv-unfold'].seek(t);return [...document.querySelectorAll('.mui-plane,#rig')].map(e=>e.style.transform);},t);
 const middle=await seek(8.5);await seek(15);await seek(2);assert.deepEqual(await seek(8.5),middle,'seek must be independent of previous time');
 await page.setViewport({width:2048,height:1051});await page.goto(studio+'/#project/kurv-unfold');
 const frame=await (async()=>{for(let n=0;n<100;n++){const f=page.frames().find(f=>f.url().includes('/preview'));if(f)return f;await new Promise(r=>setTimeout(r,100));}throw Error('Studio preview missing');})();
 await frame.waitForFunction(()=>window.__timelines?.['kurv-unfold']&&[...document.images].length===16&&[...document.images].every(i=>i.complete&&i.naturalWidth>0));
 await frame.evaluate(()=>{window.__timelines['kurv-unfold'].seek(6);});
 await page.screenshot({path:new URL('studio-2048.png',shots).pathname});
 assert.deepEqual(errors,[],'browser runtime errors');
 console.log('PASS: Studio asset base, all textures, controls, reset, mobile overflow, random-access seeking.');
}finally{await browser.close();}
