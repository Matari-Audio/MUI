"""Copy reviewable evidence into the repo and produce a labeled AA comparison."""
from pathlib import Path
import json,shutil
from PIL import Image,ImageDraw,ImageFont
root=Path(__file__).resolve().parents[3]
out=Path(__file__).resolve().parents[1]/'output'
dest=root/'docs/render-lab';dest.mkdir(parents=True,exist_ok=True)
for p in out.rglob('*.json'):
 target=dest/p.relative_to(out);target.parent.mkdir(parents=True,exist_ok=True);shutil.copy(p,target)
for name in ['reference.svg','gpui-1.png','gpui-tight-1.png','vello-area-1.png','vello-1.png','reference-1.png','glass-gpui.png','glass-vello.png','diff-gpui-1.png','diff-vello-area-1.png']:
 if (out/name).exists():shutil.copy(out/name,dest/name)
# Pixel-nearest enlargement: do not smooth away AA differences.
font=ImageFont.truetype(str(root/'crates/mui-text/tests/fonts/DejaVuSans.ttf'),19)
small=ImageFont.truetype(str(root/'crates/mui-text/tests/fonts/DejaVuSans.ttf'),14)
canvas=Image.new('RGB',(1000,820),(20,20,24));draw=ImageDraw.Draw(canvas)
draw.text((18,12),'Actual render captures: quarter-pixel strokes and sRGB gradients',font=font,fill='white')
draw.text((18,42),'llvmpipe / software Vulkan | 1x | crops enlarged 3x with nearest-neighbor sampling',font=small,fill='#bbbbc4')
for i,(name,label) in enumerate([('gpui-1.png','GPUI default'),('gpui-tight-1.png','GPUI tighter path tolerance'),('vello-area-1.png','Vello area AA')]):
 y=82+i*239;draw.text((18,y),label,font=font,fill='white')
 im=Image.open(out/name).convert('RGB')
 crop=im.crop((0,337,248,402)).resize((744,195),Image.Resampling.NEAREST)
 canvas.paste(crop,(10,y+31))
 grad=im.crop((580,16,662,82)).resize((164,132),Image.Resampling.NEAREST)
 canvas.paste(grad,(812,y+36));draw.text((786,y+179),'black -> white ramp',font=small,fill='#bbbbc4')
canvas.save(dest/'comparison.png')
print(dest)
