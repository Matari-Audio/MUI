import init, { render, parameters, hover, tap } from './pkg/mui_playground.js';
const $ = id => document.getElementById(id);
const key = 'matari-mui-playground-v1';
let ready = false, timer, params = [], mouseFrame = 0;
const code = $('code'), stage = $('stage');
const controls = ['width', 'round', 'mode', 'primary', 'bounds'];
function save() {
  try { localStorage.setItem(key, JSON.stringify({ code: code.value, ...Object.fromEntries(controls.map(id => [id, id === 'bounds' ? $(id).checked : $(id).value])) })); $('saved').textContent = 'Saved in this browser'; }
  catch { $('saved').textContent = 'Storage unavailable — export your draft'; }
}
function inspect() {
  params = parameters(code.value).trim().split('\n').filter(Boolean).map(line => {
    const [start, end, label, value, pixels] = line.split('\t');
    return { start: +start, end: +end, label, value: +value, pixels: pixels === 'true' };
  });
  $('parameters').replaceChildren();
  for (const [i, p] of params.entries()) {
    const row = document.createElement('label'); row.className = 'parameter';
    const title = document.createElement('span'); title.textContent = `.${p.label}`;
    const line = document.createElement('small'); line.textContent = `line ${code.value.slice(0,p.start).split('\n').length}`; title.append(line);
    const slider = document.createElement('input'); slider.type = 'range';
    const integer = ['Grid','cell','span'].includes(p.label);
    slider.min = integer ? '1' : '0'; slider.max = String(Math.max(p.value, integer ? 12 : ['width','height','min','max'].includes(p.label) ? 1000 : 100)); slider.step = ['grow','shrink','Track::Fraction'].includes(p.label) ? '0.1' : '1'; slider.value = p.value;
    const number = document.createElement('input'); number.type = 'number'; number.value = p.value; number.min = slider.min; number.step = slider.step; number.setAttribute('aria-label', `${p.label} value, parameter ${i+1}`); slider.setAttribute('aria-label', `${p.label}, parameter ${i+1}`);
    const update = value => {
      if (!Number.isFinite(+value) || value === '') return;
      const old = params[i], replacement = `${value}${old.pixels ? 'px' : ''}`, delta = replacement.length - (old.end-old.start);
      code.value = code.value.slice(0,old.start) + replacement + code.value.slice(old.end);
      old.end = old.start + replacement.length; old.value = +value;
      for (let j=i+1;j<params.length;j++){params[j].start+=delta;params[j].end+=delta;}
      slider.value = number.value = value; save(); draw(false);
    };
    slider.addEventListener('input', () => update(slider.value)); number.addEventListener('change', () => update(number.value)); row.append(title,slider,number); $('parameters').append(row);
  }
}
function draw(rebuild = true) {
  if (!ready) return;
  $('widthValue').textContent = $('width').value; $('roundValue').textContent = $('round').value;
  try {
    if (rebuild) inspect();
    const svg = render(code.value,+$('width').value,$('mode').value==='dark',+$('round').value,parseInt($('primary').value.slice(1),16),$('bounds').checked);
    stage.innerHTML = svg; $('error').textContent = ''; $('activity').textContent = 'Live Rust preview · hover or tap an item';
  } catch (error) { $('error').textContent = `${error}\nPreview retains the last valid result.`; if (rebuild) {$('parameters').replaceChildren();params=[];} }
}
code.addEventListener('input', () => { clearTimeout(timer); $('parameters').replaceChildren();params=[];save();timer=setTimeout(()=>draw(),180); });
for (const id of controls) $(id).addEventListener('input', () => {save();draw(false);});
function coordinates(event) {const svg=stage.querySelector('svg');if(!svg)return null;const p=new DOMPoint(event.clientX,event.clientY).matrixTransform(svg.getScreenCTM().inverse());return [p.x,p.y];}
stage.addEventListener('pointermove',event=>{if(!ready)return;const point=coordinates(event);if(!point)return;cancelAnimationFrame(mouseFrame);mouseFrame=requestAnimationFrame(()=>{try{stage.innerHTML=hover(...point,$('bounds').checked);const id=stage.querySelector('svg').dataset.hover;$('activity').textContent=id?`Hovered: ${id}`:'Live Rust preview';stage.style.cursor=id?'pointer':'';}catch(error){$('error').textContent=String(error);}});});
stage.addEventListener('pointerleave',()=>{cancelAnimationFrame(mouseFrame);if(ready){try{stage.innerHTML=hover(-1,-1,$('bounds').checked);}catch{}}stage.style.cursor='';});
stage.addEventListener('click',event=>{const p=coordinates(event);if(p){const action=tap(...p);if(action)$('activity').textContent=`Action: ${action} (playground only)`;}});
$('export').onclick=()=>{const url=URL.createObjectURL(new Blob([code.value],{type:'text/plain'}));const a=document.createElement('a');a.href=url;a.download='layout.mui';a.click();URL.revokeObjectURL(url);};
$('import').onclick=()=>$('file').click();
$('file').onchange=async()=>{const file=$('file').files[0];if(!file)return;if(file.size>65536){$('error').textContent='Import is limited to 64 KiB';return;}code.value=await file.text();save();draw();};
$('reset').onclick=async()=>{if(!confirm('Replace this draft with the example? Export first to keep it.'))return;code.value=await (await fetch('./default.mui')).text();save();draw();};
try {
  await init();let stored;
  try {stored=JSON.parse(localStorage.getItem(key));}catch{}
  code.value=typeof stored?.code==='string'?stored.code:await (await fetch('./default.mui')).text();
  for(const id of controls){if(stored?.[id]!==undefined){if(id==='bounds')$(id).checked=Boolean(stored[id]);else $(id).value=stored[id];}}
  ready=true;draw();
} catch(error){$('error').textContent=`Could not load the Rust engine: ${error}. Build the playground and serve it over HTTP.`;}
