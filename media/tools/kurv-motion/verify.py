#!/usr/bin/env python3
"""Check extracted planes reassemble at their native positions (requires Pillow)."""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageChops

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('directory',type=Path)
a=p.parse_args()
s=json.loads((a.directory/'scene.json').read_text())
full=Image.open(a.directory/'full.png').convert('RGBA')
assembled=Image.new('RGBA',full.size)
for layer in s['layers']:
    image=Image.open(a.directory/layer['src']).convert('RGBA')
    x,y,w,h=layer['rect']
    assert image.size==(round(w*s['scale']),round(h*s['scale']))
    assembled.alpha_composite(image,(round(x*s['scale']),round(y*s['scale'])))
diff=ImageChops.difference(full,assembled)
values=list(diff.get_flattened_data() if hasattr(diff, "get_flattened_data") else diff.getdata())
changed=sum(max(px)>3 for px in values)
worst=max(max(px) for px in values)
report={'pixels':len(values),'pixels_over_3':changed,'fraction_over_3':changed/len(values),'max_channel_error':worst}
(a.directory/'reassembly.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
assert changed==0,'Capture planes changed painter order or compositing; fix the partition before shipping.'
