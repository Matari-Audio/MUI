const $=id=>document.getElementById(id);
const canvas=$('canvas'),ctx=canvas.getContext('2d');
let pending=false,drag=null,lastBake=null,renderCount=0;
const positions=[{x:150,y:146},{x:344,y:114},{x:267,y:260}];
const value=id=>Number($(id).value);
function rotatedRect(x,y,w,h,r,angle) {
  if(!angle)return {x,y,w,h,r};
  const ring=[],a=angle*Math.PI/180,c=Math.cos(a),s=Math.sin(a);r=Math.min(r,w/2,h/2);
  for(let corner=0;corner<4;corner++) {
    const centers=[[w-r,r],[w-r,h-r],[r,h-r],[r,r]],v=centers[corner];
    for(let j=0;j<=8;j++) {const t=(-90+90*corner+j*90/8)*Math.PI/180,px=v[0]+r*Math.cos(t)-w/2,py=v[1]+r*Math.sin(t)-h/2;
      ring.push([x+w/2+c*px-s*py,y+h/2+s*px+c*py]);}
  }
  return {rings:[ring]};
}
function sourceList() {
  const p=positions,a=hex($('colorA').value,value('alpha')),b=hex($('colorB').value);
  let fill=a;
  if($('gradient').checked)fill={kind:'linear',from:[p[0].x,p[0].y],to:[p[0].x+188,p[0].y+148],stops:[{at:0,color:a},{at:1,color:hex('#137876',value('alpha'))}]};
  const sources=[
    {shape:{x:p[0].x,y:p[0].y,w:188,h:148,r:value('radius')},fill,border:hex($('edgeA').value),width:value('widthA')},
    {shape:rotatedRect(p[1].x,p[1].y,164,140,value('radius')*1.3,value('rotate')),fill:b,border:hex($('edgeB').value),width:value('widthB')},
  ];
  if($('third').checked)sources.push({shape:{x:p[2].x,y:p[2].y,w:105,h:88,r:44},fill:hex('#fac878'),border:hex('#fff0c8'),width:4});
  return sources;
}
function options() {
  const mode=$('mode').value;
  return { ...defaults(),progress:value('progress'),reach:value('reach'),blend:value('blend'),
    fill:mode==='border'?'keep':'blend',border:mode==='shape'?'keep':mode==='no-border'?'omit':'blend'};
}
function updateCode() {
  const o=options();let w=o.border==='keep'?'Weld::shape()':o.fill==='keep'?'Weld::borders()':'Weld::all()';
  if(o.border==='omit')w+='.border(WeldChannel::Omit)';
  $('code').textContent=`row![shape_a, shape_b]\n    .weld_with(${w}\n        .reach(${o.reach.toFixed(1)})\n        .blend(${o.blend.toFixed(1)})\n        .morph(${o.progress.toFixed(2)}))`;
  for(const id of ['progress','reach','blend','widthA','widthB','radius','rotate','alpha'])$(id+'Out').value=$(id).value;
}
function draw() {
  pending=false;updateCode();const start=performance.now(),sources=sourceList(),o=options();
  ctx.clearRect(0,0,canvas.width,canvas.height);
  try {
    // CPU reference quality. This demo is intentionally not an FPS benchmark.
    const b=bake(sources,o,.75);lastBake=b;
    const buffer=document.createElement('canvas');buffer.width=b.width;buffer.height=b.height;
    buffer.getContext('2d').putImageData(new ImageData(b.rgba,b.width,b.height),0,0);
    ctx.drawImage(buffer,b.bounds.x,b.bounds.y,b.bounds.w,b.bounds.h);
    if($('contour').checked) {
      ctx.strokeStyle='#ffffff';ctx.lineWidth=1;ctx.setLineDash([3,3]);
      for(const ring of b.contours){ctx.beginPath();ring.forEach(([x,y],i)=>i?ctx.lineTo(x,y):ctx.moveTo(x,y));ctx.closePath();ctx.stroke();}
      ctx.setLineDash([]);
    }
    ctx.textAlign='center';ctx.font='600 15px system-ui';ctx.fillStyle='#091418';
    ctx.fillText('SOURCE A',positions[0].x+94,positions[0].y+77);
    ctx.fillText('SOURCE B',positions[1].x+82,positions[1].y+74);
    if($('third').checked)ctx.fillText('C',positions[2].x+52.5,positions[2].y+48);
    renderCount++;
    $('status').textContent=`${sources.length} sources · ${b.contours.length} contour(s) · ${b.width} × ${b.height} reference raster`;
    $('cost').textContent=`${Math.round(performance.now()-start)} ms JS bake — not a Rust or GPU benchmark`;
    window.__weldDemo={renderCount,options:o,positions:positions.map(p=>({...p})),width:b.width,height:b.height,contours:b.contours.length,error:null};
  } catch(e) {$('status').textContent=e.message;window.__weldDemo={renderCount,error:e.message};}
}
function schedule() {if(!pending){pending=true;requestAnimationFrame(draw);}}
for(const input of document.querySelectorAll('input,select'))input.addEventListener('input',schedule);
function local(e) {const r=canvas.getBoundingClientRect();return {x:(e.clientX-r.left)*canvas.width/r.width,y:(e.clientY-r.top)*canvas.height/r.height};}
canvas.addEventListener('pointerdown',e=>{const p=local(e),ss=sourceList();for(let i=ss.length-1;i>=0;i--)if(distance(ss[i].shape,p.x,p.y)<=4){drag={i,dx:p.x-positions[i].x,dy:p.y-positions[i].y};canvas.setPointerCapture(e.pointerId);canvas.style.cursor='grabbing';break;}});
canvas.addEventListener('pointermove',e=>{if(!drag)return;const p=local(e);positions[drag.i]={x:clamp(p.x-drag.dx,70,510),y:clamp(p.y-drag.dy,75,270)};schedule();});
const release=()=>{drag=null;canvas.style.cursor='grab';};
canvas.addEventListener('pointerup',release);canvas.addEventListener('pointercancel',release);
$('reset').addEventListener('click',()=>{positions[0]={x:150,y:146};positions[1]={x:344,y:114};positions[2]={x:267,y:260};$('progress').value=1;$('reach').value=48;$('blend').value=130;$('rotate').value=0;$('mode').value='all';schedule();});
$('apart').addEventListener('click',()=>{positions[1]={x:410,y:100};$('progress').value=0;schedule();});
$('join').addEventListener('click',()=>{positions[1]={x:340,y:114};$('progress').value=1;schedule();});
window.__setWeldDemo=values=>{for(const [id,v]of Object.entries(values)){const e=$(id);if(!e)throw Error('unknown control');if(e.type==='checkbox')e.checked=v;else e.value=v;}schedule();};
schedule();
