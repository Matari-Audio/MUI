#version 300 es
precision highp float;
// This is a bounded analytic fast path for 1..3 rotated rounded rectangles.
// Independent source channels and morph endpoints match the CPU specification.
// No polygon traversal, contour extraction, texture baking, or readback.
struct Source { vec4 geom; vec4 params; vec4 fill0; vec4 fill1; vec4 edge; vec4 grad; };
layout(std140) uniform WeldParams { vec4 viewport; vec4 cfg; vec4 policy; vec4 crop; Source sources[3]; };
struct BoundaryEdge { vec4 geom; vec4 directions; vec4 meta; };
layout(std140) uniform CrispBoundary { BoundaryEdge edges[128]; };
uniform int boundaryCount;
float cross2(vec2 a,vec2 b){return a.x*b.y-a.y*b.x;}
vec2 nearestEdge(BoundaryEdge e,vec2 p){
 if(e.meta.x<0.5){vec2 v=e.geom.zw-e.geom.xy;return e.geom.xy+v*clamp(dot(p-e.geom.xy,v)/max(dot(v,v),1e-20),0.0,1.0);}
 vec2 v=p-e.geom.xy;float n=length(v);vec2 d=v/max(n,1e-20);
 if(n>1e-10&&cross2(e.directions.xy,d)>=-1e-6&&cross2(d,e.directions.zw)>=-1e-6)return e.geom.xy+d*e.geom.z;
 vec2 a=e.geom.xy+e.directions.xy*e.geom.z,b=e.geom.xy+e.directions.zw*e.geom.z;return distance(p,a)<=distance(p,b)?a:b;
}
vec2 nearestBoundary(vec2 p){float best=1e30;vec2 q=p;for(int i=0;i<128;i++){if(i>=boundaryCount)break;vec2 v=nearestEdge(edges[i],p);float d=dot(v-p,v-p);if(d<best){best=d;q=v;}}return q;}
in vec2 point;
out vec4 outColor;
vec2 localPoint(Source s, vec2 p) {
    vec2 q = p - s.geom.xy;
    return vec2(s.params.z*q.x + s.params.w*q.y, -s.params.w*q.x + s.params.z*q.y);
}
float rectDistance(Source s, vec2 p) {
    float r = min(s.params.x, min(s.geom.z, s.geom.w));
    vec2 q = abs(localPoint(s, p)) - s.geom.zw + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
// Projection onto a simplex: symmetric in all sources; no sequential smooth-min.
// A sorting network and two thresholds specialize the reference water-fill to 3.
vec3 weightsAt(vec3 d, float k) {
    float lo = min(d.x, min(d.y, d.z));
    if (k <= 0.000001) {
        vec3 ties = vec3(d.x == lo ? 1.0 : 0.0, d.y == lo ? 1.0 : 0.0, d.z == lo ? 1.0 : 0.0);
        return ties / (ties.x + ties.y + ties.z);
    }
    vec3 q = d - lo;
    float hi = max(q.x, max(q.y, q.z));
    // Median via min/max, rather than sum-minus-max: avoids cancellation against
    // the unused-slot sentinel on the two-source specialization.
    float mid = max(min(q.x,q.y), min(max(q.x,q.y),q.z));
    float lambda = k;
    if (mid < lambda) lambda = (k + mid)*0.5;
    if (hi < lambda) lambda = (k + mid + hi)/3.0;
    vec3 w = max(vec3(lambda) - q, 0.0)/k;
    return w / (w.x + w.y + w.z);
}
float joinedDistance(vec3 d, float k) {
    float lo = min(d.x,min(d.y,d.z));
    vec3 w = weightsAt(d,k);
    return lo + dot(w,d-lo) - k*0.5*(1.0-dot(w,w));
}
// Source endpoints are converted to Oklab once by the host, not per fragment.
vec4 gradientLab(Source s, vec2 p) {
    float t = clamp(dot(localPoint(s,p)-s.grad.xy, s.grad.zw),0.0,1.0);
    float a = mix(s.fill0.a,s.fill1.a,t);
    vec3 lab = mix(s.fill0.rgb*s.fill0.a,s.fill1.rgb*s.fill1.a,t)/max(a,1e-12);
    return vec4(lab,a);
}
vec4 labLinear(vec4 v) {
    vec3 lms = vec3(v.x + .3963377774*v.y + .2158037573*v.z,
                    v.x - .1055613458*v.y - .0638541728*v.z,
                    v.x - .0894841775*v.y - 1.2914855480*v.z);
    lms = lms*lms*lms;
    return vec4(4.0767416621*lms.x-3.3077115913*lms.y+.2309699292*lms.z,
                -1.2684380046*lms.x+2.6097574011*lms.y-.3413193965*lms.z,
                -.0041960863*lms.x-.7034186147*lms.y+1.7076147010*lms.z,v.a);
}
vec4 mixedLab(vec4 a, vec4 b, vec4 c, vec3 w) {
    float alpha = dot(vec3(a.a,b.a,c.a),w);
    return vec4((a.rgb*a.a*w.x+b.rgb*b.a*w.y+c.rgb*c.a*w.z)/max(alpha,1e-12),alpha);
}
vec4 premul(vec4 c) { return vec4(c.rgb*c.a,c.a); }
vec4 over(vec4 a, vec4 b) { return a+b*(1.0-a.a); }
float coverage(float d, float aa) { return clamp(.5-d/aa,0.0,1.0); }
vec3 encodeSrgb(vec3 c) {
    c = clamp(c,0.0,1.0);
    return mix(1.055*pow(c,vec3(1.0/2.4))-.055,12.92*c,lessThanEqual(c,vec3(.0031308)));
}
void main() {
    int n = int(cfg.w);
    vec3 ds = vec3(rectDistance(sources[0],point),1e6,1e6);
    if(n>1) ds.y = rectDistance(sources[1],point);
    if(n>2) ds.z = rectDistance(sources[2],point);
    float t = cfg.z*cfg.z*(3.0-2.0*cfg.z);
    float d = joinedDistance(ds,2.0*cfg.x*t);
    vec2 borderPoint=point;
    float hardMin=min(ds.x,min(ds.y,ds.z)),maxWidth=max(sources[0].params.y,max(sources[1].params.y,sources[2].params.y)),pixel=viewport.z/viewport.x;
    if(boundaryCount>0&&hardMin>=-maxWidth-2.0*pixel&&hardMin<=2.0*pixel){borderPoint=nearestBoundary(point);d=distance(point,borderPoint)*(hardMin<=0.0?-1.0:1.0);}
    // Derivatives are evaluated before any non-uniform return/discard.
    float derivative = length(vec2(dFdx(d),dFdy(d)));
    float aa = policy.w>0.0 ? policy.w : (boundaryCount>0?viewport.z/viewport.x:max(derivative,viewport.z/viewport.x));
    float a = coverage(d,aa);
    if(a<=0.0) { outColor=vec4(0); return; }
    vec4 labs[3]; vec4 borders[3];
    vec4 original=vec4(0),oldFill=vec4(0);
    for(int i=0;i<3;i++) {
        labs[i]=policy.x==2.0 ? vec4(0) : gradientLab(sources[i],point);
        borders[i]=policy.y==2.0 ? vec4(0) : sources[i].edge;
        if(i>=n) { labs[i]=vec4(0); borders[i]=vec4(0); continue; }
        float sa=coverage(ds[i],aa);
        float ring=max(0.0,sa-coverage(ds[i]+sources[i].params.y,aa));
        vec4 f=premul(labLinear(labs[i])),b=premul(labLinear(borders[i]));
        original=over(over(b*(ring/max(sa,1e-12)),f)*sa,original);
        oldFill=over(f*sa,oldFill);
    }
    vec4 result=original;
    if(t>0.0) {
        vec3 w=weightsAt(ds,cfg.y*t);
        result=policy.x==0.0 ? premul(labLinear(mixedLab(labs[0],labs[1],labs[2],w)))*a : (policy.x==1.0 ? oldFill : vec4(0));
        if(policy.y==0.0) {
            vec3 bw=w;
            if(boundaryCount>0){vec3 bd=vec3(abs(rectDistance(sources[0],borderPoint)),1e6,1e6);if(n>1)bd.y=abs(rectDistance(sources[1],borderPoint));if(n>2)bd.z=abs(rectDistance(sources[2],borderPoint));bw=weightsAt(bd,cfg.y*t);}
            float width=dot(vec3(sources[0].params.y,sources[1].params.y,sources[2].params.y),bw);
            vec4 b=labLinear(mixedLab(borders[0],borders[1],borders[2],bw));
            float ring=max(0.0,a-coverage(d+width,aa));
            result=result*(1.0-b.a*ring/max(a,1e-12))+premul(b)*ring;
        } else if(policy.y==1.0) {
            for(int i=0;i<3;i++) { if(i>=n) break;
                vec4 b=labLinear(borders[i]);
                float ring=max(0.0,coverage(ds[i],aa)-coverage(ds[i]+sources[i].params.y,aa));
                float ratio=min(1.0,ring/max(a,1e-12));
                result=result*(1.0-b.a*ratio)+premul(b)*ring;
            }
        }
        float base=coverage(min(ds.x,min(ds.y,ds.z)),aa);
        float added=clamp((a-base)/max(a,1e-12),0.0,1.0);
        result=mix(original,result,t+(1.0-t)*added);
    }
    // Browser canvas consumes premultiplied sRGB; the maths above was linear.
    vec3 rgb=encodeSrgb(result.rgb/max(result.a,1e-12));
    outColor=vec4(rgb*result.a,result.a);
    if(policy.z>0.0 && abs(d)<aa*1.5 && mod(floor((point.x+point.y)/5.0),2.0)<1.0) {
        float line=clamp(1.5-abs(d)/aa,0.0,1.0);
        outColor=mix(outColor,vec4(1),line);
    }
}
