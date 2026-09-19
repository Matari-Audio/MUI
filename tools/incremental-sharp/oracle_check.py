"""Independent GEOS/Shapely union oracle for the JS analytic boundary.
Input circular arcs are polygonized at 256 steps/quarter; tolerance is therefore
0.002 logical units, not an assertion of exact polygon/circle equivalence.
"""
import json,subprocess,math
from pathlib import Path
from shapely.geometry import Polygon,Point
from shapely.ops import unary_union
here=Path(__file__).resolve().parent
js=r'''
import {unionBoundary,sampleBoundary,primitives,point} from './boundary.mjs';
let seed=17;const rnd=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/4294967296};
let cases=[];
for(let n=0;n<24;n++){
 const shapes=Array.from({length:1+n%3},(_,i)=>({cx:30*(rnd()-.5)+20*i,cy:30*(rnd()-.5),hx:10+rnd()*40,hy:8+rnd()*30,r:rnd()*8,angle:rnd()*6.28}));
 const b=unionBoundary(shapes),rings=shapes.map((s,i)=>primitives(s,i).flatMap(e=>Array.from({length:e.kind?256:1},(_,j)=>point(e,j/(e.kind?256:1)))));
 const samples=Array.from({length:500},()=>{let p=[160*rnd()-60,120*rnd()-60];return [...p,sampleBoundary(shapes,b,p).distance]});
 cases.push({rings,samples});
}console.log(JSON.stringify(cases));
'''
cases=json.loads(subprocess.check_output(['node','--input-type=module','-e',js],cwd=here))
maximum=0.;count=0
for c in cases:
 union=unary_union([Polygon(r) for r in c['rings']])
 assert union.is_valid
 for x,y,d in c['samples']:
  p=Point(x,y);expected=union.boundary.distance(p)*(-1 if union.covers(p) else 1)
  error=abs(d-expected);maximum=max(maximum,error);count+=1
  assert error<.002,(x,y,d,expected,error)
print(json.dumps({'cases':len(cases),'points':count,'maximum_absolute_error':maximum,'tolerance':.002,'reference':'GEOS/Shapely polygonized exact unions','status':'PASS'}))
