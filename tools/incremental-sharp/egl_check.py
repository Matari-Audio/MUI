"""Offscreen GLES check via Mesa EGL. This is NOT a wgpu/WGSL validation.
No browser policies are modified. Fails explicitly if EGL or ES3 is absent.
"""
import ctypes as C, json, re, os
from pathlib import Path
os.environ.setdefault('LIBGL_ALWAYS_SOFTWARE','1')
E=C.CDLL('libEGL.so.1')
def ef(name,ret,args):
 f=getattr(E,name);f.restype=ret;f.argtypes=args;return f
ptr=C.c_void_p; I=C.c_int; U=C.c_uint
getproc=ef('eglGetProcAddress',ptr,[C.c_char_p])
def gl(name,ret,args):
 p=getproc(name.encode());assert p,name
 return C.CFUNCTYPE(ret,*args)(p)
getdisplay=C.CFUNCTYPE(ptr,U,ptr,ptr)(getproc(b'eglGetPlatformDisplayEXT'))
display=getdisplay(0x31DD,None,None)
assert display,'surfaceless EGL'
a,b=I(),I();assert ef('eglInitialize',U,[ptr,C.POINTER(I),C.POINTER(I)])(display,C.byref(a),C.byref(b))
assert ef('eglBindAPI',U,[U])(0x30A0)
attrs=(I*17)(0x3033,1,0x3040,0x40,0x3024,8,0x3023,8,0x3022,8,0x3021,8,0x3025,0,0x3026,0,0x3038)
config=ptr();num=I();assert ef('eglChooseConfig',U,[ptr,C.POINTER(I),C.POINTER(ptr),I,C.POINTER(I)])(display,attrs,C.byref(config),1,C.byref(num)) and num.value
surfattrs=(I*5)(0x3057,720,0x3056,420,0x3038)
surface=ef('eglCreatePbufferSurface',ptr,[ptr,ptr,C.POINTER(I)])(display,config,surfattrs)
ctxattrs=(I*3)(0x3098,3,0x3038)
context=ef('eglCreateContext',ptr,[ptr,ptr,ptr,C.POINTER(I)])(display,config,None,ctxattrs)
assert context and surface
assert ef('eglMakeCurrent',U,[ptr,ptr,ptr,ptr])(display,surface,surface,context)
getstr=gl('glGetString',C.c_char_p,[U])
print(json.dumps({'EGL':f'{a.value}.{b.value}','renderer':getstr(0x1F01).decode(),'version':getstr(0x1F02).decode()}))
create=gl('glCreateShader',U,[U]);source=gl('glShaderSource',None,[U,I,C.POINTER(C.c_char_p),C.POINTER(I)])
compile_=gl('glCompileShader',None,[U]);status=gl('glGetShaderiv',None,[U,U,C.POINTER(I)]);log=gl('glGetShaderInfoLog',None,[U,I,C.POINTER(I),C.c_char_p])
s=Path(__file__).with_name('crisp-demo.html').read_text()
shaders=[]
for kind,name in [(0x8B31,'VERTEX_SHADER'),(0x8B30,'FRAGMENT_SHADER')]:
 text=json.loads(re.search(r'const '+name+r'=(".*?");\n',s).group(1)).encode()
 sh=create(kind);c=C.c_char_p(text);source(sh,1,C.byref(c),None);compile_(sh);ok=I();status(sh,0x8B81,C.byref(ok));out=C.create_string_buffer(32768);log(sh,len(out),None,out)
 print(name, bool(ok.value),out.value.decode());assert ok.value;shaders.append(sh)
program=gl('glCreateProgram',U,[])()
for sh in shaders:gl('glAttachShader',None,[U,U])(program,sh)
gl('glLinkProgram',None,[U])(program)
ok=I();gl('glGetProgramiv',None,[U,U,C.POINTER(I)])(program,0x8B82,C.byref(ok));out=C.create_string_buffer(32768);gl('glGetProgramInfoLog',None,[U,I,C.POINTER(I),C.c_char_p])(program,len(out),None,out)
print('link',bool(ok.value),out.value.decode());assert ok.value
# Uniform ABI is checked on the executing driver, not by counting source lines.
for name,expected,binding in [(b'WeldParams',352,0),(b'CrispBoundary',6144,1)]:
 block=gl('glGetUniformBlockIndex',U,[U,C.c_char_p])(program,name);assert block!=0xFFFFFFFF
 size=I();gl('glGetActiveUniformBlockiv',None,[U,U,U,C.POINTER(I)])(program,block,0x8A40,C.byref(size));assert size.value==expected,(name,size.value)
 gl('glUniformBlockBinding',None,[U,U,U])(program,block,binding)
 print(name.decode(),'ABI',size.value)
print('GLES shader compilation, link, and both uniform ABIs: PASS')
# Render conformance: analytic CPU line/arc reference vs actual GLES fragments.
# Black opaque fill and white opaque borders make alpha and physical stroke bands
# independently measurable. These captures are diagnostic readbacks only.
import subprocess, numpy as np
from PIL import Image
HERE=Path(__file__).resolve().parent
script=r'''
import {unionBoundary,boundaryWidth,encodeBoundary} from './boundary.mjs';
const cs=[];
for(const degrees of [0,15,30,45,60,90,-30,-75]){
 const angle=degrees*Math.PI/180;
 const shapes=[{cx:244,cy:220,hx:94,hy:74,r:27,angle:0},{cx:392,cy:184,hx:82,hy:70,r:35.1,angle}];
 const edges=unionBoundary(shapes),p=new Float32Array(88);
 p.set([720,420,720,420,0,65,1,2,0,0,0,1,0,0,720,420]);
 shapes.forEach((s,i)=>{let b=16+i*24;p.set([s.cx,s.cy,s.hx,s.hy,s.r,i?4:2,Math.cos(s.angle),Math.sin(s.angle)],b);p.set([0,0,0,1,0,0,0,1,1,0,0,1,0,0,0,0],b+8);});
 const expected=[];const cov=d=>Math.max(0,Math.min(1,.5-d));
 const enc=x=>x<=.0031308?12.92*x:1.055*Math.pow(x,1/2.4)-.055;
 for(let y=0;y<420;y+=4)for(let x=0;x<720;x+=4){
  const h=boundaryWidth(shapes,edges,[x+.5,y+.5],[2,4],65),a=cov(h.distance),ring=a-cov(h.distance+h.width);
  const rgb=a?Math.round(enc(ring/a)*a*255):0;expected.push([x,y,rgb,rgb,rgb,Math.round(a*255)]);
 }
 cs.push({degrees,params:Array.from(p),boundary:Array.from(encodeBoundary(edges)),count:edges.length,expected});
}
console.log(JSON.stringify(cs));
'''
fixture=json.loads(subprocess.check_output(['node','--input-type=module','-e',script],cwd=HERE))
use=gl('glUseProgram',None,[U]);use(program)
gen=gl('glGenBuffers',None,[I,C.POINTER(U)]);bind=gl('glBindBuffer',None,[U,U]);upload=gl('glBufferData',None,[U,C.c_ssize_t,ptr,U]);base=gl('glBindBufferBase',None,[U,U,U]);buffers=(U*2)();gen(2,buffers)
vao=U();gl('glGenVertexArrays',None,[I,C.POINTER(U)])(1,C.byref(vao));gl('glBindVertexArray',None,[U])(vao)
gl('glDisable',None,[U])(0x0BE2);gl('glViewport',None,[I,I,I,I])(0,0,720,420)
loc=gl('glGetUniformLocation',I,[U,C.c_char_p])(program,b'boundaryCount')
read=gl('glReadPixels',None,[I,I,I,I,U,U,ptr]);pix=np.empty((420,720,4),dtype=np.uint8)
results=[]
for case in fixture:
 for i,v in enumerate([case['params'],case['boundary']]):
  arr=np.asarray(v,dtype=np.float32);bind(0x8A11,buffers[i]);upload(0x8A11,arr.nbytes,arr.ctypes.data,0x88E8);base(0x8A11,i,buffers[i])
 gl('glUniform1i',None,[I,I])(loc,case['count']);gl('glClearColor',None,[C.c_float]*4)(0,0,0,0);gl('glClear',None,[U])(0x4000)
 gl('glDrawArrays',None,[U,I,I])(0x0005,0,4);read(0,0,720,420,0x1908,0x1401,pix.ctypes.data)
 error=gl('glGetError',U,[])();assert error==0,hex(error)
 image=pix[::-1]
 expected=np.asarray(case['expected'],dtype=np.int32)
 actual=image[expected[:,1],expected[:,0]].astype(np.int32)
 delta=np.abs(actual-expected[:,2:]);maximum=int(delta.max());bad=int(np.any(delta>2,axis=1).sum())
 result={'degrees':case['degrees'],'segments':case['count'],'pixels':len(expected),'max_channel_error':maximum,'pixels_over_2':bad};results.append(result);print(json.dumps(result))
 assert bad==0,result
 if case['degrees']==30 and os.environ.get('MUI_GLES_CAPTURE'):Image.fromarray(image).save(os.environ['MUI_GLES_CAPTURE'])
print(json.dumps({'render_cases':len(results),'sampled_pixels':sum(x['pixels'] for x in results),'max_channel_error':max(x['max_channel_error'] for x in results),'status':'PASS','scope':'GLES software rendering; NOT WGSL/Vello/phone'}))
