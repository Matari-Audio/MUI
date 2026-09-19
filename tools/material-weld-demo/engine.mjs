/* Executable specification for mui-weld. No framework or network dependency.
 * This is a JS reference, NOT the Rust implementation running in a browser. */
export const clamp = (x, a = 0, b = 1) => Math.max(a, Math.min(b, x));
const smoothstep = x => x * x * (3 - 2 * x);
export const defaults = () => ({ fill: 'blend', border: 'blend', reach: 16, blend: 32, progress: 1 });
export function validate(sources, o = defaults()) {
  if (!sources.length || sources.length > 64) throw Error('1..64 sources required');
  for (const v of ['reach', 'blend', 'progress']) if (!Number.isFinite(o[v])) throw Error('non-finite option');
  if (o.reach < 0 || o.reach > 1e4 || o.blend < 0 || o.blend > 1e4 || o.progress < 0 || o.progress > 1) throw Error('option out of range');
  if (![o.fill, o.border].every(v => ['blend', 'keep', 'omit'].includes(v))) throw Error('channel policy');
  for (const s of sources) {
    if (!Number.isFinite(s.width) || s.width < 0 || s.width > 1e4) throw Error('border width');
    if (s.shape.rings) {
      if (!s.shape.rings.length || s.shape.rings.some(r => r.length < 3 || r.some(p => p.length !== 2 || p.some(v => !Number.isFinite(v) || Math.abs(v) > 1e7)))) throw Error('contours');
    } else {
      const { x, y, w, h, r } = s.shape;
      if (![x, y, w, h, r].every(Number.isFinite) || w <= 0 || h <= 0 || r < 0) throw Error('rounded rectangle');
    }
  }
}
export function smoothMin(d, k) {
  const min = Math.min(...d);
  if (k <= 1e-12) {
    const ties = d.filter(x => x === min).length;
    return { distance: min, weights: d.map(x => x === min ? 1 / ties : 0) };
  }
  const ordered = d.map(x => x - min).sort((a, b) => a - b);
  let sum = 0, lambda = k;
  for (let i = 1; i < ordered.length; i++) {
    if (ordered[i] >= lambda) break;
    sum += ordered[i]; lambda = (k + sum) / (i + 1);
  }
  let weights = d.map(x => Math.max(0, (lambda - (x - min)) / k));
  const total = weights.reduce((a, b) => a + b, 0);
  weights = weights.map(x => x / total);
  const linear = weights.reduce((a, w, i) => a + w * (d[i] - min), 0);
  const squares = weights.reduce((a, w) => a + w * w, 0);
  return { distance: min + linear - k / 2 * (1 - squares), weights };
}
export function distance(g, x, y) {
  if (g.rings) {
    let nearest = Infinity, winding = 0;
    for (const ring of g.rings) for (let i = 0; i < ring.length; i++) {
      const a = ring[i], b = ring[(i + 1) % ring.length];
      const vx = b[0] - a[0], vy = b[1] - a[1], len = vx * vx + vy * vy;
      const t = len > 0 ? clamp(((x - a[0]) * vx + (y - a[1]) * vy) / len) : 0;
      nearest = Math.min(nearest, Math.hypot(x - a[0] - vx * t, y - a[1] - vy * t));
      const cross = vx * (y - a[1]) - vy * (x - a[0]);
      if (a[1] <= y && b[1] > y && cross > 0) winding++;
      if (a[1] > y && b[1] <= y && cross < 0) winding--;
    }
    return winding ? -nearest : nearest;
  }
  const hx = g.w / 2, hy = g.h / 2, r = Math.min(g.r, hx, hy);
  const qx = Math.abs(x - g.x - hx) - hx + r, qy = Math.abs(y - g.y - hy) - hy + r;
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
}
export function field(sources, x, y, options) {
  const ds = sources.map(s => distance(s.shape, x, y)), t = smoothstep(options.progress);
  return { distance: smoothMin(ds, 2 * options.reach * t).distance, weights: smoothMin(ds, options.blend * t).weights };
}
export function srgb(r, g, b, a = 1) {
  const f = x => x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  return [f(r), f(g), f(b), a];
}
export function hex(s, a = 1) {
  const n = parseInt(s.replace('#', ''), 16);
  return srgb((n >> 16 & 255) / 255, (n >> 8 & 255) / 255, (n & 255) / 255, a);
}
export function rgba8(c) {
  if (c[3] <= 0) return [0, 0, 0, 0];
  const f = x => { x = clamp(x); return x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055; };
  return [...c.slice(0, 3).map(x => Math.round(255 * f(x))), Math.round(255 * clamp(c[3]))];
}
const lab = ([r, g, b]) => {
  const l = Math.cbrt(.4122214708*r + .5363325363*g + .0514459929*b);
  const m = Math.cbrt(.2119034982*r + .6806995451*g + .1073969566*b);
  const s = Math.cbrt(.0883024619*r + .2817188376*g + .6299787005*b);
  return [.2104542553*l + .7936177850*m - .0040720468*s, 1.9779984951*l - 2.4285922050*m + .4505937099*s, .0259040371*l + .7827717662*m - .8086757660*s];
};
const fromLab = ([l, a, b], alpha) => {
  const ll = (l + .3963377774*a + .2158037573*b) ** 3;
  const mm = (l - .1055613458*a - .0638541728*b) ** 3;
  const ss = (l - .0894841775*a - 1.2914855480*b) ** 3;
  return [4.0767416621*ll - 3.3077115913*mm + .2309699292*ss, -1.2684380046*ll + 2.6097574011*mm - .3413193965*ss, -.0041960863*ll - .7034186147*mm + 1.7076147010*ss, alpha];
};
export function weighted(colors, weights) {
  const v = [0, 0, 0]; let alpha = 0;
  colors.forEach((c, i) => { const wa = weights[i] * c[3], q = lab(c); alpha += wa; for (let j = 0; j < 3; j++) v[j] += wa * q[j]; });
  return alpha <= 1e-15 ? [0, 0, 0, 0] : fromLab(v.map(x => x / alpha), clamp(alpha));
}
const transparent = [0, 0, 0, 0];
const pre = c => [c[0]*c[3], c[1]*c[3], c[2]*c[3], c[3]];
const scale = (c, a) => c.map(x => x * a);
const over = (a, b) => a.map((x, j) => x + b[j] * (1-a[3]));
const mix = (a, b, t) => a.map((x, j) => x*(1-t) + b[j]*t);
const straight = c => c[3] <= 1e-15 ? transparent : [c[0]/c[3], c[1]/c[3], c[2]/c[3], c[3]];
function ramp(stops, t) {
  if (!stops.length) return transparent;
  if (t < stops[0].at) return stops[0].color;
  let j = 0; while (j < stops.length && stops[j].at <= t) j++;
  if (j === stops.length) return stops[j-1].color;
  const a = stops[j-1], b = stops[j], u = (t - a.at) / (b.at - a.at);
  return weighted([a.color, b.color], [1-u, u]);
}
export function sample(brush, x, y) {
  if (!brush) return transparent;
  if (Array.isArray(brush)) return brush;
  if (brush.kind === 'linear') {
    const [ax, ay] = brush.from, [bx, by] = brush.to, dx = bx-ax, dy = by-ay;
    return ramp(brush.stops, ((x-ax)*dx + (y-ay)*dy)/(dx*dx+dy*dy));
  }
  if (brush.kind === 'radial') return ramp(brush.stops, Math.hypot(x-brush.center[0], y-brush.center[1])/brush.radius);
  if (brush.kind === 'conic') {
    const a = Math.atan2(y-brush.center[1], x-brush.center[0]) + Math.PI/2 - brush.angle*Math.PI/180;
    return ramp(brush.stops, ((a % (2*Math.PI)) + 2*Math.PI) % (2*Math.PI) / (2*Math.PI));
  }
  throw Error('The browser reference samples solid/gradient brushes, not image brushes.');
}
export function pixel(sources, x, y, o, px = 1) {
  const ds = sources.map(s => distance(s.shape, x, y)), t = smoothstep(o.progress);
  const d = smoothMin(ds, 2*o.reach*t).distance;
  const coverage = z => clamp(.5-z/px);
  if (coverage(d) === 0) return transparent;
  const { weights } = smoothMin(ds, o.blend*t);
  let original = transparent, oldFill = transparent;
  sources.forEach((s, i) => {
    const a = coverage(ds[i]);
    const f = o.fill === 'omit' ? transparent : sample(s.fill, x, y);
    const b = o.border === 'omit' ? transparent : sample(s.border, x, y);
    const ring = Math.max(0, a-coverage(ds[i]+s.width)), ratio = a > 0 ? ring/a : 0;
    original = over(scale(over(scale(pre(b), ratio), pre(f)), a), original);
    oldFill = over(scale(pre(f), a), oldFill);
  });
  if (t === 0) return straight(original);
  const a = coverage(d);
  let result = o.fill === 'blend' ? scale(pre(weighted(sources.map(s => sample(s.fill,x,y)), weights)), a) : o.fill === 'keep' ? oldFill : transparent;
  if (o.border === 'blend') {
    const width = sources.reduce((sum,s,i) => sum+weights[i]*(s.border ? s.width : 0),0);
    const b = weighted(sources.map(s => sample(s.border,x,y)), weights), pb = pre(b);
    const ring = Math.max(0,a-coverage(d+width)), ratio = a > 0 ? ring/a : 0;
    result = result.map((v,j) => v*(1-b[3]*ratio)+pb[j]*ring);
  } else if (o.border === 'keep') {
    sources.forEach((s,i) => {
      const c=sample(s.border,x,y), pc=pre(c);
      const ring=Math.max(0,coverage(ds[i])-coverage(ds[i]+s.width)), ratio=a>0?Math.min(1,ring/a):0;
      result=result.map((v,j)=>v*(1-c[3]*ratio)+pc[j]*ring);
    });
  }
  const base = coverage(Math.min(...ds)), added = a > 0 ? clamp((a-base)/a) : 0;
  return straight(mix(original,result,t+(1-t)*added));
}
export function contours(values, w, h, b) {
  const sx=b.w/w, sy=b.h/h;
  const position=i=>[b.x+(i%(w+1))*sx,b.y+Math.floor(i/(w+1))*sy];
  const edge=(a,c)=>a<c?`${a}:${c}`:`${c}:${a}`;
  const next=new Map(), incoming=new Set();
  for(let y=0;y<h;y++) for(let x=0;x<w;x++) {
    const a=y*(w+1)+x,d=a+w+1;
    for(const ids of [[a,a+1,d+1],[a,d+1,d]]) {
      const cuts=[];
      for(let j=0;j<3;j++) {
        const u=ids[j],v=ids[(j+1)%3]; if((values[u]<0)===(values[v]<0))continue;
        const t=values[u]/(values[u]-values[v]),p=position(u),q=position(v);
        cuts.push([edge(u,v),[p[0]+(q[0]-p[0])*t,p[1]+(q[1]-p[1])*t]]);
      }
      if(!cuts.length)continue; if(cuts.length!==2)throw Error('open contour');
      const q=position(ids.find(i=>values[i]<0));let [a,z]=cuts;
      if((z[1][0]-a[1][0])*(q[1]-a[1][1])-(z[1][1]-a[1][1])*(q[0]-a[1][0])<0)[a,z]=[z,a];
      if(next.has(a[0])||incoming.has(z[0]))throw Error('branched contour');
      next.set(a[0],[z[0],a[1]]);incoming.add(z[0]);
    }
  }
  const rings=[];
  while(next.size) {
    const start=next.keys().next().value;let at=start;const ring=[];
    do {const entry=next.get(at);if(!entry)throw Error('unclosed contour');next.delete(at);ring.push(entry[1]);at=entry[0];} while(at!==start);
    if(ring.length>=3)rings.push(ring);
  }
  return rings;
}
export function bake(sources, o, scale = 1, maxPixels = 1_048_576) {
  validate(sources,o);
  if(!Number.isFinite(scale)||scale<.125||scale>8)throw Error('quality');
  const bb=sources.map(s=>s.shape.rings?{
    x:Math.min(...s.shape.rings.flat().map(p=>p[0])), y:Math.min(...s.shape.rings.flat().map(p=>p[1])),
    x1:Math.max(...s.shape.rings.flat().map(p=>p[0])), y1:Math.max(...s.shape.rings.flat().map(p=>p[1]))
  }:{x:s.shape.x,y:s.shape.y,x1:s.shape.x+s.shape.w,y1:s.shape.y+s.shape.h});
  const px=1/scale,pad=o.reach+2*px;
  const x=Math.floor((Math.min(...bb.map(b=>b.x))-pad)*scale)/scale,y=Math.floor((Math.min(...bb.map(b=>b.y))-pad)*scale)/scale;
  const w=Math.ceil((Math.max(...bb.map(b=>b.x1))+pad-x)*scale),h=Math.ceil((Math.max(...bb.map(b=>b.y1))+pad-y)*scale);
  if(w>4096||h>4096||w*h>maxPixels)throw Error('pixel budget');
  const bounds={x,y,w:w*px,h:h*px};
  const values=new Float64Array((w+1)*(h+1)),rgba=new Uint8ClampedArray(w*h*4);
  for(let j=0;j<=h;j++)for(let i=0;i<=w;i++) {const d=field(sources,x+i*px,y+j*px,o).distance;values[j*(w+1)+i]=d===0?px*1e-10:d;}
  const rings=contours(values,w,h,bounds);
  for(let j=0;j<h;j++)for(let i=0;i<w;i++)rgba.set(rgba8(pixel(sources,x+(i+.5)*px,y+(j+.5)*px,o,px)),4*(j*w+i));
  return {width:w,height:h,bounds,rgba,contours:rings};
}
