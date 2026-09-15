"""Independent Cairo supersampling oracle; diagnostics, not a blanket pass threshold."""
import json,sys,io,html
from pathlib import Path
import numpy as np
from PIL import Image
import cairosvg
out=Path(sys.argv[1] if len(sys.argv)>1 else 'output')
marks=json.loads((out/'fixture.json').read_text())
def svg():
 parts=['<svg xmlns="http://www.w3.org/2000/svg" width="768" height="512" viewBox="0 0 768 512">']
 for i,m in enumerate(marks):
  c=m['color']; fill=f'rgb({c[0]},{c[1]},{c[2]})';opacity=c[3]/255
  if m['gradient']:
   b=m['gradient']; parts.append(f'<defs><linearGradient id="g{i}" x1="0" y1="0" x2="{0 if m.get("vertical_gradient") else 1}" y2="{1 if m.get("vertical_gradient") else 0}"><stop stop-color="{fill}" stop-opacity="{opacity}"/><stop offset="1" stop-color="rgb({b[0]},{b[1]},{b[2]})" stop-opacity="{b[3]/255}"/></linearGradient></defs>'); fill=f'url(#g{i})';opacity=1
  if m['clip']:
   x,y,r,b=m['clip'];parts.append(f'<defs><clipPath id="c{i}"><rect x="{x}" y="{y}" width="{r-x}" height="{b-y}"/></clipPath></defs><g clip-path="url(#c{i})">')
  attr=f'fill="{fill}" fill-opacity="{opacity}"'
  if m['stroke'] is not None:attr=f'fill="none" stroke="{fill}" stroke-opacity="{opacity}" stroke-width="{m["stroke"]}"'
  parts.append(f'<path d="{html.escape(m["path"])}" {attr}/>')
  if m['clip']:parts.append('</g>')
 parts.append('</svg>');return ''.join(parts)
source=svg();(out/'reference.svg').write_text(source)
regions={'patches':(0,0,768,95),'merged':(10,100,360,322),'hole_star':(380,100,685,235),'thin_strokes':(0,338,768,402),'rotated_clip':(0,410,768,485)}
components=any(m["name"].startswith("port-shell-") for m in marks)
if components:
 regions={"adsr":(0,10,768,80), "ports":(0,85,768,215), "circles":(0,260,768,460), "colored_ramps":(16,478,742,502)}
report={}
references={}
for image_path in sorted(out.glob('*.png')):
 if not image_path.stem.startswith(('vello','gpui')):continue
 actual=Image.open(image_path).convert('RGBA');w,h=actual.size;scale=w/768
 if (w,h) not in references:
  references[w,h]=Image.open(io.BytesIO(cairosvg.svg2png(bytestring=source.encode(),output_width=w*4,output_height=h*4))).convert('RGBA').resize((w,h),Image.Resampling.BOX)
  references[w,h].save(out/f'reference-{scale:g}.png')
 reference=references[w,h]
 a=np.asarray(actual).astype(float);b=np.asarray(reference).astype(float);d=np.abs(a[:,:,:3]-b[:,:,:3]);
 # Cairo has no Oklab interpolation. Exclude that strip from image error metrics;
 # check_gradients.py validates it with independent analytic color conversion.
 if components:
  d=d.copy();d[int(225*scale):int(255*scale)]=np.nan

 data={'scale':scale,'reference':'Cairo 4x supersampling + box downsample, shared cubic paths; not ground truth','rgb_mae':float(np.nanmean(d)),'rgb_p99':float(np.nanpercentile(d,99)),'pixels_over_16':float((d[np.isfinite(d).all(2)].max(1)>16).mean()),'regions':{}}
 for name,(x,y,r,bt) in regions.items():
  crop=d[int(y*scale):int(bt*scale),int(x*scale):int(r*scale)];data['regions'][name]={'mae':float(crop.mean()),'p99':float(np.percentile(crop,99)),'pixels_over_16':float((crop.max(2)>16).mean())}
 if not components:
  # Analytical interiors: exact opaque primaries and alpha white over the declared canvas.
  expected=[[255,255,255],[128,128,128],[255,0,0],[0,255,0],[0,0,255],[138,138,140]]
  data['solid_patch_max_errors']=[]
  for i,e in enumerate(expected):
   pixel=a[int(45*scale),int((40+i*94)*scale),:3];data['solid_patch_max_errors'].append(float(np.abs(pixel-e).max()))
  # Explicit sRGB black-to-white ramp at pixel centers, excluding edge AA.
  x0,x1=580*scale,662*scale
  xs=np.arange(int(np.ceil(x0+2)),int(np.floor(x1-2)))
  expected_ramp=((xs+0.5-x0)/(x1-x0))*255
  measured_ramp=a[int(45*scale),xs,0]
  data['srgb_ramp_mae']=float(np.abs(measured_ramp-expected_ramp).mean())
  # Bilinear samples along the true quarter-pixel circle centerline.
  theta=np.linspace(0,2*np.pi,4096,endpoint=False)
  xx=(30+24.25*np.cos(theta))*scale-0.5
  yy=(370+24.25*np.sin(theta))*scale-0.5
  xi=np.floor(xx).astype(int);yi=np.floor(yy).astype(int);fx=xx-xi;fy=yy-yi
  luminance=(a[:,:,0]-20)/210
  sampled=(luminance[yi,xi]*(1-fx)*(1-fy)+luminance[yi,xi+1]*fx*(1-fy)+luminance[yi+1,xi]*(1-fx)*fy+luminance[yi+1,xi+1]*fx*fy)
  data['quarter_pixel_circle_centerline_min']=float(sampled.min())
  data['quarter_pixel_circle_weak_arc_fraction']=float((sampled<0.025).mean())
  # Thin-stroke coverage integral: detects loss even if global pixel MAE looks small.
  bg=np.array([20,20,24]);fg=np.array([230,230,235]);
  y0,y1=int(339*scale),int(401*scale)
  coverage=((a[y0:y1,:,:3]-bg)/(fg-bg)).clip(0,1).mean(2)
  refcoverage=((b[y0:y1,:,:3]-bg)/(fg-bg)).clip(0,1).mean(2)
  data['thin_stroke_coverage_ratio']=float(coverage.sum()/refcoverage.sum())
 Image.fromarray(np.clip(np.nan_to_num(d)*6,0,255).astype('uint8')).save(out/f'diff-{image_path.name}')
 report[image_path.name]=data
(out/'quality.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))

import hashlib
determinism={}
for mode in ['gpui','vello-area-present','vello-present']:
 paths=[out/f'{mode}-1.png']+[out/f'repeat-{i}/{mode}-1.png' for i in [2,3]]
 if all(p.exists() for p in paths):
  hashes=[hashlib.sha256(Image.open(p).convert('RGBA').tobytes()).hexdigest() for p in paths]
  determinism[mode]={'pixel_sha256':hashes,'identical':len(set(hashes))==1,'runs':3}
(out/'determinism.json').write_text(json.dumps(determinism,indent=2))
