import{mountLayers}from'/layers.mjs';
import{wireInput}from'/input.mjs';
const textures={};
const $=id=>document.getElementById(id);let socket,context,stream,analyser,capture,current,plugin,playedFrame=0,recording=false;
const held=new Set(),noteQueue=[];let sequenceGeneration=0;let active=[],pose={explode:0,angle:0,poses:{}};
const names=['C','C♯','D','D♯','E','F','F♯','G','G♯','A','A♯','B'];
const noteName=n=>names[n%12]+(Math.floor(n/12)-1);
const send=v=>{if(socket?.readyState===1)socket.send(JSON.stringify(v));};
function status(s){$('status').textContent=s;}
function fail(e){$('error').textContent=e.message||String(e);}
function noteOn(n){if(held.has(n)||!context||!plugin?.notes)return;held.add(n);send({op:'note_on',note:n,velocity:96});}
function noteOff(n){if(!held.delete(n))return;send({op:'note_off',note:n});}
function panic(){sequenceGeneration++;held.clear();send({op:'panic'});}
function fit(){if(!capture)return;const view=$('viewport');const scale=Math.min((view.clientWidth-50)/current.scene.width,(view.clientHeight-40)/current.scene.height)*(1-.32*pose.explode);$('fit').style.transform=`scale(${Math.max(.1,scale)})`;}
function present(publish=true){if(!capture)return;
 capture.world.style.transform=`rotateX(${pose.explode*26}deg) rotateY(${pose.angle}deg)`;
 [...capture.layers.keys()].forEach((id,i)=>{const p=pose.poses[id]||{};capture.setPose(id,{...p,z:(p.z||0)+(id==='background'?0:pose.explode*(70+i*40)),x:(p.x||0)+(id==='background'?0:pose.explode*(i%2?25:-25))});for(const plane of capture.layers.get(id)){plane.style.outline=pose.highlight===id?'3px solid #f58b45':'';plane.style.filter=pose.highlight&&pose.highlight!==id?'brightness(.5)':'';plane.dataset.selected=String(id===$('part').value);}});fit();
 if(publish)send({op:'pose',frame:playedFrame,pose});
}
function selection(){const p=pose.poses[$('part').value]||{};if([...$('component').options].some(o=>o.value===$('part').value)){$('component').value=$('part').value;$('component').onchange?.();}$('zoom').value=(p.scaleX||1)*100;$('stretch').value=(p.scaleX||1)*100;$('depth').value=p.z||0;present(false);}
function parameters(){const m=current?.modules?.find(m=>String(m.id)===$('module').value);$('parameters').replaceChildren();
 for(const p of m?.parameters||[]){const row=document.createElement('div'),label=document.createElement('label'),input=document.createElement('input'),out=document.createElement('output');input.id='param-'+p.id;label.htmlFor=input.id;label.textContent=p.label;input.type='range';input.min=p.min;input.max=p.max;input.step=p.step;input.value=p.value;out.value=Number(p.value).toFixed((String(p.step).split('.')[1]||'').length);input.oninput=()=>out.value=input.value;input.onchange=()=>send({op:'set',id:m.id,field:p.id,value:+input.value});row.append(label,input,out);$('parameters').append(row);}
 for(const k of ['up','down'])$(k).disabled=!m||!plugin?.move;$('delete').disabled=!m||!plugin?.delete;
}
let sceneSerial=0,sceneChain=Promise.resolve(),catalogKey='';
function catalog(value){const surfaces=value.scene.surfaces||[];const key=surfaces.map(s=>s.id).join('|');if(key===catalogKey)return;catalogKey=key;const old=$('component').value;$('component').replaceChildren();for(const s of surfaces.filter(s=>!s.id.startsWith('/')))$('component').add(new Option(s.id,s.id));if(surfaces.some(s=>s.id===old))$('component').value=old;}

async function scene(value){
 // Decode every delta in order, even if a newer visual frame supersedes it.
 for(const [name,data]of Object.entries(value.images||{})){textures[name]='data:image/png;base64,'+data;const img=new Image();img.src=textures[name];await img.decode();}
 await new Promise(resolve=>{const ready=()=>{if(!context||value.frame<=playedFrame)resolve();else requestAnimationFrame(ready);};ready();});
 catalog(value);
 const base=new URL(value.assetBase,location.origin);
 if(capture?.update(value.scene,base,textures)){
   const changed=JSON.stringify(current.modules)!==JSON.stringify(value.modules);current=value;const keep=new Set(value.scene.layers.map(l=>l.src));for(const name of Object.keys(textures))if(!keep.has(name))delete textures[name];
   if(changed&&!$('parameters').contains(document.activeElement))parameters();present(false);return;
 }
 const serial=++sceneSerial;const mount=document.createElement('div');const next=mountLayers(mount,value.scene,base,textures);await next.ready;if(serial!==sceneSerial){next.destroy();return;}
 for(const m of value.modules||[]){const previous=current?.modules?.find(old=>old.part===m.part);if(previous&&previous.id!==m.id)delete pose.poses[m.part];}
 for(const part of Object.keys(pose.poses))if(!next.layers.has(part))delete pose.poses[part];
 const selected=$('part').value,module=$('module').value;capture?.destroy();$('mount').replaceChildren(next.world);capture=next;current=value;$('empty').hidden=true;
 $('part').replaceChildren();for(const [id,planes]of capture.layers){const option=new Option(id==='background'?'Chassis':id,id);$('part').add(option);for(const plane of planes)plane.onclick=()=>{$('part').value=id;selection();};}
 $('part').disabled=false;$('part').value=capture.layers.has(selected)?selected:(capture.layers.has(value.modules?.[0]?.part)?value.modules[0].part:value.scene.groups[0]||'background');
 $('module').replaceChildren();for(const m of value.modules||[])$('module').add(new Option(`${m.kind} · ${m.id}`,m.id));$('module').disabled=!value.modules?.length;
 if(value.modules?.some(m=>String(m.id)===module))$('module').value=module;
 parameters();selection();present(false);status(`${plugin?.name||'Instrument'} connected · native scene ${value.revision}`);$('record').disabled=false;
 window.muiLive={send,get state(){return current;},get audio(){return window.muiAudioHealth;},get pose(){return pose;}};
}
$('connect').onclick=async()=>{try{
 $('connect').disabled=true;$('error').textContent='';noteQueue.length=0;active=[];playedFrame=0;recording=false;$('record').textContent='Record take';context=new AudioContext({sampleRate:48000,latencyHint:'interactive'});await context.resume();
 if(context.sampleRate!==48000)throw Error('This local bridge currently needs a 48 kHz AudioContext.');
 await context.audioWorklet.addModule('/stream-worklet.mjs');stream=new AudioWorkletNode(context,'mui-stream',{numberOfInputs:0,numberOfOutputs:1,outputChannelCount:[2],processorOptions:{bufferFrames:+$('latency').value*48}});analyser=context.createAnalyser();analyser.fftSize=2048;stream.connect(analyser);analyser.connect(context.destination);
 stream.port.onmessage=({data})=>{playedFrame=data.frame;window.muiAudioHealth=data;};
 socket=new WebSocket(`${location.protocol==='https:'?'wss':'ws'}://${location.host}/ws`);socket.binaryType='arraybuffer';
 socket.onopen=()=>{status('Loading the native editor…');$('panic').disabled=false;};
 socket.onmessage=({data})=>{if(data instanceof ArrayBuffer){stream.port.postMessage(data,[data]);return;}
 const v=JSON.parse(data);if(v.type==='hello'){plugin=v.plugin;$('title').textContent=`MUI Motion / ${plugin.name}`;document.title=`${plugin.name} · MUI Motion`;$('kind').replaceChildren();for(const k of plugin.addKinds||[])$('kind').add(new Option(k.label,k.id));$('kind').disabled=!plugin.addKinds?.length;$('add').disabled=!plugin.addKinds?.length;$('sequence').disabled=!plugin.notes;for(const b of $('keys').children)b.disabled=!plugin.notes;}
 else if(v.type==='scene'){sceneChain=sceneChain.then(()=>scene(v)).catch(fail);}else if(v.type==='note')noteQueue.push(v);else if(v.type==='error')fail(v.message);
 else if(v.type==='recording'){recording=true;$('record').textContent='Stop recording';status('Recording audio, edits and presentation poses…');}
 else if(v.type==='rendering'){status('Rendering the recorded take…');}
 else if(v.type==='rendered'){const link=document.createElement('a');link.href=v.url;link.textContent='Watch rendered MP4';link.target='_blank';const line=document.createElement('div');line.append(link);$('recordings').append(line);status('MP4 ready.');window.muiLastRender=v;}
 else if(v.type==='recorded'){recording=false;$('record').textContent='Record take';const link=document.createElement('a');link.href=v.url;link.target='_blank';link.textContent=`Open recorded composition (${v.seconds.toFixed(1)}s)`;const line=document.createElement('div');const render=document.createElement('button');render.textContent='Render MP4';render.onclick=()=>send({op:'render',take:v.take});line.append(link,document.createTextNode(' '),render);$('recordings').append(line);status('Take saved. HyperFrames project: '+v.project);window.muiLastRecording=v;}
 };
 socket.onerror=()=>fail('Could not connect. Check the local bridge terminal.');
 socket.onclose=()=>{held.clear();noteQueue.length=0;active=[];recording=false;$('record').textContent='Record take';for(const button of $('recordings').querySelectorAll('button'))button.disabled=true;stream.port.postMessage({reset:true});context.close();context=null;analyser=null;$('health').textContent='Audio stopped.';for(const k of ['record','panic','sequence','add','up','down','delete'])$(k).disabled=true;$('connect').disabled=false;status('Disconnected. Reconnect starts a fresh native session.');};
}catch(e){fail(e);if(context){await context.close();context=null;}$('connect').disabled=false;}};
$('panic').onclick=panic;window.addEventListener('blur',panic);document.addEventListener('visibilitychange',()=>{if(document.hidden)panic();});
const keyboard=[60,62,64,65,67,69,71,72];const keyMap=new Map('asdfghjk'.split('').map((k,i)=>[k,keyboard[i]]));
for(const n of keyboard){const b=document.createElement('button');b.textContent=noteName(n);b.dataset.note=n;b.disabled=true;b.onpointerdown=e=>{e.preventDefault();b.setPointerCapture(e.pointerId);noteOn(n);};b.onpointerup=()=>noteOff(n);b.onpointercancel=()=>noteOff(n);b.onlostpointercapture=()=>noteOff(n);b.onkeydown=e=>{if(['Enter',' '].includes(e.key)){e.preventDefault();if(!e.repeat)noteOn(n);}};b.onkeyup=e=>{if(['Enter',' '].includes(e.key)){e.preventDefault();noteOff(n);}};$('keys').append(b);}
window.addEventListener('keydown',e=>{if(e.target.closest('#viewport')&&$('mode').value==='play')return;if(e.repeat||/INPUT|SELECT|TEXTAREA/.test(e.target.tagName))return;const n=keyMap.get(e.key.toLowerCase());if(n!==undefined){e.preventDefault();noteOn(n);}});window.addEventListener('keyup',e=>{const n=keyMap.get(e.key.toLowerCase());if(n!==undefined)noteOff(n);});
let sequence=false;$('sequence').onclick=async()=>{if(sequence)return;sequence=true;const generation=sequenceGeneration;try{for(const n of [69,71,72]){if(generation!==sequenceGeneration)break;noteOn(n);await new Promise(r=>setTimeout(r,600));noteOff(n);await new Promise(r=>setTimeout(r,160));}}finally{sequence=false;}};
$('part').onchange=selection;$('module').onchange=parameters;
for(const id of ['explode','angle'])$(id).oninput=()=>{pose[id]=+$(id).value/(id==='explode'?100:1);present();};
for(const [id,key,divisor]of [['stretch','scaleX',100],['depth','z',1]])$(id).oninput=()=>{const part=$('part').value;if(!part)return;pose.poses[part]={...pose.poses[part],[key]:+$(id).value/divisor};present();};
$('reset-pose').onclick=()=>{pose={explode:0,angle:0,poses:{}};$('explode').value=$('angle').value=0;selection();present();};
$('add').onclick=()=>send({op:'add',kind:$('kind').value});$('delete').onclick=()=>send({op:'delete',id:+$('module').value});$('up').onclick=()=>send({op:'move',id:+$('module').value,direction:-1});$('down').onclick=()=>send({op:'move',id:+$('module').value,direction:1});
$('record').onclick=()=>{send({op:recording?'record_stop':'record_start'});};new ResizeObserver(fit).observe($('viewport'));
const samples=new Float32Array(2048);let peak=0;
function draw(){requestAnimationFrame(draw);while(noteQueue.length&&noteQueue[0].frame<=playedFrame)active=noteQueue.shift().active;
 $('notes').textContent=active.length?active.map(noteName).join(' + '):'No notes held';for(const b of $('keys').children)b.classList.toggle('active',active.includes(+b.dataset.note));
 const c=$('scope'),w=Math.round(c.clientWidth*devicePixelRatio),h=Math.round(c.clientHeight*devicePixelRatio);if(c.width!==w||c.height!==h){c.width=w;c.height=h;}const g=c.getContext('2d');g.fillStyle='#202320';g.fillRect(0,0,w,h);g.strokeStyle='#4c5549';g.lineWidth=1;g.beginPath();g.moveTo(0,h/2);g.lineTo(w,h/2);g.stroke();if(!analyser)return;analyser.getFloatTimeDomainData(samples);peak=0;g.strokeStyle='#69dbc4';g.lineWidth=2*devicePixelRatio;g.beginPath();for(let i=0;i<samples.length;i++){peak=Math.max(peak,Math.abs(samples[i]));const x=i/(samples.length-1)*w,y=h/2-samples[i]*h*.45;i?g.lineTo(x,y):g.moveTo(x,y);}g.stroke();window.muiAudioPeak=peak;
 const health=window.muiAudioHealth;$('health').textContent=health?`48 kHz native stereo · buffer ${(health.queued/48).toFixed(0)} ms · underruns ${health.underruns} · resyncs ${health.dropped} · peak ${peak.toFixed(3)}`:'Waiting for audio…';}
draw();

const cancelInput=wireInput($('viewport'),send,()=>$('mode').value==='play'&&plugin?.liveEditor,(id,x,y)=>{$('part').value=id;pose.poses[id]={...pose.poses[id],x:(pose.poses[id]?.x||0)+x,y:(pose.poses[id]?.y||0)+y};selection();present();});
$('mode').onchange=()=>{cancelInput();$('viewport').dataset.mode=$('mode').value;};

function waitUntil(test,timeout=10000){return new Promise((resolve,reject)=>{const start=performance.now();const poll=()=>{if(test())resolve();else if(performance.now()-start>timeout)reject(Error('Native editor did not acknowledge the operation.'));else setTimeout(poll,25);};poll();});}
const motion={
 get components(){return current?.scene.surfaces||[];},
 get state(){return current;},
 command:send,
 async select(ids){send({op:'input',kind:'select',ids});await waitUntil(()=>ids.length?ids.every(id=>current?.scene.groups.includes(id)):true);},
 async highlight(id){if(id&&!capture.layers.has(id))await this.select([id]);pose.highlight=id;present();},
 async transform(id,values){if(!capture.layers.has(id))await this.select([id]);pose.poses[id]={...pose.poses[id],...values};present();},
 async zoom(id,scale){return this.transform(id,{scaleX:scale,scaleY:scale});},
 async resize(id,width,height){send({op:'input',kind:'resize',id,width,height});await waitUntil(()=>{const s=current?.scene.surfaces.find(s=>s.id===id);return s&&Math.abs(s.frame[2]-width)<2&&Math.abs(s.frame[3]-height)<2;});},
 async focus(id){await this.select([id]);const s=this.components.find(s=>s.id===id);if(!s)throw Error('Unknown component');pose.explode=0;pose.angle=0;const scale=Math.min(current.scene.width*.8/s.frame[2],current.scene.height*.8/s.frame[3],4);pose.poses[id]={scaleX:scale,scaleY:scale,x:current.scene.width/2-s.frame[0]-s.frame[2]/2,y:current.scene.height/2-s.frame[1]-s.frame[3]/2};pose.highlight=id;present();},
 explode(amount=1){pose.explode=amount;present();},
 reset(){pose={explode:0,angle:0,poses:{}};present();send({op:'input',kind:'select',ids:[]});},
 async animate(id,to,seconds=1){if(!capture.layers.has(id))await this.select([id]);const from={...pose.poses[id]},start=performance.now();await new Promise(resolve=>{const tick=()=>{const t=Math.min(1,(performance.now()-start)/(seconds*1000)),ease=t*t*(3-2*t);pose.poses[id]=Object.fromEntries(Object.entries(to).map(([k,v])=>[k,(from[k]??(k.startsWith('scale')?1:0))+(v-(from[k]??(k.startsWith('scale')?1:0)))*ease]));present();if(t<1)setTimeout(tick,33);else resolve();};tick();});},
 async perform(steps){for(const step of steps){if(step.wait)await new Promise(r=>setTimeout(r,step.wait*1000));if(step.command)send(step.command);if(step.select)await this.select(step.select);if(step.highlight!==undefined)await this.highlight(step.highlight);if(step.focus)await this.focus(step.focus);if(step.explode!==undefined)this.explode(step.explode);if(step.animate)await this.animate(step.animate.id,step.animate.to,step.animate.seconds);}}
};window.muiMotion=motion;
$('extract').onclick=()=>motion.select([$('component').value]).catch(fail);
$('highlight').onclick=()=>motion.highlight($('component').value).catch(fail);
$('focus-component').onclick=()=>motion.focus($('component').value).catch(fail);
$('auto-parts').onclick=()=>motion.reset();
$('zoom').oninput=()=>motion.zoom($('part').value,+$('zoom').value/100).catch(fail);
$('reflow').onclick=()=>motion.resize($('component').value,+$('layout-width').value,+$('layout-height').value).catch(fail);
$('component').onchange=()=>{const s=motion.components.find(s=>s.id===$('component').value);if(s){$('layout-width').value=Math.round(s.frame[2]);$('layout-height').value=Math.round(s.frame[3]);}};
