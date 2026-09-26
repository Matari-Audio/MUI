/** Invert a projected native plane, including perspective and nonuniform scale. */
export function unproject(points,x,y,width,height){
 const [p0,p1,p2,p3]=points,dx1=p1.x-p2.x,dx2=p3.x-p2.x,dx3=p0.x-p1.x+p2.x-p3.x,dy1=p1.y-p2.y,dy2=p3.y-p2.y,dy3=p0.y-p1.y+p2.y-p3.y;
 const det=dx1*dy2-dx2*dy1;if(Math.abs(det)<1e-8)return null;
 const g=(dx3*dy2-dx2*dy3)/det,h=(dx1*dy3-dx3*dy1)/det;
 const a=p1.x-p0.x+g*p1.x,b=p3.x-p0.x+h*p3.x,d=p1.y-p0.y+g*p1.y,e=p3.y-p0.y+h*p3.y;
 const aa=a-x*g,bb=b-x*h,dd=d-y*g,ee=e-y*h,den=aa*ee-bb*dd;if(Math.abs(den)<1e-8)return null;
 return{x:((x-p0.x)*ee-bb*(y-p0.y))/den*width,y:(aa*(y-p0.y)-(x-p0.x)*dd)/den*height};
}
export function nativePoint(plane,x,y){
 let corners=plane.querySelectorAll('.mui-corner');
 if(!corners.length){for(const [left,top]of [[0,0],[100,0],[100,100],[0,100]]){const c=document.createElement('span');c.className='mui-corner';Object.assign(c.style,{position:'absolute',left:left+'%',top:top+'%',width:'0',height:'0',pointerEvents:'none'});plane.append(c);}corners=plane.querySelectorAll('.mui-corner');}
 const p=unproject([...corners].map(c=>{const r=c.getBoundingClientRect();return{x:r.x,y:r.y};}),x,y,parseFloat(plane.style.width),parseFloat(plane.style.height));
 return p?{x:p.x+parseFloat(plane.style.left),y:p.y+parseFloat(plane.style.top)}:null;
}
export function wireInput(viewport,send,isPlay,onArrange){
 let drag=null;
 const masks=new WeakMap();
 function hit(e){
   const seen=new Set();
   for(const element of document.elementsFromPoint(e.clientX,e.clientY)){
     const plane=element.closest('.mui-plane');if(!plane||seen.has(plane))continue;seen.add(plane);
     const img=plane.querySelector('img'),p=nativePoint(plane,e.clientX,e.clientY);if(!img?.complete||!p)continue;
     let mask=masks.get(img);if(mask?.src!==img.src){const canvas=document.createElement('canvas');canvas.width=img.naturalWidth;canvas.height=img.naturalHeight;const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(img,0,0);mask={src:img.src,context};masks.set(img,mask);}
     const x=(p.x-parseFloat(plane.style.left))/parseFloat(plane.style.width)*img.naturalWidth,y=(p.y-parseFloat(plane.style.top))/parseFloat(plane.style.height)*img.naturalHeight;
     if(x>=0&&y>=0&&x<img.naturalWidth&&y<img.naturalHeight&&mask.context.getImageData(Math.floor(x),Math.floor(y),1,1).data[3]>8)return plane;
   }return null;
 }
 const mods=e=>({shift:e.shiftKey,ctrl:e.ctrlKey,alt:e.altKey,meta:e.metaKey});
 function pointer(e){const plane=drag?.plane||hit(e);if(!plane)return;const p=nativePoint(plane,e.clientX,e.clientY);if(p)send({op:'input',kind:'pointer',...p,buttons:e.buttons,...mods(e)});}
 viewport.addEventListener('pointerdown',e=>{const plane=hit(e);if(!plane)return;e.preventDefault();viewport.focus();viewport.setPointerCapture(e.pointerId);drag={plane,x:e.clientX,y:e.clientY};if(isPlay())pointer(e);else onArrange(plane.dataset.muiId,0,0);});
 viewport.addEventListener('pointermove',e=>{if(isPlay())pointer(e);else if(drag){const scale=viewport.querySelector('.mui-world')?.getBoundingClientRect().width/parseFloat(viewport.querySelector('.mui-world')?.style.width)||1;onArrange(drag.plane.dataset.muiId,(e.clientX-drag.x)/scale,(e.clientY-drag.y)/scale);drag.x=e.clientX;drag.y=e.clientY;}});
 viewport.addEventListener('pointerup',e=>{if(isPlay())pointer(e);drag=null;});
 const cancel=()=>{drag=null;send({op:'input',kind:'cancel'});};
 viewport.addEventListener('pointercancel',cancel);viewport.addEventListener('lostpointercapture',cancel);window.addEventListener('blur',cancel);
 viewport.addEventListener('wheel',e=>{if(!isPlay())return;e.preventDefault();pointer(e);const unit=e.deltaMode===1?16:e.deltaMode===2?viewport.clientHeight:1;send({op:'input',kind:'wheel',dx:Math.max(-10000,Math.min(10000,e.deltaX*unit)),dy:Math.max(-10000,Math.min(10000,e.deltaY*unit)),...mods(e)});},{passive:false});
 viewport.addEventListener('keydown',e=>{if(!isPlay()||['Shift','Control','Alt','Meta','CapsLock'].includes(e.key))return;e.stopPropagation();if(e.key==='Tab'&&(e.ctrlKey||e.metaKey))return;e.preventDefault();send({op:'input',kind:'key',key:e.key,text:e.key.length===1&&!e.ctrlKey&&!e.metaKey?e.key:'',...mods(e)});});
 viewport.addEventListener('blur',cancel);return cancel;
}
