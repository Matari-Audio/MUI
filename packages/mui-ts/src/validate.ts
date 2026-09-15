import type { Scene, Node, Spacing } from "./index.js";
/** Validate author mistakes before emitting Rust, while Rust remains the runtime authority. */
export function validateScene(scene: Scene): void {
  const fail = (message:string):never => {throw new Error(`MUI: ${message}`);};
  const finite = (v:number) => {if (!Number.isFinite(v) || v < 0) fail(`invalid nonnegative number: ${v}`);};
  const spacing = (v:number|Spacing) => {
    if(typeof v==="number") finite(v);
    else if(v?.kind==="px") finite(v.value);
    else if(v?.kind!=="token" || !["xs","s","m","l","xl"].includes(v.value)) fail("invalid spacing token");
  };
  const keys = new Set<string>();
  const visit = (node:Node,scope="",path="0",depth=0):void => {
    if(!node || !["leaf","row","column","stack"].includes(node.kind)) fail("invalid node kind");
    if(depth>128 || keys.size>=4096) fail("layout budget exceeded");
    const p=node.props ?? {};
    if(p.scope!==undefined) {
      if(!p.scope || p.scope.includes("/") || p.scope.startsWith("@")) fail("invalid component scope");
      scope+=p.scope+"/";
    }
    if(typeof node.id!=="string" || node.id.includes("/") || node.id.startsWith("@")) fail("invalid node key");
    const key=scope+(node.id||"@"+path);
    if(keys.has(key)) fail(`duplicate layout key: ${key}`); keys.add(key);
    for(const pair of [p.min,p.max]) if(pair) pair.forEach(finite);
    if(p.min && p.max && p.min.some((n,i)=>n>p.max![i])) fail(`min exceeds max: ${key}`);
    if(p.gap!==undefined) spacing(p.gap);
    if(p.padding!==undefined) {
      if(typeof p.padding==="number" || "kind" in p.padding) spacing(p.padding);
      else Object.values(p.padding).forEach(finite);
    }
    for(const n of [p.grow,p.shrink]) if(n!==undefined) finite(n);
    for(const v of [p.width,p.height]) if(v!==undefined) {if(typeof v==="number") finite(v);else if(v!=="hug"&&v!=="fill") fail("invalid sizing");}
    if(p.axis && !["row","column","auto"].includes(p.axis)) fail("invalid axis");
    if(p.align && !["start","center","end","stretch","baseline"].includes(p.align)) fail("invalid alignment");
    if(p.justify && !["start","center","end","space-between"].includes(p.justify)) fail("invalid justification");
    if(p.overflow !== undefined && !["fit", "clip", "scroll"].includes(p.overflow)) fail("invalid overflow policy");
    if(p.wrap && p.axis==="auto") fail("wrap and auto axis are distinct policies");
    if(node.kind==="leaf") node.size.forEach(finite);
    else node.children.forEach((c,i)=>visit(c,scope,`${path}.${i}`,depth+1));
  };
  if(!scene?.root || !Array.isArray(scene.surfaces)) fail("expected a scene");
  visit(scene.root);
  const surfaces=new Map(scene.surfaces.map(s=>[s.id,s]));
  if(surfaces.size!==scene.surfaces.length) fail("duplicate surface ID");
  if(surfaces.size>2048) fail("surface budget exceeded");
  const deps=new Map<string,string[]>();
  let edges = 0;
  for(const s of scene.surfaces) {
    if(!s.id) fail("empty surface ID");
    let inputs:string[]=[];
    switch(s.kind) {
      case "frame":
        if(!keys.has(s.layout)) fail(`missing layout key: ${s.layout}`);
        if(s.radius.kind==="absolute") finite(s.radius.value);
        else if(s.radius.kind==="parent-normalized") {finite(s.radius.scale);inputs=[s.radius.parent];}
        else if(s.radius.kind!=="global") fail("invalid radius");
        if(s.extension) {
          if(!["top","right","bottom","left"].includes(s.extension.edge)) fail("invalid extension edge");
          if(!keys.has(s.extension.target)) fail(`missing extension target: ${s.extension.target}`);
        }
        break;
      case "merge":
        if(!s.inputs.length) fail(`empty merge: ${s.id}`);inputs=s.inputs;
        if(s.corners.kind==="absolute") {finite(s.corners.convex);finite(s.corners.concave);}
        else if(s.corners.kind==="global-scaled") finite(s.corners.scale);
        else if(s.corners.kind!=="global") fail("invalid corners");
        break;
      case "inset": case "outset": spacing(s.distance); inputs=[s.parent];break;
      default: fail("invalid surface kind");
    }
    edges += inputs.length; if(edges > 16384) fail("surface edge budget exceeded");
    for(const input of inputs) if(!surfaces.has(input)) fail(`missing surface: ${input}`);
    deps.set(s.id,inputs);
  }
  const done=new Map<string,number>();const active=new Set<string>();
  const depth=(id:string):number=>{
    if(active.has(id)) fail(`surface dependency cycle: ${id}`);
    if(done.has(id)) return done.get(id)!;
    if(active.size>128) fail("surface depth exceeded");
    active.add(id);let d=0;for(const parent of deps.get(id)!) d=Math.max(d,depth(parent)+1);
    active.delete(id);if(d>128) fail("surface depth exceeded");done.set(id,d);return d;
  };
  for(const id of surfaces.keys()) depth(id);
  if(scene.theme?.hoverShift !== undefined && (!Number.isFinite(scene.theme.hoverShift) || scene.theme.hoverShift<=0 || scene.theme.hoverShift>0.5)) fail("hoverShift must be in (0, 0.5]");
  if(scene.theme?.contrast) { const c=scene.theme.contrast; if(!Number.isFinite(c.text)||c.text<4.5||c.text>21||!Number.isFinite(c.graphics)||c.graphics<3||c.graphics>21) fail("contrast must meet AA minimums"); }
  if(scene.theme?.mode && !["light","dark"].includes(scene.theme.mode)) fail("invalid theme mode");
  for(const colors of [scene.theme?.colors,scene.theme?.darkColors]) if(colors) {
    if(colors.primary.length!==3 || (colors.status && colors.status.length!==4)) fail("invalid palette size");
    for(const color of [...colors.primary,colors.neutral,...(colors.status??[])]) if(!Number.isInteger(color)||color<0||color>0xffffff) fail("color must be a 24-bit RGB integer");
  }
  if(scene.theme?.corners) Object.values(scene.theme.corners).forEach(finite);
  if(scene.theme?.spacing) Object.values(scene.theme.spacing).forEach(finite);
  if(scene.theme?.strokeWidth!==undefined) finite(scene.theme.strokeWidth);
  scene.offered?.forEach(finite);
  if(scene.availableWidth !== undefined) { finite(scene.availableWidth); if(scene.offered) fail("use offered or availableWidth, not both"); }
}
